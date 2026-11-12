//! 文献方法流程图（对齐旧版 `lit_flow.py` + `lit_flow_dialog.py`）。
//!
//! 旧版的思路：让模型把论文拆成几个步骤，再由程序自己画（QPainter 画 PNG），
//! 因为模型只会写字，直接把它的 ASCII 图贴出来又丑又对不齐。
//!
//! Rust 版把「画」交给前端（CSS + SVG），后端只负责三件事：
//! 1. `generate()`：取库里这篇文献的标题 + 摘要（或前端递进来的正文），问当前 AI 配置，
//!    要一份**结构化**流程图 `{title, nodes[], edges[]}`；
//! 2. `parse_flow()`：容错解析（带 ```json 围栏 / 前后有多余的话 / 直接坏掉都能兜住，
//!    最差降级成「纯文本步骤列表」，绝不 panic）；
//! 3. 按文献 id 存 `data/lit_flows/flow-<id>.json`，重启后 `load()` 还能看。
use std::path::PathBuf;

use serde_json::{json, Value};

use crate::store::Store;

/// 最多几个节点（旧版 `MAX_STEPS = 10`，节点图多留两个）
pub const MAX_NODES: usize = 12;
/// 正文/摘要截断长度（旧版也是截 1500 字）
const BODY_LIMIT: usize = 1500;
/// 容错扫描 JSON 时最多看多少字节，防止把一大段垃圾反复 parse
const SCAN_LIMIT: usize = 20000;

// ------------------------------------------------------------------ 路径

/// 流程图目录：data/lit_flows（与旧版同名同位置）
pub fn flow_dir() -> PathBuf {
    let folder = crate::paths::data_dir().join("lit_flows");
    let _ = std::fs::create_dir_all(&folder);
    folder
}

/// 某篇文献的流程图文件：flow-<id>.json
pub fn flow_path(paper_id: i64) -> PathBuf {
    flow_dir().join(format!("flow-{paper_id}.json"))
}

// ------------------------------------------------------------------ 提示词

/// 第一轮：要结构化 JSON（nodes + edges），并给出一段示例
pub fn prompt(title: &str, body: &str) -> String {
    format!(
        "请把这篇论文的**方法流程**拆成 4–8 个步骤，每个步骤一句话（不超过 18 个字），按执行顺序排列。\n\
         只输出 JSON，不要解释，不要 Markdown 代码围栏：\n\
         {{\"title\": \"流程标题（不超过 14 字）\",\n \
           \"nodes\": [{{\"id\": \"n1\", \"label\": \"步骤名（不超过 12 字）\", \"detail\": \"这一步在做什么（不超过 40 字）\"}}],\n \
           \"edges\": [{{\"from\": \"n1\", \"to\": \"n2\", \"label\": \"条件或说明（可省）\"}}]}}\n\
         edges 表示先后关系；线性流程就给相邻节点各连一条，别让节点孤立。\n\n\
         标题：{title}\n\n摘要或正文：{body}"
    )
}

/// 第二轮（重试）：第一次没解析出 JSON 时，把要求收紧、温度降到 0
pub fn strict_prompt(title: &str, body: &str) -> String {
    format!(
        "把下面论文的方法流程拆成 4–8 步，**只输出一行 JSON**，不要任何解释、不要代码围栏：\n\
         {{\"title\":\"流程标题\",\"nodes\":[{{\"id\":\"n1\",\"label\":\"步骤\",\"detail\":\"说明\"}}],\
         \"edges\":[{{\"from\":\"n1\",\"to\":\"n2\"}}]}}\n\n\
         标题：{title}\n摘要或正文：{body}"
    )
}

// ------------------------------------------------------------------ 解析

fn clip(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

/// 按给定键名取字符串（数字也当字符串用；空串继续试下一个键）
fn json_string(value: &Value, keys: &[&str]) -> String {
    for key in keys {
        let Some(found) = value.get(*key) else { continue };
        match found {
            Value::String(text) => {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
            Value::Number(number) => return number.to_string(),
            Value::Bool(flag) => return flag.to_string(),
            _ => {}
        }
    }
    String::new()
}

/// 代码围栏里的内容（```json … ``` / ``` … ```）
fn fenced_blocks(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cursor = 0usize;
    while cursor < raw.len() {
        let Some(found) = raw[cursor..].find("```") else { break };
        let fence_start = cursor + found;
        let after = fence_start + 3;
        // ``` 后面同一行可能是语言标记（json / JSON / text），整行跳过
        let body_start = match raw[after..].find('\n') {
            Some(offset) => after + offset + 1,
            None => raw.len(),
        };
        if body_start >= raw.len() {
            break;
        }
        let (body_end, next_cursor) = match raw[body_start..].find("```") {
            Some(offset) => (body_start + offset, body_start + offset + 3),
            None => (raw.len(), raw.len()),
        };
        out.push(raw[body_start..body_end].to_string());
        cursor = next_cursor;
    }
    out
}

fn parse_json_object(text: &str) -> Option<Value> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(value) if value.is_object() => Some(value),
        _ => None,
    }
}

/// 从「前后有多余文字」的文本里，从第一个 `{` 开始按 `}` 的位置由长到短试
fn scan_json_object(raw: &str) -> Option<Value> {
    let text = if raw.len() > SCAN_LIMIT {
        &raw[..SCAN_LIMIT]
    } else {
        raw
    };
    let start = text.find('{')?;
    let tail = &text[start..];
    let mut ends: Vec<usize> = tail.match_indices('}').map(|(index, _)| index + 1).collect();
    let mut tried = 0usize;
    while let Some(end) = ends.pop() {
        tried += 1;
        if tried > 2000 {
            break;
        }
        if let Ok(value) = serde_json::from_str::<Value>(&tail[..end]) {
            if value.is_object() {
                return Some(value);
            }
        }
    }
    None
}

/// 从模型输出里抠出一个 JSON 对象（围栏 → 整段 → 扫描）
fn extract_json(raw: &str) -> Option<Value> {
    for block in fenced_blocks(raw) {
        if let Some(value) = parse_json_object(&block) {
            return Some(value);
        }
        if let Some(value) = scan_json_object(&block) {
            return Some(value);
        }
    }
    if let Some(value) = parse_json_object(raw) {
        return Some(value);
    }
    scan_json_object(raw)
}

fn node_label(item: &Value) -> String {
    match item {
        Value::String(text) => text.trim().to_string(),
        Value::Object(_) => {
            json_string(item, &["label", "text", "name", "title", "step", "action", "node"])
        }
        _ => String::new(),
    }
}

fn node_detail(item: &Value) -> String {
    match item {
        Value::Object(_) => {
            json_string(item, &["detail", "desc", "description", "note", "info", "summary"])
        }
        _ => String::new(),
    }
}

fn node_id(item: &Value, index: usize) -> String {
    if let Value::Object(_) = item {
        let found = json_string(item, &["id", "key", "node", "index"]);
        if !found.is_empty() {
            return found;
        }
    }
    format!("n{}", index + 1)
}

/// 一串原始条目（字符串 / 对象）→ 规范化节点
fn nodes_of(data: &Value, key: &str) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let Some(rows) = data.get(key).and_then(|value| value.as_array()) else {
        return out;
    };
    for (index, item) in rows.iter().enumerate() {
        let label = node_label(item);
        if label.is_empty() {
            continue;
        }
        let mut id = node_id(item, index);
        if seen.contains(&id) {
            id = format!("{id}-{}", index + 1);
        }
        seen.push(id.clone());
        out.push(json!({
            "id": id,
            "label": clip(&label, 40),
            "detail": clip(&node_detail(item), 120),
        }));
        if out.len() >= MAX_NODES {
            break;
        }
    }
    out
}

/// 相邻节点各连一条（模型没给 edges 时兜底，保证图能画出来）
fn linear_edges(nodes: &[Value]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for pair in nodes.windows(2) {
        let from = pair[0]["id"].as_str().unwrap_or("");
        let to = pair[1]["id"].as_str().unwrap_or("");
        if from.is_empty() || to.is_empty() {
            continue;
        }
        out.push(json!({ "from": from, "to": to, "label": "" }));
    }
    out
}

fn edges_of(data: &Value, nodes: &[Value]) -> Vec<Value> {
    let ids: Vec<String> = nodes
        .iter()
        .filter_map(|node| node["id"].as_str().map(|text| text.to_string()))
        .collect();
    let mut out: Vec<Value> = Vec::new();
    let Some(rows) = data.get("edges").and_then(|value| value.as_array()) else {
        return out;
    };
    for item in rows {
        let (from, to, label) = match item {
            // 也允许 ["n1", "n2"] 这种简写
            Value::Array(pair) => (
                pair.first().and_then(|one| one.as_str()).unwrap_or("").to_string(),
                pair.get(1).and_then(|one| one.as_str()).unwrap_or("").to_string(),
                String::new(),
            ),
            Value::Object(_) => (
                json_string(item, &["from", "source", "src", "start"]),
                json_string(item, &["to", "target", "dst", "end", "next"]),
                json_string(item, &["label", "condition", "note", "text"]),
            ),
            _ => continue,
        };
        if from.is_empty() || to.is_empty() {
            continue;
        }
        // 指向不存在节点的边直接丢掉（模型经常把 id 写错）
        if !ids.contains(&from) || !ids.contains(&to) {
            continue;
        }
        out.push(json!({ "from": from, "to": to, "label": clip(&label, 24) }));
    }
    out
}

/// JSON 对象 → (标题, 节点, 边)；一个可用节点都没有就返回 None
fn flow_from_value(data: &Value) -> Option<(String, Vec<Value>, Vec<Value>)> {
    let mut nodes = nodes_of(data, "nodes");
    if nodes.is_empty() {
        // 旧版 Python 的格式是 {"title","steps":[...]}
        nodes = nodes_of(data, "steps");
    }
    if nodes.is_empty() {
        nodes = nodes_of(data, "flow");
    }
    if nodes.is_empty() {
        return None;
    }
    let mut edges = edges_of(data, &nodes);
    if edges.is_empty() {
        edges = linear_edges(&nodes);
    }
    let raw_title = json_string(data, &["title", "name"]);
    let title = if raw_title.is_empty() {
        "方法流程".to_string()
    } else {
        clip(&raw_title, 24)
    };
    Some((title, nodes, edges))
}

/// 去掉「1.」「2、」「(3)」这类前缀
fn strip_leading_number(text: &str) -> String {
    let cut = text.trim_start_matches(|c: char| {
        c.is_whitespace()
            || c.is_ascii_digit()
            || matches!(
                c,
                '.' | '、' | ')' | '）' | '．' | ',' | '，' | ']' | '】' | ':' | '：' | '-' | '*'
                    | '#' | '|' | '·' | '(' | '（' | '「' | '」' | '“' | '”' | '"'
            )
    });
    let result = cut.trim();
    if result.is_empty() {
        text.trim().to_string()
    } else {
        result.to_string()
    }
}

/// 降级用：按行取步骤（跳过 JSON 残片 / 围栏 / 太短的行）
fn text_steps(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let first = trimmed.chars().next().unwrap_or(' ');
        if matches!(first, '{' | '}' | '[' | ']' | '`' | '"' | '<') {
            continue;
        }
        let cleaned = strip_leading_number(trimmed);
        if cleaned.chars().count() < 2 {
            continue;
        }
        out.push(clip(&cleaned, 60));
        if out.len() >= MAX_NODES {
            break;
        }
    }
    out
}

/// 把模型的原始输出解析成流程图。
///
/// 返回 `{ok, title, nodes, edges, fallback, error, raw?}`：
/// * `fallback = false`：真的解析出了 JSON；
/// * `fallback = true`：JSON 坏了/没有，已降级成「纯文本步骤列表」（nodes 里就是那些行，
///   前端照常见列表显示即可，不会 panic）；
/// * `ok = false`：连一行像样的步骤都没有（或输入为空），`error` 说明原因。
pub fn parse_flow(raw: &str) -> Value {
    let text = raw.trim();
    if text.is_empty() {
        return json!({
            "ok": false, "title": "", "nodes": [], "edges": [], "fallback": false,
            "error": "模型什么也没返回，再试一次",
        });
    }
    if let Some(data) = extract_json(text) {
        if let Some((title, nodes, edges)) = flow_from_value(&data) {
            return json!({
                "ok": true, "title": title, "nodes": nodes, "edges": edges,
                "fallback": false, "error": "",
            });
        }
    }
    let steps = text_steps(text);
    if steps.is_empty() {
        return json!({
            "ok": false, "title": "", "nodes": [], "edges": [], "fallback": true,
            "error": "模型没给出能用的步骤，再试一次", "raw": clip(text, 600),
        });
    }
    let nodes: Vec<Value> = steps
        .iter()
        .enumerate()
        .map(|(index, step)| {
            json!({ "id": format!("n{}", index + 1), "label": step, "detail": "" })
        })
        .collect();
    let edges = linear_edges(&nodes);
    json!({
        "ok": true, "title": "方法流程", "nodes": nodes, "edges": edges,
        "fallback": true, "error": "", "raw": clip(text, 600),
    })
}

// ------------------------------------------------------------------ 存取

/// 读回某篇文献上次存的流程图
pub fn read_saved(paper_id: i64) -> Option<Value> {
    let text = std::fs::read_to_string(flow_path(paper_id)).ok()?;
    serde_json::from_str::<Value>(&text).ok()
}

pub fn load(paper_id: i64) -> Value {
    if paper_id <= 0 {
        return json!({
            "ok": false, "title": "", "nodes": [], "edges": [], "fallback": false,
            "error": "没指定文献",
        });
    }
    match read_saved(paper_id) {
        Some(saved) if has_nodes(&saved) => json!({
            "ok": true,
            "cached": true,
            "paper_id": paper_id,
            "title": saved.get("title").cloned().unwrap_or_else(|| json!("方法流程图")),
            "nodes": saved.get("nodes").cloned().unwrap_or_else(|| json!([])),
            "edges": saved.get("edges").cloned().unwrap_or_else(|| json!([])),
            "fallback": saved.get("fallback").cloned().unwrap_or_else(|| json!(false)),
            "generated_at": saved.get("generated_at").cloned().unwrap_or_else(|| json!("")),
            "path": flow_path(paper_id).display().to_string(),
            "error": "",
        }),
        _ => json!({
            "ok": false, "title": "", "nodes": [], "edges": [], "fallback": false,
            "error": "这篇还没有生成过流程图",
        }),
    }
}

pub fn delete(paper_id: i64) -> Value {
    if paper_id <= 0 {
        return json!({ "ok": false, "deleted": false, "error": "没指定文献" });
    }
    let path = flow_path(paper_id);
    if !path.exists() {
        return json!({ "ok": true, "deleted": false, "error": "" });
    }
    match std::fs::remove_file(&path) {
        Ok(()) => json!({ "ok": true, "deleted": true, "error": "" }),
        Err(err) => json!({ "ok": false, "deleted": false, "error": format!("删不掉：{err}") }),
    }
}

fn has_nodes(value: &Value) -> bool {
    value
        .get("nodes")
        .and_then(|rows| rows.as_array())
        .map(|rows| !rows.is_empty())
        .unwrap_or(false)
}

fn node_count(value: &Value) -> usize {
    value
        .get("nodes")
        .and_then(|rows| rows.as_array())
        .map(|rows| rows.len())
        .unwrap_or(0)
}

fn write_saved(paper_id: i64, payload: &Value) -> Result<PathBuf, String> {
    let path = flow_path(paper_id);
    let text = serde_json::to_string_pretty(payload).map_err(|err| err.to_string())?;
    std::fs::write(&path, text).map_err(|err| format!("存流程图失败：{err}"))?;
    Ok(path)
}

/// 库里这篇文献能拿来拆流程的文字：摘要 → 中文概述 → 备注（都没有就空串）
fn paper_text(store: &Store, paper_id: i64) -> Result<(String, String), String> {
    let row = {
        let conn = store.connection();
        conn.query_row(
            "SELECT title, COALESCE(abstract, ''), COALESCE(summary_zh, ''), COALESCE(note, '')
             FROM papers WHERE id = ?1",
            [paper_id],
            |row| {
                Ok((
                    row.get::<_, String>(0).unwrap_or_default(),
                    row.get::<_, String>(1).unwrap_or_default(),
                    row.get::<_, String>(2).unwrap_or_default(),
                    row.get::<_, String>(3).unwrap_or_default(),
                ))
            },
        )
        .ok()
    };
    let Some((title, abstract_text, summary, note)) = row else {
        return Err(format!("找不到这篇文献（id={paper_id}）"));
    };
    let body = [abstract_text, summary, note]
        .into_iter()
        .find(|text| !text.trim().is_empty())
        .unwrap_or_default();
    Ok((title, body))
}

fn empty_result(error: &str) -> Value {
    json!({
        "ok": false, "title": "", "nodes": [], "edges": [], "fallback": false,
        "error": error, "cached": false, "saved": false, "path": "",
    })
}

// ------------------------------------------------------------------ 生成

/// 生成流程图（旧版 `lit_flow.generate()`）。
///
/// * `paper_id > 0`：先看有没有存过（没 `force` 就直接返回缓存，和旧版一致）；
///   没有的话从库里取标题 + 摘要；
/// * `text` 非空：直接用这段文字（前端已经把 PDF 正文解析出来了就传这个）；
/// * 模型没配好 / 没 Key：返回 `need_key = true` + 明确的中文提示，不白跑一次请求。
pub fn generate(store: &Store, paper_id: i64, text: &str, title: &str, force: bool) -> Value {
    let mut body = text.trim().to_string();
    let mut head = title.trim().to_string();
    if paper_id > 0 {
        if !force {
            if let Some(saved) = read_saved(paper_id) {
                if has_nodes(&saved) {
                    let mut value = saved;
                    value["ok"] = json!(true);
                    value["cached"] = json!(true);
                    value["paper_id"] = json!(paper_id);
                    value["path"] = json!(flow_path(paper_id).display().to_string());
                    value["saved"] = json!(true);
                    value["error"] = json!("");
                    return value;
                }
            }
        }
        if body.is_empty() {
            match paper_text(store, paper_id) {
                Ok((paper_title, paper_body)) => {
                    if head.is_empty() {
                        head = paper_title;
                    }
                    body = paper_body;
                }
                Err(err) => return empty_result(&err),
            }
        }
    }
    if body.trim().is_empty() {
        return empty_result(
            "没有可拆流程的内容：这篇文献没有摘要（也没给正文），先在文献库里补个摘要，或者把正文粘进来",
        );
    }
    body = clip(&body, BODY_LIMIT);

    // 没有可用配置 / 没填 Key：直接说清楚，别让按钮点了没反应
    let state = crate::ai::config_state(store);
    if state.get("has_key").and_then(|one| one.as_bool()) != Some(true) {
        let hint = state
            .get("error")
            .and_then(|one| one.as_str())
            .unwrap_or("")
            .to_string();
        let message = if hint.is_empty() {
            "还没有可用的模型：去 设置 → AI（API 服务）里填好接口地址和 API Key".to_string()
        } else {
            hint
        };
        let mut value = empty_result(&message);
        value["need_key"] = json!(true);
        return value;
    }

    let messages = vec![json!({ "role": "user", "content": prompt(&head, &body) })];
    let reply = match crate::ai::chat(store, &messages, Some(0.2)) {
        Ok(text) => text,
        Err(err) => return empty_result(&format!("调用模型失败：{err}")),
    };
    let mut parsed = parse_flow(&reply);

    // 第一次没拿到 JSON（降级成纯文本了）：温度降到 0 再要一次，仍然不行就用降级结果
    let first_fallback = parsed.get("fallback").and_then(|one| one.as_bool()).unwrap_or(false);
    let first_ok = parsed.get("ok").and_then(|one| one.as_bool()).unwrap_or(false);
    if first_fallback || !first_ok {
        let retry = vec![json!({ "role": "user", "content": strict_prompt(&head, &body) })];
        if let Ok(again) = crate::ai::chat(store, &retry, Some(0.0)) {
            let second = parse_flow(&again);
            let second_ok = second.get("ok").and_then(|one| one.as_bool()).unwrap_or(false);
            let second_fallback = second
                .get("fallback")
                .and_then(|one| one.as_bool())
                .unwrap_or(false);
            // 重试拿到真 JSON 就用它；否则只要节点更多也留着
            if second_ok && (!second_fallback || node_count(&second) > node_count(&parsed)) {
                parsed = second;
            }
        }
    }

    if parsed.get("ok").and_then(|one| one.as_bool()) != Some(true) {
        return parsed;
    }

    let fallback = parsed.get("fallback").cloned().unwrap_or_else(|| json!(false));
    let mut result = json!({
        "ok": true,
        "paper_id": paper_id,
        "title": parsed.get("title").cloned().unwrap_or_else(|| json!("方法流程")),
        "nodes": parsed.get("nodes").cloned().unwrap_or_else(|| json!([])),
        "edges": parsed.get("edges").cloned().unwrap_or_else(|| json!([])),
        "fallback": fallback,
        "generated_at": crate::now_str(),
        "cached": false,
        "saved": false,
        "path": "",
        "error": "",
    });
    if parsed.get("raw").is_some() {
        result["raw"] = parsed.get("raw").cloned().unwrap_or_else(|| json!(""));
    }
    if paper_id > 0 {
        match write_saved(paper_id, &result) {
            Ok(path) => {
                result["saved"] = json!(true);
                result["path"] = json!(path.display().to_string());
            }
            Err(err) => {
                result["save_error"] = json!(err);
            }
        }
    }
    result
}

// ------------------------------------------------------------------ 测试

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(name: &str) -> Store {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("LUMINA_DATA_DIR", &dir);
        Store::open().expect("建临时库失败")
    }

    #[test]
    fn parses_plain_json_flow() {
        let raw = r#"{"title":"实验流程","nodes":[{"id":"a","label":"采样","detail":"采集 30 份样本"},{"id":"b","label":"建模"}],"edges":[{"from":"a","to":"b","label":"清洗后"}]}"#;
        let parsed = parse_flow(raw);
        println!("正常 JSON：{parsed}");
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["fallback"], false);
        assert_eq!(parsed["title"], "实验流程");
        let nodes = parsed["nodes"].as_array().cloned().unwrap_or_default();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0]["label"], "采样");
        assert_eq!(nodes[0]["detail"], "采集 30 份样本");
        let edges = parsed["edges"].as_array().cloned().unwrap_or_default();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0]["from"], "a");
        assert_eq!(edges[0]["label"], "清洗后");
    }

    #[test]
    fn parses_fenced_and_noisy_json() {
        // 1. ```json 代码围栏 + 前后多余的话
        let fenced = "好的，这是流程图：\n```json\n{\"title\":\"方法\",\"nodes\":\
                      [{\"id\":\"n1\",\"label\":\"读数据\"},{\"id\":\"n2\",\"label\":\"训练\"}],\
                      \"edges\":[]}\n```\n希望对你有帮助。";
        let parsed = parse_flow(fenced);
        println!("围栏 JSON：{parsed}");
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["fallback"], false);
        assert_eq!(parsed["title"], "方法");
        assert_eq!(parsed["nodes"].as_array().cloned().unwrap_or_default().len(), 2);
        // 没给 edges：自动补相邻连线，否则前端画不出箭头
        assert_eq!(parsed["edges"].as_array().cloned().unwrap_or_default().len(), 1);

        // 2. 没有围栏、前后都是中文
        let noisy = "分析如下：{\"nodes\":[\"第一步 预处理\",\"第二步 训练\"],\"edges\":[]} 以上。";
        let parsed2 = parse_flow(noisy);
        println!("无围栏 JSON：{parsed2}");
        assert_eq!(parsed2["ok"], true);
        assert_eq!(parsed2["fallback"], false);
        assert_eq!(parsed2["nodes"].as_array().cloned().unwrap_or_default().len(), 2);
        assert_eq!(parsed2["edges"].as_array().cloned().unwrap_or_default().len(), 1);

        // 3. 旧版 Python 的 steps 写法也能吃
        let legacy = "```\n{\"title\":\"旧格式\",\"steps\":[{\"label\":\"甲\"},{\"label\":\"乙\"},{\"label\":\"丙\"}]}\n```";
        let parsed3 = parse_flow(legacy);
        assert_eq!(parsed3["ok"], true);
        assert_eq!(parsed3["title"], "旧格式");
        assert_eq!(parsed3["nodes"].as_array().cloned().unwrap_or_default().len(), 3);
        assert_eq!(parsed3["edges"].as_array().cloned().unwrap_or_default().len(), 2);
    }

    #[test]
    fn broken_json_falls_back_to_text_steps() {
        let raw = "{\"nodes\": [oops 这不是 JSON\n1. 先读论文\n2. 再复现实验\n3. 最后写结论";
        let parsed = parse_flow(raw);
        println!("坏 JSON 降级：{parsed}");
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["fallback"], true);
        let nodes = parsed["nodes"].as_array().cloned().unwrap_or_default();
        assert!(nodes.len() >= 3, "至少三行步骤，实际 {} 个", nodes.len());
        assert_eq!(nodes[0]["label"], "先读论文");
        assert_eq!(parsed["edges"].as_array().cloned().unwrap_or_default().len(), nodes.len() - 1);
        // 完全没法用的输出：ok=false，但仍然不 panic
        let nothing = parse_flow("{}");
        println!("没有节点：{nothing}");
        assert_eq!(nothing["ok"], false);
        assert!(!nothing["error"].as_str().unwrap_or("").is_empty());
    }

    #[test]
    fn empty_input_has_clear_error() {
        let parsed = parse_flow("   \n\n ");
        println!("空输出：{parsed}");
        assert_eq!(parsed["ok"], false);
        assert!(!parsed["error"].as_str().unwrap_or("").is_empty());

        let store = temp_store("danlume-litflow-empty");
        let result = generate(&store, 0, "", "", false);
        println!("空输入：{result}");
        assert_eq!(result["ok"], false);
        assert!(result["error"].as_str().unwrap_or("").contains("没有可拆流程"));

        let missing = generate(&store, 999999, "", "", false);
        println!("找不到文献：{missing}");
        assert_eq!(missing["ok"], false);
        assert!(missing["error"].as_str().unwrap_or("").contains("找不到这篇文献"));
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-litflow-empty"));
    }

    #[test]
    fn missing_api_key_is_reported() {
        let store = temp_store("danlume-litflow-nokey");
        let result = generate(&store, 0, "这篇论文提出了一种新的对齐方法，先采样，再训练。", "测试文献", false);
        println!("没 Key：{result}");
        assert_eq!(result["ok"], false);
        assert_eq!(result["need_key"], true);
        assert!(
            result["error"].as_str().unwrap_or("").contains("设置"),
            "错误要说清去哪儿配：{}",
            result["error"]
        );
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-litflow-nokey"));
    }

    #[test]
    fn saved_flow_survives_restart() {
        let _store = temp_store("danlume-litflow-save");
        let payload = json!({
            "paper_id": 7,
            "title": "方法流程",
            "nodes": [
                { "id": "n1", "label": "读数据", "detail": "" },
                { "id": "n2", "label": "写结论", "detail": "" },
            ],
            "edges": [{ "from": "n1", "to": "n2", "label": "" }],
            "fallback": false,
            "generated_at": "2026-01-01 00:00:00",
        });
        let path = flow_path(7);
        std::fs::write(&path, payload.to_string()).unwrap();
        println!("存到：{}", path.display());
        // 相当于重开一次程序：只凭文献 id 就能读回来
        let loaded = load(7);
        println!("读回：{loaded}");
        assert_eq!(loaded["ok"], true);
        assert_eq!(loaded["title"], "方法流程");
        assert_eq!(loaded["nodes"].as_array().cloned().unwrap_or_default().len(), 2);
        // 没有生成过的文献：明确说没有，而不是返回空图
        assert_eq!(load(8)["ok"], false);
        let deleted = delete(7);
        println!("删除：{deleted}");
        assert_eq!(deleted["ok"], true);
        assert!(!path.exists(), "删完文件应该不在了");
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-litflow-save"));
    }
}
