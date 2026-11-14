//! 教务课表抓取（对应旧版 `fdu.py`）。走「第二跳」：ID 会话 → 教务服务会话 → print-data。
//!
//! 三步：
//! 1. `uis::visit_with_session(COURSE_TABLE)` 用抓来的会话落地到课表页；
//! 2. 从落地地址（或课表页的学期下拉）**自动认出数字学期号**，例如 `semester/527`；
//! 3. 用同一份会话 GET `semester/<id>/print-data`，把返回的 JSON 解析成课程片段。
//!
//! 解析规则与旧版 `fdu.parse_print_data` 逐条对齐（见下方单测）。
use serde_json::{json, Value};

use crate::store::Store;
use crate::uis;

pub const COURSE_TABLE: &str = "https://fdjwgl.fudan.edu.cn/student/for-std/course-table";
pub const PRINT_DATA: &str =
    "https://fdjwgl.fudan.edu.cn/student/for-std/course-table/semester/{sem}/print-data";

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                  (KHTML, like Gecko) Chrome/120.0 Safari/537.36";

/// 一次 print-data 抓取的结果
pub struct PrintData {
    pub url: String,
    pub body: String,
    /// 这次实际用的学期号（用户没填时是从页面里认出来的）
    pub semester: String,
    /// 学期号是推断出来的吗（界面据此提示「已自动识别」）
    pub sniffed: bool,
}

fn agent(url: &str) -> ureq::Agent {
    crate::net::with_proxy(
        ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(20))
            .redirects(5)
            .user_agent(UA)
,
        crate::get_store(),
        url,
    )
    .build()
}

/// 学期号是不是纯数字（教务用数字 id，例如 527）
pub fn is_numeric_semester(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty() && trimmed.chars().all(|ch| ch.is_ascii_digit())
}

/// 从 URL 或页面 HTML 里认出学期号：
/// ① `…/semester/527/print-data`；② 学期下拉 `<option value="527">2026-2027 第一学期</option>`。
pub fn sniff_semester(text: &str) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    // ① URL 形式
    let needle = "semester/";
    let mut rest = text;
    while let Some(pos) = rest.find(needle) {
        let after = &rest[pos + needle.len()..];
        let digits: String = after.chars().take_while(|ch| ch.is_ascii_digit()).collect();
        if !digits.is_empty() {
            return Some(digits);
        }
        rest = after;
    }
    // ② 学期下拉（有 value 且后面跟着中文名才算，避免误认其它纯数字 value）
    for chunk in text.split("<option").skip(1) {
        let Some(value_at) = chunk.find("value=\"") else {
            continue;
        };
        let after = &chunk[value_at + "value=\"".len()..];
        let digits: String = after.chars().take_while(|ch| ch.is_ascii_digit()).collect();
        if digits.is_empty() {
            continue;
        }
        let Some(close) = after.find('>') else {
            continue;
        };
        let label: String = after[close + 1..]
            .chars()
            .take_while(|ch| *ch != '<')
            .collect();
        if label.trim().is_empty() {
            continue;
        }
        return Some(digits);
    }
    None
}

/// 先请求一次 course-table 建立服务会话；再取 print-data 原文。
///
/// `semester` 允许留空：留空时从落地地址或课表页的学期下拉里自动认。
pub fn fetch_print_data(store: &Store, semester: &str) -> Result<PrintData, String> {
    let (final_url, jar) = uis::visit_with_session(store, COURSE_TABLE)?;
    if final_url.contains("id.fudan.edu.cn") {
        return Err(format!(
            "会话对教务无效（被重定向到统一身份认证）：请先在「打开登录窗口」里登录一次，再点「抓取会话」。当前落地地址：{final_url}"
        ));
    }
    let cookie = jar.to_header();
    let given = semester.trim();
    let (sem, sniffed) = if is_numeric_semester(given) {
        (given.to_string(), false)
    } else {
        match resolve_semester(&final_url, &cookie) {
            Some(found) => (found, true),
            None => {
                return Err(format!(
                    "认不出学期号：可以在设置里手填（教务课表页 URL 里 semester/<数字> 那一串）。当前落地地址：{final_url}"
                ))
            }
        }
    };
    let url = PRINT_DATA.replace("{sem}", &sem);
    let response = agent(&url)
        .get(&url)
        .set("Cookie", &cookie)
        .call()
        .map_err(|exc| format!("请求课表失败：{exc}"))?;
    let body = response
        .into_string()
        .map_err(|exc| format!("课表内容读不出来：{exc}"))?;
    Ok(PrintData {
        url,
        body,
        semester: sem,
        sniffed,
    })
}

/// 认学期号：先看落地地址，再抓一次课表页看学期下拉。
fn resolve_semester(final_url: &str, cookie: &str) -> Option<String> {
    if let Some(found) = sniff_semester(final_url) {
        return Some(found);
    }
    let body = agent(COURSE_TABLE)
        .get(COURSE_TABLE)
        .set("Cookie", cookie)
        .call()
        .ok()
        .and_then(|response| response.into_string().ok())
        .unwrap_or_default();
    sniff_semester(&body)
}

/// 诊断：不打日志、不写库，只回「这跳有没有过、返回什么形状」
pub fn probe(store: &Store, semester: &str) -> Value {
    match fetch_print_data(store, semester) {
        Ok(data) => probe_body(&data.body, &data.url, &data.semester, data.sniffed),
        Err(exc) => json!({ "ok": false, "error": exc }),
    }
}

/// 学期第一周周一是不是「含当天在内的下一个周一」（与旧版 `fdu.first_monday_from` 一致）
pub fn first_monday_from(first_day: &str) -> Option<String> {
    use chrono::Datelike;
    let day = chrono::NaiveDate::parse_from_str(first_day.trim(), "%Y-%m-%d").ok()?;
    let offset = (7 - day.weekday().num_days_from_monday() as i64) % 7;
    Some(
        (day + chrono::Duration::days(offset))
            .format("%Y-%m-%d")
            .to_string(),
    )
}

/// 把 `2026-9-6` / `2026-09-06` 这类写法统一成 `YYYY-MM-DD`
fn normalize_ymd(raw: &str) -> Option<String> {
    if !raw.chars().all(|ch| ch.is_ascii_digit() || ch == '-') {
        return None;
    }
    let mut parts = raw.split('-');
    let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    if year.len() != 4 || month.is_empty() || month.len() > 2 || day.is_empty() || day.len() > 2 {
        return None;
    }
    let month: u32 = month.parse().ok()?;
    let day: u32 = day.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(format!("{year}-{month:02}-{day:02}"))
}

/// 从课表页正文里读「学年起始日期：YYYY-MM-DD」。
///
/// 实测（2026-11-10）教务页面上的标签是「**学期**起始日期:  2026-09-06」；
/// 旧版 `_PAGE_INFO_JS` 认的是「学年起始日期」。两个都认，避免再改版时又落空。
pub fn sniff_term_start(text: &str) -> Option<String> {
    for needle in ["学年起始日期", "学期起始日期"] {
        let mut rest = text;
        while let Some(pos) = rest.find(needle) {
            let after = &rest[pos + needle.len()..];
            // 跳过「：」「:」和空白
            let trimmed = after
                .trim_start_matches(|ch: char| ch == ':' || ch == '：' || ch.is_whitespace());
            // 日期本体：数字和连字符，最多 10 个字符
            let raw: String = trimmed
                .chars()
                .take_while(|ch| ch.is_ascii_digit() || *ch == '-')
                .take(10)
                .collect();
            if let Some(iso) = normalize_ymd(&raw) {
                return Some(iso);
            }
            rest = after;
        }
    }
    None
}

/// 把 print-data 的原文变成诊断用的 JSON（网页里现抓和 ureq 抓共用这一份）
pub fn probe_body(body: &str, url: &str, semester: &str, sniffed: bool) -> Value {
    probe_body_with(body, url, semester, sniffed, "", "")
}

/// 同上，但带上「从教务页面读到的学年起始日期」和换算出来的第一周周一
pub fn probe_body_with(
    body: &str,
    url: &str,
    semester: &str,
    sniffed: bool,
    term_start_raw: &str,
    first_monday: &str,
) -> Value {
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
    let keys: Vec<String> = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .as_object()
                .map(|map| map.keys().cloned().collect::<Vec<_>>())
        })
        .unwrap_or_default();
    // 顺手试解析一次（不写库）：诊断里直接告诉用户「能解析出几个课表格子」，
    // 比只看字节数有用（对齐旧版 diag 的行为）
    let (slots, sample) = match parse_print_data(body) {
        Ok(rows) => {
            let sample: Vec<String> = rows
                .iter()
                .take(3)
                .map(|row| row.title.clone())
                .filter(|one| !one.is_empty())
                .collect();
            (rows.len(), sample)
        }
        Err(_) => (0, Vec::new()),
    };
    json!({
        "ok": true,
        "url": url,
        "semester": semester,
        "semester_sniffed": sniffed,
        "shape": shape,
        "bytes": body.len(),
        "top_keys": keys,
        "slots": slots,
        "sample_titles": sample,
        "term_start": term_start_raw,
        "first_monday": first_monday,
        "head": head,
    })
}



// ------------------------------------------------------------------ 解析（对齐旧版 fdu.parse_print_data）
/// 默认节次时间：与旧版 `fdu.DEFAULT_UNIT_TIMES` **逐条一致**（14 节）。
///
/// 之前这里是 12 节、而且第 3/4/5 节少 5 分钟、晚上只到 21:15 之前——导入的课就有两处错位：
/// 上午的课整体后移 5 分钟、晚上第 13/14 节的课被塞进第 1 节（找不到节次就退回 08:00）。
pub(crate) fn default_unit_times() -> Vec<(i64, String, String)> {
    vec![
        (1, "08:00".to_string(), "08:45".to_string()),
        (2, "08:55".to_string(), "09:40".to_string()),
        (3, "09:55".to_string(), "10:40".to_string()),
        (4, "10:50".to_string(), "11:35".to_string()),
        (5, "11:45".to_string(), "12:30".to_string()),
        (6, "13:30".to_string(), "14:15".to_string()),
        (7, "14:25".to_string(), "15:10".to_string()),
        (8, "15:25".to_string(), "16:10".to_string()),
        (9, "16:20".to_string(), "17:05".to_string()),
        (10, "17:15".to_string(), "18:00".to_string()),
        (11, "18:30".to_string(), "19:15".to_string()),
        (12, "19:25".to_string(), "20:10".to_string()),
        (13, "20:20".to_string(), "21:05".to_string()),
        (14, "21:15".to_string(), "22:00".to_string()),
    ]
}

fn pick<'a>(node: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    let map = node.as_object()?;
    for key in keys {
        if let Some(found) = map.get(*key) {
            // 旧版 `_first` 把空字符串也当成「没给」
            let empty_text = found.as_str().map(|text| text.is_empty()).unwrap_or(false);
            if !found.is_null() && !empty_text {
                return Some(found);
            }
        }
    }
    None
}

/// 旧版 `_as_int_list`：列表逐项转整数（转不了跳过）、单个数字、字符串里的所有数字段
fn as_int_list(value: Option<&Value>) -> Vec<i64> {
    match value {
        Some(Value::Array(rows)) => rows.iter().filter_map(int_of).collect(),
        Some(Value::String(text)) => digit_runs(text),
        Some(other) => int_of(other).into_iter().collect(),
        None => Vec::new(),
    }
}

fn int_of(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.trim().parse::<i64>().ok()))
}

/// 等价于 Python 的 `re.findall(r"\d+")`："1-16" → [1, 16]，"2,4,6" → [2, 4, 6]
fn digit_runs(text: &str) -> Vec<i64> {
    let mut out = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            current.push(ch);
        } else if !current.is_empty() {
            if let Ok(value) = current.parse::<i64>() {
                out.push(value);
            }
            current.clear();
        }
    }
    if !current.is_empty() {
        if let Ok(value) = current.parse::<i64>() {
            out.push(value);
        }
    }
    out
}

/// 「像一节活动」的节点：含任一提示键，且 `weekIndexes` 不是对象/字符串（旧版 `_walk` 的筛法）
const ACTIVITY_HINTS: [&str; 4] = ["weekday", "startUnit", "endUnit", "weekIndexes"];

fn looks_like_activity(node: &Value) -> bool {
    let Some(map) = node.as_object() else {
        return false;
    };
    if !ACTIVITY_HINTS.iter().any(|key| map.contains_key(*key)) {
        return false;
    }
    !matches!(
        map.get("weekIndexes"),
        Some(Value::Object(_)) | Some(Value::String(_))
    )
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
            if looks_like_activity(value) {
                out.push(value);
            }
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
#[derive(Debug)]
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
    let data: Value =
        serde_json::from_str(body).map_err(|exc| format!("课表数据不是合法 JSON：{exc}"))?;
    let mut nodes: Vec<&Value> = Vec::new();
    walk_nodes(&data, &mut nodes);
    let mut out: Vec<Slot> = Vec::new();
    // 旧版按「标题 + 周几 + 起止节 + 排序后的周次」去重
    let mut seen: std::collections::HashSet<(String, i64, i64, i64, Vec<i64>)> =
        std::collections::HashSet::new();
    for node in nodes {
        let Some(weekday) = pick_int(node, &["weekday", "dayOfWeek", "weekDay", "weekdayIndex"]) else {
            continue;
        };
        if !(1..=7).contains(&weekday) {
            continue;
        }
        let start_unit = pick_int(node, &["startUnit", "startUnitIndex", "start"]).unwrap_or(0);
        let end_unit = pick_int(node, &["endUnit", "endUnitIndex", "end"]).unwrap_or(start_unit);
        let title = pick_text(node, &["courseName", "name", "lessonName"])
            .trim()
            .to_string();
        if title.is_empty() {
            continue;
        }
        let mut weeks = as_int_list(pick(node, &["weekIndexes", "weekIndices", "weeks"]));
        if weeks.is_empty() {
            // 旧版：没给周次就当整学期 1–16 周
            weeks = (1..=16).collect();
        }
        let room = pick_text(node, &["room", "place", "classroom", "lessonPlace"])
            .trim()
            .to_string();
        let teachers = pick_text(node, &["teachers", "teacherName", "teacher"])
            .trim()
            .to_string();
        let mut sorted = weeks.clone();
        sorted.sort_unstable();
        if !seen.insert((title.clone(), weekday, start_unit, end_unit, sorted)) {
            continue;
        }
        out.push(Slot {
            weekday,
            start_unit,
            end_unit,
            weeks,
            title,
            room,
            teachers,
        });
    }
    Ok(out)
}

/// 解析用户填的日期：与旧版 `_first_monday` 一样接受三种写法
pub fn parse_day(text: &str) -> Option<chrono::NaiveDate> {
    let trimmed = text.trim();
    for fmt in ["%Y-%m-%d", "%Y/%m/%d", "%Y%m%d"] {
        if let Ok(day) = chrono::NaiveDate::parse_from_str(trimmed, fmt) {
            return Some(day);
        }
    }
    None
}

/// 某一天所在周的周一（旧版 `_monday_of`）
pub fn monday_of(day: chrono::NaiveDate) -> chrono::NaiveDate {
    use chrono::Datelike;
    day - chrono::Duration::days(day.weekday().num_days_from_monday() as i64)
}

/// 第一周周一从哪来（对齐旧版 `_first_monday` + `_on_fdu_data`，**永远给得出一个值**）：
/// ① 用户填的（三种写法）→ ② 教务页面上的起始日期换算成周一 → ③ 设置里上次存下的 →
/// ④ 今天所在周的周一。旧版就是这样：没有起始日期也不报错，只是日期可能不对。
pub fn resolve_term_start(given: &str, page_raw: &str, saved: &str) -> String {
    if let Some(day) = parse_day(given) {
        return day.format("%Y-%m-%d").to_string();
    }
    // 页面上可能是整段正文（带「学期起始日期:」），也可能已经是纯日期
    let page_day = sniff_term_start(page_raw).or_else(|| {
        let text = page_raw.trim();
        if text.is_empty() {
            None
        } else {
            Some(text.to_string())
        }
    });
    if let Some(raw) = page_day {
        if let Some(monday) = first_monday_from(&raw) {
            return monday;
        }
    }
    if let Some(day) = parse_day(saved) {
        return day.format("%Y-%m-%d").to_string();
    }
    monday_of(chrono::Local::now().date_naive())
        .format("%Y-%m-%d")
        .to_string()
}

/// 一节课的起止：优先用教务给的节次时间，没有就退回默认作息（旧版 `_unit_range`）
fn unit_range(
    times: &[(i64, String, String)],
    start_unit: i64,
    end_unit: i64,
) -> (String, String) {
    let start = times.iter().find(|(unit, _, _)| *unit == start_unit);
    let end = times
        .iter()
        .find(|(unit, _, _)| *unit == end_unit)
        .or(start);
    match (start, end) {
        (Some((_, begin, _)), Some((_, _, finish))) => (begin.clone(), finish.clone()),
        // 旧版这里是写死的 08:00–09:40
        _ => ("08:00".to_string(), "09:40".to_string()),
    }
}

/// 「课程表」分组：有就复用，没有就建一个（旧版 `ensure_course_group`）
fn ensure_course_group(store: &Store, color: &str) -> Option<i64> {
    let conn = store.connection();
    let found: Option<i64> = conn
        .query_row(
            "SELECT id FROM event_groups WHERE name = ?1 LIMIT 1",
            ["课程表"],
            |row| row.get(0),
        )
        .ok();
    if found.is_some() {
        return found;
    }
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "INSERT INTO event_groups (name, color, created_at) VALUES (?1, ?2, ?3)",
        ( "课程表", color, stamp ),
    )
    .ok()?;
    Some(conn.last_insert_rowid())
}

/// 把课程片段写进 events（按「日期+开始时间+标题」去重），返回 (新增, 跳过)
pub fn import_slots(store: &Store, slots: &[Slot], term_start: &str) -> Result<(usize, usize), String> {
    // 与旧版一致：用户填的日期看不懂就用今天所在周的周一，不报错
    let monday = parse_day(term_start)
        .map(monday_of)
        .unwrap_or_else(|| monday_of(chrono::Local::now().date_naive()));
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
    let group_id = ensure_course_group(store, "sky");
    let mut seen = existing;
    let mut added = 0usize;
    let mut skipped = 0usize;
    for slot in slots {
        let (start, end) = unit_range(&times, slot.start_unit, slot.end_unit);
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
            // 描述顺序与旧版 `to_rows` 一致：教师 → 地点
            let mut description = String::new();
            if !slot.teachers.is_empty() {
                description.push_str(&format!("教师：{}", slot.teachers));
            }
            if !slot.room.is_empty() {
                if !description.is_empty() {
                    description.push('\n');
                }
                description.push_str(&format!("地点：{}", slot.room));
            }
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
            match store.add_event(&input) {
                Ok(id) => {
                    if let Some(group) = group_id {
                        let _ = store.set_event_group(id, group);
                    }
                    added += 1;
                    seen.push(key);
                }
                Err(_) => skipped += 1,
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
        let store = crate::testdata::real_store("timetable_remote");
        let result = probe(&store, "");
        println!("诊断结果：{result}");
    }

    #[test]
    fn sniffs_semester_from_url_and_page() {
        assert_eq!(
            sniff_semester(
                "https://fdjwgl.fudan.edu.cn/student/for-std/course-table/semester/527/print-data"
            ),
            Some("527".to_string())
        );
        assert_eq!(
            sniff_semester("https://fdjwgl.fudan.edu.cn/student/for-std/course-table"),
            None
        );
        let page = r#"<select id="semester"><option value="">请选择</option>
            <option value="527" selected>2026-2027 第一学期</option>
            <option value="528">2026-2027 第二学期</option></select>"#;
        assert_eq!(sniff_semester(page), Some("527".to_string()));
        // 没有中文名的纯数字 value 不算（避免把别的下拉当成学期）
        assert_eq!(sniff_semester(r#"<option value="12"></option>"#), None);

        assert!(is_numeric_semester("527"));
        assert!(is_numeric_semester(" 527 "));
        assert!(!is_numeric_semester("2025-2026-1"));
        assert!(!is_numeric_semester(""));
    }

    #[test]
    fn weeks_parsing_matches_python() {
        assert_eq!(digit_runs("1-16"), vec![1, 16]);
        assert_eq!(digit_runs("2,4,6"), vec![2, 4, 6]);
        assert_eq!(digit_runs(" 3 5 7 "), vec![3, 5, 7]);
        assert_eq!(digit_runs("没有数字"), Vec::<i64>::new());
        let node = json!({ "weekIndexes": ["2", 3, "x", 5] });
        assert_eq!(as_int_list(node.get("weekIndexes")), vec![2, 3, 5]);
        let node = json!({ "weeks": 7 });
        assert_eq!(as_int_list(node.get("weeks")), vec![7]);
    }

    #[test]
    fn parses_sample_print_data() {
        let sample = json!({
            "result": {
                "semesterId": 527,
                "activities": [
                    {"courseName": "高等代数", "weekday": 1, "startUnit": 1, "endUnit": 2,
                     "weekIndexes": [1, 2, 3], "room": "H3101", "teachers": [{"name": "张三"}]},
                    {"courseName": "大学英语", "weekday": "3", "startUnit": "5", "endUnit": "",
                     "weeks": "1-16", "lessonPlace": "H2201", "teacherName": "李四"},
                    {"courseName": "重复项", "weekday": 2, "startUnit": 3, "endUnit": 4,
                     "weekIndexes": [1, 2]},
                    {"courseName": "重复项", "weekday": 2, "startUnit": 3, "endUnit": 4,
                     "weekIndexes": [2, 1]},
                    {"courseName": "没有周次", "weekday": 4, "startUnit": 1, "endUnit": 1},
                    {"weekday": 5, "startUnit": 1, "endUnit": 1}
                ]
            }
        })
        .to_string();
        let slots = parse_print_data(&sample).expect("样例应当能解析");
        assert_eq!(
            slots.len(),
            4,
            "重复项与没有标题的都该被处理掉：{:?}",
            slots
                .iter()
                .map(|one| (one.title.clone(), one.weekday, one.weeks.len()))
                .collect::<Vec<_>>()
        );

        let first = &slots[0];
        assert_eq!(first.title, "高等代数");
        assert_eq!(first.weekday, 1);
        assert_eq!((first.start_unit, first.end_unit), (1, 2));
        assert_eq!(first.weeks, vec![1, 2, 3]);
        assert_eq!(first.room, "H3101");
        assert!(first.teachers.contains("张三"), "{}", first.teachers);

        let english = slots
            .iter()
            .find(|one| one.title == "大学英语")
            .expect("要有大学英语");
        assert_eq!(
            (english.start_unit, english.end_unit),
            (5, 5),
            "结束节缺省等于开始节"
        );
        assert_eq!(english.weeks, vec![1, 16], "字符串周次按数字段解析（与旧版一致）");
        assert_eq!(english.room, "H2201");

        let no_weeks = slots
            .iter()
            .find(|one| one.title == "没有周次")
            .expect("要有没周次的课");
        assert_eq!(no_weeks.weeks.len(), 16, "没给周次默认 1-16 周");
        assert_eq!(no_weeks.weeks.first(), Some(&1));
        assert_eq!(no_weeks.weeks.last(), Some(&16));

        assert_eq!(
            slots.iter().filter(|one| one.title == "重复项").count(),
            1,
            "同一节重复给两次时只留一条"
        );
    }

    /// 学期第一周周一：与旧版 `fdu.first_monday_from` 一致（含当天的下一个周一）
    #[test]
    fn first_monday_matches_python_version() {
        // 2026-09-07 是周一
        assert_eq!(first_monday_from("2026-09-07").as_deref(), Some("2026-09-07"));
        // 前一天是周日 → 次日周一
        assert_eq!(first_monday_from("2026-09-06").as_deref(), Some("2026-09-07"));
        // 周二 → 下周一
        assert_eq!(first_monday_from("2026-09-08").as_deref(), Some("2026-09-14"));
        assert_eq!(first_monday_from("不是日期"), None);
    }

    /// 从课表页正文里认起始日期（真实页面写的是「学期起始日期」）
    #[test]
    fn sniffs_term_start_from_page_text() {
        assert_eq!(
            sniff_term_start("学年起始日期：2026-09-06\n").as_deref(),
            Some("2026-09-06")
        );
        assert_eq!(
            sniff_term_start("课表\n2026-2027学年1学期\n学期起始日期:  2026-09-06\n").as_deref(),
            Some("2026-09-06")
        );
        assert_eq!(
            sniff_term_start("学年起始日期: 2026-9-6").as_deref(),
            Some("2026-09-06")
        );
        assert_eq!(sniff_term_start("没有这一项"), None);
        assert_eq!(sniff_term_start("学年起始日期：待定"), None);
    }

    /// 起始日期永远给得出（旧版 `_first_monday` 的兜底行为：不报错）
    #[test]
    fn resolve_term_start_never_fails() {
        // ① 用户填的：三种写法都认，且不强制是周一
        assert_eq!(resolve_term_start("2026-09-07", "", ""), "2026-09-07");
        assert_eq!(resolve_term_start("2026/09/08", "", ""), "2026-09-08");
        assert_eq!(resolve_term_start("20260909", "", ""), "2026-09-09");
        // ② 教务页面上的起始日期（周日）→ 换算成周一
        assert_eq!(resolve_term_start("", "2026-09-06", ""), "2026-09-07");
        assert_eq!(resolve_term_start("", "学期起始日期:  2026-09-06", ""), "2026-09-07");
        // ③ 设置里上次存下的
        assert_eq!(resolve_term_start("", "", "2026-09-07"), "2026-09-07");
        // ④ 都没有 → 今天所在周的周一
        let today = chrono::Local::now().date_naive();
        assert_eq!(
            resolve_term_start("", "", ""),
            monday_of(today).format("%Y-%m-%d").to_string()
        );
    }

    /// 节次时间表必须与旧版 `fdu.DEFAULT_UNIT_TIMES` 一致（14 节）
    #[test]
    fn default_unit_times_match_python() {
        let times = default_unit_times();
        assert_eq!(times.len(), 14);
        assert_eq!(times[0], (1, "08:00".to_string(), "08:45".to_string()));
        assert_eq!(times[2], (3, "09:55".to_string(), "10:40".to_string()));
        assert_eq!(times[3], (4, "10:50".to_string(), "11:35".to_string()));
        assert_eq!(times[13], (14, "21:15".to_string(), "22:00".to_string()));
        // 晚上第 13/14 节以前不存在，会被退回「08:00–09:40」，这是课前错位的根源
        let (start, end) = unit_range(&times, 14, 14);
        assert_eq!((start.as_str(), end.as_str()), ("21:15", "22:00"));
    }

    #[test]
    fn rejects_non_json_body() {
        let err = parse_print_data("<html><body>请先登录</body></html>").unwrap_err();
        assert!(err.contains("不是合法 JSON"), "{err}");
    }

    /// 教务真实 print-data 的外形（2026-11-10 实测）：顶层是 `studentTableVms`，
    /// 里面是按周排的格子数组。这里用同名结构造一份合成样本，防止解析对真实外形失灵。
    #[test]
    fn parses_real_student_table_vms_shape() {
        let sample = json!({
            "studentTableVms": {
                "semesterId": 527,
                "unitTimes": {"1": {"start": 800, "end": 845}, "2": {"start": 855, "end": 940}},
                "tableVms": [
                    {"weekday": 1, "startUnit": 1, "endUnit": 2, "weekIndexes": [1, 2],
                     "courseName": "排球（一）", "room": "南区球场", "teachers": ["王五"]},
                    {"weekday": 3, "startUnit": 5, "endUnit": 6, "weekIndexes": [2, 3],
                     "courseName": "心理健康与大学生活", "lessonPlace": "H2101"},
                    {"weekday": 5, "startUnit": 9, "endUnit": 10, "weekIndexes": [1, 2, 3],
                     "courseName": "新时代实践教育I"}
                ]
            }
        })
        .to_string();
        let slots = parse_print_data(&sample).expect("真实外形应当能解析");
        assert_eq!(slots.len(), 3, "{slots:?}");
        assert_eq!(slots[0].title, "排球（一）");
        assert_eq!(slots[0].room, "南区球场");
        assert_eq!(slots[1].weeks, vec![2, 3]);
        assert_eq!(slots[2].start_unit, 9);
    }
}
