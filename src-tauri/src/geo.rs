//! 定位与出行估算（对应旧版 `positioning.py` / `geo.py` 的核心）。
//!
//! 设置键：
//! * `location_mode`：`manual`（手填坐标）/ `ip`（按公网 IP 估城市级）/ `windows`（沿用上一次结果）
//! * `location_lat` / `location_lon`：手填坐标
//! * `travel_speed_walk` / `travel_speed_bike`：米每分钟（默认 80 / 250）
//! * `loc_points` 表：保存过的地点（名字 + 坐标），用来把日程里的「三教 301」之类对上坐标
//!
//! 出门时间 = 上课时间 − 路上时间 − 缓冲；路上时间 = 直线距离 × 1.25（绕路系数）/ 速度。
use serde_json::{json, Value};

use crate::store::Store;

const EARTH_RADIUS_KM: f64 = 6371.0;
/// 直线变实际路程的绕路系数（旧版同样不是按直线算的）
const DETOUR: f64 = 1.25;

fn setting_f64(store: &Store, key: &str, fallback: f64) -> f64 {
    store
        .setting(key)
        .and_then(|text| text.trim().parse::<f64>().ok())
        .filter(|value| *value > 0.0)
        .unwrap_or(fallback)
}

/// 两点球面距离（公里）
pub fn haversine_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let to_rad = |deg: f64| deg * std::f64::consts::PI / 180.0;
    let dlat = to_rad(lat2 - lat1);
    let dlon = to_rad(lon2 - lon1);
    let a = (dlat / 2.0).sin().powi(2)
        + to_rad(lat1).cos() * to_rad(lat2).cos() * (dlon / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_KM * a.sqrt().asin()
}

/// 当前坐标：手填优先；没有就按 IP 估一次并缓存
pub fn location(store: &Store) -> Value {
    let mode = store
        .setting("location_mode")
        .unwrap_or_else(|| "manual".to_string())
        .to_lowercase();
    let lat = store.setting("location_lat").and_then(|t| t.trim().parse::<f64>().ok());
    let lon = store.setting("location_lon").and_then(|t| t.trim().parse::<f64>().ok());
    // manual：只认手填；ip / windows：没有手填坐标就去估（windows 在 v2 里同样是 IP 兜底，
    // 系统定位 API 需要额外的权限位，暂时不做，但来源标注成 windows-ip 以免误以为是手填的）
    if mode != "ip" && mode != "windows" {
        if let (Some(lat), Some(lon)) = (lat, lon) {
            return json!({ "ok": true, "lat": lat, "lon": lon, "source": "manual" });
        }
    }
    if mode == "manual" {
        if let (Some(lat), Some(lon)) = (lat, lon) {
            return json!({ "ok": true, "lat": lat, "lon": lon, "source": "manual" });
        }
    }
    // 按 IP 估（城市级）：一次请求，成功就记住
    match ip_location() {
        Ok((lat, lon, city)) => {
            let _ = store.set_setting("location_lat", &lat.to_string());
            let _ = store.set_setting("location_lon", &lon.to_string());
            let _ = store.set_setting("location_ip_city", &city);
            let source = if mode == "windows" { "windows-ip" } else { "ip" };
            json!({ "ok": true, "lat": lat, "lon": lon, "source": source, "city": city })
        }
        Err(exc) => {
            // 退回到上次缓存/手填
            if let (Some(lat), Some(lon)) = (lat, lon) {
                json!({ "ok": true, "lat": lat, "lon": lon, "source": "cache", "error": exc })
            } else {
                json!({ "ok": false, "error": exc })
            }
        }
    }
}

fn ip_location() -> Result<(f64, f64, String), String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(8))
        .user_agent("DanLume/2.0")
        .build();
    let body: Value = agent
        .get("https://ipapi.co/json/")
        .call()
        .map_err(|err| format!("按 IP 定位失败：{err}"))?
        .into_json()
        .map_err(|err| format!("定位返回的不是 JSON：{err}"))?;
    let lat = body
        .get("latitude")
        .and_then(|one| one.as_f64())
        .ok_or_else(|| "定位结果里没有纬度".to_string())?;
    let lon = body
        .get("longitude")
        .and_then(|one| one.as_f64())
        .ok_or_else(|| "定位结果里没有经度".to_string())?;
    let city = body
        .get("city")
        .and_then(|one| one.as_str())
        .unwrap_or("")
        .to_string();
    Ok((lat, lon, city))
}

/// 保存过的地点里，按名字模糊匹配（取名字最长的命中，避免「三教」吃掉「三教 301」）
pub fn match_place(store: &Store, text: &str) -> Option<(String, f64, f64)> {
    let wanted = text.trim();
    if wanted.is_empty() {
        return None;
    }
    let rows: Vec<(String, f64, f64)> = {
        let conn = store.connection();
        // 旧库里本来就有这张表；新建的库如果没有就补一张空的（不影响任何逻辑）
        let _ = conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS loc_points (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 name TEXT NOT NULL,
                 lat REAL NOT NULL,
                 lon REAL NOT NULL,
                 note TEXT NOT NULL DEFAULT '',
                 source TEXT NOT NULL DEFAULT '',
                 created_at TEXT NOT NULL
             );",
        );
        let mut out = Vec::new();
        if let Ok(mut stmt) = conn.prepare("SELECT name, lat, lon FROM loc_points") {
            if let Ok(mapped) =
                stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?, row.get::<_, f64>(2)?)))
            {
                for one in mapped.flatten() {
                    out.push(one);
                }
            }
        }
        out
    };
    let mut best: Option<(String, f64, f64)> = None;
    for (name, lat, lon) in rows {
        if !wanted.contains(&name) {
            continue;
        }
        let better = best
            .as_ref()
            .map(|(current, _, _)| name.chars().count() > current.chars().count())
            .unwrap_or(true);
        if better {
            best = Some((name, lat, lon));
        }
    }
    best
}

/// 从日程描述里抠地点文字（「地点：三教 301」「@三教 301」都认）
pub fn parse_location(description: &str) -> String {
    for line in description.lines() {
        let line = line.trim();
        for prefix in ["地点：", "地点:", "位置：", "位置:", "@"] {
            if let Some(rest) = line.strip_prefix(prefix) {
                let value = rest.trim();
                if !value.is_empty() {
                    return value.to_string();
                }
            }
        }
    }
    String::new()
}

/// 某条日程要提前多久出发（分钟）；对不上地点就返回 None，交给调用方用默认值
pub fn travel_minutes(store: &Store, description: &str, mode: &str) -> Option<i64> {
    let place_text = parse_location(description);
    if place_text.is_empty() {
        return None;
    }
    let (_, lat, lon) = match match_place(store, &place_text) {
        Some(one) => one,
        None => return None,
    };
    let here = location(store);
    if here["ok"] != true {
        return None;
    }
    let from_lat = here["lat"].as_f64()?;
    let from_lon = here["lon"].as_f64()?;
    let km = haversine_km(from_lat, from_lon, lat, lon) * DETOUR;
    let speed = setting_f64(
        store,
        &format!("travel_speed_{}", if mode == "bike" { "bike" } else { "walk" }),
        if mode == "bike" { 250.0 } else { 80.0 },
    );
    let minutes = (km * 1000.0 / speed).ceil() as i64;
    Some(minutes.clamp(1, 240))
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
    fn haversine_is_reasonable() {
        // 复旦邯郸校区 → 人民广场，直线约 8~9 公里
        let km = haversine_km(31.2989, 121.5036, 31.2304, 121.4737);
        println!("直线距离 {km:.2} km");
        assert!(km > 6.0 && km < 12.0, "距离算得不对：{km}");
        assert!(haversine_km(31.3, 121.5, 31.3, 121.5) < 0.001);
    }

    #[test]
    fn parse_location_reads_common_formats() {
        assert_eq!(parse_location("上课\n地点：三教 301"), "三教 301");
        assert_eq!(parse_location("@图书馆"), "图书馆");
        assert_eq!(parse_location("没有地点"), "");
    }

    #[test]
    fn travel_minutes_uses_saved_place() {
        let store = temp_store("danlume-geo-test");
        let _ = store.set_setting("location_mode", "manual");
        let _ = store.set_setting("location_lat", "31.2989");
        let _ = store.set_setting("location_lon", "121.5036");
        {
            let conn = store.connection();
            let _ = conn.execute(
                "INSERT INTO loc_points(name, lat, lon, note, source, created_at)
                 VALUES ('三教', 31.3010, 121.5060, '', 'test', 'now')",
                [],
            );
            let _ = conn.execute(
                "INSERT INTO loc_points(name, lat, lon, note, source, created_at)
                 VALUES ('人民广场', 31.2304, 121.4737, '', 'test', 'now')",
                [],
            );
        }
        let near = travel_minutes(&store, "地点：三教 301", "walk");
        let far = travel_minutes(&store, "地点：人民广场", "walk");
        println!("解析地点：{:?}", parse_location("地点：三教 301"));
        println!("匹配地点：{:?}", match_place(&store, "三教 301"));
        println!("当前位置：{}", location(&store));
        println!("三教 {near:?} 分钟 / 人民广场 {far:?} 分钟");
        assert!(near.is_some() && near.unwrap() >= 1 && near.unwrap() <= 10);
        assert!(far.is_some() && far.unwrap() > near.unwrap(), "远处的应该更久");
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-geo-test"));
    }
}
