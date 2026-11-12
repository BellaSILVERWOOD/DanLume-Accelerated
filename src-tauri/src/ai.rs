//! AI 助手：读旧库的 `llm_profiles`（多套配置）+ `llm_profile_key_<id>`（DPAPI 密文），
//! 走 OpenAI 兼容的 `/chat/completions`；历史会话存 `chat_sessions`。
use serde_json::{json, Value};

use crate::store::Store;

/// 当前用哪套配置：设置里的 `ai_profile`（id），没有就用第一套
///
/// 兼容旧版「单配置」写法：库里没有任何 `llm_profiles` 行时，退回读
/// `llm_base_url` / `llm_model_name`（旧版 page_settings 用的键），Key 仍走 `llm_api_key`。
fn active_profile(store: &Store) -> Option<(i64, String, String, String)> {
    if let Some(found) = active_profile_db(store) {
        return Some(found);
    }
    let base = store
        .setting("llm_base_url")
        .filter(|text| !text.trim().is_empty())?;
    let model = store.setting("llm_model_name").unwrap_or_default();
    let name = store
        .setting("llm_preset")
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| "旧版单配置".to_string());
    Some((0, name, base.trim().to_string(), model.trim().to_string()))
}

/// 建 AI 用到的两张表（`chat_sessions` / `llm_profiles`）。
///
/// Rust 版以前只读不建：老用户因为库里早就有这两张表（Python 版建的）看不出问题，
/// 但**全新的数据目录**里历史会话和多套 API 配置会直接报 "no such table"。
/// DDL 与旧版 `chat_store._ensure()` / `llm_profiles.ensure()` 保持一致。
fn ensure_ai_schema(store: &Store) {
    let conn = store.connection();
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS chat_sessions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            model TEXT NOT NULL DEFAULT '',
            scene TEXT NOT NULL DEFAULT '',
            payload TEXT NOT NULL DEFAULT '[]'
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS llm_profiles (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL DEFAULT '',
            base_url TEXT NOT NULL DEFAULT '',
            model TEXT NOT NULL DEFAULT '',
            preset TEXT NOT NULL DEFAULT '',
            note TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
        [],
    );
}

/// 旧版 `chat_store.KEEP_SESSIONS`：历史会话只留最近这么多条
const KEEP_SESSIONS: i64 = 200;

fn active_profile_db(store: &Store) -> Option<(i64, String, String, String)> {
    ensure_ai_schema(store);
    let wanted = store
        .setting("ai_profile")
        .and_then(|text| text.parse::<i64>().ok());
    let conn = store.connection();
    let sql = match wanted {
        Some(_) => "SELECT id, name, base_url, model FROM llm_profiles WHERE id = ?1",
        None => "SELECT id, name, base_url, model FROM llm_profiles ORDER BY id LIMIT 1",
    };
    let mut stmt = conn.prepare(sql).ok()?;
    let mapper = |row: &rusqlite::Row<'_>| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1).unwrap_or_default(),
            row.get::<_, String>(2).unwrap_or_default(),
            row.get::<_, String>(3).unwrap_or_default(),
        ))
    };
    let mut rows = match wanted {
        Some(id) => stmt.query_map([id], mapper).ok()?,
        None => stmt.query_map([], mapper).ok()?,
    };
    rows.next()?.ok()
}

fn profile_key(store: &Store, profile_id: i64) -> String {
    let encrypted = store
        .setting(&format!("llm_profile_key_{profile_id}"))
        .or_else(|| store.setting("llm_api_key"))
        .unwrap_or_default();
    if encrypted.is_empty() {
        return String::new();
    }
    crate::secrets::unprotect(&encrypted).unwrap_or_default()
}

/// 给前端看的配置状态（永不返回密钥本身）
pub fn config_state(store: &Store) -> Value {
    match active_profile(store) {
        Some((id, name, base_url, model)) => {
            let key = profile_key(store, id);
            json!({
                "ok": !key.is_empty(),
                "id": id,
                "name": name,
                "base_url": base_url,
                "model": model,
                "has_key": !key.is_empty(),
                "error": if key.is_empty() {
                    "这套配置还没填 API Key（设置 → AI）"
                } else {
                    ""
                },
            })
        }
        None => json!({
            "ok": false,
            "id": 0,
            "name": "",
            "base_url": "",
            "model": "",
            "has_key": false,
            "error": "还没有 AI 配置：设置 → AI（API 服务）里填一次",
        }),
    }
}

/// 列出所有配置（给设置页用）
///
/// 注意：拿着连接锁的时候**不能再调 `store.setting()`**（同一线程的 Mutex 不重入，会死锁），
/// 所以先把行读出来、放掉锁，再逐条解密 Key。
pub fn profiles(store: &Store) -> Vec<Value> {
    let rows: Vec<(i64, String, String, String)> = {
        let conn = store.connection();
        let Ok(mut stmt) =
            conn.prepare("SELECT id, name, base_url, model FROM llm_profiles ORDER BY id")
        else {
            return Vec::new();
        };
        let Ok(mapped) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1).unwrap_or_default(),
                row.get::<_, String>(2).unwrap_or_default(),
                row.get::<_, String>(3).unwrap_or_default(),
            ))
        }) else {
            return Vec::new();
        };
        mapped.flatten().collect()
    };
    rows.into_iter()
        .map(|(id, name, base_url, model)| {
            let has_key = !profile_key(store, id).is_empty();
            json!({
                "id": id, "name": name, "base_url": base_url, "model": model,
                "has_key": has_key,
            })
        })
        .collect()
}

/// 发一轮对话（messages = [{role, content}, …]），返回助手回复
pub fn chat(store: &Store, messages: &[Value], temperature: Option<f64>) -> Result<String, String> {
    let body = post(store, messages, temperature, None)?;
    let content = body
        .get("choices")
        .and_then(|value| value.as_array())
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if content.trim().is_empty() {
        // 有些模型把内容放在 reasoning_content 里
        let reasoning = body
            .get("choices")
            .and_then(|value| value.as_array())
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("message"))
            .and_then(|message| message.get("reasoning_content"))
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if reasoning.trim().is_empty() {
            return Err(format!("模型没返回内容：{}", trim_json(&body)));
        }
        return Ok(reasoning.to_string());
    }
    Ok(content.to_string())
}

/// 把前端给的消息整理成真正发给模型的 history（人设 / 当前情况 / 长期记忆 / 图片都在这里加）
///
/// `chat_with_tools`（一次性）和 `stream_with_tools`（流式）共用这一份，
/// 免得两条路走出来的上下文不一样。返回 `(history, 注入的记忆条数)`。
fn prepare_history(
    store: &Store,
    messages: &[Value],
    images: &[String],
    scene: Option<&str>,
) -> (Vec<Value>, usize) {
    let mut history: Vec<Value> = messages.to_vec();
    // 图片附件（旧版 `ai_files.image_parts`）：OpenAI 兼容的多模态写法，
    // 把**最后一条 user 消息**的 content 换成 [text, image_url…] 数组。
    if !images.is_empty() {
        if let Some(index) = history
            .iter()
            .rposition(|one| one.get("role").and_then(|v| v.as_str()) == Some("user"))
        {
            let text = history[index]
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let mut parts = vec![json!({ "type": "text", "text": text })];
            for url in images {
                if url.starts_with("data:") {
                    parts.push(json!({ "type": "image_url", "image_url": { "url": url } }));
                }
            }
            if parts.len() > 1 {
                history[index] = json!({ "role": "user", "content": parts });
            }
        }
    }
    // 长期记忆（旧版 `lumina/memory.py` 的 `memory.context_block()`）：
    // 旧版是把它拼在系统提示词的**最后一段**，这里同样作为最后一条 system 消息塞进去；
    // 返回的 `memory_injected` 给前端显示「本次注入了 N 条记忆」。
    let (memory_block, memory_used) = match crate::memory::injected(store) {
        Some((block, count)) => (Some(block), count),
        None => (None, 0),
    };
    if let Some(block) = memory_block {
        history.insert(0, json!({ "role": "system", "content": block }));
    }
    // 基础人设（旧版 `prompts.system("assistant")`，也就是提示词库里「通用助手」那一条）。
    // 顺序照旧版 build_messages：当前情况 → 人设 → … → 长期记忆（记忆在最后）。
    // 如果界面上选了「场景提示词」，就用那条场景的 prompt 当人设（旧版 `_scene_name()`）。
    let persona = match scene.map(|name| crate::prompts::scene(store, name)).flatten() {
        Some(text) if !text.trim().is_empty() => text,
        _ => crate::prompts::system(store, "assistant"),
    };
    if !persona.trim().is_empty() {
        history.insert(0, json!({ "role": "system", "content": persona }));
    }
    // 旧版 prompt_prepend_context / prompt_public_ip / prompt_gen_*：给模型塞一段「当前情况」
    if let Some(context) = context_message(store) {
        history.insert(0, json!({ "role": "system", "content": context }));
    }
    (history, memory_used)
}

/// 带工具的对话循环（对应旧版 `ai_api_panel.py` 的 `MAX_TOOL_ROUNDS=6`）。
///
/// 返回 `{"text": 最终回复, "rounds": 跑了几轮, "log": [{name, args, ok, summary}]}`。
/// 这个入口**不经过用户审核**（单测、脚本用）；界面走 `chat_with_tools_confirm`。
pub fn chat_with_tools(
    store: &Store,
    messages: &[Value],
    temperature: Option<f64>,
    images: &[String],
    scene: Option<&str>,
) -> Result<Value, String> {
    chat_with_tools_inner(
        store,
        messages,
        temperature,
        images,
        scene,
        &mut |_name, _args| true,
    )
}

/// 同上，但危险工具动手前先问用户：`confirm` 返回 false 就不执行这一条
pub fn chat_with_tools_confirm(
    store: &Store,
    messages: &[Value],
    temperature: Option<f64>,
    images: &[String],
    scene: Option<&str>,
    confirm: &mut dyn FnMut(&str, &Value) -> bool,
) -> Result<Value, String> {
    chat_with_tools_inner(store, messages, temperature, images, scene, confirm)
}

fn chat_with_tools_inner(
    store: &Store,
    messages: &[Value],
    temperature: Option<f64>,
    images: &[String],
    scene: Option<&str>,
    confirm: &mut dyn FnMut(&str, &Value) -> bool,
) -> Result<Value, String> {
    let tools = crate::ai_tools::definitions(store);
    // 使用记账（旧版 usage_report.mark_activity）：每轮提问算 1 次请求
    crate::usage::mark_activity(store, 0.0, 1);
    let started = std::time::Instant::now();
    let tool_list = if tools.is_empty() {
        None
    } else {
        Some(json!(tools))
    };
    let (mut history, memory_used) = prepare_history(store, messages, images, scene);
    let mut log: Vec<Value> = Vec::new();
    let mut last_text = String::new();
    // 「思考过程」（DeepSeek-R1 / 豆包等会返回 reasoning_content）与 token 用量：
    // 旧版 `ai_api_panel` 把思考单独折叠一块、右下角显示 token 数，这里一并带回去。
    let mut reasoning = String::new();
    let mut usage = json!({ "prompt": 0, "completion": 0, "total": 0 });
    for round in 1..=crate::ai_tools::MAX_TOOL_ROUNDS {
        let body = post(store, &history, temperature, tool_list.as_ref())?;
        if let Some(extra) = body
            .pointer("/choices/0/message/reasoning_content")
            .and_then(|value| value.as_str())
        {
            if !extra.trim().is_empty() {
                reasoning.push_str(extra);
            }
        }
        if let Some(one) = body.get("usage") {
            let pick = |key: &str| one.get(key).and_then(|v| v.as_u64()).unwrap_or(0);
            usage = json!({
                "prompt": pick("prompt_tokens"),
                "completion": pick("completion_tokens"),
                "total": pick("total_tokens"),
            });
        }
        let message = body
            .pointer("/choices/0/message")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let content = message
            .get("content")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        if !content.trim().is_empty() {
            last_text = content.clone();
        }
        let calls = message
            .get("tool_calls")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        if calls.is_empty() {
            crate::usage::mark_activity(store, started.elapsed().as_secs_f64(), 0);
            let raw = if last_text.trim().is_empty() {
                format!("（模型没有返回正文，第 {round} 轮结束）")
            } else {
                last_text
            };
            // 回答里出现 `[[记住]] …`：摘出来写进记忆库，正文里不留（同旧版 memory.extract）
            let (text, remembered) = crate::memory::absorb(store, &raw);
            return Ok(json!({
                "text": text,
                "rounds": round,
                "log": log,
                "tools_on": tools.len(),
                "memory_injected": memory_used,
                "remembered": remembered,
                "reasoning": reasoning,
                "usage": usage,
            }));
        }
        // 把助手的 tool_calls 消息放回历史，再逐个执行
        history.push(message.clone());
        for call in &calls {
            let name = call
                .pointer("/function/name")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string();
            let raw_args = call
                .pointer("/function/arguments")
                .and_then(|value| value.as_str())
                .unwrap_or("{}");
            let args: Value = serde_json::from_str(raw_args).unwrap_or_else(|_| json!({}));
            let result = crate::ai_tools::dispatch_approved(store, &name, &args, confirm);
            log.push(json!({
                "name": name,
                "args": args,
                "ok": result.get("ok").and_then(|one| one.as_bool()).unwrap_or(false),
                "denied": result.get("denied").and_then(|one| one.as_bool()).unwrap_or(false),
                "summary": result.get("summary").and_then(|one| one.as_str()).unwrap_or(""),
            }));
            let call_id = call
                .get("id")
                .and_then(|value| value.as_str())
                .unwrap_or("call_0");
            history.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": result.to_string(),
            }));
        }
    }
    let rounds = history.len();
    let raw = if last_text.trim().is_empty() {
        format!("用了 {} 步工具还没得出结论，换个说法再试一次？", rounds)
    } else {
        last_text
    };
    let (text, remembered) = crate::memory::absorb(store, &raw);
    Ok(json!({
        "text": text,
        "rounds": crate::ai_tools::MAX_TOOL_ROUNDS,
        "log": log,
        "tools_on": tools.len(),
        "memory_injected": memory_used,
        "remembered": remembered,
        "reasoning": reasoning,
        "usage": usage,
    }))
}

// ------------------------------------------------------------------ 流式对话
//
// 对应旧版 `ai_api_panel` 的「边收边显示」和「停止生成」。做法：
// 后端用 `stream: true` 读 SSE，每收到一小段就通过回调推给前端（Tauri event），
// 前端把增量拼进同一条消息；用户点「停止」就给 cancel 表里塞一个 id，读流的循环看到就收手。

/// 被要求停止的流式请求 id（用户点了「停止生成」）
static CANCELLED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// 请停止某个流式请求（前端 `ai_cancel_stream` 调）
pub fn cancel_stream(id: &str) {
    if let Ok(mut list) = CANCELLED.lock() {
        if !list.iter().any(|one| one == id) {
            list.push(id.to_string());
        }
    }
}

/// 这个请求是否已被要求停止（**取一次就清掉**，避免下次同名请求被误杀）
pub fn is_cancelled(id: &str) -> bool {
    if let Ok(mut list) = CANCELLED.lock() {
        if let Some(pos) = list.iter().position(|one| one == id) {
            list.remove(pos);
            return true;
        }
    }
    false
}

/// 一行 SSE 里 `data:` 后面的内容（`[DONE]` 原样返回；注释行 / 空行返回 None）
fn sse_payload(line: &str) -> Option<String> {
    let text = line.trim();
    if text.is_empty() || text.starts_with(':') {
        return None;
    }
    let rest = text.strip_prefix("data:")?;
    Some(rest.trim().to_string())
}

/// 流式响应的累加器：正文 / 思考过程 / 工具调用 / token 用量
#[derive(Default)]
struct StreamState {
    text: String,
    reasoning: String,
    /// 工具调用：[{id, name, arguments}]，`arguments` 是一段段拼起来的 JSON 字符串
    calls: Vec<Value>,
    usage: Value,
}

impl StreamState {
    /// 吃一条 chunk，返回这次要推给前端的增量（`kind` = text / reasoning）
    fn absorb(&mut self, chunk: &Value) -> Vec<(&'static str, String)> {
        let mut out: Vec<(&'static str, String)> = Vec::new();
        // 用量只会在最后那条 chunk 里出现（要 `stream_options.include_usage`）
        if let Some(usage) = chunk.get("usage").filter(|one| !one.is_null()) {
            let pick = |key: &str| usage.get(key).and_then(|v| v.as_u64()).unwrap_or(0);
            let total = pick("total_tokens");
            if total > 0 {
                self.usage = json!({
                    "prompt": pick("prompt_tokens"),
                    "completion": pick("completion_tokens"),
                    "total": total,
                });
            }
        }
        let Some(delta) = chunk.pointer("/choices/0/delta") else {
            return out;
        };
        if let Some(extra) = delta.get("reasoning_content").and_then(|v| v.as_str()) {
            if !extra.is_empty() {
                self.reasoning.push_str(extra);
                out.push(("reasoning", extra.to_string()));
            }
        }
        if let Some(text) = delta.get("content").and_then(|v| v.as_str()) {
            if !text.is_empty() {
                self.text.push_str(text);
                out.push(("text", text.to_string()));
            }
        }
        if let Some(items) = delta.get("tool_calls").and_then(|v| v.as_array()) {
            for item in items {
                let index = item.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                while self.calls.len() <= index {
                    self.calls.push(json!({ "id": "", "name": "", "arguments": "" }));
                }
                let slot = &mut self.calls[index];
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    if !id.is_empty() {
                        slot["id"] = json!(id);
                    }
                }
                if let Some(name) = item.pointer("/function/name").and_then(|v| v.as_str()) {
                    if !name.is_empty() {
                        slot["name"] = json!(name);
                    }
                }
                if let Some(args) = item.pointer("/function/arguments").and_then(|v| v.as_str()) {
                    let old = slot
                        .get("arguments")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    slot["arguments"] = json!(format!("{old}{args}"));
                }
            }
        }
        out
    }

    /// 攒好的工具调用 → 能塞回 history 的 assistant 消息（和一次性那条路的格式一致）
    fn assistant_with_calls(&self) -> Value {
        let calls: Vec<Value> = self
            .calls
            .iter()
            .filter(|one| {
                one.get("name")
                    .and_then(|v| v.as_str())
                    .map(|name| !name.is_empty())
                    .unwrap_or(false)
            })
            .map(|one| {
                let get = |key: &str| {
                    one.get(key)
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string()
                };
                json!({
                    "id": get("id"),
                    "type": "function",
                    "function": { "name": get("name"), "arguments": get("arguments") },
                })
            })
            .collect();
        json!({
            "role": "assistant",
            "content": if self.text.is_empty() { Value::Null } else { json!(self.text) },
            "tool_calls": calls,
        })
    }
}

/// 发一次流式请求，边读边把增量交给 `on_delta`；返回 `(累加结果, 是不是被用户停了)`
fn post_stream(
    store: &Store,
    messages: &[Value],
    temperature: Option<f64>,
    tools: Option<&Value>,
    on_delta: &mut dyn FnMut(&str, &str),
    cancelled: &dyn Fn() -> bool,
) -> Result<(StreamState, bool), String> {
    let (url, key, model) = endpoint(store)?;
    let mut payload = json!({
        "model": model,
        "messages": messages,
        "temperature": temperature.unwrap_or(0.7),
        "stream": true,
        // 不加这个，流式回来的 chunk 里没有 usage，右下角的 token 数就一直是 0
        "stream_options": { "include_usage": true },
    });
    if let Some(list) = tools {
        payload["tools"] = list.clone();
        payload["tool_choice"] = json!("auto");
    }
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(300))
        .user_agent("DanLume/2.0")
        .build();
    let response = match agent
        .post(&url)
        .set("Content-Type", "application/json")
        .set("Accept", "text/event-stream")
        .set("Authorization", &format!("Bearer {key}"))
        .send_string(&payload.to_string())
    {
        Ok(value) => value,
        Err(ureq::Error::Status(code, response)) => {
            let body = response.into_string().unwrap_or_default();
            return Err(explain(code, &body));
        }
        Err(err) => return Err(format!("请求失败：{err}")),
    };
    use std::io::BufRead;
    let reader = std::io::BufReader::new(response.into_reader());
    let mut state = StreamState::default();
    let mut stopped = false;
    for line in reader.lines() {
        if cancelled() {
            stopped = true;
            break;
        }
        let Ok(line) = line else { break };
        let Some(data) = sse_payload(&line) else {
            continue;
        };
        if data == "[DONE]" {
            break;
        }
        let Ok(chunk) = serde_json::from_str::<Value>(&data) else {
            continue;
        };
        for (kind, text) in state.absorb(&chunk) {
            on_delta(kind, &text);
        }
    }
    Ok((state, stopped))
}

/// 带工具的**流式**对话循环：和 `chat_with_tools` 一样的返回，多一个 `stopped`
///
/// `on_delta(kind, text)`：kind 是 `text`（正文增量）/ `reasoning`（思考增量）/
/// `tool`（正在调用哪个工具，text 是工具名）。
pub fn stream_with_tools<F, C>(
    store: &Store,
    messages: &[Value],
    temperature: Option<f64>,
    images: &[String],
    scene: Option<&str>,
    mut on_delta: F,
    cancelled: C,
    confirm: &mut dyn FnMut(&str, &Value) -> bool,
) -> Result<Value, String>
where
    F: FnMut(&str, &str),
    C: Fn() -> bool,
{
    let tools = crate::ai_tools::definitions(store);
    crate::usage::mark_activity(store, 0.0, 1);
    let started = std::time::Instant::now();
    let tool_list = if tools.is_empty() {
        None
    } else {
        Some(json!(tools))
    };
    let (mut history, memory_used) = prepare_history(store, messages, images, scene);
    let mut log: Vec<Value> = Vec::new();
    let mut reasoning = String::new();
    let mut usage = json!({ "prompt": 0, "completion": 0, "total": 0 });
    let mut last_text = String::new();
    let mut rounds = 0;
    let mut stopped = false;
    for round in 1..=crate::ai_tools::MAX_TOOL_ROUNDS {
        rounds = round;
        let (state, was_stopped) = post_stream(
            store,
            &history,
            temperature,
            tool_list.as_ref(),
            &mut on_delta,
            &cancelled,
        )?;
        if !state.reasoning.is_empty() {
            reasoning.push_str(&state.reasoning);
        }
        if state
            .usage
            .get("total")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
            > 0
        {
            usage = state.usage.clone();
        }
        if !state.text.trim().is_empty() {
            last_text = state.text.clone();
        }
        if was_stopped {
            stopped = true;
            break;
        }
        let message = state.assistant_with_calls();
        let calls = message
            .get("tool_calls")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if calls.is_empty() {
            break;
        }
        history.push(message);
        for call in &calls {
            if cancelled() {
                stopped = true;
                break;
            }
            let name = call
                .pointer("/function/name")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string();
            on_delta("tool", &name);
            let raw_args = call
                .pointer("/function/arguments")
                .and_then(|value| value.as_str())
                .unwrap_or("{}");
            // 流式拼出来的 arguments 偶尔会是半截 JSON，解析不了就当空参数
            let args: Value = serde_json::from_str(raw_args).unwrap_or_else(|_| json!({}));
            let result = crate::ai_tools::dispatch_approved(store, &name, &args, confirm);
            log.push(json!({
                "name": name,
                "args": args,
                "ok": result.get("ok").and_then(|one| one.as_bool()).unwrap_or(false),
                "denied": result.get("denied").and_then(|one| one.as_bool()).unwrap_or(false),
                "summary": result.get("summary").and_then(|one| one.as_str()).unwrap_or(""),
            }));
            let call_id = call
                .get("id")
                .and_then(|value| value.as_str())
                .unwrap_or("call_0");
            history.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": result.to_string(),
            }));
        }
        if stopped {
            break;
        }
    }
    if !stopped {
        crate::usage::mark_activity(store, started.elapsed().as_secs_f64(), 0);
    }
    let raw = if last_text.trim().is_empty() {
        if stopped {
            "（已停止）".to_string()
        } else {
            format!("用了 {rounds} 步工具还没得出结论，换个说法再试一次？")
        }
    } else {
        last_text
    };
    let (text, remembered) = crate::memory::absorb(store, &raw);
    Ok(json!({
        "text": text,
        "rounds": rounds,
        "log": log,
        "tools_on": tools.len(),
        "memory_injected": memory_used,
        "remembered": remembered,
        "reasoning": reasoning,
        "usage": usage,
        "stopped": stopped,
    }))
}

/// 给模型的「当前情况」前缀（旧版 `prompt_prepend_context` 那套）
fn context_message(store: &Store) -> Option<String> {
    let prepend = store
        .setting("prompt_prepend_context")
        .map(|text| text.trim() != "0")
        .unwrap_or(false);
    let want_ip = store
        .setting("prompt_public_ip")
        .map(|text| text.trim() == "1")
        .unwrap_or(false);
    let gen_enabled = store
        .setting("prompt_gen_enabled")
        .map(|text| text.trim() == "1")
        .unwrap_or(false);
    if !prepend && !want_ip && !gen_enabled {
        return None;
    }
    let now = chrono::Local::now();
    let mut lines: Vec<String> = Vec::new();
    if prepend {
        let today = crate::today_str();
        let events = store.events_on(&today).map(|rows| rows.len()).unwrap_or(0);
        let unread = store.notice_unread_count();
        lines.push(format!(
            "现在是 {}（星期{}）。今天有 {} 条日程，{} 条未读通知。",
            now.format("%Y-%m-%d %H:%M"),
            "一二三四五六日".chars().nth(now.format("%u").to_string().parse::<usize>().unwrap_or(1) - 1).unwrap_or('一'),
            events,
            unread
        ));
    }
    if want_ip {
        match public_ip() {
            Ok(ip) => lines.push(format!("本机公网 IP：{ip}")),
            Err(exc) => {
                let _ = store.set_setting("prompt_gen_last_error", &exc);
            }
        }
    }
    if gen_enabled {
        let command = store.setting("prompt_gen_command").unwrap_or_default();
        if !command.trim().is_empty() {
            match run_prompt_command(&command) {
                Ok(out) if !out.trim().is_empty() => {
                    let trimmed: String = out.chars().take(2000).collect();
                    lines.push(format!("外部命令给出的补充提示：\n{trimmed}"));
                }
                Ok(_) => {}
                Err(exc) => {
                    let _ = store.set_setting("prompt_gen_last_error", &exc);
                }
            }
        }
    }
    if lines.is_empty() {
        None
    } else {
        Some(format!("（DanLume 自动附加的上下文，供你参考）\n{}", lines.join("\n")))
    }
}

fn public_ip() -> Result<String, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(6))
        .user_agent("DanLume/2.0")
        .build();
    let text = agent
        .get("https://api.ipify.org")
        .call()
        .map_err(|err| format!("取公网 IP 失败：{err}"))?
        .into_string()
        .map_err(|err| format!("读公网 IP 失败：{err}"))?;
    Ok(text.trim().to_string())
}

/// 跑旧版 `prompt_gen_command`：把命令交给 `cmd /C`，取它的 stdout
fn run_prompt_command(command: &str) -> Result<String, String> {
    #[cfg(windows)]
    let output = std::process::Command::new("cmd")
        .args(["/C", command])
        .output();
    #[cfg(not(windows))]
    let output = std::process::Command::new("sh")
        .args(["-c", command])
        .output();
    let output = output.map_err(|err| format!("提示词命令跑不起来：{err}"))?;
    if !output.status.success() {
        let text = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(format!("提示词命令失败：{}", text.chars().take(300).collect::<String>()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// 拼出「往哪发、用什么 Key、哪个模型」（`post` 和流式共用）
fn endpoint(store: &Store) -> Result<(String, String, String), String> {
    let Some((id, _name, base_url, model)) = active_profile(store) else {
        return Err("还没有 AI 配置：设置 → AI（API 服务）里填一次".to_string());
    };
    let key = profile_key(store, id);
    if key.is_empty() {
        return Err("这套配置还没填 API Key（设置 → AI）".to_string());
    }
    if base_url.trim().is_empty() {
        return Err("这套配置没有接口地址（设置 → AI）".to_string());
    }
    let base = base_url.trim_end_matches('/');
    let url = if base.ends_with("/chat/completions") {
        base.to_string()
    } else if base.ends_with("/v1") || base.contains("/v1/") {
        format!("{base}/chat/completions")
    } else {
        format!("{base}/v1/chat/completions")
    };
    Ok((url, key, model))
}

fn post(
    store: &Store,
    messages: &[Value],
    temperature: Option<f64>,
    tools: Option<&Value>,
) -> Result<Value, String> {
    let (url, key, model) = endpoint(store)?;
    let mut payload = json!({
        "model": model,
        "messages": messages,
        "temperature": temperature.unwrap_or(0.7),
        "stream": false,
    });
    if let Some(list) = tools {
        payload["tools"] = list.clone();
        payload["tool_choice"] = json!("auto");
    }
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(180))
        .user_agent("DanLume/2.0")
        .build();
    let response = agent
        .post(&url)
        .set("Content-Type", "application/json")
        .set("Authorization", &format!("Bearer {key}"))
        .send_string(&payload.to_string());
    let response = match response {
        Ok(value) => value,
        Err(ureq::Error::Status(code, response)) => {
            let body = response.into_string().unwrap_or_default();
            return Err(explain(code, &body));
        }
        Err(err) => return Err(format!("请求失败：{err}")),
    };
    response
        .into_json()
        .map_err(|err| format!("响应不是 JSON：{err}"))
}

fn trim_json(value: &Value) -> String {
    let text = value.to_string();
    text.chars().take(300).collect()
}

fn explain(code: u16, body: &str) -> String {
    let snippet = body.chars().take(200).collect::<String>();
    match code {
        401 | 403 => format!("Key 被拒绝（{code}）：去 设置 → AI 里检查 API Key。服务端说：{snippet}"),
        404 => format!("接口地址不对（404）：检查 设置 → AI 的接口地址（要能以 /v1 结尾）。{snippet}"),
        429 => format!("请求太频繁（429）：等一会儿再试。{snippet}"),
        400 => format!("请求被拒绝（400）：模型名可能不对。{snippet}"),
        _ => format!("请求失败（{code}）：{snippet}"),
    }
}

// ------------------------------------------------------------------ 历史会话

pub fn sessions(store: &Store, limit: i64) -> Vec<Value> {
    ensure_ai_schema(store);
    let conn = store.connection();
    let mut out = Vec::new();
    let Ok(mut stmt) = conn.prepare(
        "SELECT id, title, model, scene, updated_at, payload FROM chat_sessions
         ORDER BY updated_at DESC LIMIT ?1",
    ) else {
        return out;
    };
    let Ok(rows) = stmt.query_map([limit], |row| {
        Ok(json!({
            "id": row.get::<_, i64>(0)?,
            "title": row.get::<_, String>(1).unwrap_or_default(),
            "model": row.get::<_, String>(2).unwrap_or_default(),
            "scene": row.get::<_, String>(3).unwrap_or_default(),
            "updated_at": row.get::<_, String>(4).unwrap_or_default(),
            "payload": row.get::<_, String>(5).unwrap_or_default(),
        }))
    }) else {
        return out;
    };
    for row in rows.flatten() {
        out.push(row);
    }
    out
}

pub fn save_session(store: &Store, title: &str, model: &str, payload: &str) -> Result<i64, String> {
    ensure_ai_schema(store);
    let conn = store.connection();
    let now = crate::now_str();
    // 旧版 `chat_store.MAX_TITLE = 40`
    let title: String = title.chars().take(40).collect();
    conn.execute(
        "INSERT INTO chat_sessions(title, created_at, updated_at, model, scene, payload)
         VALUES (?1, ?2, ?2, ?3, 'api', ?4)",
        rusqlite::params![title, now, model, payload],
    )
    .map_err(|err| err.to_string())?;
    let id = conn.last_insert_rowid();
    // 旧版 `chat_store._trim()`：只留最近 200 条，免得库越用越大
    let _ = conn.execute(
        "DELETE FROM chat_sessions WHERE id NOT IN (
            SELECT id FROM chat_sessions ORDER BY updated_at DESC, id DESC LIMIT ?1
        )",
        [KEEP_SESSIONS],
    );
    Ok(id)
}

/// 新增 / 修改一套配置（id = 0 表示新增）
pub fn save_profile(
    store: &Store,
    id: i64,
    name: &str,
    base_url: &str,
    model: &str,
) -> Result<i64, String> {
    ensure_ai_schema(store);
    let conn = store.connection();
    let now = crate::now_str();
    if id > 0 {
        conn.execute(
            "UPDATE llm_profiles SET name = ?1, base_url = ?2, model = ?3, updated_at = ?4
             WHERE id = ?5",
            rusqlite::params![name, base_url, model, now, id],
        )
        .map_err(|err| err.to_string())?;
        Ok(id)
    } else {
        conn.execute(
            "INSERT INTO llm_profiles(name, base_url, model, preset, note, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'custom', '', ?4, ?4)",
            rusqlite::params![name, base_url, model, now],
        )
        .map_err(|err| err.to_string())?;
        Ok(conn.last_insert_rowid())
    }
}

pub fn delete_profile(store: &Store, id: i64) -> Result<bool, String> {
    ensure_ai_schema(store);
    let conn = store.connection();
    let changed = conn
        .execute("DELETE FROM llm_profiles WHERE id = ?1", [id])
        .map_err(|err| err.to_string())?;
    // Key 也一起清掉
    let _ = store.set_setting(&format!("llm_profile_key_{id}"), "");
    Ok(changed > 0)
}

/// 改历史会话的标题（用户 2026-11-11：历史会话要能重命名）
pub fn rename_session(store: &Store, id: i64, title: &str) -> Result<(), String> {
    if id <= 0 {
        return Err("这条会话还没存进历史".to_string());
    }
    let title = title.trim();
    let title: String = title.chars().take(40).collect();
    let title = title.as_str();
    let title = if title.is_empty() { "（未命名）" } else { title };
    ensure_ai_schema(store);
    let conn = store.connection();
    let changed = conn
        .execute(
            "UPDATE chat_sessions SET title = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![title, crate::now_str(), id],
        )
        .map_err(|err| err.to_string())?;
    if changed == 0 {
        return Err("没找到这条历史会话".to_string());
    }
    Ok(())
}

/// 删掉一条历史会话
pub fn delete_session(store: &Store, id: i64) -> Result<(), String> {
    ensure_ai_schema(store);
    let conn = store.connection();
    let changed = conn
        .execute("DELETE FROM chat_sessions WHERE id = ?1", [id])
        .map_err(|err| err.to_string())?;
    if changed == 0 {
        return Err("没找到这条历史会话".to_string());
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    /// 真数据用例：数据来自临时副本（见 `crate::testdata`），不碰用户的真实库。
    fn real_store() -> Store {
        crate::testdata::real_store("ai")
    }

    #[test]
    #[ignore = "真数据用例：需要开发机上的真实数据目录（见 crate::testdata）"]
    fn config_reads_legacy_profiles() {
        let store = real_store();
        let state = config_state(&store);
        println!("AI 配置：{}", state);
        let rows = profiles(&store);
        println!("配置套数：{}", rows.len());
        for row in &rows {
            println!("  {}", row);
        }
        assert!(state.get("id").is_some());
    }

    /// 记忆注入：不联网也能验（会在真正发请求之前就报「没配 AI」）
    #[test]
    fn chat_with_tools_injects_memory() {
        // 环境变量是进程级的：跑完要恢复，别影响同进程其它用例；目录名带进程 id，
        // 免得同时有另一个 `cargo test` 在同名目录上撞车。
        let previous = std::env::var_os("LUMINA_DATA_DIR");
        let dir = std::env::temp_dir().join(format!("danlume-ai-memory-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("LUMINA_DATA_DIR", &dir);
        let store = Store::open().expect("建临时库失败");
        crate::memory::add(&store, "我不吃香菜", "手动", "").unwrap();
        let messages = vec![serde_json::json!({ "role": "user", "content": "你好" })];
        let err = chat_with_tools(&store, &messages, Some(0.2), &[], None).unwrap_err();
        println!("没配 AI 时的报错：{err}");
        assert!(err.contains("AI 配置"), "应当先撞上「还没配置」");
        // 注入发生在发请求之前，所以命中次数已经记上了
        let rows = crate::memory::list(&store, 10);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].hits, 1, "这次对话把它带上了");
        assert!(rows[0].last_used_at.is_some());
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
        match previous {
            Some(value) => std::env::set_var("LUMINA_DATA_DIR", value),
            None => std::env::remove_var("LUMINA_DATA_DIR"),
        }
    }

    /// 真联网：`cargo test ai_real_call -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn ai_real_call() {
        let store = real_store();
        let messages = vec![serde_json::json!({
            "role": "user",
            "content": "只回四个字：连接成功"
        })];
        match chat(&store, &messages, Some(0.2)) {
            Ok(reply) => println!("模型回复：{}", reply),
            Err(err) => panic!("调用失败：{}", err),
        }
    }

    /// 真联网 + 真工具：`cargo test ai_tool_loop_real -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn ai_tool_loop_real() {
        let store = real_store();
        let messages = vec![serde_json::json!({
            "role": "user",
            "content": "用工具查一下现在几点、今天有几条日程，然后一句话汇报。"
        })];
        match chat_with_tools(&store, &messages, Some(0.2), &[], None) {
            Ok(result) => {
                println!("最终回复：{}", result["text"]);
                println!("轮次：{}", result["rounds"]);
                println!("可用工具：{}", result["tools_on"]);
                for entry in result["log"].as_array().cloned().unwrap_or_default() {
                    println!("  工具 {} → {} ({})", entry["name"], entry["summary"], entry["ok"]);
                }
            }
            Err(err) => panic!("调用失败：{}", err),
        }
    }

    /// 真联网的流式：`cargo test ai_stream_real -- --ignored --nocapture`
    ///
    /// 断言「确实是一小块一小块回来的」——如果只回调了一次，说明服务端没按 SSE 流式返回。
    #[test]
    #[ignore]
    fn ai_stream_real() {
        let store = real_store();
        let messages = vec![serde_json::json!({
            "role": "user",
            "content": "用三句话介绍一下复旦大学的课程表，不要用工具。"
        })];
        let mut chunks: Vec<String> = Vec::new();
        let result = stream_with_tools(
            &store,
            &messages,
            Some(0.3),
            &[],
            None,
            |kind, text| {
                if kind == "text" {
                    chunks.push(text.to_string());
                }
            },
            || false,
            &mut |_name, _args| true,
        )
        .expect("流式调用失败");
        println!("收到 {} 段增量", chunks.len());
        println!("拼起来的回复：{}", result["text"]);
        println!("用量：{}", result["usage"]);
        assert!(chunks.len() >= 3, "应该分成多段回来，实际 {} 段", chunks.len());
        assert!(!result["text"].as_str().unwrap_or("").trim().is_empty());
    }

    /// 历史会话能改名、能删（临时库，不碰真实数据）
    #[test]
    fn session_rename_and_delete_roundtrip() {
        let previous = std::env::var_os("LUMINA_DATA_DIR");
        let dir = std::env::temp_dir().join(format!("danlume-ai-session-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("LUMINA_DATA_DIR", &dir);
        let store = Store::open().expect("建临时库失败");
        let id = save_session(&store, "旧标题", "deepseek-chat", "[]").expect("存会话");
        rename_session(&store, id, "  新标题  ").expect("改名");
        let rows = sessions(&store, 10);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["title"], json!("新标题"), "两头空格应当被去掉");
        // 空标题写成「（未命名）」，前端不显示空白标题
        rename_session(&store, id, "   ").expect("改成空标题");
        assert_eq!(sessions(&store, 10)[0]["title"], json!("（未命名）"));
        // 删掉之后列表里就没有了
        delete_session(&store, id).expect("删除");
        assert!(sessions(&store, 10).is_empty());
        // 不存在的 id 要报错，前端才好提示
        assert!(rename_session(&store, 999_999, "x").is_err());
        assert!(delete_session(&store, 999_999).is_err());
        // 全新数据目录里「多套 API 配置」也要能存（以前这两张表是 Python 版建的，Rust 只读不建）
        let pid = save_profile(
            &store,
            0,
            "测试配置",
            "https://api.deepseek.com/v1",
            "deepseek-chat",
        )
        .expect("全新库里存配置");
        assert!(pid > 0);
        assert!(profiles(&store).iter().any(|one| one["id"] == json!(pid)));
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
        match previous {
            Some(value) => std::env::set_var("LUMINA_DATA_DIR", value),
            None => std::env::remove_var("LUMINA_DATA_DIR"),
        }
    }
}

/// 流式解析的纯本地逻辑（不联网）：SSE 行、增量合并、工具调用拼装、停止标记
#[cfg(test)]
mod stream_tests {
    use super::*;

    #[test]
    fn sse_lines_are_parsed() {
        assert_eq!(sse_payload("data: {\"a\":1}").as_deref(), Some("{\"a\":1}"));
        assert_eq!(sse_payload("data:[DONE]").as_deref(), Some("[DONE]"));
        assert_eq!(sse_payload(": keep-alive"), None);
        assert_eq!(sse_payload(""), None);
        assert_eq!(sse_payload("event: ping"), None);
    }

    #[test]
    fn deltas_accumulate_text_and_reasoning() {
        let mut state = StreamState::default();
        let first = state.absorb(&json!({ "choices": [{ "delta": { "reasoning_content": "想一下" } }] }));
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].0, "reasoning");
        let second = state.absorb(&json!({ "choices": [{ "delta": { "content": "你好" } }] }));
        assert_eq!(second[0], ("text", "你好".to_string()));
        state.absorb(&json!({ "choices": [{ "delta": { "content": "，世界" } }] }));
        assert_eq!(state.text, "你好，世界");
        assert_eq!(state.reasoning, "想一下");
    }

    #[test]
    fn tool_call_arguments_are_merged_by_index() {
        let mut state = StreamState::default();
        state.absorb(&json!({ "choices": [{ "delta": { "tool_calls": [{
            "index": 0, "id": "call_1",
            "function": { "name": "fs_list", "arguments": "{\"pa" } }] } }] }));
        state.absorb(&json!({ "choices": [{ "delta": { "tool_calls": [{
            "index": 0, "function": { "arguments": "th\":\".\"}" } }] } }] }));
        let message = state.assistant_with_calls();
        let call = &message["tool_calls"][0];
        assert_eq!(call["id"], json!("call_1"));
        assert_eq!(call["function"]["name"], json!("fs_list"));
        // 拼出来的必须是合法 JSON，否则下一轮请求会被服务端 400
        let raw = call["function"]["arguments"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(raw).expect("参数应当是合法 JSON");
        assert_eq!(parsed["path"], json!("."));
    }

    #[test]
    fn usage_is_picked_from_last_chunk_only() {
        let mut state = StreamState::default();
        state.absorb(&json!({ "choices": [{ "delta": { "content": "x" } }] }));
        assert_eq!(state.usage, Value::Null, "普通 chunk 不带用量");
        state.absorb(&json!({
            "choices": [],
            "usage": { "prompt_tokens": 3, "completion_tokens": 5, "total_tokens": 8 }
        }));
        assert_eq!(state.usage["total"], json!(8));
    }

    #[test]
    fn cancel_flag_is_one_shot() {
        cancel_stream("req-test-1");
        assert!(is_cancelled("req-test-1"));
        assert!(!is_cancelled("req-test-1"), "取过一次就该清掉，否则下一轮会被误杀");
    }
}
