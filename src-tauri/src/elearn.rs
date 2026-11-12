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


// ---------------------------------------------------------------- 截止时间面板 + 当天提醒
//
// 用户 2026-11-12：eLearning 的 `/dash` 上能拿到作业截止时间，
// ① 在日历页右侧列一张「作业截止」卡；② 到期当天早上九点来钟提醒一次。

/// 提醒总开关（"0" = 关）
const REMIND_SWITCH_KEY: &str = "elearn_remind";
/// 提前多少分钟开始提醒（用户 2026-11-12：改成「到期前 24 小时」）
const REMIND_AHEAD_MINUTES: i64 = 24 * 60;
/// 已经提醒过的记录：{"keys": ["课程|标题|2026-11-12 23:59", ...]}
///
/// 不按天清空：截止在 23:59 的作业，24 小时窗口会跨过半夜，
/// 按天清空的话第二天会再提一次（用户会被同一条烦两次）。
const REMIND_SENT_KEY: &str = "elearn_remind_sent";

/// 「due」字段 → (哪一天, 几点)，解析不了就 None
fn due_parts(item: &Value) -> Option<(NaiveDate, String)> {
    let due = item
        .get("due")
        .or_else(|| item.get("deadline"))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .replace('T', " ");
    if due.is_empty() {
        return None;
    }
    let day = due.get(0..10).and_then(|text| NaiveDate::parse_from_str(text, "%Y-%m-%d").ok())?;
    let clock = due
        .get(11..16)
        .map(|text| text.to_string())
        .filter(|text| text.contains(':'))
        .unwrap_or_else(|| "23:59".to_string());
    Some((day, clock))
}

fn item_title(item: &Value) -> String {
    item.get("title")
        .or_else(|| item.get("name"))
        .and_then(|value| value.as_str())
        .unwrap_or("作业")
        .trim()
        .to_string()
}

fn item_course(item: &Value) -> String {
    item.get("course")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

/// 未来 `days` 天（含今天）内的作业截止，按截止时间排序
///
/// 日历页右侧那张卡用它。`days_left` = 还差几天（0 = 今天），`overdue` = 已经过了截止。
pub fn upcoming(store: &Store, days: i64) -> Value {
    let today = chrono::Local::now().date_naive();
    let horizon = today + Duration::days(days.clamp(1, 180));
    let mut rows: Vec<Value> = Vec::new();
    for item in cached_items(store) {
        let Some((day, clock)) = due_parts(&item) else {
            continue;
        };
        if day > horizon {
            continue;
        }
        let days_left = (day - today).num_days();
        let minutes = minutes_left(day, &clock);
        // 太早过去的（超过 7 天）就不列了，卡里只放「还有用」的
        if days_left < -7 {
            continue;
        }
        rows.push(json!({
            "title": item_title(&item),
            "course": item_course(&item),
            "due": format!("{day} {clock}"),
            "day": day.format("%Y-%m-%d").to_string(),
            "clock": clock,
            "days_left": days_left,
            "minutes_left": minutes,
            "left_text": left_text(minutes),
            "overdue": minutes < 0,
            "url": item.get("url").and_then(|value| value.as_str()).unwrap_or(""),
            "kind": item.get("kind").and_then(|value| value.as_str()).unwrap_or(""),
        }));
    }
    rows.sort_by(|a, b| {
        let left = a.get("due").and_then(|one| one.as_str()).unwrap_or("");
        let right = b.get("due").and_then(|one| one.as_str()).unwrap_or("");
        left.cmp(right)
    });
    let total = rows.len();
    let sync_at = store.setting("homework_assignments_at").unwrap_or_default();
    json!({ "ok": true, "items": rows, "total": total, "synced_at": sync_at })
}

/// 距离截止还有多少分钟（负数 = 已经过了）
fn minutes_left(day: NaiveDate, clock: &str) -> i64 {
    let mut parts = clock.split(':');
    let hour: u32 = parts.next().and_then(|one| one.trim().parse().ok()).unwrap_or(23);
    let minute: u32 = parts.next().and_then(|one| one.trim().parse().ok()).unwrap_or(59);
    let Some(due) = day.and_hms_opt(hour.min(23), minute.min(59), 0) else {
        return 0;
    };
    (due - chrono::Local::now().naive_local()).num_minutes()
}

/// 「还有多久」说成一句话：<1 小时给分钟，<24 小时给小时，再远就给天
///
/// 用户 2026-11-12 的原话：「显示还有多久（少于 24 小时显示还有几小时）和截止时间」。
pub fn left_text(minutes: i64) -> String {
    if minutes < 0 {
        let passed = -minutes;
        if passed < 60 {
            return format!("已过 {passed} 分钟");
        }
        if passed < 24 * 60 {
            return format!("已过 {} 小时", passed / 60);
        }
        return format!("已过 {} 天", passed / (24 * 60));
    }
    if minutes < 60 {
        return format!("还有 {minutes} 分钟");
    }
    if minutes < 24 * 60 {
        return format!("还有 {} 小时", minutes / 60);
    }
    let days = minutes / (24 * 60);
    let hours = (minutes % (24 * 60)) / 60;
    if hours == 0 {
        format!("还有 {days} 天")
    } else {
        format!("还有 {days} 天 {hours} 小时")
    }
}

fn sent_keys(store: &Store) -> Vec<String> {
    let raw = store.setting(REMIND_SENT_KEY).unwrap_or_default();
    let Ok(stored) = serde_json::from_str::<Value>(&raw) else {
        return Vec::new();
    };
    stored
        .get("keys")
        .and_then(|one| one.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|one| one.as_str().map(|text| text.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn save_sent_keys(store: &Store, keys: &[String]) {
    // 只留最近的 60 条，免得设置越存越大
    let tail: Vec<String> = keys.iter().rev().take(60).rev().cloned().collect();
    let payload = json!({ "keys": tail });
    let _ = store.set_setting(REMIND_SENT_KEY, &payload.to_string());
}

/// 快到截止时间的作业（进入「往前 24 小时」的窗口就提醒一次）
///
/// 判断放在后端：① 系统通知也是后端发的；② 窗口缩到托盘时前端可能没在跑。
/// 已经过期的、已经提醒过的、还早着的（>24 小时）都不返回。
pub fn due_reminders(store: &Store, mark: bool) -> Vec<Value> {
    if store.setting(REMIND_SWITCH_KEY).as_deref() == Some("0") {
        return Vec::new();
    }
    let mut sent = sent_keys(store);
    let mut out: Vec<Value> = Vec::new();
    for item in cached_items(store) {
        let Some((day, clock)) = due_parts(&item) else {
            continue;
        };
        let minutes = minutes_left(day, &clock);
        if minutes <= 0 || minutes > REMIND_AHEAD_MINUTES {
            continue;
        }
        let due_text = format!("{day} {clock}");
        let key = format!("{}|{}|{}", item_course(&item), item_title(&item), due_text);
        if sent.contains(&key) {
            continue;
        }
        let left = left_text(minutes);
        let deadline = format!("{} {} 截止", day.format("%m-%d"), clock);
        out.push(json!({
            "key": key,
            "title": item_title(&item),
            "course": item_course(&item),
            "clock": clock,
            "due": due_text,
            "minutes_left": minutes,
            "left_text": left,
            "url": item.get("url").and_then(|value| value.as_str()).unwrap_or(""),
            "text": if item_course(&item).is_empty() {
                format!("{} {left}（{deadline}）", item_title(&item))
            } else {
                format!("{} · {} {left}（{deadline}）", item_course(&item), item_title(&item))
            },
        }));
        sent.push(key);
    }
    if mark && !out.is_empty() {
        save_sent_keys(store, &sent);
    }
    out
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

    /// 临时数据目录（`LUMINA_DATA_DIR` 是进程级的，用完必须还原）
    struct Temp(Option<std::ffi::OsString>, std::path::PathBuf);

    impl Temp {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "danlume-elearn-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let previous = std::env::var_os("LUMINA_DATA_DIR");
            std::env::set_var("LUMINA_DATA_DIR", &dir);
            Self(previous, dir)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            match self.0.take() {
                Some(value) => std::env::set_var("LUMINA_DATA_DIR", value),
                None => std::env::remove_var("LUMINA_DATA_DIR"),
            }
            let _ = std::fs::remove_dir_all(&self.1);
        }
    }

    fn seed(store: &Store, items: Value) {
        store
            .set_setting("homework_assignments", &items.to_string())
            .expect("写作业缓存");
    }

    /// 面板：只列未来 N 天、按截止时间排序，算好「还差几天」
    #[test]
    fn upcoming_lists_nearest_first_with_days_left() {
        let _temp = Temp::new("upcoming");
        let store = Store::open().expect("临时库");
        let today = chrono::Local::now().date_naive();
        let tomorrow = today + Duration::days(1);
        let far = today + Duration::days(40);
        let old = today - Duration::days(30);
        seed(
            &store,
            json!([
                { "course": "线性代数", "title": "第三章习题", "due": format!("{far} 23:59") },
                { "course": "程序设计", "title": "实验四", "due": format!("{tomorrow} 10:00") },
                { "course": "旧课", "title": "很久以前", "due": format!("{old} 23:59") },
                { "course": "没截止", "title": "不列出来" }
            ]),
        );
        let result = upcoming(&store, 14);
        let items = result["items"].as_array().unwrap();
        assert_eq!(items.len(), 1, "40 天后和 30 天前的不该出现：{result}");
        assert_eq!(items[0]["title"], json!("实验四"));
        assert_eq!(items[0]["days_left"], json!(1));
        assert_eq!(items[0]["clock"], json!("10:00"));
        assert_eq!(items[0]["overdue"], json!(false));
        // 放宽到 60 天，远的也进来，但顺序还是按截止时间
        let wide = upcoming(&store, 60);
        let titles: Vec<String> = wide["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|one| one["title"].as_str().unwrap_or("").to_string())
            .collect();
        assert_eq!(titles, vec!["实验四".to_string(), "第三章习题".to_string()]);
    }

    /// 「还有多久」的说法：<1 小时给分钟，<24 小时给小时，再远给天
    #[test]
    fn left_text_wording() {
        assert_eq!(left_text(25), "还有 25 分钟");
        assert_eq!(left_text(60), "还有 1 小时");
        assert_eq!(left_text(5 * 60 + 59), "还有 5 小时");
        assert_eq!(left_text(24 * 60), "还有 1 天");
        assert_eq!(left_text(50 * 60), "还有 2 天 2 小时");
        assert_eq!(left_text(-30), "已过 30 分钟");
        assert_eq!(left_text(-3 * 60), "已过 3 小时");
        assert_eq!(left_text(-50 * 60), "已过 2 天");
    }

    /// 提醒：进入「截止前 24 小时」才提，同一份作业只提一次（跨过半夜也不重复）
    #[test]
    fn due_reminders_fire_within_24h_once() {
        let _temp = Temp::new("remind");
        let store = Store::open().expect("临时库");
        let now = chrono::Local::now().naive_local();
        let within_24h = now + Duration::hours(5);
        let far_away = now + Duration::days(3);
        let passed = now - Duration::hours(2);
        seed(
            &store,
            json!([
                { "course": "程序设计", "title": "实验四",
                  "due": within_24h.format("%Y-%m-%d %H:%M").to_string(), "url": "https://example.org/a" },
                { "course": "线性代数", "title": "第三章",
                  "due": far_away.format("%Y-%m-%d %H:%M").to_string() },
                { "course": "旧课", "title": "已经过期",
                  "due": passed.format("%Y-%m-%d %H:%M").to_string() }
            ]),
        );
        let first = due_reminders(&store, true);
        assert_eq!(first.len(), 1, "只有 24 小时内那条：{first:?}");
        assert_eq!(first[0]["title"], json!("实验四"));
        assert!(first[0]["text"].as_str().unwrap().contains("还有"));
        assert!(first[0]["minutes_left"].as_i64().unwrap() > 0);
        // 标过之后不再重复（哪怕过了半夜、换了日期）
        assert!(due_reminders(&store, true).is_empty());
        assert!(due_reminders(&store, false).is_empty());
        // 开关关掉就彻底不提醒
        store.set_setting("elearn_remind", "0").expect("关开关");
        store.set_setting(REMIND_SENT_KEY, "").expect("清记录");
        assert!(due_reminders(&store, true).is_empty());
        // 打开后又能提醒（记录被清了）
        store.set_setting("elearn_remind", "1").expect("开开关");
        assert_eq!(due_reminders(&store, false).len(), 1);
    }

    /// 面板里也要有「还有多久」和截止时间（用户 2026-11-12 的要求）
    #[test]
    fn upcoming_carries_left_text_and_deadline() {
        let _temp = Temp::new("left");
        let store = Store::open().expect("临时库");
        let now = chrono::Local::now().naive_local();
        let soon = now + Duration::hours(3);
        let later = now + Duration::days(2);
        seed(
            &store,
            json!([
                { "course": "程序设计", "title": "实验四", "due": soon.format("%Y-%m-%d %H:%M").to_string() },
                { "course": "线性代数", "title": "第三章", "due": later.format("%Y-%m-%d %H:%M").to_string() }
            ]),
        );
        let result = upcoming(&store, 14);
        let items = result["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        // 排序：近的在前
        assert_eq!(items[0]["title"], json!("实验四"));
        let left = items[0]["left_text"].as_str().unwrap();
        assert!(left.starts_with("还有") && left.contains("小时"), "实际：{left}");
        assert_eq!(items[0]["clock"], json!(soon.format("%H:%M").to_string()));
        assert!(items[1]["left_text"].as_str().unwrap().contains("天"));
        assert!(items[0]["minutes_left"].as_i64().unwrap() < 24 * 60);
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
        println!("现在（Rust 视角）：{}", chrono::Local::now().naive_local());
        println!("utc：{}", chrono::Utc::now().naive_local());
        println!("面板 upcoming：{}", upcoming(&store, 14));
        println!("提醒 due_reminders：{}", serde_json::to_string(&due_reminders(&store, false)).unwrap_or_default());
    }
}
