//! 通知自动抓取（对应旧版 `notices.py` 的后台轮询）。
//!
//! 设置键（与旧版同名）：
//! * `notice_enabled`：总开关（默认开）；
//! * `notice_interval_hours`：多久抓一次（默认 6 小时）；
//! * `notice_sources`：多个来源，一行一个（也兼容逗号分隔）；
//! * `notice_last_run` / `notice_last_count`：上次抓取时间与新增条数；
//! * `notice_quiet_from` / `notice_quiet_to`：静默时段（这段时间不弹提示，但照样入库）；
//! * 兼容旧的单来源键 `dashsrc:notice` / `notice_url`。
use chrono::{Local, NaiveDateTime, NaiveTime, Timelike};
use serde_json::{json, Value};

use crate::store::Store;

pub const DEFAULT_INTERVAL_HOURS: i64 = 6;

fn setting_i64(store: &Store, key: &str, fallback: i64) -> i64 {
    store
        .setting(key)
        .and_then(|text| text.trim().parse::<i64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(fallback)
}

pub fn enabled(store: &Store) -> bool {
    store
        .setting("notice_enabled")
        .map(|text| text.trim() != "0")
        .unwrap_or(true)
}

pub fn interval_hours(store: &Store) -> i64 {
    // 旧版两代键名都认：notice_interval_hours（新）→ notice_interval（旧）→ 默认 6
    let value = store
        .setting("notice_interval_hours")
        .filter(|text| !text.trim().is_empty())
        .or_else(|| {
            store
                .setting("notice_interval")
                .filter(|text| !text.trim().is_empty())
        });
    value
        .and_then(|text| text.trim().parse::<i64>().ok())
        .filter(|hours| *hours > 0)
        .unwrap_or(DEFAULT_INTERVAL_HOURS)
}

/// 来源列表：`notice_sources`（一行一个）优先，其次旧单来源键
pub fn sources(store: &Store) -> Vec<String> {
    let raw = store
        .setting("notice_sources")
        .filter(|text| !text.trim().is_empty())
        .or_else(|| store.setting("dashsrc:notice"))
        .or_else(|| store.setting("notice_url"))
        .unwrap_or_default();
    raw.split(['\n', '\r', ',', ';', '；', '，'])
        .map(|one| one.trim().to_string())
        .filter(|one| one.starts_with("http"))
        .collect()
}

fn parse_hhmm(text: &str) -> Option<NaiveTime> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    NaiveTime::parse_from_str(text, "%H:%M").ok()
}

/// 静默时段：`from` 到 `to`（支持跨零点，例如 23:00 → 7:00）
pub fn in_quiet_hours(store: &Store, now: NaiveTime) -> bool {
    let Some(from) = store.setting("notice_quiet_from").and_then(|t| parse_hhmm(&t)) else {
        return false;
    };
    let Some(to) = store.setting("notice_quiet_to").and_then(|t| parse_hhmm(&t)) else {
        return false;
    };
    let n = now.hour() * 3600 + now.minute() * 60;
    let f = from.hour() * 3600 + from.minute() * 60;
    let t = to.hour() * 3600 + to.minute() * 60;
    if f <= t {
        n >= f && n < t
    } else {
        n >= f || n < t
    }
}

/// 到点了没有：从没抓过 → 该抓；间隔未到 → 不抓
pub fn due(store: &Store) -> bool {
    if !enabled(store) {
        return false;
    }
    let last = store.setting("notice_last_run").unwrap_or_default();
    let Ok(at) = NaiveDateTime::parse_from_str(last.trim(), "%Y-%m-%d %H:%M:%S") else {
        return true;
    };
    let elapsed = Local::now().naive_local() - at;
    elapsed.num_hours() >= interval_hours(store)
}

/// 抓一轮：每个来源拉一次、按 URL/标题去重入库，记录时间与条数
pub fn run_once(store: &Store) -> Value {
    let list = sources(store);
    if list.is_empty() {
        return json!({
            "ok": false, "added": 0, "sources": 0,
            "error": "还没配通知来源：设置 → 仪表板 → 教务通知 里填一次地址",
        });
    }
    let mut added = 0;
    let mut skipped = 0;
    let mut errors: Vec<String> = Vec::new();
    for url in &list {
        match crate::notices::fetch(url) {
            Ok(rows) => {
                for (title, link, published) in rows {
                    // 旧版 notice_only_edu：只看教务/校园域名
                    if only_edu(store) && !looks_edu(&link, &title) {
                        skipped += 1;
                        continue;
                    }
                    if store
                        .add_notice("notice", &title, &link, &published)
                        .unwrap_or(false)
                    {
                        added += 1;
                    }
                }
            }
            Err(exc) => errors.push(format!("{url}：{exc}")),
        }
    }
    let stamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let _ = store.set_setting("notice_last_run", &stamp);
    let _ = store.set_setting("notice_last_count", &added.to_string());
    let quiet = in_quiet_hours(store, Local::now().time());
    json!({
        "ok": errors.len() < list.len(),
        "added": added,
        "skipped": skipped,
        "sources": list.len(),
        "at": stamp,
        "quiet": quiet,
        // 旧版 notice_toast：关掉后静默时段之外也不弹提示
        "toast": !quiet && toast_enabled(store),
        "error": errors.join("；"),
    })
}

/// 旧版 `notice_only_edu`：默认关；开了就只留教务/校园域名的通知
pub fn only_edu(store: &Store) -> bool {
    store
        .setting("notice_only_edu")
        .map(|text| text.trim() == "1")
        .unwrap_or(false)
}

/// 旧版 `notice_toast`：默认开
pub fn toast_enabled(store: &Store) -> bool {
    store
        .setting("notice_toast")
        .map(|text| text.trim() != "0")
        .unwrap_or(true)
}

fn looks_edu(link: &str, title: &str) -> bool {
    let low = link.to_lowercase();
    low.contains(".edu") || low.contains("edu.cn") || low.contains("/jw") || low.contains("teaching")
        || title.contains("教务") || title.contains("通知") || title.contains("选课")
}

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
    fn sources_parses_multiple_lines() {
        let store = temp_store("danlume-notice-test");
        let _ = store.set_setting(
            "notice_sources",
            "https://a.edu.cn/n\nhttps://b.edu.cn/n，不是地址\nhttps://c.edu.cn/n",
        );
        let list = sources(&store);
        println!("来源：{list:?}");
        assert_eq!(list.len(), 3, "应过滤掉非地址那行");
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-notice-test"));
    }

    #[test]
    fn quiet_hours_handles_midnight() {
        let store = temp_store("danlume-quiet-test");
        let _ = store.set_setting("notice_quiet_from", "23:00");
        let _ = store.set_setting("notice_quiet_to", "07:00");
        let at = |h: u32, m: u32| NaiveTime::from_hms_opt(h, m, 0).unwrap();
        assert!(in_quiet_hours(&store, at(23, 30)));
        assert!(in_quiet_hours(&store, at(2, 0)));
        assert!(!in_quiet_hours(&store, at(8, 0)));
        assert!(!in_quiet_hours(&store, at(22, 59)));
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-quiet-test"));
    }

    #[test]
    fn due_respects_interval_and_switch() {
        let store = temp_store("danlume-due-test");
        assert!(due(&store), "从没抓过 → 该抓");
        let _ = store.set_setting("notice_last_run", &Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
        assert!(!due(&store), "刚抓过 → 不该抓");
        let _ = store.set_setting("notice_enabled", "0");
        assert!(!due(&store), "关掉总开关就不抓");
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-due-test"));
    }
}
