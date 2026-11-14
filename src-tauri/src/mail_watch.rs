//! Agent 盯邮箱（对应旧版 `mail_watch.py`）：
//! 只看**指定发件人**的新邮件，把里面的「要做的事」抽出来，等用户确认再加进日程。
//!
//! 与旧版一致的部分：
//! * 设置键 `mail_watch_enabled` / `mail_watch_senders`（逗号分隔）/ `mail_watch_minutes`（2–180，默认 10）
//!   / `mail_watch_seen`（处理过的 `uid`，最多 300 条）；
//! * 只处理「发件人关键词命中」且「没处理过」的邮件；
//! * 抽取走大模型（`lib::mail_events_for`，和邮件页「可能要加的日程」同一套，带缓存）；
//! * 抽出的候选交给用户确认，加了日程才记「已处理」。
//!
//! 与旧版不同的一点：旧版会遍历所有 IMAP 账户，Rust 的收件箱同步目前用「第一个配置好的账户」
//! （`mail::config`），这里跟随它，免得出现「邮件页只显示 A 账户、盯邮箱却在看 B 账户」。
use serde_json::{json, Value};

use crate::store::Store;

pub const ENABLED_KEY: &str = "mail_watch_enabled";
pub const SENDERS_KEY: &str = "mail_watch_senders";
pub const MINUTES_KEY: &str = "mail_watch_minutes";
pub const SEEN_KEY: &str = "mail_watch_seen";
/// 最多记多少封「处理过」的（旧版就是 300）
const SEEN_LIMIT: usize = 300;
const DEFAULT_MINUTES: i64 = 10;
/// 每次看几个账户的几封（旧版 5）
const PER_CHECK: usize = 5;
/// 一次最多给模型几封（省 token；旧版也是 5）
const MAX_TO_LLM: usize = 5;

// ---------------------------------------------------------------- 设置

pub fn enabled(store: &Store) -> bool {
    store.setting(ENABLED_KEY).as_deref() == Some("1")
}

pub fn set_enabled(store: &Store, on: bool) {
    let _ = store.set_setting(ENABLED_KEY, if on { "1" } else { "0" });
}

/// 发件人关键词（中英文逗号都认）
pub fn senders(store: &Store) -> Vec<String> {
    let raw = store.setting(SENDERS_KEY).unwrap_or_default();
    raw.replace('，', ",")
        .split(',')
        .map(|one| one.trim().to_string())
        .filter(|one| !one.is_empty())
        .collect()
}

pub fn set_senders(store: &Store, text: &str) {
    let _ = store.set_setting(SENDERS_KEY, text.trim());
}

pub fn interval_minutes(store: &Store) -> i64 {
    store
        .setting(MINUTES_KEY)
        .and_then(|text| text.trim().parse::<f64>().ok())
        .map(|value| value as i64)
        .unwrap_or(DEFAULT_MINUTES)
        .clamp(2, 180)
}

pub fn set_interval(store: &Store, minutes: i64) {
    let _ = store.set_setting(MINUTES_KEY, &minutes.clamp(2, 180).to_string());
}

fn seen(store: &Store) -> Vec<String> {
    let raw = store.setting(SEEN_KEY).unwrap_or_default();
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return Vec::new();
    };
    let mut out: Vec<String> = value
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|one| one.as_str().map(|text| text.to_string()))
                .collect()
        })
        .unwrap_or_default();
    if out.len() > SEEN_LIMIT {
        out = out.split_off(out.len() - SEEN_LIMIT);
    }
    out
}

fn mark_seen(store: &Store, keys: &[String]) {
    let mut merged = seen(store);
    for key in keys {
        if !key.trim().is_empty() && !merged.contains(key) {
            merged.push(key.clone());
        }
    }
    if merged.len() > SEEN_LIMIT {
        merged = merged.split_off(merged.len() - SEEN_LIMIT);
    }
    let payload = Value::Array(merged.iter().map(|one| json!(one)).collect());
    let _ = store.set_setting(SEEN_KEY, &payload.to_string());
}

pub fn reset_seen(store: &Store) {
    let _ = store.set_setting(SEEN_KEY, "[]");
}

pub fn seen_count(store: &Store) -> usize {
    seen(store).len()
}

// ---------------------------------------------------------------- 匹配

/// 发件人（含账户名）里有没有关键词（大小写不敏感）
pub fn matches(from: &str, account_name: &str, keywords: &[String]) -> bool {
    if keywords.is_empty() {
        return false;
    }
    let blob = format!("{from} {account_name}").to_lowercase();
    keywords
        .iter()
        .any(|word| !word.trim().is_empty() && blob.contains(&word.trim().to_lowercase()))
}

// ---------------------------------------------------------------- 抓取 + 抽取

/// 只看命中的新邮件（不含已处理过的），返回 (邮件, 错误)
pub fn fetch_matching(store: &Store) -> (Vec<Value>, Vec<String>) {
    let keywords = senders(store);
    if keywords.is_empty() {
        return (Vec::new(), vec!["还没设置发件人关键词".to_string()]);
    }
    let known = seen(store);
    let summaries = match crate::mail::list_inbox(store, PER_CHECK * 4) {
        Ok(rows) => rows,
        Err(exc) => return (Vec::new(), vec![exc]),
    };
    let account_name = store.setting("imap_user").unwrap_or_default();
    let mut out: Vec<Value> = Vec::new();
    for row in summaries {
        if known.contains(&row.uid) {
            continue;
        }
        if !matches(&row.from, &account_name, &keywords) {
            continue;
        }
        out.push(json!({
            "uid": row.uid,
            "subject": row.subject,
            "from": row.from,
            "date": row.date,
            "unread": row.unread,
        }));
        if out.len() >= PER_CHECK {
            break;
        }
    }
    (out, Vec::new())
}

/// 抓一次 + 抽一次：返回 `{ok, mails, items, errors}`
///
/// `items` 里每条都带 `uid`，前端「加入日程」时用它把邮件记成已处理。
///
/// 注意：这里会调 `lib::mail_events_for`（要 `&'static Store`），所以必须传 `get_store()` 进来的
/// store 也是同一个单例 —— 命令层就是从 `get_store()` 取的。
pub fn check(store: &'static Store) -> Value {
    let (mails, mut errors) = fetch_matching(store);
    if mails.is_empty() {
        return json!({ "ok": true, "mails": [], "items": [], "errors": errors });
    }
    let has_key = crate::ai::config_state(store)
        .get("has_key")
        .and_then(|one| one.as_bool())
        .unwrap_or(false);
    if !has_key {
        errors.push("没配 AI 接口：只能列出命中的邮件，抽不出待办".to_string());
        return json!({ "ok": true, "mails": mails, "items": [], "errors": errors });
    }
    let mut items: Vec<Value> = Vec::new();
    for mail in mails.iter().take(MAX_TO_LLM) {
        let uid = mail.get("uid").and_then(|one| one.as_str()).unwrap_or("");
        if uid.is_empty() {
            continue;
        }
        let Some(body) = crate::mail::fetch_body(store, uid).ok() else {
            continue;
        };
        let found = crate::mail_events_for(
            store,
            uid,
            &body.subject,
            &body.from,
            &body.date,
            &body.text,
            false,
        );
        for item in found {
            let mut item = item;
            if let Some(map) = item.as_object_mut() {
                map.insert("uid".to_string(), json!(uid));
                map.insert("subject".to_string(), json!(body.subject));
                map.insert("from".to_string(), json!(body.from));
            }
            items.push(item);
        }
    }
    json!({ "ok": true, "mails": mails, "items": items, "errors": errors })
}

/// 用户看过这批邮件了：记下来别再提示
pub fn consume(store: &Store, uids: &[String]) {
    mark_seen(store, uids);
}

/// 设置页/面板上的一句话状态
pub fn summary(store: &Store) -> String {
    if !enabled(store) {
        return "盯邮箱：关着（打开后只处理指定发件人的新邮件）".to_string();
    }
    let keywords = senders(store);
    if keywords.is_empty() {
        return "盯邮箱：开着，但还没填发件人关键词".to_string();
    }
    let minutes = interval_minutes(store);
    // 「配没配收件箱」只看服务器和账号填没填：口令缺失是另一回事，
    // 不该让「每 N 分钟」这行信息消失（写单测时踩到过）。
    let host = store.setting("imap_host").unwrap_or_default();
    let user = store.setting("imap_user").unwrap_or_default();
    let configured = crate::mail::config(store).is_ok()
        || (!host.trim().is_empty() && !user.trim().is_empty());
    if !configured {
        return "盯邮箱：开着，但收件箱还没配置".to_string();
    }
    format!(
        "盯邮箱：每 {minutes} 分钟看一次，只看 {}（已处理 {} 封）",
        keywords.join(" / "),
        seen_count(store)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Temp(Option<std::ffi::OsString>, std::path::PathBuf);

    impl Temp {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "danlume-watch-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let previous = std::env::var_os("LUMINA_DATA_DIR");
            std::env::set_var("LUMINA_DATA_DIR", &dir);
            Self(previous, dir)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            match self.0.take() {
                Some(value) => std::env::set_var("LUMINA_DATA_DIR", value),
                None => std::env::remove_var("LUMINA_DATA_DIR"),
            }
            let _ = std::fs::remove_dir_all(&self.1);
        }
    }

    #[test]
    fn senders_parse_both_commas() {
        let _temp = Temp::new("senders");
        let store = Store::open().expect("临时库");
        set_senders(&store, " 教务处，辅导员 , noreply@fudan.edu.cn ");
        assert_eq!(
            senders(&store),
            vec![
                "教务处".to_string(),
                "辅导员".to_string(),
                "noreply@fudan.edu.cn".to_string()
            ]
        );
    }

    #[test]
    fn interval_is_clamped() {
        let _temp = Temp::new("interval");
        let store = Store::open().expect("临时库");
        assert_eq!(interval_minutes(&store), 10, "默认 10 分钟");
        set_interval(&store, 1);
        assert_eq!(interval_minutes(&store), 2, "最少 2 分钟");
        set_interval(&store, 999);
        assert_eq!(interval_minutes(&store), 180, "最多 3 小时");
        store.set_setting(MINUTES_KEY, "15").unwrap();
        assert_eq!(interval_minutes(&store), 15);
    }

    #[test]
    fn matches_sender_keywords() {
        let keywords = vec!["教务处".to_string(), "noreply@fudan".to_string()];
        assert!(matches("教务处 <jwc@fudan.edu.cn>", "", &keywords));
        assert!(matches("NoReply@Fudan.edu.cn", "", &keywords), "大小写不敏感");
        assert!(matches("someone", "教务处班长群", &keywords), "账户名也算");
        assert!(!matches("导员 <abc@qq.com>", "", &keywords));
        assert!(!matches("教务处", "", &[]), "没关键词就什么都不匹配");
    }

    #[test]
    fn seen_list_is_capped_and_deduped() {
        let _temp = Temp::new("seen");
        let store = Store::open().expect("临时库");
        for index in 0..(SEEN_LIMIT + 50) {
            mark_seen(&store, &[format!("uid-{index}")]);
        }
        let rows = seen(&store);
        assert_eq!(rows.len(), SEEN_LIMIT, "最多记 300 条");
        assert_eq!(rows.last().unwrap(), &format!("uid-{}", SEEN_LIMIT + 49));
        // 重复记不重复加
        let before = seen_count(&store);
        mark_seen(&store, &[rows.last().unwrap().clone()]);
        assert_eq!(seen_count(&store), before);
        // 重置
        reset_seen(&store);
        assert_eq!(seen_count(&store), 0);
    }

    /// 拿**真实**收件箱跑一遍：IMAP 拉信 → 匹配发件人 → 抽待办（默认跳过）。
    ///
    /// 跑法（要真实数据 + 网络 + 已配好的 IMAP/AI）：
    ///   cargo test mail_watch::tests::real_mail_watch_check -- --ignored --nocapture --test-threads=1
    ///
    /// 用真实库的**副本**，不动真库。
    #[test]
    #[ignore = "需要真实收件箱 + 网络 + 已配好的 AI Key"]
    fn real_mail_watch_check() {
        let Some(source) = crate::semantic::real_client_db() else {
            println!("找不到本机客户端数据库，跳过");
            return;
        };
        let dir = std::env::temp_dir().join(format!("danlume-watch-real-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::copy(&source, dir.join("lumina.db")).expect("复制数据库");
        for suffix in ["-wal", "-shm"] {
            let from = source.with_extension(format!("db{suffix}"));
            if from.exists() {
                let _ = std::fs::copy(&from, dir.join(format!("lumina.db{suffix}")));
            }
        }
        let previous = std::env::var_os("LUMINA_DATA_DIR");
        std::env::set_var("LUMINA_DATA_DIR", &dir);
        let store = crate::get_store();
        set_senders(store, "elearning");
        println!("状态：{}", summary(store));
        let result = check(store);
        println!(
            "邮件 {} 封，候选 {} 条，错误 {:?}",
            result["mails"].as_array().map(|rows| rows.len()).unwrap_or(0),
            result["items"].as_array().map(|rows| rows.len()).unwrap_or(0),
            result["errors"]
        );
        for row in result["mails"].as_array().cloned().unwrap_or_default() {
            println!("  邮件：{} | {}", row["from"].as_str().unwrap_or(""), row["subject"].as_str().unwrap_or(""));
        }
        for row in result["items"].as_array().cloned().unwrap_or_default() {
            println!(
                "  候选：{} {} {}",
                row["date"].as_str().unwrap_or(""),
                row["start_time"].as_str().unwrap_or(""),
                row["title"].as_str().unwrap_or("")
            );
        }
        match previous {
            Some(value) => std::env::set_var("LUMINA_DATA_DIR", value),
            None => std::env::remove_var("LUMINA_DATA_DIR"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn summary_says_what_is_missing() {
        let _temp = Temp::new("summary");
        let store = Store::open().expect("临时库");
        assert!(summary(&store).contains("关着"));
        set_enabled(&store, true);
        assert!(summary(&store).contains("还没填发件人关键词"));
        set_senders(&store, "教务处");
        // 没配收件箱 → 提示配置
        assert!(summary(&store).contains("收件箱还没配置"));
        set_interval(&store, 20);
        store.set_setting("imap_host", "imap.example.com").unwrap();
        store.set_setting("imap_user", "lena@example.com").unwrap();
        store.set_setting("imap_port", "993").unwrap();
        let text = summary(&store);
        assert!(text.contains("每 20 分钟"), "{text}");
        assert!(text.contains("教务处"));
    }
}
