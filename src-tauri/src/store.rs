//! SQLite 数据层：schema 与旧版 `lumina/store.py` **逐字段一致**，
//! 所以两个版本可以打开同一个 `lumina.db`。
use std::sync::Mutex;

use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

use crate::paths;

/// 建表语句（与旧版一致，多出来的列在新旧版本之间互相忽略不了，所以必须同名）
/// `pub(crate)`：backup.rs 的测试要拿它造临时库
pub(crate) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    date TEXT NOT NULL,
    start_time TEXT NOT NULL DEFAULT '09:00',
    end_time TEXT NOT NULL DEFAULT '10:00',
    remind_minutes INTEGER,
    remind_at TEXT,
    notified INTEGER NOT NULL DEFAULT 0,
    done INTEGER NOT NULL DEFAULT 0,
    color TEXT NOT NULL DEFAULT 'violet',
    repeat TEXT NOT NULL DEFAULT '',
    repeat_skip TEXT NOT NULL DEFAULT '',
    repeat_until TEXT NOT NULL DEFAULT '',
    done_dates TEXT NOT NULL DEFAULT '',
    notified_dates TEXT NOT NULL DEFAULT '',
    snooze_until TEXT NOT NULL DEFAULT '',
    group_id INTEGER,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_events_date ON events(date);
CREATE TABLE IF NOT EXISTS mails (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    to_addr TEXT NOT NULL DEFAULT '',
    subject TEXT NOT NULL DEFAULT '',
    html TEXT NOT NULL DEFAULT '',
    text TEXT NOT NULL DEFAULT '',
    tasks_json TEXT NOT NULL DEFAULT '[]',
    attachments_json TEXT NOT NULL DEFAULT '[]',
    note TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'draft',
    error TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    sent_at TEXT
);
CREATE TABLE IF NOT EXISTS contacts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL DEFAULT '',
    email TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS memos (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL DEFAULT '',
    html TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS notices (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source TEXT NOT NULL,
    title TEXT NOT NULL,
    url TEXT NOT NULL,
    fetched_at TEXT NOT NULL,
    published_at TEXT NOT NULL DEFAULT '',
    read_at TEXT
);
-- 服务器公告（account.rs / notify.rs 从自家账号服务器拉下来的，与上面抓网页的 notices 是两回事：
-- id 直接用服务器给的 id，方便按 last_announcement_id 增量拉；旧版没有这张表，它会忽略）
CREATE TABLE IF NOT EXISTS announcements (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL DEFAULT '',
    body TEXT NOT NULL DEFAULT '',
    level TEXT NOT NULL DEFAULT 'info',
    pinned INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT '',
    expires_at TEXT NOT NULL DEFAULT '',
    received_at TEXT NOT NULL,
    read_at TEXT
);
CREATE TABLE IF NOT EXISTS event_groups (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    color TEXT NOT NULL DEFAULT 'teal',
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS important_dates (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    date TEXT NOT NULL,
    note TEXT NOT NULL DEFAULT '',
    color TEXT NOT NULL DEFAULT 'rose',
    kind TEXT NOT NULL DEFAULT 'custom',
    event_id INTEGER,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL DEFAULT ''
);
-- AI 记忆库（memory.rs）。旧版 PySide6 是把记忆写成 <data>/memory.md 的纯文本，
-- 没有这张表；这里换成表存，字段在旧版语义（一条内容 + 写入时间）上多了来源 / 标签 /
-- 命中次数 / 最近使用时间。旧版读到不认识的表会直接忽略，两边可以共用同一个 lumina.db。
CREATE TABLE IF NOT EXISTS memories (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    content TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT '',
    tags TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    hits INTEGER NOT NULL DEFAULT 0,
    last_used_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_memories_updated ON memories(updated_at);
CREATE TABLE IF NOT EXISTS loc_points (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    lat REAL NOT NULL,
    lon REAL NOT NULL,
    note TEXT NOT NULL DEFAULT '',
    source TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);
"#;

/// 老库补列 + 修一次历史数据（幂等：什么都不缺时只是几条 PRAGMA/UPDATE）
fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let mut columns: Vec<String> = Vec::new();
    {
        let mut stmt = conn.prepare("PRAGMA table_info(events)")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            columns.push(row.get(1).unwrap_or_default());
        }
    }
    let has = |name: &str| columns.iter().any(|one| one == name);
    if !has("done_dates") {
        conn.execute(
            "ALTER TABLE events ADD COLUMN done_dates TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }
    // 重复日程「哪几次已经提醒过」「哪一次被稍后提醒到什么时候」：按天记，
    // 否则一次提醒/一次稍后会把整条系列都算掉。
    if !has("notified_dates") {
        conn.execute(
            "ALTER TABLE events ADD COLUMN notified_dates TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }
    if !has("snooze_until") {
        conn.execute(
            "ALTER TABLE events ADD COLUMN snooze_until TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }
    // 日程分组（旧版 `event_groups` / `events.group_id`）。旧库里通常已经有了，
    // 但 Rust 版自己建库时以前没写这两列，导致导入课表时没法挂到「课程表」分组。
    if !has("group_id") {
        conn.execute("ALTER TABLE events ADD COLUMN group_id INTEGER", [])?;
    }
    // 历史 bug 修复：以前「标记完成」「过期自动完成」直接写母事件的 done，
    // 于是整条重复系列（含以后每一次）全变成已完成。这里把那次完成挪到它自己
    // 那一天，母事件复位；之后再标记完成只会记进 done_dates，不再带坏整条系列。
    conn.execute(
        "UPDATE events SET done_dates = substr(date, 1, 10), done = 0
         WHERE repeat IS NOT NULL AND repeat <> '' AND done = 1
           AND (done_dates IS NULL OR done_dates = '')",
        [],
    )?;
    Ok(())
}

/// 重复规则规整：只认旧版那四种（''=不重复）
pub fn normalize_repeat(value: &str) -> String {
    match value.trim().to_lowercase().as_str() {
        "day" | "daily" | "每天" | "每日" => "daily".to_string(),
        "week" | "weekly" | "每周" | "每星期" => "weekly".to_string(),
        "month" | "monthly" | "每月" => "monthly".to_string(),
        "year" | "yearly" | "每年" => "yearly".to_string(),
        _ => String::new(),
    }
}

/// 逗号分隔的日期列表 → Vec（去空、去空白）
pub fn split_dates(text: &str) -> Vec<String> {
    text.split(',')
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

/// "9:40" → "09:40"（老数据里可能存着单位数小时）
pub fn clock5(text: &str) -> String {
    let raw = text.chars().take(5).collect::<String>();
    match raw.split_once(':') {
        Some((hour, minute)) if hour.len() == 1 => format!("0{hour}:{minute}"),
        _ if raw.is_empty() => "09:00".to_string(),
        _ => raw,
    }
}

/// 把「从哪天开始、按什么规则重复」展开成 [start, end] 区间里的日期。
///
/// 与旧版 `store.py::recurrence_days` 逐条对齐：daily 每天；weekly 每 7 天（同一个
/// 星期几）；monthly 每月同一个「几号」（那个月没有这一天就跳过，1/31 → 3/31）；
/// yearly 每年同月同日（2/29 只在闰年出现）。
pub fn recurrence_days(start_date: &str, repeat: &str, start: &str, end: &str) -> Vec<String> {
    use chrono::Datelike;
    const LIMIT: usize = 400;
    let rule = normalize_repeat(repeat);
    if rule.is_empty() {
        return Vec::new();
    }
    let parse = |text: &str| {
        chrono::NaiveDate::parse_from_str(text.get(..10).unwrap_or(text), "%Y-%m-%d").ok()
    };
    let (Some(begin), Some(low), Some(high)) = (parse(start_date), parse(start), parse(end)) else {
        return Vec::new();
    };
    if high < begin {
        return Vec::new();
    }
    let mut days: Vec<String> = Vec::new();
    match rule.as_str() {
        "daily" => {
            let mut cursor = begin.max(low);
            while cursor <= high && days.len() < LIMIT {
                days.push(cursor.format("%Y-%m-%d").to_string());
                cursor += chrono::Duration::days(1);
            }
        }
        "weekly" => {
            let mut cursor = begin;
            if cursor < low {
                cursor += chrono::Duration::days(7 * ((low - cursor).num_days() / 7));
                while cursor < low {
                    cursor += chrono::Duration::days(7);
                }
            }
            while cursor <= high && days.len() < LIMIT {
                days.push(cursor.format("%Y-%m-%d").to_string());
                cursor += chrono::Duration::days(7);
            }
        }
        "monthly" => {
            let (mut year, mut month) = (begin.year(), begin.month());
            while (year, month) <= (high.year(), high.month()) && days.len() < LIMIT {
                if let Some(occurrence) = chrono::NaiveDate::from_ymd_opt(year, month, begin.day()) {
                    if occurrence >= begin && occurrence >= low && occurrence <= high {
                        days.push(occurrence.format("%Y-%m-%d").to_string());
                    }
                }
                month += 1;
                if month > 12 {
                    month = 1;
                    year += 1;
                }
            }
        }
        _ => {
            for year in begin.year()..=high.year() {
                if days.len() >= LIMIT {
                    break;
                }
                if let Some(occurrence) = chrono::NaiveDate::from_ymd_opt(year, begin.month(), begin.day()) {
                    if occurrence >= begin && occurrence >= low && occurrence <= high {
                        days.push(occurrence.format("%Y-%m-%d").to_string());
                    }
                }
            }
        }
    }
    days
}

/// 一条日程（字段名与旧版一致，前端直接用）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRow {
    pub id: i64,
    pub title: String,
    pub description: String,
    pub date: String,
    pub start_time: String,
    pub end_time: String,
    pub remind_minutes: Option<i64>,
    /// 提醒时间（`YYYY-mm-dd HH:MM:SS`），没有提醒就是 None
    pub remind_at: Option<String>,
    pub color: String,
    pub done: bool,
    pub group_id: Option<i64>,
    pub repeat: String,
    /// 重复日程里被单独跳过的那几天（逗号分隔，对应旧版 `repeat_skip`）
    #[serde(default)]
    pub repeat_skip: String,
    /// 重复到哪一天为止（空 = 一直重复，对应旧版 `repeat_until`）
    #[serde(default)]
    pub repeat_until: String,
    /// 重复日程里**已经完成的那几次**（逗号分隔日期）。
    /// 这是本次修 bug 新增的列：以前「标记完成」写的是母事件的 `done`，
    /// 结果整条重复系列（包括以后每一次）全变成已完成。
    #[serde(default)]
    pub done_dates: String,
    /// 重复日程里**已经提醒过的那几次**（逗号分隔日期）
    #[serde(default)]
    pub notified_dates: String,
    /// 「稍后 10 分钟」按下的新时间（`YYYY-mm-dd HH:MM:SS`，只对那一天有效）
    #[serde(default)]
    pub snooze_until: String,
    /// 这一次是不是已经提醒过了（重复日程按天算）
    #[serde(default)]
    pub notified: bool,
    /// 展开出来的一次重复：母事件 id（母事件本身那一天为 None）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat_of: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

impl EventRow {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            title: row.get("title")?,
            description: row.get("description")?,
            date: row.get("date")?,
            start_time: row.get("start_time")?,
            end_time: row.get("end_time")?,
            remind_minutes: row.get("remind_minutes").ok(),
            remind_at: row.get::<_, Option<String>>("remind_at").unwrap_or_default(),
            color: row.get("color")?,
            done: row.get::<_, i64>("done").unwrap_or(0) != 0,
            group_id: row.get("group_id").ok(),
            repeat: row.get::<_, String>("repeat").unwrap_or_default(),
            repeat_skip: row.get::<_, String>("repeat_skip").unwrap_or_default(),
            repeat_until: row.get::<_, String>("repeat_until").unwrap_or_default(),
            done_dates: row.get::<_, String>("done_dates").unwrap_or_default(),
            notified_dates: row.get::<_, String>("notified_dates").unwrap_or_default(),
            snooze_until: row.get::<_, String>("snooze_until").unwrap_or_default(),
            notified: row.get::<_, i64>("notified").unwrap_or(0) != 0,
            repeat_of: None,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }
}

/// 新建 / 修改日程时前端传来的字段（都可选，只改传了的）
#[derive(Debug, Clone, Default, Deserialize)]
pub struct EventInput {
    pub title: Option<String>,
    pub description: Option<String>,
    pub date: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub remind_minutes: Option<i64>,
    #[serde(default)]
    pub clear_remind: bool,
    pub color: Option<String>,
    pub done: Option<bool>,
    pub repeat: Option<String>,
}

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open() -> rusqlite::Result<Self> {
        paths::ensure_data_dir();
        let conn = Connection::open(paths::db_path())?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|err| err.into_inner())
    }

    /// 别的模块（文献库等）需要拿连接自己查
    pub fn connection(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.lock()
    }

    // ---------------------------------------------------------- 日程
    /// 某一天的所有日程（重复日程会按规则展开，这一天的那一次在这里）
    pub fn events_on(&self, date: &str) -> rusqlite::Result<Vec<EventRow>> {
        self.events_between(date, date)
    }

    /// [start, end] 里的日程（**含重复日程展开出来的每一次**，对齐旧版
    /// `store.py::events_between`）。展开出来的那几次 `repeat_of` 指向母事件，
    /// `done` 按 `done_dates` 单独算，所以「第一次完成」不会把以后每一次都带成完成。
    pub fn events_between(&self, start: &str, end: &str) -> rusqlite::Result<Vec<EventRow>> {
        let conn = self.lock();
        let mut out: Vec<EventRow> = Vec::new();
        {
            let mut stmt = conn.prepare(
                "SELECT * FROM events WHERE (repeat IS NULL OR repeat = '')
                 AND date >= ?1 AND date <= ?2",
            )?;
            let rows = stmt.query_map(params![start, end], EventRow::from_row)?;
            for one in rows {
                out.push(one?);
            }
        }
        {
            let mut stmt = conn.prepare(
                "SELECT * FROM events WHERE repeat IS NOT NULL AND repeat <> '' AND date <= ?1",
            )?;
            let rows = stmt.query_map(params![end], EventRow::from_row)?;
            for one in rows {
                let master = one?;
                let skips: Vec<String> = master
                    .repeat_skip
                    .split(',')
                    .map(|part| part.trim().to_string())
                    .filter(|part| !part.is_empty())
                    .collect();
                let done_days: Vec<String> = master
                    .done_dates
                    .split(',')
                    .map(|part| part.trim().to_string())
                    .filter(|part| !part.is_empty())
                    .collect();
                let notified_days: Vec<String> = master
                    .notified_dates
                    .split(',')
                    .map(|part| part.trim().to_string())
                    .filter(|part| !part.is_empty())
                    .collect();
                let until: String = master.repeat_until.chars().take(10).collect();
                for day in recurrence_days(&master.date, &master.repeat, start, end) {
                    if !until.is_empty() && day > until {
                        continue;
                    }
                    if skips.iter().any(|one| one == &day) {
                        continue;
                    }
                    let done = master.done || done_days.iter().any(|one| one == &day);
                    let notified = master.notified || notified_days.iter().any(|one| one == &day);
                    let mut row = master.clone();
                    row.done = done;
                    row.notified = notified;
                    if day != master.date.chars().take(10).collect::<String>() {
                        row.date = day;
                        row.repeat_of = Some(master.id);
                    }
                    out.push(row);
                }
            }
        }
        out.sort_by(|a, b| {
            (a.date.as_str(), a.start_time.as_str(), a.id)
                .cmp(&(b.date.as_str(), b.start_time.as_str(), b.id))
        });
        Ok(out)
    }

    /// [start, end] 之间的日程，按日期分组（旧版 event_index）
    pub fn event_index(
        &self,
        start: &str,
        end: &str,
    ) -> rusqlite::Result<std::collections::BTreeMap<String, Vec<EventRow>>> {
        let rows = self.events_between(start, end)?;
        let mut out: std::collections::BTreeMap<String, Vec<EventRow>> =
            std::collections::BTreeMap::new();
        for event in rows {
            out.entry(event.date.clone()).or_default().push(event);
        }
        Ok(out)
    }

    pub fn get_event(&self, id: i64) -> rusqlite::Result<Option<EventRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT * FROM events WHERE id = ?1")?;
        let mut rows = stmt.query_map(params![id], EventRow::from_row)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn add_event(&self, input: &EventInput) -> rusqlite::Result<i64> {
        let now = crate::now_str();
        let title = input.title.clone().unwrap_or_else(|| "未命名".to_string());
        let date = input
            .date
            .clone()
            .unwrap_or_else(|| crate::today_str());
        let start = input
            .start_time
            .clone()
            .unwrap_or_else(|| "09:00".to_string());
        let end = input.end_time.clone().unwrap_or_else(|| "10:00".to_string());
        let remind = if input.clear_remind {
            None
        } else {
            input.remind_minutes
        };
        let remind_at = remind.map(|minutes| crate::compute_remind_at(&date, &start, minutes));
        // 重复日程不把「完成」写进母行（否则整条系列一起变完成），改记「哪一天」完成
        let rule = normalize_repeat(&input.repeat.clone().unwrap_or_default());
        let want_done = input.done.unwrap_or(false);
        let done_value = if rule.is_empty() { want_done as i64 } else { 0 };
        let done_dates = if !rule.is_empty() && want_done {
            date.chars().take(10).collect::<String>()
        } else {
            String::new()
        };
        let conn = self.lock();
        conn.execute(
            "INSERT INTO events(title, description, date, start_time, end_time,
              remind_minutes, remind_at, notified, done, color, repeat, done_dates,
              created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,0,?8,?9,?10,?11,?12,?12)",
            params![
                title.trim(),
                input.description.clone().unwrap_or_default(),
                date,
                start,
                end,
                remind,
                remind_at,
                done_value,
                input.color.clone().unwrap_or_else(|| "violet".to_string()),
                rule,
                done_dates,
                now,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// 把一条日程挂到分组（旧版 `events.group_id`；导入课表时用）
    pub fn set_event_group(&self, id: i64, group_id: i64) -> rusqlite::Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE events SET group_id = ?1 WHERE id = ?2",
            rusqlite::params![group_id, id],
        )?;
        Ok(())
    }

    pub fn update_event(&self, id: i64, input: &EventInput) -> rusqlite::Result<bool> {
        let Some(current) = self.get_event(id)? else {
            return Ok(false);
        };
        let prev_date = current.date.clone();
        let prev_start = current.start_time.clone();
        let title = input.title.clone().unwrap_or(current.title);
        let date = input.date.clone().unwrap_or_else(|| current.date.clone());
        let start = input.start_time.clone().unwrap_or(current.start_time);
        let end = input.end_time.clone().unwrap_or(current.end_time);
        let remind = if input.clear_remind {
            None
        } else {
            input.remind_minutes.or(current.remind_minutes)
        };
        let remind_at = remind.map(|minutes| crate::compute_remind_at(&date, &start, minutes));
        let time_changed = date != prev_date || start != prev_start;
        let rule = input
            .repeat
            .clone()
            .map(|text| normalize_repeat(&text))
            .unwrap_or_else(|| normalize_repeat(&current.repeat));
        // 重复日程的「完成」走 set_done（按天记），母行的 done 保持不动
        let mut done_value = if rule.is_empty() {
            input.done.unwrap_or(current.done)
        } else {
            current.done
        };
        if !rule.is_empty() {
            if let Some(flag) = input.done {
                let day = if time_changed { date.clone() } else { current.date.clone() };
                let _ = self.set_done(id, flag, Some(day.as_str()));
                done_value = current.done;
            }
        }
        let conn = self.lock();
        conn.execute(
            "UPDATE events SET title=?1, description=?2, date=?3, start_time=?4,
               end_time=?5, remind_minutes=?6, remind_at=?7, done=?8, color=?9,
               repeat=?10, updated_at=?11,
               notified = CASE WHEN ?12 THEN 0 ELSE notified END
             WHERE id=?13",
            params![
                title.trim(),
                input.description.clone().unwrap_or(current.description),
                date,
                start,
                end,
                remind,
                remind_at,
                done_value as i64,
                input.color.clone().unwrap_or(current.color),
                rule,
                crate::now_str(),
                time_changed as i64,
                id,
            ],
        )?;
        Ok(true)
    }

    /// 标记完成 / 取消完成。
    ///
    /// 不重复的日程就是改 `done`；重复日程按 `day`（哪一天的那一次）记进
    /// `done_dates`，所以「这一次完成了」不会影响以后每一次——这正是用户反馈的那个
    /// 问题的根：以前点一次完成，整个系列都变成已完成。
    pub fn set_done(&self, id: i64, done: bool, day: Option<&str>) -> rusqlite::Result<bool> {
        let Some(event) = self.get_event(id)? else {
            return Ok(false);
        };
        let conn = self.lock();
        if normalize_repeat(&event.repeat).is_empty() {
            conn.execute(
                "UPDATE events SET done = ?1, updated_at = ?2 WHERE id = ?3",
                params![done as i64, crate::now_str(), id],
            )?;
            return Ok(true);
        }
        let target = day
            .map(|one| one.chars().take(10).collect::<String>())
            .filter(|one| one.len() == 10)
            .unwrap_or_else(|| event.date.chars().take(10).collect::<String>());
        let mut days: Vec<String> = event
            .done_dates
            .split(',')
            .map(|part| part.trim().to_string())
            .filter(|part| !part.is_empty())
            .collect();
        if done {
            if !days.iter().any(|one| one == &target) {
                days.push(target);
            }
        } else {
            days.retain(|one| one != &target);
        }
        days.sort();
        conn.execute(
            "UPDATE events SET done_dates = ?1, done = 0, updated_at = ?2 WHERE id = ?3",
            params![days.join(","), crate::now_str(), id],
        )?;
        Ok(true)
    }

    /// 记「这一次已经提醒过了」。重复日程按天记进 `notified_dates`，不动母行的
    /// `notified`——否则提醒过一次，以后每一次都不再提醒。
    pub fn mark_notified(&self, id: i64, day: Option<&str>) -> rusqlite::Result<bool> {
        let Some(event) = self.get_event(id)? else {
            return Ok(false);
        };
        let conn = self.lock();
        if normalize_repeat(&event.repeat).is_empty() {
            conn.execute("UPDATE events SET notified = 1 WHERE id = ?1", params![id])?;
            return Ok(true);
        }
        let target = day
            .map(|one| one.chars().take(10).collect::<String>())
            .filter(|one| one.len() == 10)
            .unwrap_or_else(|| event.date.chars().take(10).collect::<String>());
        let mut days = split_dates(&event.notified_dates);
        if !days.iter().any(|one| one == &target) {
            days.push(target);
        }
        days.sort();
        conn.execute(
            "UPDATE events SET notified_dates = ?1 WHERE id = ?2",
            params![days.join(","), id],
        )?;
        Ok(true)
    }

    /// 「稍后 10 分钟」。普通日程改 `remind_at` 并把 `notified` 复位；重复日程只推迟
    /// 那一天（`snooze_until`），并把那一天从 `notified_dates` 里去掉。
    pub fn set_snooze(&self, id: i64, minutes: i64, day: Option<&str>) -> rusqlite::Result<String> {
        let Some(event) = self.get_event(id)? else {
            return Ok(String::new());
        };
        let shift = minutes.max(1);
        let target = day
            .map(|one| one.chars().take(10).collect::<String>())
            .filter(|one| one.len() == 10)
            .unwrap_or_else(|| event.date.chars().take(10).collect::<String>());
        let base = chrono::NaiveDateTime::parse_from_str(
            &format!("{} {}", target, clock5(&event.start_time)),
            "%Y-%m-%d %H:%M",
        )
        .unwrap_or_else(|_| chrono::Local::now().naive_local());
        let next = base + chrono::Duration::minutes(shift);
        let text = next.format("%Y-%m-%d %H:%M:%S").to_string();
        let conn = self.lock();
        if normalize_repeat(&event.repeat).is_empty() {
            conn.execute(
                "UPDATE events SET remind_at = ?1, notified = 0 WHERE id = ?2",
                params![text, id],
            )?;
            return Ok(text);
        }
        let days: Vec<String> = split_dates(&event.notified_dates)
            .into_iter()
            .filter(|one| one != &target)
            .collect();
        conn.execute(
            "UPDATE events SET snooze_until = ?1, notified_dates = ?2 WHERE id = ?3",
            params![text, days.join(","), id],
        )?;
        Ok(text)
    }

    /// 整条重复系列一起标完成 / 取消。`done_dates = '*'` 是「整串」的标记，
    /// 迁移时看到这一列非空就不会再把 done 还原成「只有第一天完成」。
    pub fn set_series_done(&self, id: i64, done: bool) -> rusqlite::Result<bool> {
        let conn = self.lock();
        let changed = conn.execute(
            "UPDATE events SET done = ?1, done_dates = ?2, updated_at = ?3 WHERE id = ?4",
            params![done as i64, if done { "*" } else { "" }, crate::now_str(), id],
        )?;
        Ok(changed > 0)
    }

    pub fn delete_event(&self, id: i64) -> rusqlite::Result<bool> {
        let conn = self.lock();
        let changed = conn.execute("DELETE FROM events WHERE id = ?1", params![id])?;
        Ok(changed > 0)
    }

    // ---------------------------------------------------------- 设置
    pub fn settings(&self) -> rusqlite::Result<std::collections::BTreeMap<String, String>> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut out = std::collections::BTreeMap::new();
        for row in rows {
            let (key, value) = row?;
            out.insert(key, value);
        }
        Ok(out)
    }

    pub fn setting(&self, key: &str) -> Option<String> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1").ok()?;
        let mut rows = stmt.query(params![key]).ok()?;
        let row = rows.next().ok()??;
        row.get::<_, String>(0).ok()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    // ---------------------------------------------------------- 通知 / 作业
    pub fn notices(&self, limit: i64) -> rusqlite::Result<Vec<serde_json::Value>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, title, url, source, fetched_at, published_at FROM notices
             ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "title": row.get::<_, String>(1).unwrap_or_default(),
                "url": row.get::<_, String>(2).unwrap_or_default(),
                "source": row.get::<_, String>(3).unwrap_or_default(),
                "fetched_at": row.get::<_, String>(4).unwrap_or_default(),
                "published_at": row.get::<_, String>(5).unwrap_or_default(),
            }))
        })?;
        rows.collect()
    }

    /// 通知页用：带「读没读」，可只看未读
    pub fn notice_list(&self, limit: i64, unread_only: bool)
        -> rusqlite::Result<Vec<serde_json::Value>> {
        let conn = self.lock();
        let sql = if unread_only {
            "SELECT id, title, url, source, fetched_at, published_at, read_at
             FROM notices WHERE read_at IS NULL
             ORDER BY COALESCE(NULLIF(published_at, ''), fetched_at) DESC, id DESC
             LIMIT ?1"
        } else {
            "SELECT id, title, url, source, fetched_at, published_at, read_at
             FROM notices
             ORDER BY COALESCE(NULLIF(published_at, ''), fetched_at) DESC, id DESC
             LIMIT ?1"
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "title": row.get::<_, String>(1).unwrap_or_default(),
                "url": row.get::<_, String>(2).unwrap_or_default(),
                "source": row.get::<_, String>(3).unwrap_or_default(),
                "fetched_at": row.get::<_, String>(4).unwrap_or_default(),
                "published_at": row.get::<_, String>(5).unwrap_or_default(),
                "read_at": row.get::<_, Option<String>>(6).unwrap_or_default(),
            }))
        })?;
        rows.collect()
    }

    pub fn mark_notice_read(&self, id: i64) -> rusqlite::Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE notices SET read_at = COALESCE(read_at, ?2) WHERE id = ?1",
            params![id, crate::now_str()],
        )?;
        Ok(())
    }

    pub fn mark_all_notices_read(&self) -> rusqlite::Result<i64> {
        let conn = self.lock();
        let changed = conn.execute(
            "UPDATE notices SET read_at = ?1 WHERE read_at IS NULL",
            params![crate::now_str()],
        )?;
        Ok(changed as i64)
    }

    pub fn notice_unread_count(&self) -> i64 {
        let conn = self.lock();
        conn.query_row(
            "SELECT COUNT(*) FROM notices WHERE read_at IS NULL",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
    }

    pub fn add_notice(&self, source: &str, title: &str, url: &str,
                      published: &str) -> rusqlite::Result<bool> {
        let conn = self.lock();
        // 同一来源 + 同一标题 + 同一链接：已经有了就不重复插
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM notices WHERE source = ?1 AND title = ?2 AND url = ?3",
            params![source, title, url],
            |row| row.get(0),
        )?;
        if exists > 0 {
            return Ok(false);
        }
        conn.execute(
            "INSERT INTO notices(source, title, url, fetched_at, published_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![source, title, url, crate::now_str(), published],
        )?;
        Ok(true)
    }

    // ---------------------------------------------------------- 服务器公告
    /// 已经收到过的最大公告 id（没有就是 0），增量拉取用
    pub fn announcement_max_id(&self) -> i64 {
        let conn = self.lock();
        conn.query_row("SELECT COALESCE(MAX(id), 0) FROM announcements", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap_or(0)
    }

    /// 落一条服务器公告（服务器 id 做主键，重复拉不会重复插）；返回是否新插入
    pub fn add_announcement(&self, item: &serde_json::Value) -> rusqlite::Result<bool> {
        let id = item.get("id").and_then(|one| one.as_i64()).unwrap_or(0);
        if id <= 0 {
            return Ok(false);
        }
        let text = |key: &str| {
            item.get(key)
                .and_then(|one| one.as_str())
                .unwrap_or_default()
                .to_string()
        };
        let pinned = item
            .get("pinned")
            .map(|one| one.as_bool().unwrap_or(false) || one.as_i64().unwrap_or(0) != 0)
            .unwrap_or(false);
        let conn = self.lock();
        let changed = conn.execute(
            "INSERT INTO announcements(id, title, body, level, pinned, created_at, expires_at,
                    received_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO NOTHING",
            params![
                id,
                text("title"),
                text("body"),
                if text("level").is_empty() { "info".to_string() } else { text("level") },
                pinned as i64,
                text("created_at"),
                text("expires_at"),
                crate::now_str(),
            ],
        )?;
        Ok(changed > 0)
    }

    /// 通知页「服务器公告」分区 / 标题栏弹层用（新→旧，置顶排前面）
    pub fn announcements(&self, limit: i64) -> rusqlite::Result<Vec<serde_json::Value>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, title, body, level, pinned, created_at, expires_at, received_at, read_at
             FROM announcements
             ORDER BY pinned DESC, id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "title": row.get::<_, String>(1).unwrap_or_default(),
                "body": row.get::<_, String>(2).unwrap_or_default(),
                "level": row.get::<_, String>(3).unwrap_or_default(),
                "pinned": row.get::<_, i64>(4).unwrap_or(0) != 0,
                "created_at": row.get::<_, String>(5).unwrap_or_default(),
                "expires_at": row.get::<_, String>(6).unwrap_or_default(),
                "received_at": row.get::<_, String>(7).unwrap_or_default(),
                "read_at": row.get::<_, Option<String>>(8).unwrap_or_default(),
            }))
        })?;
        rows.collect()
    }

    pub fn announcement_unread_count(&self) -> i64 {
        let conn = self.lock();
        conn.query_row(
            "SELECT COUNT(*) FROM announcements WHERE read_at IS NULL",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
    }

    /// `id=None` 表示全部标为已读；返回改动条数
    pub fn mark_announcements_read(&self, id: Option<i64>) -> rusqlite::Result<i64> {
        let conn = self.lock();
        let changed = match id {
            Some(one) => conn.execute(
                "UPDATE announcements SET read_at = COALESCE(read_at, ?2) WHERE id = ?1",
                params![one, crate::now_str()],
            )?,
            None => conn.execute(
                "UPDATE announcements SET read_at = ?1 WHERE read_at IS NULL",
                params![crate::now_str()],
            )?,
        };
        Ok(changed as i64)
    }

    // ---------------------------------------------------------- 备忘
    pub fn memos(&self) -> rusqlite::Result<Vec<serde_json::Value>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, title, html, created_at, updated_at FROM memos
             ORDER BY updated_at DESC, id DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "title": row.get::<_, String>(1).unwrap_or_default(),
                "html": row.get::<_, String>(2).unwrap_or_default(),
                "created_at": row.get::<_, String>(3).unwrap_or_default(),
                "updated_at": row.get::<_, String>(4).unwrap_or_default(),
            }))
        })?;
        rows.collect()
    }

    pub fn add_memo(&self, title: &str, html: &str) -> rusqlite::Result<i64> {
        let now = crate::now_str();
        let conn = self.lock();
        conn.execute(
            "INSERT INTO memos(title, html, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
            params![title, html, now],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn update_memo(&self, id: i64, title: &str, html: &str) -> rusqlite::Result<bool> {
        let conn = self.lock();
        let changed = conn.execute(
            "UPDATE memos SET title = ?1, html = ?2, updated_at = ?3 WHERE id = ?4",
            params![title, html, crate::now_str(), id],
        )?;
        Ok(changed > 0)
    }

    pub fn delete_memo(&self, id: i64) -> rusqlite::Result<bool> {
        let conn = self.lock();
        Ok(conn.execute("DELETE FROM memos WHERE id = ?1", params![id])? > 0)
    }

    // ---------------------------------------------------------- 联系人
    pub fn contacts(&self) -> rusqlite::Result<Vec<serde_json::Value>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, name, email, created_at FROM contacts ORDER BY name ASC, id ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "name": row.get::<_, String>(1).unwrap_or_default(),
                "email": row.get::<_, String>(2).unwrap_or_default(),
                "created_at": row.get::<_, String>(3).unwrap_or_default(),
            }))
        })?;
        rows.collect()
    }

    pub fn add_contact(&self, name: &str, email: &str) -> rusqlite::Result<i64> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO contacts(name, email, created_at) VALUES (?1, ?2, ?3)",
            params![name, email, crate::now_str()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn delete_contact(&self, id: i64) -> rusqlite::Result<bool> {
        let conn = self.lock();
        Ok(conn.execute("DELETE FROM contacts WHERE id = ?1", params![id])? > 0)
    }

    // ---------------------------------------------------------- 发件箱
    pub fn mails(&self, limit: i64) -> rusqlite::Result<Vec<serde_json::Value>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, to_addr, subject, html, text, tasks_json, attachments_json,
                    note, status, error, created_at, sent_at
             FROM mails ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "to": row.get::<_, String>(1).unwrap_or_default(),
                "subject": row.get::<_, String>(2).unwrap_or_default(),
                "html": row.get::<_, String>(3).unwrap_or_default(),
                "text": row.get::<_, String>(4).unwrap_or_default(),
                "tasks_json": row.get::<_, String>(5).unwrap_or_default(),
                "attachments": row.get::<_, String>(6).unwrap_or_default(),
                "note": row.get::<_, String>(7).unwrap_or_default(),
                "status": row.get::<_, String>(8).unwrap_or_default(),
                "error": row.get::<_, String>(9).unwrap_or_default(),
                "created_at": row.get::<_, String>(10).unwrap_or_default(),
                "sent_at": row.get::<_, Option<String>>(11).unwrap_or_default(),
            }))
        })?;
        rows.collect()
    }

    pub fn save_mail(
        &self,
        to: &str,
        subject: &str,
        html: &str,
        text: &str,
        attachments_json: &str,
        status: &str,
        error: &str,
    ) -> rusqlite::Result<i64> {
        let conn = self.lock();
        let sent = if status == "sent" {
            Some(crate::now_str())
        } else {
            None
        };
        conn.execute(
            "INSERT INTO mails(to_addr, subject, html, text, tasks_json,
                    attachments_json, note, status, error, created_at, sent_at)
             VALUES (?1, ?2, ?3, ?4, '[]', ?5, '', ?6, ?7, ?8, ?9)",
            params![
                to,
                subject,
                html,
                text,
                attachments_json,
                status,
                error,
                crate::now_str(),
                sent
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn update_mail_status(
        &self,
        id: i64,
        status: &str,
        error: &str,
    ) -> rusqlite::Result<()> {
        let conn = self.lock();
        let sent = if status == "sent" {
            Some(crate::now_str())
        } else {
            None
        };
        conn.execute(
            "UPDATE mails SET status = ?1, error = ?2, sent_at = ?3 WHERE id = ?4",
            params![status, error, sent, id],
        )?;
        Ok(())
    }

    pub fn delete_mail(&self, id: i64) -> rusqlite::Result<bool> {
        let conn = self.lock();
        Ok(conn.execute("DELETE FROM mails WHERE id = ?1", params![id])? > 0)
    }
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
    fn announcements_are_deduped_and_unread_counts() {
        let store = temp_store("danlume-announce-test");
        let first = serde_json::json!({
            "id": 7, "title": "维护通知", "body": "今晚 23:00 停机",
            "level": "warning", "pinned": true,
            "created_at": "2026-01-01T10:00:00Z", "expires_at": "",
        });
        assert!(store.add_announcement(&first).unwrap(), "第一次应当插入");
        assert!(!store.add_announcement(&first).unwrap(), "同一个 id 不重复插");
        assert!(store
            .add_announcement(&serde_json::json!({
                "id": 9, "title": "新版本", "body": "", "level": "critical",
            }))
            .unwrap());
        // id <= 0 的脏数据直接丢掉
        assert!(!store.add_announcement(&serde_json::json!({ "title": "没有 id" })).unwrap());
        assert_eq!(store.announcement_max_id(), 9);
        assert_eq!(store.announcement_unread_count(), 2);

        let rows = store.announcements(10).unwrap();
        assert_eq!(rows.len(), 2);
        // 置顶的排前面
        assert_eq!(rows[0]["id"], 7);
        assert_eq!(rows[0]["level"], "warning");
        assert_eq!(rows[1]["level"], "critical");

        assert_eq!(store.mark_announcements_read(Some(7)).unwrap(), 1);
        assert_eq!(store.announcement_unread_count(), 1);
        assert_eq!(store.mark_announcements_read(None).unwrap(), 1);
        assert_eq!(store.announcement_unread_count(), 0);
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-announce-test"));
    }

    // ------------------------------------------------ 重复日程 / 提醒（新 bug 修复）

    /// 临时库 + 结束后把 `LUMINA_DATA_DIR` 还原（测试改的是进程级环境变量）
    struct EnvGuard {
        previous: Option<std::ffi::OsString>,
        dir: std::path::PathBuf,
    }

    impl EnvGuard {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "danlume-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let previous = std::env::var_os("LUMINA_DATA_DIR");
            std::env::set_var("LUMINA_DATA_DIR", &dir);
            Self { previous, dir }
        }

        fn store(&self) -> Store {
            Store::open().expect("建临时库失败")
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var("LUMINA_DATA_DIR", value),
                None => std::env::remove_var("LUMINA_DATA_DIR"),
            }
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn recurrence_rules_match_old_version() {
        // 每周：同一个星期几
        assert_eq!(
            recurrence_days("2026-09-07", "weekly", "2026-09-01", "2026-10-05"),
            vec!["2026-09-07", "2026-09-14", "2026-09-21", "2026-09-28", "2026-10-05"]
        );
        // 每月：没有 31 号的月份跳过
        assert_eq!(
            recurrence_days("2026-01-31", "monthly", "2026-01-01", "2026-06-30"),
            vec!["2026-01-31", "2026-03-31", "2026-05-31"]
        );
        // 每年：2/29 只在闰年出现
        assert_eq!(
            recurrence_days("2024-02-29", "yearly", "2024-01-01", "2029-12-31"),
            vec!["2024-02-29", "2028-02-29"]
        );
        // 不重复 / 区间在开始之前
        assert!(recurrence_days("2026-09-07", "", "2026-09-01", "2026-12-31").is_empty());
        assert!(recurrence_days("2026-09-07", "weekly", "2026-01-01", "2026-09-06").is_empty());
    }

    #[test]
    fn repeat_completion_is_per_occurrence() {
        let guard = EnvGuard::new("store-repeat");
        let store = guard.store();
        let id = store
            .add_event(&EventInput {
                title: Some("线性代数A".to_string()),
                date: Some("2026-09-07".to_string()),
                start_time: Some("08:00".to_string()),
                end_time: Some("09:40".to_string()),
                remind_minutes: Some(10),
                repeat: Some("weekly".to_string()),
                ..Default::default()
            })
            .unwrap();

        // 第一次标完成：只应影响 09-07
        store.set_done(id, true, Some("2026-09-07")).unwrap();
        let master = store.get_event(id).unwrap().expect("母事件不见了");
        assert!(!master.done, "重复日程不该把母行标成已完成");
        assert_eq!(master.done_dates, "2026-09-07");

        let rows = store.events_between("2026-09-01", "2026-10-01").unwrap();
        let dates: Vec<&str> = rows.iter().map(|one| one.date.as_str()).collect();
        assert_eq!(dates, vec!["2026-09-07", "2026-09-14", "2026-09-21", "2026-09-28"]);
        assert!(rows[0].done, "第一次应当是已完成");
        assert!(
            rows[1..].iter().all(|one| !one.done),
            "后面几次不能被带成已完成：{:?}",
            rows.iter().map(|one| (one.date.as_str(), one.done)).collect::<Vec<_>>()
        );
        assert_eq!(rows[1].repeat_of, Some(id), "展开出来的那次要指回母事件");

        // 取消完成
        store.set_done(id, false, Some("2026-09-07")).unwrap();
        assert_eq!(store.get_event(id).unwrap().unwrap().done_dates, "");
        let rows = store.events_between("2026-09-01", "2026-10-01").unwrap();
        assert!(rows.iter().all(|one| !one.done), "取消后应当又都没完成");
    }

    #[test]
    fn update_event_done_on_repeat_is_routed_to_done_dates() {
        let guard = EnvGuard::new("store-repeat-update");
        let store = guard.store();
        let id = store
            .add_event(&EventInput {
                title: Some("程序设计".to_string()),
                date: Some("2026-09-17".to_string()),
                start_time: Some("13:30".to_string()),
                end_time: Some("16:10".to_string()),
                repeat: Some("weekly".to_string()),
                ..Default::default()
            })
            .unwrap();
        // 老路径（update_event 带 done）也不能把整串带成已完成
        store
            .update_event(
                id,
                &EventInput {
                    done: Some(true),
                    ..Default::default()
                },
            )
            .unwrap();
        let master = store.get_event(id).unwrap().unwrap();
        assert!(!master.done);
        assert_eq!(master.done_dates, "2026-09-17");
        let rows = store.events_between("2026-09-01", "2026-10-10").unwrap();
        assert_eq!(rows.len(), 4);
        assert!(rows[0].done);
        assert!(rows[1..].iter().all(|one| !one.done));
    }

    #[test]
    fn legacy_poisoned_repeat_is_repaired() {
        let guard = EnvGuard::new("store-repair");
        let store = guard.store();
        let id = store
            .add_event(&EventInput {
                title: Some("中国概况".to_string()),
                date: Some("2026-10-29".to_string()),
                start_time: Some("17:00".to_string()),
                end_time: Some("18:00".to_string()),
                repeat: Some("weekly".to_string()),
                ..Default::default()
            })
            .unwrap();
        // 模拟老版本留下的坏数据：整串被标成已完成
        {
            let conn = store.connection();
            conn.execute(
                "UPDATE events SET done = 1, done_dates = '' WHERE id = ?1",
                rusqlite::params![id],
            )
            .unwrap();
        }
        // 重新打开库 → 迁移把那一次挪进 done_dates，母行复位
        let store = guard.store();
        let master = store.get_event(id).unwrap().unwrap();
        assert!(!master.done, "母行必须复位");
        assert_eq!(master.done_dates, "2026-10-29");
        let rows = store.events_between("2026-10-01", "2026-11-30").unwrap();
        assert!(rows[0].done);
        assert!(rows[1..].iter().all(|one| !one.done), "以后几次要恢复成未完成");
    }
}
