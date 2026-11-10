//! 教务课表抓取（对应旧版 `fdu.py`）。走"第二跳"：ID 会话 → 教务服务会话 → print-data。
//!
//! 现状说明（如实）：
//! * 鉴权与请求链路已实现并可用 `jwgl_probe` 诊断；
//! * 返回体的**字段解析**要按真实样本对齐（旧版 `parse_print_data` 吃的是 JSON 结构，
//!   但学校换新 ID 系统后接口可能已变），所以先提供诊断命令，拿到真实样本后再定解析。
use serde_json::{json, Value};

use crate::store::Store;
use crate::uis;

pub const COURSE_TABLE: &str = "https://fdjwgl.fudan.edu.cn/student/for-std/course-table";
pub const PRINT_DATA: &str =
    "https://fdjwgl.fudan.edu.cn/student/for-std/course-table/semester/{sem}/print-data";

/// 先请求一次 course-table 建立服务会话；再取 print-data 原文
pub fn fetch_print_data(store: &Store, semester: &str) -> Result<(String, String), String> {
    let (final_url, jar) = uis::visit_with_session(store, COURSE_TABLE)?;
    if final_url.contains("id.fudan.edu.cn") {
        return Err(format!(
            "会话对教务无效（被重定向到统一身份认证）：请先在「打开登录窗口」里登录一次，再点「抓取会话」。当前落地地址：{final_url}"
        ));
    }
    let url = PRINT_DATA.replace("{sem}", semester);
    let response = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(20))
        .redirects(5)
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/120.0 Safari/537.36",
        )
        .build()
        .get(&url)
        .set("Cookie", &jar.to_header())
        .call()
        .map_err(|exc| format!("请求课表失败：{exc}"))?;
    let body = response
        .into_string()
        .map_err(|exc| format!("课表内容读不出来：{exc}"))?;
    Ok((url, body))
}

/// 诊断：不打日志、不写库，只回"这跳有没有过、返回什么形状"
pub fn probe(store: &Store, semester: &str) -> Value {
    match fetch_print_data(store, semester) {
        Ok((url, body)) => {
            let head: String = body.chars().take(400).collect();
            let shape = if body.trim_start().starts_with('{') || body.trim_start().starts_with('[') {
                "json"
            } else if body.contains("<table") {
                "html-table"
            } else if body.contains("<html") {
                "html"
            } else {
                "unknown"
            };
            let keys: Vec<String> = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|value| {
                    value.as_object().map(|map| {
                        map.keys().cloned().collect::<Vec<_>>()
                    })
                })
                .unwrap_or_default();
            json!({
                "ok": true,
                "url": url,
                "shape": shape,
                "bytes": body.len(),
                "top_keys": keys,
                "head": head,
            })
        }
        Err(exc) => json!({ "ok": false, "error": exc }),
    }
}


// ------------------------------------------------------------------ 解析（对齐旧版 fdu.parse_print_data）
/// 默认节次时间（复旦常见作息；payload 里若给了 unit 时间表会优先用它）
fn default_unit_times() -> Vec<(i64, String, String)> {
    vec![
        (1, "08:00".to_string(), "08:45".to_string()),
        (2, "08:55".to_string(), "09:40".to_string()),
        (3, "10:00".to_string(), "10:45".to_string()),
        (4, "10:55".to_string(), "11:40".to_string()),
        (5, "13:30".to_string(), "14:15".to_string()),
        (6, "14:25".to_string(), "15:10".to_string()),
        (7, "15:30".to_string(), "16:15".to_string()),
        (8, "16:25".to_string(), "17:10".to_string()),
        (9, "18:30".to_string(), "19:15".to_string()),
        (10, "19:25".to_string(), "20:10".to_string()),
        (11, "20:20".to_string(), "21:05".to_string()),
        (12, "21:15".to_string(), "22:00".to_string()),
    ]
}

fn pick<'a>(node: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    let map = node.as_object()?;
    for key in keys {
        if let Some(found) = map.get(*key) {
            if !found.is_null() {
                return Some(found);
            }
        }
    }
    None
}

fn pick_int(node: &Value, keys: &[&str]) -> Option<i64> {
    let value = pick(node, keys)?;
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
}

fn pick_text(node: &Value, keys: &[&str]) -> String {
    match pick(node, keys) {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(rows)) => rows
            .iter()
            .filter_map(|one| {
                one.as_str()
                    .map(|text| text.to_string())
                    .or_else(|| pick_text(one, &["name", "teacherName"]).into())
            })
            .collect::<Vec<_>>()
            .join("、"),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

fn walk_nodes<'a>(value: &'a Value, out: &mut Vec<&'a Value>) {
    match value {
        Value::Object(map) => {
            out.push(value);
            for (_, child) in map {
                walk_nodes(child, out);
            }
        }
        Value::Array(rows) => {
            for row in rows {
                walk_nodes(row, out);
            }
        }
        _ => {}
    }
}

/// 一条课程片段（周几 + 起止节 + 周次 + 名字 + 地点 + 教师）
pub struct Slot {
    pub weekday: i64,
    pub start_unit: i64,
    pub end_unit: i64,
    pub weeks: Vec<i64>,
    pub title: String,
    pub room: String,
    pub teachers: String,
}

pub fn parse_print_data(body: &str) -> Result<Vec<Slot>, String> {
    let data: Value = serde_json::from_str(body)
        .map_err(|exc| format!("课表数据不是合法 JSON：{exc}"))?;
    let mut nodes: Vec<&Value> = Vec::new();
    walk_nodes(&data, &mut nodes);
    let mut out: Vec<Slot> = Vec::new();
    for node in nodes {
        let Some(weekday) = pick_int(node, &["weekday", "dayOfWeek", "weekDay", "weekdayIndex"]) else {
            continue;
        };
        if !(1..=7).contains(&weekday) {
            continue;
        }
        let Some(start_unit) = pick_int(node, &["startUnit", "startUnitIndex", "start"]) else {
            continue;
        };
        let end_unit = pick_int(node, &["endUnit", "endUnitIndex", "end"]).unwrap_or(start_unit);
        let title = pick_text(node, &["courseName", "name", "lessonName"]);
        if title.trim().is_empty() {
            continue;
        }
        let weeks = match pick(node, &["weekIndexes", "weekIndices", "weeks"]) {
            Some(Value::Array(rows)) => rows.iter().filter_map(|one| one.as_i64()).collect(),
            Some(Value::String(text)) => text
                .split([',', ' '])
                .filter_map(|one| one.trim().parse::<i64>().ok())
                .collect(),
            _ => Vec::new(),
        };
        out.push(Slot {
            weekday,
            start_unit,
            end_unit,
            weeks,
            title: title.trim().to_string(),
            room: pick_text(node, &["room", "place", "classroom", "lessonPlace"]),
            teachers: pick_text(node, &["teachers", "teacherName", "teacher"]),
        });
    }
    Ok(out)
}

/// 把课程片段写进 events（按「日期+开始时间+标题」去重），返回 (新增, 跳过)
pub fn import_slots(store: &Store, slots: &[Slot], term_start: &str) -> Result<(usize, usize), String> {
    let monday = chrono::NaiveDate::parse_from_str(term_start, "%Y-%m-%d")
        .map_err(|_| format!("开学日期看不懂：{term_start}（要 YYYY-MM-DD）"))?;
    let times = default_unit_times();
    let existing: Vec<(String, String, String)> = {
        let conn = store.connection();
        let mut out = Vec::new();
        if let Ok(mut stmt) = conn.prepare("SELECT date, start_time, title FROM events") {
            if let Ok(mapped) = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
            }) {
                for one in mapped.flatten() {
                    out.push(one);
                }
            }
        }
        out
    };
    let mut seen = existing;
    let mut added = 0usize;
    let mut skipped = 0usize;
    for slot in slots {
        let Some((_, start, _)) = times.iter().find(|(unit, _, _)| *unit == slot.start_unit) else {
            continue;
        };
        let Some((_, _, end)) = times.iter().find(|(unit, _, _)| *unit == slot.end_unit) else {
            continue;
        };
        for week in &slot.weeks {
            if *week < 1 || *week > 30 {
                continue;
            }
            let day = monday + chrono::Duration::days((week - 1) * 7 + (slot.weekday - 1));
            let date = day.format("%Y-%m-%d").to_string();
            let key = (date.clone(), start.clone(), slot.title.clone());
            if seen.contains(&key) {
                skipped += 1;
                continue;
            }
            let mut description = String::new();
            if !slot.room.is_empty() {
                description.push_str(&format!("地点：{}", slot.room));
            }
            if !slot.teachers.is_empty() {
                if !description.is_empty() {
                    description.push('\n');
                }
                description.push_str(&format!("教师：{}", slot.teachers));
            }
            if !description.is_empty() {
                description.push('\n');
            }
            description.push_str("[课表]");
            let input = crate::store::EventInput {
                title: Some(slot.title.clone()),
                description: Some(description),
                date: Some(date),
                start_time: Some(start.clone()),
                end_time: Some(end.clone()),
                remind_minutes: Some(10),
                clear_remind: false,
                color: Some("sky".to_string()),
                done: None,
                repeat: None,
            };
            if store.add_event(&input).is_ok() {
                added += 1;
                seen.push(key);
            } else {
                skipped += 1;
            }
        }
    }
    let _ = store.set_setting("timetable_status", &format!("教务课表：新增 {added} 节、跳过 {skipped} 节"));
    Ok((added, skipped))
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_data_url_has_placeholder() {
        assert!(PRINT_DATA.contains("{sem}"));
        assert_eq!(
            PRINT_DATA.replace("{sem}", "2025-2026-1"),
            "https://fdjwgl.fudan.edu.cn/student/for-std/course-table/semester/2025-2026-1/print-data"
        );
    }

    /// 真联网诊断：cargo test --lib -- --ignored --nocapture jwgl_real_probe
    #[test]
    #[ignore]
    fn jwgl_real_probe() {
        std::env::set_var(
            "LUMINA_DATA_DIR",
            r"E:\FIles\Lena‘s Files\Work\znw\Agent\data",
        );
        let store = Store::open().expect("打不开数据库");
        let result = probe(&store, "2025-2026-1");
        println!("诊断结果：{result}");
    }
}
