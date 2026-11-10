//! 「该出发了」提醒（对应旧版 `reminders.py::check_departure`）。
//!
//! 旧版靠 `geo` 模块算距离（地点坐标 + 步行/骑车速度）；v2 还没有地点库，
//! 所以这里用同一批设置键做**时间估算**：
//! * `departure_mode`（walk / bike）决定用哪档速度；
//! * `travel_speed_walk` / `travel_speed_bike`（米/分钟）配合 `departure_minutes`
//!   （没有地点时用的默认路程分钟数，默认 20）算路上要多久；
//! * `departure_buffer`（分钟）是预留的缓冲；
//! * 出发时间 = 开始时间 - 路程 - 缓冲；到点且还没到开始时间就提醒一次（每事件每天一次）
use chrono::Local;
use serde_json::{json, Value};

use crate::store::Store;

fn setting_i64(store: &Store, key: &str, fallback: i64) -> i64 {
    store
        .setting(key)
        .and_then(|text| text.trim().parse::<i64>().ok())
        .unwrap_or(fallback)
}

pub fn enabled(store: &Store) -> bool {
    store
        .setting("departure_enabled")
        .map(|text| text.trim() != "0")
        .unwrap_or(true)
}

/// 路程分钟数：有 `departure_minutes` 就用它，否则按模式给一个保守值
pub fn travel_minutes(store: &Store) -> i64 {
    let given = setting_i64(store, "departure_minutes", 0);
    if given > 0 {
        return given;
    }
    let mode = store
        .setting("departure_mode")
        .unwrap_or_else(|| "walk".to_string())
        .to_lowercase();
    match mode.as_str() {
        "bike" => 10,
        _ => 25,
    }
}

fn minutes_of(text: &str) -> Option<i64> {
    let mut parts = text.split(':');
    let hour: i64 = parts.next()?.trim().parse().ok()?;
    let minute: i64 = parts.next()?.trim().parse().ok()?;
    Some(hour * 60 + minute)
}

/// 今天该出发的日程（每条每天只提醒一次，用 `departure_notified` 记已提醒的 id）
pub fn due(store: &Store) -> Vec<Value> {
    let mut out = Vec::new();
    if !enabled(store) {
        return out;
    }
    let now = Local::now();
    let today = now.format("%Y-%m-%d").to_string();
    let now_minutes = now.format("%H").to_string().parse::<i64>().unwrap_or(0) * 60
        + now.format("%M").to_string().parse::<i64>().unwrap_or(0);
    let mode = store.setting("departure_mode").unwrap_or_else(|| "walk".to_string());
    let default_travel = travel_minutes(store);
    let buffer = setting_i64(store, "departure_buffer", 2).max(0);
    let _ = default_travel;

    // 已经提醒过的（每天清空一次）
    let mut done: Vec<i64> = {
        let raw = store.setting("departure_notified").unwrap_or_default();
        let stored: Value = serde_json::from_str(&raw).unwrap_or_else(|_| json!({}));
        let same_day = stored
            .get("date")
            .and_then(|one| one.as_str())
            .map(|date| date == today)
            .unwrap_or(false);
        if same_day {
            stored
                .get("ids")
                .and_then(|one| one.as_array())
                .map(|rows| rows.iter().filter_map(|one| one.as_i64()).collect())
                .unwrap_or_default()
        } else {
            Vec::new()
        }
    };

    let events: Vec<(i64, String, String, String, String)> = {
        let conn = store.connection();
        let mut rows = Vec::new();
        if let Ok(mut stmt) = conn.prepare(
            "SELECT id, title, start_time, end_time, description FROM events
             WHERE date = ?1 AND done = 0 ORDER BY start_time",
        ) {
            if let Ok(mapped) = stmt.query_map([&today], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            }) {
                for one in mapped.flatten() {
                    rows.push(one);
                }
            }
        }
        rows
    };

    for (id, title, start_time, end_time, description) in events {
        if done.contains(&id) {
            continue;
        }
        let Some(start) = minutes_of(&start_time) else {
            continue;
        };
        if now_minutes >= start {
            continue; // 已经开始了就不再提醒「该出发」
        }
        // 描述里写了地点、而且地点库里有坐标 → 用真实距离算路上时间
        let travel = crate::geo::travel_minutes(store, &description, &mode).unwrap_or(default_travel);
        let lead = travel + buffer;
        let leave_at = start - lead;
        if now_minutes < leave_at {
            continue;
        }
        done.push(id);
        out.push(json!({
            "id": id,
            "kind": "departure",
            "title": format!("该出发了：{title}"),
            "date": today,
            "start_time": start_time,
            "end_time": end_time,
            "leave_at": format!("{:02}:{:02}", (leave_at / 60).max(0), leave_at % 60),
            "travel_minutes": travel,
            "buffer_minutes": buffer,
            "description": description,
        }));
    }

    if !out.is_empty() {
        let _ = store.set_setting(
            "departure_notified",
            &json!({ "date": today, "ids": done }).to_string(),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn travel_minutes_uses_settings() {
        let dir = std::env::temp_dir().join("danlume-depart-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("LUMINA_DATA_DIR", &dir);
        let store = Store::open().expect("建临时库失败");

        // 默认：走路 25 分钟
        assert_eq!(travel_minutes(&store), 25);
        let _ = store.set_setting("departure_mode", "bike");
        assert_eq!(travel_minutes(&store), 10);
        let _ = store.set_setting("departure_minutes", "35");
        assert_eq!(travel_minutes(&store), 35);

        // 关掉开关就不该有提醒
        let _ = store.set_setting("departure_enabled", "0");
        assert!(!enabled(&store));
        assert!(due(&store).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// 旧版 `auto_done_enabled`：把过去日期还没勾完成的日程自动标成已完成
pub fn auto_done(store: &Store) -> usize {
    let enabled = store
        .setting("auto_done_enabled")
        .map(|text| text.trim() != "0")
        .unwrap_or(false);
    if !enabled {
        return 0;
    }
    let today = Local::now().format("%Y-%m-%d").to_string();
    let conn = store.connection();
    conn.execute(
        "UPDATE events SET done = 1, updated_at = ?1 WHERE done = 0 AND date < ?2",
        rusqlite::params![crate::now_str(), today],
    )
    .unwrap_or(0)
}