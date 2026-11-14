//! 考试安排导入（对应旧版 `exams.py` / `exam_dialog.py`）。
//!
//! 教务（fdjwgl）是统一身份认证 + SPA，进程内拿 cookie 复现会被弹回登录页，
//! 所以沿用课程表那套：**在已经登录的登录窗口里**落到考试页，用 JS 把表格行和正文捞出来
//! （`PAGE_JS`），再交给这里的解析器——票由页面带，我们只读页面自己已经渲染出来的东西。
//!
//! 解析尽量宽容（和旧版逐条对齐）：
//! * 一行里能同时找到「日期」和「时间」才算一条；
//! * 标题取那一行里最像课程名的单元格；
//! * 地点取带「楼/室/教室/校区/馆/厅」的单元格；
//! * 表格解析不出来时，退回「按行扫正文」。
use chrono::{Datelike, NaiveDate};
use serde_json::{json, Value};

use crate::store::Store;
use crate::EventInput;

/// 教务「我的考试」几个可能的地址（旧版就是这个顺序）
pub const EXAM_URLS: [&str; 3] = [
    "https://fdjwgl.fudan.edu.cn/student/for-std/exam-arrange",
    "https://fdjwgl.fudan.edu.cn/student/for-std/exam-arrange/index",
    "https://fdjwgl.fudan.edu.cn/student/for-std/exam",
];

/// 在页面里：把表格行、卡片行、正文都收进一个对象返回（旧版 `PAGE_JS`）
pub const PAGE_JS: &str = r#"(function(){
  function cellText(cell){ return (cell.innerText || cell.textContent || '').replace(/\s+/g, ' ').trim(); }
  var rows = [];
  document.querySelectorAll('table tr').forEach(function(tr){
    var cells = [];
    tr.querySelectorAll('th,td').forEach(function(td){ cells.push(cellText(td)); });
    if (cells.length >= 2) rows.push(cells);
  });
  document.querySelectorAll('.ant-table-row, .el-table__row, [role="row"]').forEach(function(r){
    var cells = [];
    r.querySelectorAll('[role="cell"], .ant-table-cell, td').forEach(function(td){ cells.push(cellText(td)); });
    if (cells.length >= 2) rows.push(cells);
  });
  var text = (document.body.innerText || '').replace(/\u00a0/g, ' ');
  return { rows: rows, text: text, title: document.title || '', url: location.href,
           ready: document.readyState };
})()"#;

// ---------------------------------------------------------------- 小工具

fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

fn push_digits(out: &mut String, text: &str, max: usize) -> Option<()> {
    let digits: String = text
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }
    if digits.len() > max {
        return None;
    }
    out.push_str(&digits);
    Some(())
}

/// 从一段文字里找日期：先找「年-月-日」，再找「M月D日」，最后找「M-D」（旧版 `find_date`）
pub fn find_date(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    // ① 2026-11-12 / 2026/11/12 / 2026年11月12日
    for index in 0..chars.len() {
        if index + 4 > chars.len() {
            break;
        }
        let year: String = chars[index..index + 4].iter().collect();
        if !(year.starts_with("20") && year.chars().all(|ch| ch.is_ascii_digit())) {
            continue;
        }
        let mut cursor = index + 4;
        while cursor < chars.len() && matches!(chars[cursor], ' ' | '-' | '/' | '.' | '年') {
            cursor += 1;
        }
        let mut month = String::new();
        let mut month_len = 0;
        while cursor + month_len < chars.len()
            && chars[cursor + month_len].is_ascii_digit()
            && month_len < 2
        {
            month.push(chars[cursor + month_len]);
            month_len += 1;
        }
        if month.is_empty() {
            continue;
        }
        cursor += month_len;
        while cursor < chars.len() && matches!(chars[cursor], ' ' | '-' | '/' | '.' | '月') {
            cursor += 1;
        }
        let mut day = String::new();
        let mut day_len = 0;
        while cursor + day_len < chars.len()
            && chars[cursor + day_len].is_ascii_digit()
            && day_len < 2
        {
            day.push(chars[cursor + day_len]);
            day_len += 1;
        }
        if day.is_empty() {
            continue;
        }
        if let Some(value) = NaiveDate::from_ymd_opt(
            year.parse().ok()?,
            month.parse().ok()?,
            day.parse().ok()?,
        ) {
            return Some(value.format("%Y-%m-%d").to_string());
        }
    }
    // ② 11月12日（补今年）
    let mut rest = String::new();
    let mut month = String::new();
    let mut day = String::new();
    let mut stage = 0;
    for (index, ch) in chars.iter().enumerate() {
        if stage == 0 && ch.is_ascii_digit() && (index == 0 || !chars[index - 1].is_ascii_digit()) {
            month.clear();
            day.clear();
            month.push(*ch);
            rest.push(*ch);
            stage = 1;
            continue;
        }
        match stage {
            1 => {
                if ch.is_ascii_digit() {
                    month.push(*ch);
                    rest.push(*ch);
                } else if *ch == '月' || *ch == '-' || *ch == '/' || *ch == '.' {
                    stage = 2;
                } else {
                    stage = 0;
                    rest.clear();
                }
            }
            2 => {
                if ch.is_ascii_digit() && day.len() < 2 {
                    day.push(*ch);
                } else if !day.is_empty() {
                    let want_month = *ch == '月';
                    if want_month || matches!(ch, '日' | '号' | ' ' | '-' | '/' | '.') {
                        if let (Ok(m), Ok(d)) = (month.parse::<u32>(), day.parse::<u32>()) {
                            if (1..=12).contains(&m) && (1..=31).contains(&d) {
                                let year = if want_month { today().year() } else { today().year() };
                                if let Some(value) = NaiveDate::from_ymd_opt(year, m, d) {
                                    let mut value = value;
                                    // 明显是去年的（差 >120 天）推到下一年
                                    if (value - today()).num_days() < -120 {
                                        if let Some(next) = value.with_year(year + 1) {
                                            value = next;
                                        }
                                    }
                                    return Some(value.format("%Y-%m-%d").to_string());
                                }
                            }
                        }
                    }
                    stage = 0;
                    rest.clear();
                } else {
                    stage = 0;
                    rest.clear();
                }
            }
            _ => {}
        }
    }
    None
}

/// 找时间范围：`13:30-15:00` / `13:30~15:00` / 单个 `13:30`（结束推一小时）→ 旧版 `find_time`
pub fn find_time(text: &str) -> (String, String) {
    let chars: Vec<char> = text.chars().collect();
    let mut singles: Vec<(String, usize)> = Vec::new();
    let mut index = 0;
    while index + 4 < chars.len() {
        if chars[index].is_ascii_digit()
            && chars[index + 1].is_ascii_digit()
            && chars[index + 2] == ':'
            && chars[index + 3].is_ascii_digit()
            && chars[index + 4].is_ascii_digit()
        {
            let head_digits = if index > 0 { chars.get(index - 1) } else { None };
            if head_digits.map(|ch| ch.is_ascii_digit()).unwrap_or(false) {
                index += 1;
                continue;
            }
            let raw: String = chars[index..index + 5].iter().collect();
            let start = if norm_time(&raw).is_empty() { String::new() } else { raw };
            if start.is_empty() {
                index += 1;
                continue;
            }
            // 后面紧跟「- 到 至 ~ — –」再加一个时间 → 当作范围
            let mut cursor = index + 5;
            while cursor < chars.len() && chars[cursor] == ' ' {
                cursor += 1;
            }
            if cursor < chars.len() && "-–—~～−至到".contains(chars[cursor]) {
                let mut after = cursor + 1;
                while after < chars.len() && chars[after] == ' ' {
                    after += 1;
                }
                if after + 4 < chars.len() {
                    let tail: String = chars[after..after + 5].iter().collect();
                    if !norm_time(&tail).is_empty() {
                        return (norm_time(&start), norm_time(&tail));
                    }
                }
            }
            singles.push((norm_time(&start), index));
            index += 5;
            continue;
        }
        index += 1;
    }
    if let Some((start, _)) = singles.first() {
        let hour: u32 = start.split(':').next().unwrap_or("09").parse().unwrap_or(9);
        let minute = start.split(':').nth(1).unwrap_or("00").to_string();
        return (
            start.clone(),
            format!("{:02}:{}", (hour + 1).min(23), minute),
        );
    }
    ("09:00".to_string(), String::new())
}

fn norm_time(value: &str) -> String {
    let trimmed = value.trim();
    let mut parts = trimmed.split([':', '：']);
    let Some(hour) = parts.next().and_then(|one| one.trim().parse::<u32>().ok()) else {
        return String::new();
    };
    let Some(minute) = parts.next().and_then(|one| one.trim().parse::<u32>().ok()) else {
        return String::new();
    };
    if hour > 23 || minute > 59 {
        return String::new();
    }
    format!("{hour:02}:{minute:02}")
}

fn looks_like_location(text: &str) -> bool {
    let raw = text.trim();
    !raw.is_empty()
        && raw.chars().count() <= 30
        && ["楼", "室", "教室", "校区", "馆", "厅", "中心", "号楼", "座"]
            .iter()
            .any(|hint| raw.contains(hint))
}

fn looks_like_title(text: &str) -> bool {
    let raw = text.trim();
    if raw.is_empty() || raw.chars().count() > 40 {
        return false;
    }
    if find_date(raw).is_some() {
        return false;
    }
    let (start, _) = find_time(raw);
    let has_time = raw.contains(':') && start != "09:00";
    if has_time {
        return false;
    }
    if raw
        .chars()
        .all(|ch| ch.is_ascii_digit() || " :：-/.年".contains(ch))
    {
        return false;
    }
    raw.chars().any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
}

fn clean_title(text: &str) -> String {
    let mut raw = text.trim().to_string();
    // 去掉日期/时间片段，压掉分隔符
    for token in ["-", "/", ":", "：", "~", "—", "–", "至", "到"] {
        let _ = token;
    }
    if let Some(day) = find_date(&raw) {
        raw = raw.replace(&day, " ");
        // 中文日期写法也清掉
        for pattern in ["年", "月", "日", "号"] {
            let _ = pattern;
        }
    }
    let (start, end) = find_time(&raw);
    for piece in [start.as_str(), end.as_str()] {
        if !piece.is_empty() {
            raw = raw.replace(piece, " ");
        }
    }
    let cleaned: String = raw
        .chars()
        .map(|ch| {
            if ch.is_whitespace() || "|｜,，、;；:：".contains(ch) {
                ' '
            } else {
                ch
            }
        })
        .collect();
    let squeezed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    squeezed
        .trim_matches(|ch: char| ch == '-' || ch == '—' || ch == '·' || ch == ' ')
        .chars()
        .take(40)
        .collect()
}

fn seat_of(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    for (index, window) in chars.windows(2).enumerate() {
        let pair: String = window.iter().collect();
        if pair == "座位" || pair == "座号" || pair == "考号" {
            let mut cursor = index + 2;
            while cursor < chars.len() && (chars[cursor] == ':' || chars[cursor] == '：' || chars[cursor] == ' ')
            {
                cursor += 1;
            }
            let mut value = String::new();
            while cursor < chars.len()
                && (chars[cursor].is_ascii_alphanumeric() || chars[cursor] == '-')
                && value.len() < 20
            {
                value.push(chars[cursor]);
                cursor += 1;
            }
            if !value.is_empty() {
                return value;
            }
        }
    }
    String::new()
}

// ---------------------------------------------------------------- 解析

/// 表格行 → 考试条目（旧版 `parse_rows`）
pub fn parse_rows(rows: &[Value]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut seen: Vec<(String, String, String)> = Vec::new();
    for row in rows {
        let cells: Vec<String> = row
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|one| one.as_str().unwrap_or("").trim().to_string())
                    .collect()
            })
            .unwrap_or_default();
        if cells.len() < 2 {
            continue;
        }
        let joined = cells.join(" ");
        let Some(day) = find_date(&joined) else {
            continue;
        };
        let (start, end) = find_time(&joined);
        let mut location = String::new();
        for cell in &cells {
            if looks_like_location(cell) && find_date(cell).is_none() {
                location = cell.clone();
                break;
            }
        }
        let mut title = String::new();
        for cell in &cells {
            if looks_like_title(cell) && *cell != location {
                title = clean_title(cell);
                if !title.is_empty() {
                    break;
                }
            }
        }
        if title.is_empty() {
            title = clean_title(&joined);
            if title.is_empty() {
                title = "考试".to_string();
            }
        }
        let key = (title.clone(), day.clone(), start.clone());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(json!({
            "title": title,
            "date": day,
            "start_time": start,
            "end_time": end,
            "location": location,
            "seat": seat_of(&joined),
            "note": joined.chars().take(160).collect::<String>(),
            "confidence": "table",
        }));
    }
    out
}

/// 正文兜底（旧版 `parse_text`）
pub fn parse_text(text: &str) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut seen: Vec<(String, String, String)> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let length = line.chars().count();
        if length < 4 || length > 160 {
            continue;
        }
        let Some(day) = find_date(line) else {
            continue;
        };
        let (start, end) = find_time(line);
        if start == "09:00" && !line.contains(':') {
            continue;
        }
        let mut location = String::new();
        for token in line.split([' ', ',', '，', '、', '|', '｜', ';', '；']) {
            if looks_like_location(token) {
                location = token.trim().to_string();
                break;
            }
        }
        let mut title = clean_title(line);
        if title.is_empty() {
            title = "考试".to_string();
        }
        let key = (title.clone(), day.clone(), start.clone());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(json!({
            "title": title.chars().take(40).collect::<String>(),
            "date": day,
            "start_time": start,
            "end_time": end,
            "location": location,
            "seat": seat_of(line),
            "note": line.chars().take(160).collect::<String>(),
            "confidence": "text",
        }));
    }
    out
}

/// 先按表格解析；表格没结果就扫正文（旧版 `parse_exams`）
pub fn parse_exams(raw: &Value) -> Vec<Value> {
    let rows = raw
        .get("rows")
        .and_then(|one| one.as_array())
        .cloned()
        .unwrap_or_default();
    let items = parse_rows(&rows);
    if !items.is_empty() {
        return items;
    }
    let text = raw.get("text").and_then(|one| one.as_str()).unwrap_or("");
    parse_text(text)
}

// ---------------------------------------------------------------- 写进日历

/// 写进日历（分组不做了：Rust 的 events 没有分组字段）+ 可选建重要日
///
/// 返回 `{ok, added, skipped, items}`：已存在的（标题+日期+开始时间都一样）算跳过。
pub fn add_exams(store: &Store, items: &[Value], as_important: bool) -> Value {
    let mut existing: Vec<(String, String, String)> = Vec::new();
    {
        let conn = store.connection();
        let mut rows: Vec<(String, String, String)> = Vec::new();
        {
            if let Ok(mut stmt) = conn.prepare("SELECT title, date, start_time FROM events") {
                if let Ok(mapped) = stmt.query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                }) {
                    rows.extend(mapped.flatten());
                }
            }
        }
        existing.extend(rows);
    }
    let mut added = 0usize;
    let mut skipped = 0usize;
    let mut written: Vec<Value> = Vec::new();
    for item in items {
        let title = item
            .get("title")
            .and_then(|one| one.as_str())
            .unwrap_or("考试")
            .trim()
            .to_string();
        let day = item.get("date").and_then(|one| one.as_str()).unwrap_or("").to_string();
        let start_raw = item
            .get("start_time")
            .and_then(|one| one.as_str())
            .unwrap_or("")
            .to_string();
        let start = {
            let value = norm_time(&start_raw);
            if value.is_empty() {
                "09:00".to_string()
            } else {
                value
            }
        };
        let end = {
            let value = norm_time(
                item.get("end_time").and_then(|one| one.as_str()).unwrap_or(""),
            );
            if value.is_empty() {
                let hour: u32 = start.split(':').next().unwrap_or("09").parse().unwrap_or(9);
                let minute = start.split(':').nth(1).unwrap_or("00").to_string();
                format!("{:02}:{}", (hour + 2).min(23), minute)
            } else {
                value
            }
        };
        if day.is_empty() || NaiveDate::parse_from_str(&day, "%Y-%m-%d").is_err() {
            skipped += 1;
            continue;
        }
        let key = (title.clone(), day.clone(), start.clone());
        if existing.contains(&key) {
            skipped += 1;
            continue;
        }
        let location = item
            .get("location")
            .and_then(|one| one.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let seat = item
            .get("seat")
            .and_then(|one| one.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let note = item
            .get("note")
            .and_then(|one| one.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let mut pieces: Vec<String> = Vec::new();
        if !location.is_empty() {
            pieces.push(format!("地点：{location}"));
        }
        if !seat.is_empty() {
            pieces.push(format!("座位：{seat}"));
        }
        if !note.is_empty() {
            pieces.push(note.clone());
        }
        let input = EventInput {
            title: Some(title.clone()),
            description: Some(pieces.join("\n")),
            date: Some(day.clone()),
            start_time: Some(start.clone()),
            end_time: Some(end.clone()),
            // 考试提前一小时提醒（旧版就是 60 分钟）
            remind_minutes: Some(60),
            clear_remind: false,
            color: Some("rose".to_string()),
            done: None,
            repeat: None,
        };
        match store.add_event(&input) {
            Ok(_id) => {
                added += 1;
                existing.push(key);
                if as_important {
                    let important_note = if location.is_empty() {
                        note.chars().take(60).collect::<String>()
                    } else {
                        location.clone()
                    };
                    save_important(store, &title, &day, &important_note);
                }
                written.push(json!({
                    "title": title, "date": day, "start_time": start, "end_time": end,
                    "location": location, "seat": seat,
                }));
            }
            Err(_) => skipped += 1,
        }
    }
    json!({ "ok": true, "added": added, "skipped": skipped, "items": written })
}

/// 顺手建一条「重要日」倒计时（kind = exam，标题前面不加前缀，日历页按 kind 上色）
fn save_important(store: &Store, title: &str, day: &str, note: &str) {
    let conn = store.connection();
    let already: Option<i64> = conn
        .query_row(
            "SELECT id FROM important_dates WHERE title = ?1 AND date = ?2",
            rusqlite::params![title, day],
            |row| row.get(0),
        )
        .ok();
    if already.is_some() {
        return;
    }
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let _ = conn.execute(
        "INSERT INTO important_dates(title, date, note, color, kind, created_at)
         VALUES (?1, ?2, ?3, 'rose', 'exam', ?4)",
        rusqlite::params![title, day, note, stamp],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Temp(Option<std::ffi::OsString>, std::path::PathBuf);

    impl Temp {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "danlume-exams-{tag}-{}",
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

    #[test]
    fn finds_dates_in_three_shapes() {
        assert_eq!(find_date("2026-12-20 13:30"), Some("2026-12-20".to_string()));
        assert_eq!(find_date("2026年1月5日 08:00"), Some("2026-01-05".to_string()));
        let year = today().year();
        assert_eq!(
            find_date("12月20日 13:30-15:00"),
            Some(format!("{year}-12-20"))
        );
        assert_eq!(find_date("没有日期"), None);
    }

    #[test]
    fn finds_time_ranges_and_single_times() {
        assert_eq!(
            find_time("13:30-15:00"),
            ("13:30".to_string(), "15:00".to_string())
        );
        assert_eq!(
            find_time("08:00～09:40"),
            ("08:00".to_string(), "09:40".to_string())
        );
        assert_eq!(
            find_time("14:00"),
            ("14:00".to_string(), "15:00".to_string())
        );
        assert_eq!(find_time("没时间"), ("09:00".to_string(), String::new()));
    }

    /// 表格一行：课程、日期、时间、地点、座位
    #[test]
    fn parses_exam_table_rows() {
        let rows = json!([
            ["程序设计", "2026-12-20", "13:30-15:00", "邯郸校区计算中心 301", "座位：A-102"],
            ["线性代数", "2026-12-22", "08:00-09:40", "第六教学楼 301", ""],
            ["只有标题", "", "", "", ""]
        ]);
        let items = parse_rows(rows.as_array().unwrap());
        assert_eq!(items.len(), 2, "{items:?}");
        assert_eq!(items[0]["title"], json!("程序设计"));
        assert_eq!(items[0]["date"], json!("2026-12-20"));
        assert_eq!(items[0]["start_time"], json!("13:30"));
        assert_eq!(items[0]["end_time"], json!("15:00"));
        assert!(items[0]["location"].as_str().unwrap().contains("计算中心"));
        assert_eq!(items[0]["seat"], json!("A-102"));
        assert_eq!(items[1]["title"], json!("线性代数"));
    }

    /// 表格解析不出来时退回扫正文
    #[test]
    fn falls_back_to_text_scan() {
        let raw = json!({
            "rows": [],
            "text": "选课说明第一行\n程序设计 2026-12-20 13:30-15:00 邯郸校区计算中心301\n无关的一行文字\n"
        });
        let items = parse_exams(&raw);
        assert_eq!(items.len(), 1, "{items:?}");
        assert_eq!(items[0]["date"], json!("2026-12-20"));
        assert_eq!(items[0]["start_time"], json!("13:30"));
    }

    /// 写进日历 + 重要日；第二次导入应该全部跳过
    #[test]
    fn writes_events_and_important_dates_once() {
        let _temp = Temp::new("write");
        let store = Store::open().expect("临时库");
        let items = json!([
            { "title": "程序设计", "date": "2026-12-20", "start_time": "13:30",
              "end_time": "15:00", "location": "邯郸校区计算中心 301", "seat": "A-102",
              "note": "考试", "confidence": "table" }
        ]);
        let first = add_exams(&store, items.as_array().unwrap(), true);
        assert_eq!(first["added"], json!(1), "{first}");
        assert_eq!(first["skipped"], json!(0));
        let second = add_exams(&store, items.as_array().unwrap(), true);
        assert_eq!(second["added"], json!(0), "{second}");
        assert_eq!(second["skipped"], json!(1));
        // 日程真的写进去了，带地点和座位
        let day = store.events_on("2026-12-20").expect("读当天日程");
        assert_eq!(day.len(), 1);
        assert_eq!(day[0].title, "程序设计");
        assert_eq!(day[0].start_time, "13:30");
        assert!(day[0].description.contains("地点：邯郸校区计算中心 301"));
        assert!(day[0].description.contains("座位：A-102"));
        // 重要日也建了，而且只有一条
        let conn = store.connection();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM important_dates", [], |row| row.get(0))
            .unwrap_or(0);
        assert_eq!(count, 1);
        let kind: String = conn
            .query_row("SELECT kind FROM important_dates LIMIT 1", [], |row| row.get(0))
            .unwrap_or_default();
        assert_eq!(kind, "exam");
    }
}
