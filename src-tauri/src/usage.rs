//! 使用上报（对应旧版 `usage_report.py` 的客户端那一半）。
//!
//! 设置键与旧版完全一致：`report_enabled` / `report_url` / `report_interval_minutes`
//! / `daily_limit_hours` / `usage_day` / `usage_seconds` / `usage_requests` /
//! `usage_bypass` / `report_last_at` / `report_last_sig` / `report_fail_count` /
//! `ban_at` / `ban_reason`。
//!
//! 语义：
//! * 默认**开**（与旧版一致，`report_enabled` 没设过就当开）；上报地址默认走旧版那个内网地址；
//! * `usage_bypass=1` 的人（例如管理员自己）只记账、不上报；
//! * 上报成功写 `report_last_at` 与「有没有变化」的指纹，失败累加 `report_fail_count` 并退避；
//! * 服务器回 `action=ban` 才写 `ban_at` / `ban_reason`——**本地不会自己封自己**，
//!   这点和旧版不同（旧版超每日限额会本地升级为警告），故意改掉：本地误封自己没法恢复。
use std::time::Duration;

use chrono::Local;
use serde_json::{json, Value};

use crate::store::Store;

pub const DEFAULT_URL: &str = "http://10.137.44.215:18765/lumina-usage";
pub const DEFAULT_INTERVAL_MINUTES: i64 = 10;
pub const DEFAULT_DAILY_LIMIT_HOURS: i64 = 12;

fn setting_i64(store: &Store, key: &str, fallback: i64) -> i64 {
    store
        .setting(key)
        .and_then(|text| text.trim().parse::<f64>().ok())
        .map(|value| value as i64)
        .filter(|value| *value > 0)
        .unwrap_or(fallback)
}

pub fn enabled(store: &Store) -> bool {
    store
        .setting("report_enabled")
        .map(|text| text.trim() != "0")
        .unwrap_or(true)
}

pub fn bypass(store: &Store) -> bool {
    store
        .setting("usage_bypass")
        .map(|text| text.trim() == "1")
        .unwrap_or(false)
}

pub fn report_url(store: &Store) -> String {
    store
        .setting("report_url")
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_URL.to_string())
}

pub fn interval_minutes(store: &Store) -> i64 {
    setting_i64(store, "report_interval_minutes", DEFAULT_INTERVAL_MINUTES)
}

pub fn daily_limit_hours(store: &Store) -> i64 {
    setting_i64(store, "daily_limit_hours", DEFAULT_DAILY_LIMIT_HOURS)
}

fn today() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

fn number(store: &Store, key: &str) -> f64 {
    store
        .setting(key)
        .and_then(|text| text.trim().parse::<f64>().ok())
        .unwrap_or(0.0)
}

/// 记一次活动（秒数 + 请求数）；跨天自动清零
pub fn mark_activity(store: &Store, seconds: f64, requests: i64) {
    let day = today();
    if store.setting("usage_day").unwrap_or_default() != day {
        let _ = store.set_setting("usage_day", &day);
        let _ = store.set_setting("usage_seconds", "0");
        let _ = store.set_setting("usage_requests", "0");
    }
    let seconds = number(store, "usage_seconds") + seconds.max(0.0);
    let requests = number(store, "usage_requests") + requests.max(0) as f64;
    let _ = store.set_setting("usage_seconds", &format!("{seconds:.0}"));
    let _ = store.set_setting("usage_requests", &format!("{requests:.0}"));
}

/// 上报内容（字段与旧版 snapshot 对齐）
pub fn snapshot(store: &Store) -> Value {
    let seconds = number(store, "usage_seconds");
    json!({
        "v": 1,
        "kind": "usage",
        "username": store.setting("user_name").unwrap_or_default(),
        "day": store.setting("usage_day").unwrap_or_else(today),
        "used_seconds": seconds as i64,
        "requests": number(store, "usage_requests") as i64,
        "limit_hours": daily_limit_hours(store),
        "over_limit": seconds >= (daily_limit_hours(store) * 3600) as f64,
        "bypass": bypass(store),
        "version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
    })
}

/// 到点了没有：关着 / 绕过 / 间隔没到 → 不报
pub fn due(store: &Store) -> bool {
    if !enabled(store) || bypass(store) {
        return false;
    }
    let last = store.setting("report_last_at").unwrap_or_default();
    let Ok(at) = chrono::NaiveDateTime::parse_from_str(last.trim(), "%Y-%m-%d %H:%M:%S")
    else {
        return true;
    };
    let elapsed = Local::now().naive_local() - at;
    elapsed.num_minutes() >= interval_minutes(store)
}

/// 真正发一次；返回 `{ok, action, message}`
pub fn send(store: &Store) -> Value {
    if !enabled(store) {
        return json!({ "ok": false, "action": "off", "message": "上报已关闭（report_enabled=0）" });
    }
    if bypass(store) {
        return json!({ "ok": false, "action": "bypass", "message": "这台机器设了 usage_bypass=1，只记账不发送" });
    }
    let url = report_url(store);
    let payload = snapshot(store);
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(6))
        .user_agent("DanLume/2.0")
        .build();
    let response = agent
        .post(&url)
        .set("Content-Type", "application/json")
        .send_string(&payload.to_string());
    match response {
        Ok(res) => {
            let body: Value = res.into_json().unwrap_or_else(|_| json!({}));
            let action = body
                .get("action")
                .and_then(|one| one.as_str())
                .unwrap_or("ok")
                .to_string();
            // 只有服务器明确要求才封
            if action == "ban" {
                let reason = body
                    .get("message")
                    .and_then(|one| one.as_str())
                    .unwrap_or("管理员封禁");
                let _ = store.set_setting("ban_at", &Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
                let _ = store.set_setting("ban_reason", reason);
            }
            let _ = store.set_setting(
                "report_last_at",
                &Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            );
            let _ = store.set_setting("report_last_sig", &payload["used_seconds"].to_string());
            let _ = store.set_setting("report_fail_count", "0");
            json!({
                "ok": true, "action": action,
                "message": body.get("message").and_then(|one| one.as_str()).unwrap_or("已上报"),
                "sent": payload,
            })
        }
        Err(exc) => {
            let fails = setting_i64(store, "report_fail_count", 0) + 1;
            let _ = store.set_setting("report_fail_count", &fails.to_string());
            json!({ "ok": false, "action": "error", "message": format!("上报失败（连续 {fails} 次）：{exc}") })
        }
    }
}

/// 状态（设置页 / 关于页展示用）
pub fn status(store: &Store) -> Value {
    json!({
        "enabled": enabled(store),
        "bypass": bypass(store),
        "url": report_url(store),
        "interval_minutes": interval_minutes(store),
        "daily_limit_hours": daily_limit_hours(store),
        "snapshot": snapshot(store),
        "last_at": store.setting("report_last_at").unwrap_or_default(),
        "fail_count": setting_i64(store, "report_fail_count", 0),
        "ban_at": store.setting("ban_at").unwrap_or_default(),
        "ban_reason": store.setting("ban_reason").unwrap_or_default(),
    })
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
    fn counters_accumulate_and_reset_by_day() {
        let store = temp_store("danlume-usage-test");
        mark_activity(&store, 12.0, 1);
        mark_activity(&store, 30.0, 2);
        let snap = snapshot(&store);
        println!("快照：{snap}");
        assert_eq!(snap["used_seconds"], 42);
        assert_eq!(snap["requests"], 3);
        // 手动把日期改成昨天，再记一次应当清零
        let _ = store.set_setting("usage_day", "2000-01-01");
        mark_activity(&store, 5.0, 1);
        let snap = snapshot(&store);
        assert_eq!(snap["used_seconds"], 5);
        assert_eq!(snap["requests"], 1);
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-usage-test"));
    }

    #[test]
    fn bypass_and_switch_block_sending() {
        let store = temp_store("danlume-usage-off-test");
        let _ = store.set_setting("report_enabled", "0");
        assert_eq!(send(&store)["action"], "off");
        assert!(!due(&store));
        let _ = store.set_setting("report_enabled", "1");
        let _ = store.set_setting("usage_bypass", "1");
        assert_eq!(send(&store)["action"], "bypass");
        assert!(!due(&store));
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-usage-off-test"));
    }
}
