//! 天气：和旧版一样走 `wttr.in`（上海），结果缓存进 settings，取不到就用缓存。
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::store::Store;

const CITY: &str = "上海";
const CITY_QUERY: &str = "Shanghai";
/// 旧版缓存 10 分钟
const TTL_SECONDS: u64 = 600;
const CACHE_KEY: &str = "weather_text";
const CACHE_TS_KEY: &str = "weather_ts";

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0)
}

/// 拼成旧版那句：「17°C  晴 · 湿度 82% · 风速 5 km/h」
pub fn format_text(payload: &Value) -> Option<String> {
    let current = payload
        .get("current_condition")
        .and_then(|value| value.as_array())
        .and_then(|rows| rows.first())?;
    let temp = current.get("temp_C")?.as_str()?.to_string();
    let desc = current
        .get("weatherDesc")
        .and_then(|value| value.as_array())
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("value"))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let humidity = current
        .get("humidity")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let wind = current
        .get("windspeedKmph")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let mut bits = vec![format!("{temp}°C")];
    if !desc.is_empty() {
        bits.push(desc);
    }
    if !humidity.is_empty() {
        bits.push(format!("湿度 {humidity}%"));
    }
    if !wind.is_empty() {
        bits.push(format!("风速 {wind} km/h"));
    }
    Some(bits.join("  "))
}

fn fetch() -> Result<String, String> {
    let url = format!("https://wttr.in/{CITY_QUERY}?format=j1");
    let agent = crate::net::with_proxy(
     ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(6))
        .user_agent("curl/8.0 (DanLume)"),
     crate::get_store(),
     &url,
 )
 .build();
    let response = agent.get(&url).call().map_err(|err| err.to_string())?;
    let payload: Value = response.into_json().map_err(|err| err.to_string())?;
    format_text(&payload).ok_or_else(|| "天气数据格式不认识".to_string())
}

/// 返回给前端的天气信息（永不 panic）
pub fn weather_info(store: &Store) -> serde_json::Value {
    let cached = store.setting(CACHE_KEY).unwrap_or_default();
    let cached_at = store
        .setting(CACHE_TS_KEY)
        .and_then(|text| text.parse::<u64>().ok())
        .unwrap_or(0);
    let fresh = !cached.is_empty() && now().saturating_sub(cached_at) < TTL_SECONDS;

    if fresh {
        return serde_json::json!({
            "ok": true, "text": cached, "city": CITY, "cached": true, "error": "",
        });
    }

    match fetch() {
        Ok(text) => {
            let _ = store.set_setting(CACHE_KEY, &text);
            let _ = store.set_setting(CACHE_TS_KEY, &now().to_string());
            serde_json::json!({
                "ok": true, "text": text, "city": CITY, "cached": false, "error": "",
            })
        }
        Err(err) => serde_json::json!({
            "ok": !cached.is_empty(),
            "text": cached,
            "city": CITY,
            "cached": !cached.is_empty(),
            "error": if cached.is_empty() {
                format!("天气取不到（{err}）")
            } else {
                String::new()
            },
        }),
    }
}
