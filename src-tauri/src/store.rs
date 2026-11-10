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
    /// 某一天的所有日程（按开始时间排）
    pub fn events_on(&self, date: &str) -> rusqlite::Result<Vec<EventRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT * FROM events WHERE date = ?1 ORDER BY start_time ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![date], EventRow::from_row)?;
        rows.collect()
    }

    /// [start, end] 之间的日程，按日期分组（旧版 event_index）
    pub fn event_index(
        &self,
        start: &str,
        end: &str,
    ) -> rusqlite::Result<std::collections::BTreeMap<String, Vec<EventRow>>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT * FROM events WHERE date >= ?1 AND date <= ?2
             ORDER BY date ASC, start_time ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![start, end], EventRow::from_row)?;
        let mut out: std::collections::BTreeMap<String, Vec<EventRow>> =
            std::collections::BTreeMap::new();
        for row in rows {
            let event = row?;
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
        let conn = self.lock();
        conn.execute(
            "INSERT INTO events(title, description, date, start_time, end_time,
              remind_minutes, remind_at, notified, done, color, repeat, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,0,?8,?9,?10,?11,?11)",
            params![
                title.trim(),
                input.description.clone().unwrap_or_default(),
                date,
                start,
                end,
                remind,
                remind_at,
                input.done.unwrap_or(false) as i64,
                input.color.clone().unwrap_or_else(|| "violet".to_string()),
                input.repeat.clone().unwrap_or_default(),
                now,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn update_event(&self, id: i64, input: &EventInput) -> rusqlite::Result<bool> {
        let Some(current) = self.get_event(id)? else {
            return Ok(false);
        };
        let prev_date = current.date.clone();
        let prev_start = current.start_time.clone();
        let title = input.title.clone().unwrap_or(current.title);
        let date = input.date.clone().unwrap_or(current.date);
        let start = input.start_time.clone().unwrap_or(current.start_time);
        let end = input.end_time.clone().unwrap_or(current.end_time);
        let remind = if input.clear_remind {
            None
        } else {
            input.remind_minutes.or(current.remind_minutes)
        };
        let remind_at = remind.map(|minutes| crate::compute_remind_at(&date, &start, minutes));
        let time_changed = date != prev_date || start != prev_start;
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
                input.done.unwrap_or(current.done) as i64,
                input.color.clone().unwrap_or(current.color),
                input.repeat.clone().unwrap_or(current.repeat),
                crate::now_str(),
                time_changed as i64,
                id,
            ],
        )?;
        Ok(true)
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
}
