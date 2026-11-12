//! 提示词库 + 提示词生成器（对应旧版 `lumina/prompts.py` 与 `lumina/prompt_gen.py`）。
//!
//! 旧版把提示词写在 `<data>/prompts.json`（结构 `{version, note, system, scenes}`）；
//! 这一版**放进 settings 表**，键名 `prompts_json`，值就是那份 JSON 的全文 ——
//! 里面的字段名（`system` / `scenes` / `assistant` / `weekly_report` …）与旧版逐字一致，
//! 所以想互导的话：把旧版 `prompts.json` 的内容整份贴进这一行即可。
//!
//! 与旧版的关系：
//! * 内置提示词的默认值（含中文字符串）照抄 `prompts.DEFAULTS`；
//! * 旧版的「生成器」是跑外部程序（prompt_gen.py，stdin 喂 JSON、stdout 收文本）；
//!   这一版改成**让模型自己写一份提示词**（复用 `ai.rs` 的调用能力），
//!   输入一段需求描述 → 输出结构化提示词 → 预览 → 保存成模板；
//! * 系统的几条（assistant / weekly_report / …）只能改不能删；场景模板可增删。
use serde_json::{json, Map, Value};

use crate::store::Store;

/// settings 表里存整份提示词文档的键
pub const SETTING_KEY: &str = "prompts_json";
/// 文档版本（与旧版 `prompts.VERSION = 1` 一致）
pub const VERSION: i64 = 1;
/// 助手人格（与旧版 `prompts.PERSONA` 一致）
pub const PERSONA: &str = "你是 Lume（卢梅），DanLume 里的写作与学习助手。";
/// 文档里的说明（与旧版 `DEFAULTS["note"]` 一致）
pub const DEFAULT_NOTE: &str = "这里是 DanLume 的提示词配置，可以自己改；改完重启软件生效。";
/// 单条提示词的长度上限（旧版 prompt_gen 的 `MAX_CHARS = 8000`）
pub const MAX_TEXT_CHARS: usize = 8000;
/// 模板标识的长度上限
pub const MAX_KEY_CHARS: usize = 48;

/// 一条内置提示词
pub struct Builtin {
    pub key: &'static str,
    /// 中文显示名（旧版 ai_prompt_panel 的 labels / scenes.title）
    pub title: &'static str,
    pub text: &'static str,
}

/// 内置的系统提示词（顺序与旧版 `prompts.DEFAULTS["system"]` 一致）
pub const BUILTIN_SYSTEM: &[Builtin] = &[
    Builtin {
        key: "assistant",
        title: "通用助手",
        text: "你是 Lume（卢梅），DanLume 里的写作与学习助手。回答用简体中文，简洁直接，\
               先给结论再给要点；不确定的地方要说明不确定，不要编造。",
    },
    Builtin {
        key: "weekly_report",
        title: "周报",
        text: "把下面这一周的日程与完成情况整理成周报：分「本周完成」「遇到的问题」\
               「下周计划」三段，每段用短句，不要客套话，总字数控制在 400 字以内。",
    },
    Builtin {
        key: "homework",
        title: "作业 / 课件摘要",
        text: "阅读这份课件/作业，输出：1) 三句话摘要；2) 需要动手完成的任务清单；\
               3) 截止时间（如果提到了）；4) 需要用到的知识点。",
    },
    Builtin {
        key: "latex",
        title: "公式 / LaTeX",
        text: "把这段自然语言里的数学内容转成 LaTeX 公式，只用数学模式内容，\
               不要加解释文字；多个公式用换行分隔。",
    },
    Builtin {
        key: "wechat_todo",
        title: "微信待办识别",
        text: "你是日程助手。从用户给的聊天记录里找出「用户自己需要去做、且有明确时间」的事项，\
               忽略纯通知、广告、别人的安排、没有时间的闲聊。\
               只输出 JSON 数组，不要任何解释文字，格式：\n\
               [{\"title\":\"简短标题\",\"date\":\"YYYY-MM-DD\",\"start_time\":\"HH:MM\",\
               \"end_time\":\"HH:MM或空\",\"location\":\"地点或空\",\"note\":\"原话摘录\"}]\n\
               今天的日期是 {today}。没有合适事项就输出 []。",
    },
];

/// 内置的场景模板（顺序与旧版 `prompts.DEFAULTS["scenes"]` 一致）
pub const BUILTIN_SCENES: &[Builtin] = &[
    Builtin {
        key: "weekly_report",
        title: "生成周报",
        text: "根据我的日程数据写一份周报。（场景模板，可自行修改）",
    },
    Builtin {
        key: "homework_summary",
        title: "作业摘要",
        text: "总结这份作业要求，列出要做的事和截止时间。",
    },
];

/// 生成器用的「元提示词」：让模型产出一份结构化提示词
const GENERATOR_PROMPT: &str = "你是提示词工程师。把用户给的「需求描述」写成一份可以直接\
给 AI 用的结构化提示词。要求：\n\
1) 只输出提示词正文，不要解释、不要 Markdown 代码块围栏；\n\
2) 用简体中文写，按小节组织：角色 / 目标 / 输入 / 输出格式 / 约束（没有内容的节可以省略）；\n\
3) 具体、可执行，不要空话；需要用户填的地方用 {花括号} 标出；\n\
4) 全文控制在 600 字以内。";

/// 默认文档（每次调用给一份新的，等价旧版 `json.loads(json.dumps(DEFAULTS))`）
pub fn defaults() -> Value {
    let system: Map<String, Value> = BUILTIN_SYSTEM
        .iter()
        .map(|item| (item.key.to_string(), Value::String(item.text.to_string())))
        .collect();
    let scenes: Map<String, Value> = BUILTIN_SCENES
        .iter()
        .map(|item| {
            (
                item.key.to_string(),
                json!({ "title": item.title, "prompt": item.text, "enabled": true }),
            )
        })
        .collect();
    json!({
        "version": VERSION,
        "note": DEFAULT_NOTE,
        "system": Value::Object(system),
        "scenes": Value::Object(scenes),
        // 旧版没有这个键：删掉的场景记在这儿，否则每次 load 合并默认值又会长回来
        "deleted": Vec::<String>::new(),
    })
}

/// 深合并（同旧版 `_deep_merge`：只递归对象，其它直接覆盖）
fn deep_merge(base: &Value, extra: &Value) -> Value {
    match (base, extra) {
        (Value::Object(left), Value::Object(right)) => {
            let mut out = left.clone();
            for (key, value) in right {
                let merged = match out.get(key) {
                    Some(old) => deep_merge(old, value),
                    None => value.clone(),
                };
                out.insert(key.clone(), merged);
            }
            Value::Object(out)
        }
        _ => extra.clone(),
    }
}

/// 读文档：settings 里没有 / 读坏了就返回默认值；有就深合并默认值（缺的键补齐）
pub fn load(store: &Store) -> Value {
    let raw = store.setting(SETTING_KEY).unwrap_or_default();
    let stored = if raw.trim().is_empty() {
        None
    } else {
        serde_json::from_str::<Value>(&raw)
            .ok()
            .filter(|value| value.is_object())
    };
    let mut doc = match stored {
        Some(value) => deep_merge(&defaults(), &value),
        None => defaults(),
    };
    // 删掉的场景不要再长回来
    let deleted: Vec<String> = doc
        .get("deleted")
        .and_then(|value| value.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|one| one.as_str().map(|text| text.to_string()))
                .collect()
        })
        .unwrap_or_default();
    if !deleted.is_empty() {
        if let Some(scenes) = doc.get_mut("scenes").and_then(|value| value.as_object_mut()) {
            for key in &deleted {
                scenes.remove(key);
            }
        }
    }
    doc
}

fn write(store: &Store, doc: &Value) -> Result<(), String> {
    store
        .set_setting(SETTING_KEY, &doc.to_string())
        .map_err(|err| format!("提示词写不进设置：{err}"))
}

/// 整份文档写回（会先把默认值补齐、打上版本与时间）
pub fn save(store: &Store, patch: &Value) -> Result<Value, String> {
    if !patch.is_object() {
        return Err("提示词配置必须是对象".to_string());
    }
    let mut doc = deep_merge(&load(store), patch);
    if let Some(object) = doc.as_object_mut() {
        object.insert("version".to_string(), json!(VERSION));
        object.insert(
            "updated_at".to_string(),
            json!(chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()),
        );
    }
    write(store, &doc)?;
    Ok(doc)
}

fn builtin_text(kind: &str, key: &str) -> Option<&'static str> {
    match kind {
        "system" => BUILTIN_SYSTEM
            .iter()
            .find(|item| item.key == key)
            .map(|item| item.text),
        "scene" => BUILTIN_SCENES
            .iter()
            .find(|item| item.key == key)
            .map(|item| item.text),
        _ => None,
    }
}

/// 库里**用户真的写过**的那一条（没写过就 None，不会因为合并默认值而有值）
fn stored_system(store: &Store, name: &str) -> Option<String> {
    let raw = store.setting(SETTING_KEY).unwrap_or_default();
    let doc: Value = serde_json::from_str(&raw).ok()?;
    let text = doc
        .pointer(&format!("/system/{name}"))?
        .as_str()?
        .trim()
        .to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// 取一条系统提示词。
///
/// 取法顺序（对齐旧版 `prompts.system()`，外加一个 v2 的兼容）：
/// 1. 提示词库里用户写过的那条；
/// 2. 设置页旧卡「系统提示词」（键 `system_prompt`，v2 早期只有这一张卡）；
/// 3. 内置默认（`prompts.DEFAULTS`）。
pub fn system(store: &Store, name: &str) -> String {
    if let Some(text) = stored_system(store, name) {
        return text;
    }
    if name == "assistant" {
        let legacy = store
            .setting("system_prompt")
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty());
        if let Some(text) = legacy {
            return text;
        }
    }
    builtin_text("system", name).unwrap_or_default().to_string()
}

/// 取一条场景模板（只取还活着的）
pub fn scene(store: &Store, name: &str) -> Option<String> {
    let doc = load(store);
    let item = doc.pointer(&format!("/scenes/{name}"))?;
    if item.get("enabled").and_then(|one| one.as_bool()) == Some(false) {
        return None;
    }
    item.get("prompt")
        .and_then(|value| value.as_str())
        .map(|text| text.to_string())
}

fn builtin_title(kind: &str, key: &str) -> Option<&'static str> {
    match kind {
        "system" => BUILTIN_SYSTEM
            .iter()
            .find(|item| item.key == key)
            .map(|item| item.title),
        "scene" => BUILTIN_SCENES
            .iter()
            .find(|item| item.key == key)
            .map(|item| item.title),
        _ => None,
    }
}

/// 全部模板（系统提示词 + 场景模板），给「提示词」卡 / 抽屉用
pub fn templates(store: &Store) -> Vec<Value> {
    let doc = load(store);
    let mut out: Vec<Value> = Vec::new();
    if let Some(system) = doc.get("system").and_then(|value| value.as_object()) {
        // 先按内置顺序，再补上用户自己加的（正常情况下不会有）
        let mut keys: Vec<String> = BUILTIN_SYSTEM
            .iter()
            .map(|item| item.key.to_string())
            .filter(|key| system.contains_key(key))
            .collect();
        let mut extra: Vec<String> = system
            .keys()
            .filter(|key| builtin_title("system", key).is_none())
            .cloned()
            .collect();
        extra.sort();
        keys.extend(extra);
        for key in keys {
            let text = system
                .get(&key)
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string();
            let title = builtin_title("system", &key)
                .map(|one| one.to_string())
                .unwrap_or_else(|| key.clone());
            out.push(json!({
                "key": key,
                "kind": "system",
                "title": title,
                "text": text,
                "enabled": true,
                "builtin": builtin_title("system", &key).is_some(),
            }));
        }
    }
    if let Some(scenes) = doc.get("scenes").and_then(|value| value.as_object()) {
        let mut keys: Vec<String> = BUILTIN_SCENES
            .iter()
            .map(|item| item.key.to_string())
            .filter(|key| scenes.contains_key(key))
            .collect();
        let mut extra: Vec<String> = scenes
            .keys()
            .filter(|key| builtin_title("scene", key).is_none())
            .cloned()
            .collect();
        extra.sort();
        keys.extend(extra);
        for key in keys {
            let item = scenes.get(&key).cloned().unwrap_or_else(|| json!({}));
            let title = item
                .get("title")
                .and_then(|value| value.as_str())
                .filter(|text| !text.trim().is_empty())
                .map(|text| text.to_string())
                .unwrap_or_else(|| key.clone());
            let text = item
                .get("prompt")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string();
            out.push(json!({
                "key": key,
                "kind": "scene",
                "title": title,
                "text": text,
                "enabled": item.get("enabled").and_then(|one| one.as_bool()).unwrap_or(true),
                "builtin": builtin_title("scene", &key).is_some(),
            }));
        }
    }
    out
}

/// 校验模板标识：中文也行（旧版场景名就是用户随手起的名字），但不能带空格 / 冒号 / 斜杠
pub fn validate_key(raw: &str) -> Result<String, String> {
    let key = raw.trim();
    if key.is_empty() {
        return Err("模板标识不能为空".to_string());
    }
    if key.chars().count() > MAX_KEY_CHARS {
        return Err(format!("模板标识最多 {MAX_KEY_CHARS} 个字"));
    }
    if key.chars().any(|ch| {
        ch.is_whitespace() || matches!(ch, ':' | '/' | '\\' | '|' | '?' | '*' | '"' | '<' | '>')
    }) {
        return Err("模板标识不能有空格，也不能有 : / \\ | ? * \" < > 这些符号".to_string());
    }
    Ok(key.to_string())
}

/// 校验模板正文
pub fn validate_text(raw: &str) -> Result<String, String> {
    let text = raw.trim();
    if text.is_empty() {
        return Err("提示词内容不能为空".to_string());
    }
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(format!("一条提示词最多 {MAX_TEXT_CHARS} 字，这条太长了"));
    }
    Ok(text.to_string())
}

/// 新增 / 修改一条模板。
///
/// * `kind = "system"`：只能改内置那几条（旧版 UI 也是这样，系统提示词不给加）；
/// * `kind = "scene"`：可新增（key 已存在就是覆盖）。
pub fn template_save(
    store: &Store,
    kind: &str,
    key: &str,
    title: &str,
    text: &str,
) -> Result<Value, String> {
    let kind = kind.trim();
    if kind != "system" && kind != "scene" {
        return Err("模板类型只能是 system 或 scene".to_string());
    }
    let key = validate_key(key)?;
    let text = validate_text(text)?;
    if kind == "system" && builtin_text("system", &key).is_none() {
        return Err(format!("系统提示词只有内置那几条，「{key}」不是其中之一"));
    }
    let mut doc = load(store);
    if kind == "system" {
        if let Some(system) = doc.get_mut("system").and_then(|value| value.as_object_mut()) {
            system.insert(key.clone(), Value::String(text));
        }
    } else {
        let title = if title.trim().is_empty() {
            builtin_title("scene", &key)
                .map(|one| one.to_string())
                .unwrap_or_else(|| key.clone())
        } else {
            title.trim().to_string()
        };
        if let Some(scenes) = doc.get_mut("scenes").and_then(|value| value.as_object_mut()) {
            scenes.insert(
                key.clone(),
                json!({ "title": title, "prompt": text, "enabled": true }),
            );
        }
        // 之前删过同名的，现在又保存了：从「已删除」里摘掉
        if let Some(deleted) = doc.get_mut("deleted").and_then(|value| value.as_array_mut()) {
            deleted.retain(|one| one.as_str() != Some(key.as_str()));
        }
    }
    doc = save(store, &doc)?;
    Ok(json!({
        "ok": true,
        "kind": kind,
        "key": key,
        "total": count_templates(&doc),
    }))
}

/// 删除一条场景模板（系统提示词不能删，跟旧版一样）
pub fn template_delete(store: &Store, kind: &str, key: &str) -> Result<Value, String> {
    let kind = kind.trim();
    let key = validate_key(key)?;
    if kind == "system" {
        return Err("内置系统提示词不能删除（可以改内容，或用「恢复默认」）".to_string());
    }
    if kind != "scene" {
        return Err("模板类型只能是 system 或 scene".to_string());
    }
    let mut doc = load(store);
    let existed = doc
        .get("scenes")
        .and_then(|value| value.as_object())
        .map(|scenes| scenes.contains_key(&key))
        .unwrap_or(false);
    if !existed {
        return Err(format!("没有这条场景模板：「{key}」"));
    }
    if let Some(scenes) = doc.get_mut("scenes").and_then(|value| value.as_object_mut()) {
        scenes.remove(&key);
    }
    if let Some(deleted) = doc.get_mut("deleted").and_then(|value| value.as_array_mut()) {
        if !deleted.iter().any(|one| one.as_str() == Some(key.as_str())) {
            deleted.push(Value::String(key.clone()));
        }
    }
    save(store, &doc)?;
    Ok(json!({ "ok": true, "key": key }))
}

fn count_templates(doc: &Value) -> usize {
    let systems = doc
        .get("system")
        .and_then(|value| value.as_object())
        .map(|map| map.len())
        .unwrap_or(0);
    let scenes = doc
        .get("scenes")
        .and_then(|value| value.as_object())
        .map(|map| map.len())
        .unwrap_or(0);
    systems + scenes
}

/// 校验生成器的需求描述
pub fn validate_requirement(raw: &str) -> Result<String, String> {
    let text = raw.trim();
    if text.is_empty() {
        return Err("先写一句你要什么（比如「帮我把论文摘要压成三句话」）".to_string());
    }
    if text.chars().count() < 4 {
        return Err("需求描述太短了，多写几个字".to_string());
    }
    if text.chars().count() > 2000 {
        return Err("需求描述最多 2000 字".to_string());
    }
    Ok(text.to_string())
}

/// 生成器发给模型的 messages（纯函数，方便单测）
pub fn build_generate_messages(requirement: &str) -> Vec<Value> {
    vec![
        json!({ "role": "system", "content": GENERATOR_PROMPT }),
        json!({ "role": "user", "content": format!("需求描述：{requirement}") }),
    ]
}

/// 清理模型输出：去掉 ``` 围栏、去掉开头的「提示词：」之类，按 8000 字截断
pub fn clean_generated(raw: &str) -> String {
    let mut text = raw.trim().to_string();
    // 去掉 ```lang … ``` 围栏（模型爱加）
    if text.starts_with("```") {
        if let Some(first_break) = text.find('\n') {
            text = text[first_break + 1..].to_string();
        } else {
            text = String::new();
        }
        if let Some(tail) = text.trim_end().strip_suffix("```") {
            text = tail.to_string();
        }
    }
    let mut lines: Vec<String> = text.lines().map(|line| line.to_string()).collect();
    // 开头的「提示词：」/「系统提示词：」这类引导语去掉
    const LABELS: [&str; 5] = [
        "提示词：",
        "提示词:",
        "系统提示词：",
        "系统提示词:",
        "以下是提示词：",
    ];
    let mut start = 0usize;
    while start < lines.len() {
        let trimmed = lines[start].trim();
        if LABELS.contains(&trimmed) {
            start += 1;
        } else {
            break;
        }
    }
    lines.drain(..start);
    let text = lines.join("\n").trim().to_string();
    if text.chars().count() > MAX_TEXT_CHARS {
        let mut cut: String = text.chars().take(MAX_TEXT_CHARS).collect();
        cut.push_str("\n…（生成内容过长，已截断）");
        return cut;
    }
    text
}

/// 生成器：一段需求描述 → 一份结构化提示词。
///
/// 没配 API Key / 接口地址时**不报错也不抛异常**，返回 `{ok:false, error, hint}`，
/// 前端直接显示那句中文提示就行（对应旧版「错误不影响聊天」的处理方式）。
pub fn generate(store: &Store, requirement: &str) -> Value {
    let text = match validate_requirement(requirement) {
        Ok(value) => value,
        Err(err) => return json!({ "ok": false, "error": err, "hint": "" }),
    };
    let config = crate::ai::config_state(store);
    if config.get("ok").and_then(|one| one.as_bool()) != Some(true) {
        let detail = config
            .get("error")
            .and_then(|one| one.as_str())
            .unwrap_or("")
            .to_string();
        return json!({
            "ok": false,
            "error": "还没配好 AI，生成不了提示词：设置 → AI（API 服务）里填一次接口地址、模型名和 API Key。",
            "hint": detail,
        });
    }
    let messages = build_generate_messages(&text);
    match crate::ai::chat(store, &messages, Some(0.4)) {
        Ok(raw) => {
            let cleaned = clean_generated(&raw);
            if cleaned.trim().is_empty() {
                return json!({
                    "ok": false,
                    "error": "模型没返回内容，换个说法再试一次",
                    "hint": "",
                });
            }
            json!({
                "ok": true,
                "text": cleaned,
                "model": config.get("model").cloned().unwrap_or_else(|| json!("")),
                "chars": cleaned.chars().count(),
            })
        }
        Err(err) => json!({ "ok": false, "error": format!("生成失败：{err}"), "hint": "" }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 进程级环境变量：测试结束必须恢复，否则会污染同进程其它模块的测试。
    struct EnvGuard(Option<std::ffi::OsString>);

    impl EnvGuard {
        fn new() -> Self {
            EnvGuard(std::env::var_os("LUMINA_DATA_DIR"))
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.0 {
                Some(value) => std::env::set_var("LUMINA_DATA_DIR", value),
                None => std::env::remove_var("LUMINA_DATA_DIR"),
            }
        }
    }

    /// 临时目录名带上进程 id：万一同时有另一个 `cargo test` 在跑同一个 crate，
    /// 两边不会用同一个临时库（同名目录互删会把对方的行读进来）。
    fn test_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("{name}-{}", std::process::id()))
    }

    fn temp_store(name: &str) -> (Store, EnvGuard) {
        let guard = EnvGuard::new();
        let dir = test_dir(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("LUMINA_DATA_DIR", &dir);
        let store = Store::open().expect("建临时库失败");
        (store, guard)
    }

    fn cleanup(name: &str) {
        let _ = std::fs::remove_dir_all(test_dir(name));
    }

    /// 默认模板能读出来，中文照抄旧版
    #[test]
    fn defaults_are_readable() {
        let (store, _guard) = temp_store("danlume-prompts-default-test");
        let items = templates(&store);
        assert_eq!(items.len(), BUILTIN_SYSTEM.len() + BUILTIN_SCENES.len());
        let kinds: Vec<&str> = items
            .iter()
            .map(|one| one["kind"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(kinds.iter().filter(|one| **one == "system").count(), 5);
        assert_eq!(kinds.iter().filter(|one| **one == "scene").count(), 2);
        // 顺序：先系统（assistant 在最前），再场景
        assert_eq!(items[0]["key"], "assistant");
        assert_eq!(items[0]["title"], "通用助手");
        assert!(items[0]["builtin"].as_bool().unwrap());
        let assistant = items[0]["text"].as_str().unwrap();
        assert!(assistant.starts_with("你是 Lume（卢梅），DanLume 里的写作与学习助手。"));
        assert!(assistant.contains("回答用简体中文，简洁直接"));

        // `system()` 直接取（旧版 prompts.system("assistant") 的等价物）
        assert_eq!(system(&store, "assistant"), assistant);
        assert!(system(&store, "weekly_report").contains("本周完成"));
        assert!(system(&store, "homework").contains("三句话摘要"));
        assert!(system(&store, "latex").contains("LaTeX"));
        assert!(system(&store, "wechat_todo").contains("{today}"));
        assert_eq!(system(&store, "不存在"), "");

        // 场景模板
        assert_eq!(scene(&store, "weekly_report").unwrap(), "根据我的日程数据写一份周报。（场景模板，可自行修改）");
        assert_eq!(scene(&store, "homework_summary").unwrap(), "总结这份作业要求，列出要做的事和截止时间。");
        assert!(scene(&store, "没有这个").is_none());

        // 没写过设置时，库里不该有 prompts_json 这一行（读不写库）
        assert!(store.setting(SETTING_KEY).is_none());
        cleanup("danlume-prompts-default-test");
    }

    /// 保存 / 覆盖 / 删除 / 恢复默认
    #[test]
    fn save_overwrite_delete() {
        let (store, _guard) = temp_store("danlume-prompts-save-test");
        // 改系统提示词
        let result = template_save(&store, "system", "assistant", "", "你是我的私人助教。").unwrap();
        assert_eq!(result["ok"], true);
        assert_eq!(system(&store, "assistant"), "你是我的私人助教。");
        // 其它几条不受影响
        assert!(system(&store, "latex").contains("LaTeX"));
        // 存进 settings 的是旧版 prompts.json 那份结构
        let raw = store.setting(SETTING_KEY).unwrap();
        let doc: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(doc["version"], 1);
        assert_eq!(doc["system"]["assistant"], "你是我的私人助教。");
        assert_eq!(doc["scenes"]["weekly_report"]["title"], "生成周报");
        assert!(doc["updated_at"].as_str().unwrap().contains('T'));

        // 新增场景模板
        template_save(&store, "scene", "论文速读", "论文速读", "用三句话讲清这篇论文的贡献。").unwrap();
        assert_eq!(scene(&store, "论文速读").unwrap(), "用三句话讲清这篇论文的贡献。");
        let found = templates(&store)
            .into_iter()
            .find(|one| one["key"] == "论文速读")
            .expect("新模板要能列出来");
        assert_eq!(found["title"], "论文速读");
        assert!(!found["builtin"].as_bool().unwrap());

        // 覆盖同一 key：改内容 + 改标题
        template_save(&store, "scene", "论文速读", "论文快读", "用两句话讲清。").unwrap();
        let found = templates(&store)
            .into_iter()
            .find(|one| one["key"] == "论文速读")
            .unwrap();
        assert_eq!(found["title"], "论文快读");
        assert_eq!(found["text"], "用两句话讲清。");
        assert_eq!(
            templates(&store)
                .iter()
                .filter(|one| one["key"] == "论文速读")
                .count(),
            1,
            "覆盖不能变成两条"
        );

        // 删自定义场景
        template_delete(&store, "scene", "论文速读").unwrap();
        assert!(scene(&store, "论文速读").is_none());
        assert_eq!(templates(&store).len(), BUILTIN_SYSTEM.len() + BUILTIN_SCENES.len());
        // 删过的不能因为合并默认值又冒出来
        let again = load(&store);
        assert!(again["scenes"].get("论文速读").is_none());

        // 删内置场景：真的删掉，并且不会被 deep_merge 复活
        template_delete(&store, "scene", "weekly_report").unwrap();
        assert!(scene(&store, "weekly_report").is_none());
        let items = templates(&store);
        assert!(!items.iter().any(|one| one["key"] == "weekly_report" && one["kind"] == "scene"));

        // 把内置场景重新保存回来：正常
        template_save(&store, "scene", "weekly_report", "生成周报", "再写一份周报。").unwrap();
        assert_eq!(scene(&store, "weekly_report").unwrap(), "再写一份周报。");

        // 系统提示词可以「恢复默认」：把内容改回默认值
        template_save(&store, "system", "assistant", "", BUILTIN_SYSTEM[0].text).unwrap();
        assert_eq!(system(&store, "assistant"), BUILTIN_SYSTEM[0].text);
        cleanup("danlume-prompts-save-test");
    }

    /// 非法输入一律拒绝，且不写坏原来的设置
    #[test]
    fn rejects_invalid_input() {
        let (store, _guard) = temp_store("danlume-prompts-invalid-test");
        // 空内容
        assert!(template_save(&store, "system", "assistant", "", "   ").is_err());
        // 不存在的系统提示词
        let err = template_save(&store, "system", "my_prompt", "", "内容").unwrap_err();
        assert!(err.contains("不是其中之一"));
        // 类型只能是 system / scene
        assert!(template_save(&store, "other", "x", "", "内容").is_err());
        // 标识不能为空 / 带空格
        assert!(template_save(&store, "scene", "   ", "", "内容").is_err());
        assert!(template_save(&store, "scene", "有 空格", "", "内容").is_err());
        assert!(template_save(&store, "scene", "带/斜杠", "", "内容").is_err());
        // 标识太长
        assert!(template_save(&store, "scene", &"长".repeat(60), "", "内容").is_err());
        // 正文太长
        assert!(template_save(&store, "scene", "太长的", "", &"字".repeat(MAX_TEXT_CHARS + 1)).is_err());
        // 系统提示词不能删
        let err = template_delete(&store, "system", "assistant").unwrap_err();
        assert!(err.contains("不能删除"));
        // 删不存在的
        assert!(template_delete(&store, "scene", "不存在的模板").is_err());
        // 拒绝之后，库里还是默认值
        assert!(store.setting(SETTING_KEY).is_none());
        assert!(system(&store, "assistant").starts_with(PERSONA));
        assert_eq!(templates(&store).len(), BUILTIN_SYSTEM.len() + BUILTIN_SCENES.len());
        cleanup("danlume-prompts-invalid-test");
    }

    /// 生成器：没配 API Key 时给友好提示（不联网、不抛错）
    #[test]
    fn generator_without_key_is_friendly() {
        let (store, _guard) = temp_store("danlume-prompts-gen-test");
        let result = generate(&store, "帮我把论文摘要压成三句话");
        println!("没配 AI 时：{result}");
        assert_eq!(result["ok"], false);
        let error = result["error"].as_str().unwrap();
        assert!(error.contains("设置 → AI"), "要指路去设置页：{error}");
        // 需求描述为空也要先拦住
        let result = generate(&store, "   ");
        assert_eq!(result["ok"], false);
        assert!(result["error"].as_str().unwrap().contains("先写一句"));
        cleanup("danlume-prompts-gen-test");
    }

    /// 设置页旧卡「系统提示词」（`system_prompt`）还能兜住：库里有值就以库为准
    #[test]
    fn legacy_system_prompt_is_honoured_until_library_is_used() {
        let (store, _guard) = temp_store("danlume-prompts-legacy-test");
        store.set_setting("system_prompt", "你是旧卡里写的人设。").unwrap();
        assert_eq!(system(&store, "assistant"), "你是旧卡里写的人设。");
        // 其它几条不受影响，还是内置默认
        assert!(system(&store, "latex").contains("LaTeX"));
        // 一旦在提示词库里改了 assistant，就以库为准
        template_save(&store, "system", "assistant", "", "你是提示词库里的人设。").unwrap();
        assert_eq!(system(&store, "assistant"), "你是提示词库里的人设。");
        cleanup("danlume-prompts-legacy-test");
    }

    /// 生成器用到的纯函数：校验、提示词组装、输出清理
    #[test]
    fn generator_helpers() {
        assert!(validate_requirement("").is_err());
        assert!(validate_requirement("短").is_err());
        assert!(validate_requirement(&"长".repeat(2001)).is_err());
        assert_eq!(validate_requirement("  写个周报  ").unwrap(), "写个周报");

        let messages = build_generate_messages("写个周报");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert!(messages[0]["content"].as_str().unwrap().contains("提示词工程师"));
        assert_eq!(messages[1]["content"], "需求描述：写个周报");

        // 围栏与开头标签都清掉
        let raw = "```markdown\n提示词：\n角色：你是周报助手。\n目标：把日程写成周报。\n```";
        let cleaned = clean_generated(raw);
        assert_eq!(cleaned, "角色：你是周报助手。\n目标：把日程写成周报。");
        // 正常文本不动
        assert_eq!(clean_generated("  角色：助教  "), "角色：助教");
        // 超长截断
        let long = clean_generated(&"字".repeat(MAX_TEXT_CHARS + 10));
        assert!(long.ends_with("（生成内容过长，已截断）"));
        assert!(long.starts_with(&"字".repeat(MAX_TEXT_CHARS)));
    }
}
