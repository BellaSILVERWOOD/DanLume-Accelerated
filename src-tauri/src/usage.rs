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

/// 上报里的用户名：登录了服务器就报**服务器账号名**（服务器的判罚要拿它跟令牌比），
/// 没登录才用本机的 `user_name`。
///
/// ⚠️ 别用 `user_name` 顶上去：那是旧版「本机账户」的名字，跟服务器账号往往不一样，
/// 混着用会让服务器把正常用户判成「上报的账号名和令牌对不上」。
fn report_username(store: &Store) -> String {
    let server_name = store.setting("server_username").unwrap_or_default();
    if !server_name.trim().is_empty() && crate::account::logged_in(store) {
        return server_name;
    }
    store.setting("user_name").unwrap_or_default()
}

/// 上报内容（字段与旧版 snapshot 对齐）
pub fn snapshot(store: &Store) -> Value {
    let seconds = number(store, "usage_seconds");
    json!({
        "v": 1,
        "kind": "usage",
        "username": report_username(store),
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

/// 真正发一次；返回 `{ok, action, count, message}`
///
/// 两条路：
/// 1. **登录了自家账号服务器** → `POST {server}/api/v1/usage`（带令牌），服务器会顺手判违规
///    （J2-16：时长超限 / 同一台设备换账号 / 账号名对不上令牌 —— **不限制 API 调用**）；
/// 2. 没登录（或设置里填了自定义 `report_url`）→ 走旧版那个内网地址。
pub fn send(store: &Store) -> Value {
    if !enabled(store) {
        return json!({ "ok": false, "action": "off", "message": "上报已关闭（report_enabled=0）" });
    }
    if bypass(store) {
        return json!({ "ok": false, "action": "bypass", "message": "这台机器设了 usage_bypass=1，只记账不发送" });
    }
    let payload = snapshot(store);
    // 登录了自家账号 → 报给它（能判账号真伪，也是唯一能判「同一台设备换账号」的地方）
    let logged_in = crate::account::logged_in(store) && !crate::account::server_url(store).is_empty();
    if logged_in {
        return send_to_server(store, &payload);
    }
    // 没登录、也没自己填上报地址 → **不上报**（用户 2026-11-15：不要限制，也别白失败）。
    // 旧版那个内网地址（10.137.44.215）是局域网时代的，现在这台机器基本不通，
    // 以前每 10 分钟白失败一次，`report_fail_count` 能涨到几千。
    if custom_report_url(store).is_none() {
        let _ = store.set_setting("report_fail_count", "0");
        return json!({
            "ok": false, "action": "idle",
            "message": "没登录 DanLume 账号、也没填上报地址，先不上报（只在本机记账）",
        });
    }
    send_to_lan(store, &payload)
}

/// 设置里**自己填的**上报地址（没填返回 None；默认那个旧内网地址不算）
fn custom_report_url(store: &Store) -> Option<String> {
    store
        .setting("report_url")
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

/// 报给自家账号服务器（带令牌，401 会自动续一次）
fn send_to_server(store: &Store, payload: &Value) -> Value {
    match crate::account::post_authed(store, "/api/v1/usage", payload) {
        Ok(body) => finish(store, payload, &body),
        Err(err) => {
            // 令牌失效之外的错误都按「这次没报成」处理，下一次再试
            if err.http_status == 403 {
                // 被封的账号连 /usage 都进不来：把这句当封号理由
                let _ = store.set_setting("ban_at", &Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
                let _ = store.set_setting("ban_reason", &err.error);
            }
            let fails = setting_i64(store, "report_fail_count", 0) + 1;
            let _ = store.set_setting("report_fail_count", &fails.to_string());
            json!({
                "ok": false, "action": "error",
                "message": format!("上报失败（连续 {fails} 次）：{}", err.error),
            })
        }
    }
}

/// 报给旧版那个内网地址（没登录账号服务器时的兜底）
fn send_to_lan(store: &Store, payload: &Value) -> Value {
    let url = report_url(store);
    let agent = crate::net::with_proxy(
        ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(6))
            .user_agent("DanLume/2.0"),
        crate::get_store(),
        &url,
    )
    .build();
    let response = agent
        .post(&url)
        .set("Content-Type", "application/json")
        .send_string(&payload.to_string());
    match response {
        Ok(res) => {
            let body: Value = res.into_json().unwrap_or_else(|_| json!({}));
            finish(store, payload, &body)
        }
        Err(exc) => {
            let fails = setting_i64(store, "report_fail_count", 0) + 1;
            let _ = store.set_setting("report_fail_count", &fails.to_string());
            json!({ "ok": false, "action": "error", "message": format!("上报失败（连续 {fails} 次）：{exc}") })
        }
    }
}

/// 收到服务器回应后的记账：`action` / `count` / `message` 都存下来给界面看，
/// 只有服务器明确说 `ban` 才写 `ban_at`（本地不自己封自己）。
fn finish(store: &Store, payload: &Value, body: &Value) -> Value {
    let action = body
        .get("action")
        .and_then(|one| one.as_str())
        .unwrap_or("ok")
        .to_string();
    let count = body.get("count").and_then(|one| one.as_i64()).unwrap_or(0);
    let message = body
        .get("message")
        .and_then(|one| one.as_str())
        .unwrap_or("已上报")
        .to_string();
    let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    if action == "ban" {
        let _ = store.set_setting("ban_at", &now);
        let _ = store.set_setting("ban_reason", &message);
    }
    let _ = store.set_setting("report_last_at", &now);
    let _ = store.set_setting("report_last_sig", &payload["used_seconds"].to_string());
    let _ = store.set_setting("report_fail_count", "0");
    let _ = store.set_setting("usage_action", &action);
    let _ = store.set_setting("usage_action_at", &now);
    let _ = store.set_setting("usage_flag_count", &count.to_string());
    json!({
        "ok": true, "action": action, "count": count,
        "message": message, "sent": payload,
    })
}

/// 这个处置要不要弹通知（提醒 / 警告 / 封号要，正常和失败不要）
pub fn needs_notice(action: &str) -> bool {
    matches!(action, "remind" | "warn" | "ban")
}

/// 状态（设置页 / 关于页展示用）
pub fn status(store: &Store) -> Value {
    json!({
        "enabled": enabled(store),
        "bypass": bypass(store),
        "url": report_url(store),
        "server": crate::account::server_url(store),
        "logged_in": crate::account::logged_in(store),
        "interval_minutes": interval_minutes(store),
        "daily_limit_hours": daily_limit_hours(store),
        "snapshot": snapshot(store),
        "last_at": store.setting("report_last_at").unwrap_or_default(),
        "fail_count": setting_i64(store, "report_fail_count", 0),
        // J2-16：服务器给的处置（提醒 / 警告 / 封号）与累计次数
        "action": store.setting("usage_action").unwrap_or_default(),
        "action_at": store.setting("usage_action_at").unwrap_or_default(),
        "flag_count": setting_i64(store, "usage_flag_count", 0),
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

    /// 上报里的用户名：登录了服务器要报**服务器账号名**（服务器的判罚拿它跟令牌比）
    #[test]
    fn snapshot_uses_server_username_when_logged_in() {
        let store = temp_store("danlume-usage-name-test");
        let _ = store.set_setting("user_name", "本机账户");
        assert_eq!(snapshot(&store)["username"], "本机账户", "没登录就用本机名");
        // 登录后（有令牌）+ 有服务器账号名 → 报服务器那个名字
        crate::account::set_token(&store, crate::account::TOKEN_KEY, "fake-token").unwrap();
        let _ = store.set_setting("server_username", "HelenaLee");
        assert_eq!(snapshot(&store)["username"], "HelenaLee");
        // 令牌没了就退回本机名
        crate::account::clear_tokens(&store);
        assert_eq!(snapshot(&store)["username"], "本机账户");
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-usage-name-test"));
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

    /// 没登录账号、也没自己填上报地址 → 不发（旧内网地址已废弃，别每 10 分钟白失败一次）
    #[test]
    fn idle_without_login_or_custom_url() {
        let store = temp_store("danlume-usage-idle-test");
        let _ = store.set_setting("report_fail_count", "42");
        let result = send(&store);
        assert_eq!(result["action"], "idle");
        assert_eq!(result["ok"], false);
        assert!(result["message"].as_str().unwrap_or("").contains("先不上报"));
        assert_eq!(store.setting("report_fail_count").unwrap_or_default(), "0");
        // 自己填了地址就不算 idle（走联网那条路，这里不测网络，只看判定）
        let _ = store.set_setting("report_url", "http://127.0.0.1:1/usage");
        assert_eq!(custom_report_url(&store).as_deref(), Some("http://127.0.0.1:1/usage"));
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-usage-idle-test"));
    }

    /// J2-16：服务器回什么就记什么；只有 `ban` 才写封号时间；提醒/警告/封号才弹通知
    #[test]
    fn server_verdict_is_recorded() {
        let store = temp_store("danlume-usage-verdict-test");
        let payload = json!({ "used_seconds": 43200, "requests": 7 });

        let warn = finish(
            &store,
            &payload,
            &json!({ "action": "warn", "count": 2, "message": "第 2 次警告：当天使用超过 12 小时" }),
        );
        assert_eq!(warn["action"], "warn");
        assert_eq!(warn["count"], 2);
        assert_eq!(store.setting("usage_action").unwrap_or_default(), "warn");
        assert_eq!(store.setting("usage_flag_count").unwrap_or_default(), "2");
        assert!(!store.setting("report_last_at").unwrap_or_default().is_empty());
        assert!(
            store.setting("ban_at").unwrap_or_default().is_empty(),
            "警告不该写封号时间"
        );
        assert!(needs_notice("warn") && needs_notice("remind") && needs_notice("ban"));
        assert!(!needs_notice("ok") && !needs_notice("error"));

        let ban = finish(
            &store,
            &payload,
            &json!({ "action": "ban", "count": 4, "message": "已封号：当天使用超过 12 小时" }),
        );
        assert_eq!(ban["action"], "ban");
        assert!(!store.setting("ban_at").unwrap_or_default().is_empty(), "封号要写时间");
        assert!(store.setting("ban_reason").unwrap_or_default().contains("已封号"));

        let ok = finish(&store, &payload, &json!({ "action": "ok", "count": 0, "message": "" }));
        assert_eq!(ok["action"], "ok");
        assert!(!needs_notice("ok"));

        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-usage-verdict-test"));
    }

    /// 真连一次线上账号服务器（默认跳过）：
    /// `DANLUME_TEST_TOKEN=<令牌> cargo test usage::tests::real_report_to_server -- --ignored --nocapture --test-threads=1`
    ///
    /// 验证的是「客户端 → 部署后的 /api/v1/usage → 判定回执落本地」这一整条，
    /// 令牌可以在服务器上临时签一个（见 `_待办清单.md`），30 分钟过期。
    #[test]
    #[ignore]
    fn real_report_to_server() {
        let token = std::env::var("DANLUME_TEST_TOKEN").expect("要 DANLUME_TEST_TOKEN");
        let store = temp_store("danlume-usage-real-test");
        let _ = store.set_setting("report_enabled", "1");
        let _ = store.set_setting("usage_bypass", "0");
        // 用「没用过」的时长，避免真在管理员账号上记一次违规
        let _ = store.set_setting("usage_day", &today());
        let _ = store.set_setting("usage_seconds", "0");
        let _ = store.set_setting("usage_requests", "0");
        let _ = store.set_setting("report_last_at", "");

        crate::account::save_server_url(&store, crate::account::DEFAULT_SERVER);
        crate::account::set_token(&store, crate::account::TOKEN_KEY, &token).unwrap();

        let status = crate::account::authed(&store, "GET", "/api/v1/usage/status", None)
            .expect("读自己的上报状态失败");
        println!("服务器上的状态：{status}");
        assert!(status.get("limit_hours").is_some(), "回执字段不对：{status}");

        let result = send(&store);
        println!("上报结果：{result}");
        assert_eq!(result["ok"], true, "{result}");
        assert_eq!(result["action"], "ok", "{result}");
        assert!(
            !store.setting("report_last_at").unwrap_or_default().is_empty(),
            "成功的上报要写时间戳"
        );
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-usage-real-test"));
    }
}
