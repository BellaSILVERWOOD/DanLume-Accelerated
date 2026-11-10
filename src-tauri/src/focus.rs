//! 专注（番茄钟）里的「离开检测」（对应旧版 `focus.py`）。
//!
//! * `focus_minutes` / `focus_break`：一轮专注多久、休息多久；
//! * `focus_away_enabled`：专注期间是否统计「切走了」；
//! * `focus_away_minutes`：连续离开多久算走神；
//! * `focus_whitelist`：允许切过去的窗口（逗号分隔，按窗口标题子串匹配，大小写不敏感）。
//!
//! 判定方式说明：v2 用**前台窗口标题**判断在不在 DanLume 里（拿进程名要再加一组
//! Windows 权限位，收益不大）。标题里含 `DanLume` 就算在专注，否则算离开，
//! 命中白名单的关键词也不算离开。
use chrono::Local;
use serde_json::{json, Value};

use crate::store::Store;

pub const DEFAULT_MINUTES: i64 = 25;
pub const DEFAULT_BREAK: i64 = 5;
pub const DEFAULT_AWAY_MINUTES: i64 = 3;

fn setting_i64(store: &Store, key: &str, fallback: i64) -> i64 {
    store
        .setting(key)
        .and_then(|text| text.trim().parse::<i64>().ok())
        .unwrap_or(fallback)
        .max(0)
}

pub fn away_enabled(store: &Store) -> bool {
    store
        .setting("focus_away_enabled")
        .map(|text| text.trim() != "0")
        .unwrap_or(true)
}

pub fn whitelist(store: &Store) -> Vec<String> {
    store
        .setting("focus_whitelist")
        .unwrap_or_default()
        .split([',', '，', ';', '\n'].as_ref())
        .map(|one| one.trim().to_lowercase())
        .filter(|one| !one.is_empty())
        .collect()
}

/// 标题算不算「在专注」
pub fn in_focus(title: &str, whitelist_items: &[String]) -> bool {
    let low = title.to_lowercase();
    if low.contains("danlume") || low.is_empty() {
        return true;
    }
    whitelist_items.iter().any(|one| low.contains(one))
}

/// 会话状态：剩余秒数、累计离开秒数、是否该提醒
pub fn status(store: &Store) -> Value {
    let started = store.setting("focus_started_at").unwrap_or_default();
    let minutes = {
        let given = setting_i64(store, "focus_minutes", 0);
        if given > 0 {
            given
        } else {
            DEFAULT_MINUTES
        }
    };
    let label = store.setting("focus_label").unwrap_or_default();
    let away_seconds = setting_i64(store, "focus_away_seconds", 0);
    let threshold = {
        let given = setting_i64(store, "focus_away_minutes", 0);
        if given > 0 {
            given
        } else {
            DEFAULT_AWAY_MINUTES
        }
    } * 60;
    if started.is_empty() {
        return json!({
            "active": false, "label": label, "minutes": minutes,
            "remaining": 0, "away_seconds": away_seconds, "away_threshold": threshold,
            "away_enabled": away_enabled(store), "over": false,
        });
    }
    let remaining = match chrono::NaiveDateTime::parse_from_str(&started, "%Y-%m-%d %H:%M:%S") {
        Ok(at) => {
            let end = at + chrono::Duration::minutes(minutes);
            (end - Local::now().naive_local()).num_seconds().max(0)
        }
        Err(_) => 0,
    };
    json!({
        "active": remaining > 0,
        "label": label,
        "minutes": minutes,
        "started_at": started,
        "remaining": remaining,
        "away_seconds": away_seconds,
        "away_threshold": threshold,
        "away_enabled": away_enabled(store),
        "over": threshold > 0 && away_seconds >= threshold,
    })
}

/// 开始一轮专注（旧版 pomodoro_start 的那几个设置键同名）
pub fn start(store: &Store, minutes: Option<i64>, label: &str) -> Value {
    let minutes = minutes
        .map(|one| one.clamp(1, 180))
        .unwrap_or_else(|| {
            let given = setting_i64(store, "focus_minutes", 0);
            if given > 0 {
                given
            } else {
                DEFAULT_MINUTES
            }
        });
    let started = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let _ = store.set_setting("focus_minutes", &minutes.to_string());
    let _ = store.set_setting("focus_label", label);
    let _ = store.set_setting("focus_started_at", &started);
    let _ = store.set_setting("focus_away_seconds", "0");
    json!({ "ok": true, "minutes": minutes, "started_at": started, "label": label })
}

pub fn stop(store: &Store) -> Value {
    let _ = store.set_setting("focus_started_at", "");
    let away = setting_i64(store, "focus_away_seconds", 0);
    let _ = store.set_setting("focus_away_seconds", "0");
    json!({ "ok": true, "away_seconds": away })
}

/// 一次「心跳」：会话进行中就看一眼前台窗口，累计离开时间，超阈值写一条通知
pub fn tick(store: &Store) -> Value {
    let state = status(store);
    if state["active"] != true {
        return state;
    }
    if !away_enabled(store) {
        return state;
    }
    let title = crate::ai_tools::foreground_title();
    let focused = in_focus(&title, &whitelist(store));
    let mut away = setting_i64(store, "focus_away_seconds", 0);
    if focused {
        // 回来了：清零连续离开
        if away != 0 {
            let _ = store.set_setting("focus_away_seconds", "0");
        }
        return status(store);
    }
    away += 15; // 后台每 15 秒一次
    let _ = store.set_setting("focus_away_seconds", &away.to_string());
    let threshold = state["away_threshold"].as_i64().unwrap_or(180);
    if threshold > 0 && away >= threshold {
        // 同一个会话只提醒一次
        let stamp = format!(
            "{}-{}",
            store.setting("focus_started_at").unwrap_or_default(),
            threshold
        );
        let last = store.setting("focus_away_alerted").unwrap_or_default();
        if last != stamp {
            let _ = store.set_setting("focus_away_alerted", &stamp);
            let _ = store.add_notice(
                "focus",
                &format!("专注中走神了：离开超过 {} 分钟（当前窗口：{}）", threshold / 60, title),
                "",
                &Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            );
        }
    }
    status(store)
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
    fn in_focus_matches_title_and_whitelist() {
        let wl = vec!["visual studio code".to_string(), "typora".to_string()];
        assert!(in_focus("DanLume 2.0", &wl), "标题里有 DanLume 就算在专注");
        assert!(in_focus("main.rs - Visual Studio Code", &wl), "白名单子串要命中");
        assert!(!in_focus("哔哩哔哩", &wl));
        assert!(in_focus("", &wl), "拿不到标题时不算离开（别误报）");
    }

    #[test]
    fn start_and_status_roundtrip() {
        let store = temp_store("danlume-focus-test");
        let started = start(&store, Some(30), "写论文");
        println!("开始：{started}");
        let state = status(&store);
        println!("状态：{state}");
        assert_eq!(state["active"], true);
        assert_eq!(state["minutes"], 30);
        assert!(state["remaining"].as_i64().unwrap_or(0) > 1700);
        let stopped = stop(&store);
        println!("结束：{stopped}");
        assert_eq!(status(&store)["active"], false);
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-focus-test"));
    }
}
