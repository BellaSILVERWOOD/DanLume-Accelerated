//! `.ics` 课程表导入（对应旧版 `timetable.py` 的 import_rows 那条路）。
//!
//! 为什么是 .ics：旦夕 / 教务都支持导出日历文件，这条通路不需要登录态，
//! 所以 v2 用「导入 .ics」来兑现 `timetable_fetch` 这个工具（旧版另有一条要
//! 内嵌浏览器会话的抓取路径，v2 的架构里没有那个会话）。
use chrono::{Datelike, NaiveDate, NaiveTime};
use serde_json::json;

use crate::store::{EventInput, Store};

fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.lines() {
        if (raw.starts_with(' ') || raw.starts_with('\t')) && !lines.is_empty() {
            let last = lines.last_mut().unwrap();
            last.push_str(&raw[1..]);
        } else {
            lines.push(raw.trim_end_matches('\r').to_string());
        }
    }
    lines
}

fn prop(event: &[String], name: &str) -> String {
    for line in event {
        if let Some(rest) = line.strip_prefix(&format!("{name}:")) {
            return rest.trim().to_string();
        }
        if let Some(rest) = line.strip_prefix(&format!("{name};")) {
            if let Some(pos) = rest.find(':') {
                return rest[pos + 1..].trim().to_string();
            }
        }
    }
    String::new()
}

fn unescape(value: &str) -> String {
    value
        .replace("\\n", "\n")
        .replace("\\N", "\n")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
}

/// `20261026T080000` / `20261026` → (日期, 可选时间)
fn parse_stamp(text: &str) -> Option<(NaiveDate, Option<NaiveTime>)> {
    let clean: String = text.chars().filter(|ch| *ch != 'Z').collect();
    if let Some((date, time)) = clean.split_once('T') {
        let day = NaiveDate::parse_from_str(date, "%Y%m%d").ok()?;
        let raw = time.chars().take(6).collect::<String>();
        let hour: u32 = raw.get(0..2)?.parse().ok()?;
        let minute: u32 = raw.get(2..4)?.parse().ok()?;
        let second: u32 = raw.get(4..6).unwrap_or("0").parse().unwrap_or(0);
        Some((day, Some(NaiveTime::from_hms_opt(hour, minute, second)?)))
    } else {
        NaiveDate::parse_from_str(&clean, "%Y%m%d").ok().map(|day| (day, None))
    }
}

fn rrule(value: &str) -> (i64, Option<i64>, Option<NaiveDate>) {
    let mut interval = 1;
    let mut count = None;
    let mut until = None;
    for part in value.split(';') {
        if let Some(rest) = part.strip_prefix("INTERVAL=") {
            interval = rest.parse().unwrap_or(1).max(1);
        } else if let Some(rest) = part.strip_prefix("COUNT=") {
            count = rest.parse().ok();
        } else if let Some(rest) = part.strip_prefix("UNTIL=") {
            until = parse_stamp(rest).map(|(day, _)| day);
        }
    }
    (interval, count, until)
}

fn shift(time: NaiveTime, minutes: i64) -> NaiveTime {
    use chrono::Timelike;
    let secs = time.hour() as i64 * 3600 + time.minute() as i64 * 60 + time.second() as i64
        + minutes * 60;
    let wrapped = ((secs % 86_400) + 86_400) % 86_400;
    NaiveTime::from_num_seconds_from_midnight_opt(wrapped as u32, 0).unwrap_or(time)
}

pub fn parse(text: &str) -> Vec<serde_json::Value> {
    let lines = unfold(text);
    let mut out = Vec::new();
    let mut buffer: Vec<String> = Vec::new();
    let mut inside = false;
    for line in lines {
        if line.trim() == "BEGIN:VEVENT" {
            inside = true;
            buffer.clear();
            continue;
        }
        if line.trim() == "END:VEVENT" {
            inside = false;
            if let Some(one) = build(&buffer) {
                out.push(one);
            }
            buffer.clear();
            continue;
        }
        if inside {
            buffer.push(line);
        }
    }
    out
}

fn build(event: &[String]) -> Option<serde_json::Value> {
    let title = unescape(&prop(event, "SUMMARY"));
    if title.is_empty() {
        return None;
    }
    let (start_day, start_at) = parse_stamp(&prop(event, "DTSTART"))?;
    let (end_day, end_at) = parse_stamp(&prop(event, "DTEND")).unwrap_or((start_day, None));
    let start = start_at.unwrap_or_else(|| NaiveTime::from_hms_opt(9, 0, 0).unwrap());
    let end = match end_at {
        Some(value) if value > start => value,
        _ => shift(start, 60),
    };
    let location = unescape(&prop(event, "LOCATION"));
    let description = unescape(&prop(event, "DESCRIPTION"));
    let rule = prop(event, "RRULE");
    let (interval, count, until) = rrule(&rule);
    let days = if end_day > start_day {
        (end_day - start_day).num_days()
    } else {
        0
    };

    let mut occurrences = Vec::new();
    let mut cursor = start_day;
    let limit = count.unwrap_or(1).clamp(1, 80);
    let mut index = 0;
    while index < limit {
        if let Some(last) = until {
            if cursor > last {
                break;
            }
        }
        occurrences.push(cursor);
        index += 1;
        if rule.is_empty() {
            break;
        }
        cursor += chrono::Duration::weeks(interval);
        if index >= 80 {
            break;
        }
    }
    Some(json!({
        "title": title,
        "location": location,
        "description": description,
        "start_time": start.format("%H:%M").to_string(),
        "end_time": end.format("%H:%M").to_string(),
        "days": days,
        "dates": occurrences.iter().map(|day| day.format("%Y-%m-%d").to_string()).collect::<Vec<_>>(),
    }))
}

/// 标记：从 .ics 导入的日程，description 里带这个尾巴，重新同步时才认得出来
pub const MARK: &str = "[课表]";

#[derive(Debug, Default, Clone)]
pub struct Report {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub kept: usize,
}

impl Report {
    pub fn summary(&self) -> String {
        format!(
            "课表已同步：新增 {} 节、改时间 {} 节、删掉 {} 节、没变 {} 节",
            self.added, self.updated, self.removed, self.kept
        )
    }
}

/// 每天的同步：以 .ics 为准——新增缺失的、改掉时间变了的、删掉课程表里已经没有的。
///
/// 「后面几周会有变动」就靠这个：调课/停课在 .ics 里变了，第二天同步就把日程纠正过来，
/// 而不是像 `import` 那样只增不改。
pub fn sync(store: &Store, path: &str) -> Result<Report, String> {
    let text = std::fs::read_to_string(path).map_err(|err| format!("读不到 .ics：{err}"))?;
    let rows = parse(&text);
    if rows.is_empty() {
        return Err("这个 .ics 里没有课程（一条 VEVENT 都没解析出来）".to_string());
    }
    // 期望的课程集合：(日期, 开始, 标题) → (结束, 描述)
    let mut wanted: Vec<(String, String, String, String, String)> = Vec::new();
    for row in &rows {
        let title = row["title"].as_str().unwrap_or_default().to_string();
        let start = row["start_time"].as_str().unwrap_or("09:00").to_string();
        let end = row["end_time"].as_str().unwrap_or("10:00").to_string();
        let location = row["location"].as_str().unwrap_or_default();
        let mut description = row["description"].as_str().unwrap_or_default().to_string();
        if !location.is_empty() {
            description = if description.is_empty() {
                format!("地点：{location}")
            } else {
                format!("{description}\n地点：{location}")
            };
        }
        if !description.is_empty() {
            description.push('\n');
        }
        description.push_str(MARK);
        for date in row["dates"].as_array().cloned().unwrap_or_default() {
            let date = date.as_str().unwrap_or_default().to_string();
            if date.is_empty() {
                continue;
            }
            wanted.push((date, start.clone(), title.clone(), end.clone(), description.clone()));
        }
    }

    // 已经由课表生成的那些日程
    let existing: Vec<(i64, String, String, String, String, String)> = {
        let conn = store.connection();
        let mut out = Vec::new();
        if let Ok(mut stmt) = conn.prepare(
            "SELECT id, date, start_time, title, end_time, description FROM events
             WHERE description LIKE ?1",
        ) {
            let like = format!("%{MARK}%");
            if let Ok(mapped) = stmt.query_map([like], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            }) {
                for one in mapped.flatten() {
                    out.push(one);
                }
            }
        }
        out
    };

    let mut report = Report::default();
    let mut touched: Vec<i64> = Vec::new();
    for (date, start, title, end, description) in &wanted {
        let hit = existing
            .iter()
            .find(|one| &one.1 == date && &one.2 == start && &one.3 == title);
        match hit {
            Some((id, _, _, _, current_end, current_desc)) => {
                touched.push(*id);
                if current_end != end || current_desc != description {
                    let conn = store.connection();
                    let _ = conn.execute(
                        "UPDATE events SET end_time = ?1, description = ?2, updated_at = ?3
                         WHERE id = ?4",
                        rusqlite::params![end, description, crate::now_str(), id],
                    );
                    report.updated += 1;
                } else {
                    report.kept += 1;
                }
            }
            None => {
                let input = EventInput {
                    title: Some(title.clone()),
                    description: Some(description.clone()),
                    date: Some(date.clone()),
                    start_time: Some(start.clone()),
                    end_time: Some(end.clone()),
                    remind_minutes: Some(10),
                    clear_remind: false,
                    color: Some("sky".to_string()),
                    done: None,
                    repeat: None,
                };
                if store.add_event(&input).is_ok() {
                    report.added += 1;
                }
            }
        }
    }
    // 课表里已经没有的（调课 / 停课）→ 从日程里删掉
    for (id, _, _, _, _, _) in &existing {
        if !touched.contains(id) {
            let conn = store.connection();
            let _ = conn.execute("DELETE FROM events WHERE id = ?1", [id]);
            report.removed += 1;
        }
    }
    let _ = store.set_setting("timetable_synced_at", &crate::now_str());
    let _ = store.set_setting("timetable_status", &report.summary());
    Ok(report)
}

/// 把 .ics 里的课写进日程，返回（新增, 跳过）
pub fn import(store: &Store, path: &str) -> Result<(usize, usize), String> {
    let text = std::fs::read_to_string(path).map_err(|err| format!("读不到文件：{err}"))?;
    let rows = parse(&text);
    if rows.is_empty() {
        return Err("这个 .ics 里没有课程（一条 VEVENT 都没解析出来）".to_string());
    }
    let mut existing: Vec<(String, String, String)> = {
        let conn = store.connection();
        let mut out = Vec::new();
        if let Ok(mut stmt) = conn.prepare("SELECT date, start_time, title FROM events") {
            if let Ok(mapped) = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            }) {
                for one in mapped.flatten() {
                    out.push(one);
                }
            }
        }
        out
    };
    let mut added = 0usize;
    let mut skipped = 0usize;
    for row in rows {
        let title = row["title"].as_str().unwrap_or_default().to_string();
        let start = row["start_time"].as_str().unwrap_or("09:00").to_string();
        let end = row["end_time"].as_str().unwrap_or("10:00").to_string();
        let mut description = row["description"].as_str().unwrap_or_default().to_string();
        let location = row["location"].as_str().unwrap_or_default();
        if !location.is_empty() {
            description = if description.is_empty() {
                format!("地点：{location}")
            } else {
                format!("{description}\n地点：{location}")
            };
        }
        for date in row["dates"].as_array().cloned().unwrap_or_default() {
            let date = date.as_str().unwrap_or_default().to_string();
            let key = (date.clone(), start.clone(), title.clone());
            if existing.contains(&key) {
                skipped += 1;
                continue;
            }
            let input = EventInput {
                title: Some(title.clone()),
                description: Some(description.clone()),
                date: Some(date),
                start_time: Some(start.clone()),
                end_time: Some(end.clone()),
                remind_minutes: Some(10),
                clear_remind: false,
                color: Some("sky".to_string()),
                done: None,
                repeat: None,
            };
            match store.add_event(&input) {
                Ok(_) => {
                    added += 1;
                    existing.push(key);
                }
                Err(_) => skipped += 1,
            }
        }
    }
    let _ = store.set_setting("timetable_status", &format!("已导入 {added} 节课"));
    Ok((added, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nSUMMARY:高等数学\\, 上\r\nLOCATION:三教 301\r\nDTSTART:20261026T080000\r\nDTEND:20261026T094000\r\nRRULE:FREQ=WEEKLY;COUNT=3\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn parses_weekly_course() {
        let rows = parse(SAMPLE);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row["title"], "高等数学, 上");
        assert_eq!(row["start_time"], "08:00");
        assert_eq!(row["end_time"], "09:40");
        assert_eq!(row["location"], "三教 301");
        let dates = row["dates"].as_array().unwrap();
        assert_eq!(dates.len(), 3, "COUNT=3 应该展开成 3 次");
        assert_eq!(dates[0], "2026-10-26");
        assert_eq!(dates[1], "2026-11-02");
    }

    #[test]
    fn ignores_events_without_summary() {
        let rows = parse("BEGIN:VEVENT\r\nDTSTART:20261026T080000\r\nEND:VEVENT\r\n");
        assert!(rows.is_empty());
    }

    /// 真跑一遍同步（临时数据目录，不碰真实库）：`cargo test timetable_sync_roundtrip -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn timetable_sync_roundtrip() {
        let dir = std::env::temp_dir().join("danlume-ics-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("LUMINA_DATA_DIR", &dir);
        let store = Store::open().expect("建临时库失败");

        let file = dir.join("timetable.ics");
        std::fs::write(
            &file,
            "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nSUMMARY:线性代数\r\nLOCATION:三教 301\r\n\
             DTSTART:20261026T080000\r\nDTEND:20261026T094000\r\nRRULE:FREQ=WEEKLY;COUNT=2\r\n\
             END:VEVENT\r\nEND:VCALENDAR\r\n",
        )
        .unwrap();

        let first = sync(&store, file.to_str().unwrap()).unwrap();
        println!("第一次：{}", first.summary());
        assert_eq!(first.added, 2);
        assert_eq!(first.kept, 0);

        let second = sync(&store, file.to_str().unwrap()).unwrap();
        println!("第二次（没改动）：{}", second.summary());
        assert_eq!(second.added, 0);
        assert_eq!(second.updated, 0);
        assert_eq!(second.removed, 0);
        assert_eq!(second.kept, 2);

        // 调课：时间改成 10:00–11:40，且只上一次（第二次课取消）
        std::fs::write(
            &file,
            "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nSUMMARY:线性代数\r\nLOCATION:三教 301\r\n\
             DTSTART:20261026T100000\r\nDTEND:20261026T114000\r\n\
             END:VEVENT\r\nEND:VCALENDAR\r\n",
        )
        .unwrap();
        let third = sync(&store, file.to_str().unwrap()).unwrap();
        println!("第三次（调课 + 取消一节）：{}", third.summary());
        assert_eq!(third.added, 1, "10 点那节课要新增");
        assert_eq!(third.removed, 2, "8 点那节 + 第二节 8 点要删掉");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
