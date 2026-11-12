//! 作业：读旧版已经缓存好的清单（`homework_assignments` 设置）+ 本地文件索引
//! （`elearning_file_index`），并把截止时间写进日程 —— 与旧版修复后的规则一致：
//! 日程块是「截止前 30 分钟 → 截止时间」，重复的不重复加，旧格式的块会原地改时间。
use chrono::{Duration, NaiveDate, NaiveDateTime};
use serde_json::{json, Value};

use crate::store::Store;
use crate::EventInput;

const TITLE_PREFIX: &str = "作业截止：";
const LEAD_MINUTES: i64 = 30;
const MAP_KEY: &str = "elearning_event_map";
const VERSION_KEY: &str = "elearning_blocks_version";
const VERSION: &str = "2";

/// 缓存里的作业清单（旧版存在 settings.homework_assignments）
pub fn cached_items(store: &Store) -> Vec<Value> {
    let raw = store.setting("homework_assignments").unwrap_or_default();
    if raw.trim().is_empty() {
        return Vec::new();
    }
    match serde_json::from_str::<Vec<Value>>(&raw) {
        Ok(items) => items,
        Err(_) => Vec::new(),
    }
}

/// 本地文件索引（旧版 elearning_file_index：[{course, assignment, name, course_name}]）
fn file_index(store: &Store) -> Vec<Value> {
    let raw = store.setting("elearning_file_index").unwrap_or_default();
    if raw.trim().is_empty() {
        return Vec::new();
    }
    serde_json::from_str::<Vec<Value>>(&raw).unwrap_or_default()
}

fn downloads_dir(store: &Store) -> std::path::PathBuf {
    let value = store.setting("dir_downloads").unwrap_or_default();
    if value.trim().is_empty() {
        return crate::paths::data_dir().join("elearning");
    }
    let path = std::path::PathBuf::from(&value);
    if path.is_absolute() {
        path
    } else {
        crate::paths::data_dir().join(value)
    }
}

/// 「第八周作业」→ ["8"]（中文数字 + 阿拉伯数字都抽出来）
pub fn number_tokens(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut digits = String::new();
    let mut cn = String::new();
    let push = |out: &mut Vec<String>, value: &str| {
        if !value.is_empty() && !out.iter().any(|item| item == value) {
            out.push(value.to_string());
        }
    };
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            push(&mut out, &cn);
            cn.clear();
            digits.push(ch);
            continue;
        }
        if let Some(value) = cn_digit(ch) {
            push(&mut out, &digits);
            digits.clear();
            cn.push(ch);
            let _ = value;
            continue;
        }
        push(&mut out, &digits);
        push(&mut out, &cn_number(&cn));
        digits.clear();
        cn.clear();
    }
    push(&mut out, &digits);
    push(&mut out, &cn_number(&cn));
    out
}

fn cn_digit(ch: char) -> Option<u32> {
    match ch {
        '零' => Some(0),
        '一' => Some(1),
        '二' | '两' => Some(2),
        '三' => Some(3),
        '四' => Some(4),
        '五' => Some(5),
        '六' => Some(6),
        '七' => Some(7),
        '八' => Some(8),
        '九' => Some(9),
        '十' => Some(10),
        _ => None,
    }
}

fn cn_number(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    if !text.contains('十') {
        return text
            .chars()
            .next()
            .and_then(cn_digit)
            .map(|value| value.to_string())
            .unwrap_or_default();
    }
    let (left, right) = text.split_once('十').unwrap_or(("", ""));
    let tens = if left.is_empty() {
        1
    } else {
        left.chars().next().and_then(cn_digit).unwrap_or(1)
    };
    let ones = if right.is_empty() {
        0
    } else {
        right.chars().next().and_then(cn_digit).unwrap_or(0)
    };
    (tens * 10 + ones).to_string()
}

fn push_file(found: &mut Vec<String>, path: &std::path::Path) {
    let text = path.display().to_string();
    if path.is_file() && !found.iter().any(|item| item == &text) {
        found.push(text);
    }
}

/// 给一条作业找本地文件：索引 → 附件名/作业名 → 同课程编号 → 直接扫目录
pub fn local_files(store: &Store, item: &Value) -> Vec<String> {
    let dir = downloads_dir(store);
    let course = item
        .get("course_id")
        .or_else(|| item.get("course"))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let assignment = item
        .get("id")
        .or_else(|| item.get("assignment"))
        .and_then(|value| {
            value
                .as_str()
                .map(|text| text.to_string())
                .or_else(|| value.as_i64().map(|num| num.to_string()))
        })
        .unwrap_or_default();
    let name = item
        .get("name")
        .or_else(|| item.get("title"))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();

    let mut found: Vec<String> = Vec::new();

    // ① 索引里记过这个作业的文件
    for entry in file_index(store) {
        let entry_course = entry.get("course").and_then(|value| value.as_str()).unwrap_or("");
        let entry_assignment = entry
            .get("assignment")
            .and_then(|value| {
                value
                    .as_str()
                    .map(|text| text.to_string())
                    .or_else(|| value.as_i64().map(|num| num.to_string()))
            })
            .unwrap_or_default();
        if !assignment.is_empty() && entry_assignment == assignment && entry_course == course {
            if let Some(file_name) = entry.get("name").and_then(|value| value.as_str()) {
                push_file(&mut found, &dir.join(file_name));
            }
        }
    }
    // ② 名字直接对得上（精确 / 忽略大小写 / 去扩展名）
    let mut names: Vec<String> = Vec::new();
    if let Some(attachments) = item.get("attachments").and_then(|value| value.as_array()) {
        for attachment in attachments {
            if let Some(value) = attachment.get("name").and_then(|value| value.as_str()) {
                names.push(value.to_string());
            }
        }
    }
    if !name.is_empty() {
        names.push(name.clone());
    }
    for wanted in &names {
        let direct = dir.join(wanted);
        if direct.is_file() {
            push_file(&mut found, &direct);
        }
    }
    if found.is_empty() {
        if let Ok(reader) = std::fs::read_dir(&dir) {
            for entry in reader.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let file_name = entry.file_name().to_string_lossy().to_lowercase();
                let stem = path
                    .file_stem()
                    .map(|value| value.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                if names.iter().any(|wanted| {
                    let key = wanted.to_lowercase();
                    let key_stem = key
                        .rsplit_once('.')
                        .map(|(left, _)| left.to_string())
                        .unwrap_or(key.clone());
                    file_name == key || stem == key_stem
                }) {
                    push_file(&mut found, &path);
                }
            }
        }
    }
    // ③ 同课程里编号对得上（「第八周作业」→ HW8.x）
    if found.is_empty() {
        let tokens = number_tokens(&name);
        if !tokens.is_empty() {
            for entry in file_index(store) {
                let entry_course = entry.get("course").and_then(|value| value.as_str()).unwrap_or("");
                if course.is_empty() || entry_course != course {
                    continue;
                }
                let Some(file_name) = entry.get("name").and_then(|value| value.as_str()) else {
                    continue;
                };
                let file_tokens = number_tokens(file_name);
                if tokens.iter().any(|token| file_tokens.iter().any(|other| other == token)) {
                    push_file(&mut found, &dir.join(file_name));
                }
            }
        }
    }
    // ④ 索引里也没有：直接扫下载目录
    if found.is_empty() {
        let tokens = number_tokens(&name);
        if !tokens.is_empty() {
            if let Ok(reader) = std::fs::read_dir(&dir) {
                for entry in reader.flatten() {
                    let path = entry.path();
                    if !path.is_file() {
                        continue;
                    }
                    let stem = path
                        .file_stem()
                        .map(|value| value.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let file_tokens = number_tokens(&stem);
                    if tokens.iter().any(|token| file_tokens.iter().any(|other| other == token)) {
                        push_file(&mut found, &path);
                        if found.len() >= 12 {
                            break;
                        }
                    }
                }
            }
        }
    }
    found
}

/// 一条作业 → 日程字段（块 = 截止前 30 分钟 → 截止）
pub fn event_fields(item: &Value) -> Option<Value> {
    let due = item
        .get("due")
        .or_else(|| item.get("deadline"))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if due.is_empty() {
        return None;
    }
    let when = due.replace('T', " ");
    let day = when.get(0..10)?.to_string();
    let clock = when.get(11..16).unwrap_or("23:59").to_string();
    let due_at = NaiveDateTime::parse_from_str(&format!("{day} {clock}"), "%Y-%m-%d %H:%M")
        .or_else(|_| {
            NaiveDate::parse_from_str(&day, "%Y-%m-%d").map(|date| {
                date.and_hms_opt(23, 59, 0).unwrap_or_else(|| date.and_hms_opt(0, 0, 0).unwrap())
            })
        })
        .ok()?;
    let start = due_at - Duration::minutes(LEAD_MINUTES);
    let course = item.get("course").and_then(|value| value.as_str()).unwrap_or("");
    let name = item
        .get("title")
        .or_else(|| item.get("name"))
        .and_then(|value| value.as_str())
        .unwrap_or("作业");
    let title = if course.is_empty() {
        format!("{TITLE_PREFIX}{name}")
    } else {
        format!("{TITLE_PREFIX}{course} · {name}")
    };
    let description = format!(
        "课程：{course}\n截止：{when}\n日程块：截止前 {LEAD_MINUTES} 分钟（{}–{}）\n（由 eLearning 的 dash 自动写入）",
        start.format("%H:%M"),
        due_at.format("%H:%M")
    );
    Some(json!({
        "title": title,
        "date": start.format("%Y-%m-%d").to_string(),
        "start_time": start.format("%H:%M").to_string(),
        "end_time": due_at.format("%H:%M").to_string(),
        "description": description,
        "remind_minutes": 0,
        "color": "rose",
        "due": when,
    }))
}

fn load_map(store: &Store) -> serde_json::Map<String, Value> {
    let raw = store.setting(MAP_KEY).unwrap_or_default();
    if raw.trim().is_empty() {
        return serde_json::Map::new();
    }
    match serde_json::from_str::<Value>(&raw) {
        Ok(Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    }
}

fn save_map(store: &Store, map: &serde_json::Map<String, Value>) {
    let _ = store.set_setting(MAP_KEY, &Value::Object(map.clone()).to_string());
}

fn dedupe_key(item: &Value) -> String {
    if let Some(url) = item.get("url").and_then(|value| value.as_str()) {
        if !url.trim().is_empty() {
            return url.trim().to_string();
        }
    }
    let course = item.get("course").and_then(|value| value.as_str()).unwrap_or("");
    let title = item
        .get("title")
        .or_else(|| item.get("name"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let due = item.get("due").and_then(|value| value.as_str()).unwrap_or("");
    format!("{course}|{title}|{due}")
}

/// 把截止时间写进日程：返回 {ok, added, updated, skipped, error}
pub fn write_deadlines(store: &Store, force: bool) -> Value {
    if !force && store.setting("elearning_to_calendar").as_deref() == Some("0") {
        return json!({ "ok": false, "added": 0, "updated": 0, "skipped": 0,
                       "error": "自动写入关着（作业页的「每天自动」没勾）" });
    }
    if !force {
        let fresh = store.setting(VERSION_KEY).as_deref() == Some(VERSION);
        let last = store
            .setting("elearning_event_sync_at")
            .and_then(|text| text.parse::<u64>().ok())
            .unwrap_or(0);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_secs())
            .unwrap_or(0);
        if fresh && now.saturating_sub(last) < 20 * 3600 {
            return json!({ "ok": false, "added": 0, "updated": 0, "skipped": 0,
                           "error": "今天已经同步过了" });
        }
    }
    let items = cached_items(store);
    let mut map = load_map(store);
    let mut added = 0;
    let mut updated = 0;
    let mut skipped = 0;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    for item in &items {
        let Some(fields) = event_fields(item) else {
            skipped += 1;
            continue;
        };
        let due_day = fields
            .get("due")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .get(0..10)
            .unwrap_or("")
            .to_string();
        if due_day < today {
            skipped += 1;
            continue;
        }
        let key = dedupe_key(item);
        let known = map.get(&key).and_then(|value| value.as_i64());

        // 已经有这条日程：只在时间范围变了的时候改
        if let Some(id) = known {
            if let Some(current) = store.get_event(id).ok().flatten() {
                let want_start = fields["start_time"].as_str().unwrap_or("");
                let want_end = fields["end_time"].as_str().unwrap_or("");
                if current.start_time != want_start || current.end_time != want_end {
                    let patch = EventInput {
                        date: Some(fields["date"].as_str().unwrap_or("").to_string()),
                        start_time: Some(want_start.to_string()),
                        end_time: Some(want_end.to_string()),
                        description: fields["description"].as_str().map(|text| text.to_string()),
                        remind_minutes: Some(0),
                        ..Default::default()
                    };
                    let _ = store.update_event(id, &patch);
                    updated += 1;
                } else {
                    skipped += 1;
                }
                continue;
            }
        }

        // 映射丢了：靠「标题 + 日期」认一遍，认出来就更新
        let title = fields["title"].as_str().unwrap_or("");
        let date = fields["date"].as_str().unwrap_or("");
        let existing = store
            .events_on(date)
            .unwrap_or_default()
            .into_iter()
            .find(|row| row.title == title);
        if let Some(row) = existing {
            let want_start = fields["start_time"].as_str().unwrap_or("");
            if row.start_time != want_start {
                let patch = EventInput {
                    start_time: Some(want_start.to_string()),
                    end_time: fields["end_time"].as_str().map(|text| text.to_string()),
                    remind_minutes: Some(0),
                    description: fields["description"].as_str().map(|text| text.to_string()),
                    ..Default::default()
                };
                let _ = store.update_event(row.id, &patch);
                updated += 1;
            } else {
                skipped += 1;
            }
            map.insert(key, json!(row.id));
            continue;
        }

        let input = EventInput {
            title: Some(title.to_string()),
            description: fields["description"].as_str().map(|text| text.to_string()),
            date: Some(date.to_string()),
            start_time: Some(fields["start_time"].as_str().unwrap_or("09:00").to_string()),
            end_time: Some(fields["end_time"].as_str().unwrap_or("10:00").to_string()),
            remind_minutes: Some(0),
            clear_remind: false,
            color: Some("rose".to_string()),
            done: None,
            repeat: None,
        };
        match store.add_event(&input) {
            Ok(id) => {
                map.insert(key, json!(id));
                added += 1;
            }
            Err(_) => skipped += 1,
        }
    }
    save_map(store, &map);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0);
    let _ = store.set_setting("elearning_event_sync_at", &now.to_string());
    let _ = store.set_setting(VERSION_KEY, VERSION);
    json!({ "ok": true, "added": added, "updated": updated, "skipped": skipped,
            "error": "", "total": items.len() })
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens() {
        assert_eq!(number_tokens("第八周作业"), vec!["8".to_string()]);
        assert_eq!(
            number_tokens("HW8.2"),
            vec!["8".to_string(), "2".to_string()]
        );
    }

    /// 真数据用例（默认跳过）：数据来自临时副本，见 `crate::testdata`。
    #[test]
    #[ignore]
    fn homework_from_real_db() {
        let store = crate::testdata::real_store("elearn");
        let items = cached_items(&store);
        println!("缓存里的作业：{} 条", items.len());
        for item in &items {
            let files = local_files(&store, item);
            println!(
                "  {} / {} → {:?}",
                item.get("course").and_then(|value| value.as_str()).unwrap_or("?"),
                item.get("title").and_then(|value| value.as_str()).unwrap_or("?"),
                files
            );
        }
        assert!(!items.is_empty(), "缓存里应该有作业");
        let fields = event_fields(&items[0]).expect("第一条应该有截止时间");
        println!("对应日程字段：{}", fields);
    }
}
