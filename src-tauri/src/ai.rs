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

fn active_profile_db(store: &Store) -> Option<(i64, String, String, String)> {
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

/// 带工具的对话循环（对应旧版 `ai_api_panel.py` 的 `MAX_TOOL_ROUNDS=6`）。
///
/// 返回 `{"text": 最终回复, "rounds": 跑了几轮, "log": [{name, args, ok, summary}]}`。
pub fn chat_with_tools(
    store: &Store,
    messages: &[Value],
    temperature: Option<f64>,
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
    let mut history: Vec<Value> = messages.to_vec();
    // 旧版 prompt_prepend_context / prompt_public_ip / prompt_gen_*：给模型塞一段「当前情况」
    if let Some(context) = context_message(store) {
        history.insert(0, json!({ "role": "system", "content": context }));
    }
    let mut log: Vec<Value> = Vec::new();
    let mut last_text = String::new();
    for round in 1..=crate::ai_tools::MAX_TOOL_ROUNDS {
        let body = post(store, &history, temperature, tool_list.as_ref())?;
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
            return Ok(json!({
                "text": if last_text.trim().is_empty() {
                    format!("（模型没有返回正文，第 {round} 轮结束）")
                } else {
                    last_text
                },
                "rounds": round,
                "log": log,
                "tools_on": tools.len(),
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
            let result = crate::ai_tools::dispatch(store, &name, &args);
            log.push(json!({
                "name": name,
                "args": args,
                "ok": result.get("ok").and_then(|one| one.as_bool()).unwrap_or(false),
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
    Ok(json!({
        "text": if last_text.trim().is_empty() {
            format!("用了 {} 步工具还没得出结论，换个说法再试一次？", rounds)
        } else {
            last_text
        },
        "rounds": crate::ai_tools::MAX_TOOL_ROUNDS,
        "log": log,
        "tools_on": tools.len(),
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

fn post(
    store: &Store,
    messages: &[Value],
    temperature: Option<f64>,
    tools: Option<&Value>,
) -> Result<Value, String> {
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
    let conn = store.connection();
    let now = crate::now_str();
    conn.execute(
        "INSERT INTO chat_sessions(title, created_at, updated_at, model, scene, payload)
         VALUES (?1, ?2, ?2, ?3, 'api', ?4)",
        rusqlite::params![title, now, model, payload],
    )
    .map_err(|err| err.to_string())?;
    Ok(conn.last_insert_rowid())
}

/// 新增 / 修改一套配置（id = 0 表示新增）
pub fn save_profile(
    store: &Store,
    id: i64,
    name: &str,
    base_url: &str,
    model: &str,
) -> Result<i64, String> {
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
    let conn = store.connection();
    let changed = conn
        .execute("DELETE FROM llm_profiles WHERE id = ?1", [id])
        .map_err(|err| err.to_string())?;
    // Key 也一起清掉
    let _ = store.set_setting(&format!("llm_profile_key_{id}"), "");
    Ok(changed > 0)
}


#[cfg(test)]
mod tests {
    use super::*;

    fn real_store() -> Store {
        std::env::set_var(
            "LUMINA_DATA_DIR",
            r"E:\FIles\Lena‘s Files\Work\znw\Agent\data",
        );
        Store::open().expect("打不开数据库")
    }

    #[test]
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
        match chat_with_tools(&store, &messages, Some(0.2)) {
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
}
