//! 学生工作：通讯录 + 定时通知。
//!
//! 用户 2026-11-12 凌晨提的需求：
//! * 设置里新增「通讯录」：姓名 / 职位 / 邮箱 / 手机号 / 微信备注名 / QQ / 其它联系方式；
//! * **定时给某（些）同学、通过某渠道（邮箱 / 微信 / 自定义）发某内容**；
//! * AI 页新增「允许读取通讯录」，默认关，勾选状态存进设置。
//!
//! 说明：微信**没有开放的发消息接口**（个人号自动化属于外挂，风险高且违反平台规则），
//! 所以「微信」渠道做的是「把内容整理好 + 一键复制」，由用户自己在微信里粘贴发送；
//! 邮箱走现有的 SMTP，自定义渠道走 webhook（POST JSON）。
use serde_json::{json, Value};

use crate::store::Store;

fn ensure_schema(store: &Store) {
    let conn = store.connection();
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS student_contacts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL DEFAULT '',
            title TEXT NOT NULL DEFAULT '',
            email TEXT NOT NULL DEFAULT '',
            phone TEXT NOT NULL DEFAULT '',
            wechat TEXT NOT NULL DEFAULT '',
            qq TEXT NOT NULL DEFAULT '',
            extra TEXT NOT NULL DEFAULT '',
            note TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS student_notices (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL DEFAULT '',
            body TEXT NOT NULL DEFAULT '',
            channel TEXT NOT NULL DEFAULT 'email',
            targets TEXT NOT NULL DEFAULT '[]',
            send_at TEXT NOT NULL DEFAULT '',
            repeat_rule TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'pending',
            last_error TEXT NOT NULL DEFAULT '',
            last_sent_at TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_student_contacts_name ON student_contacts(name)",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_student_notices_at ON student_notices(send_at)",
        [],
    );
}

fn text_of(item: &Value, key: &str) -> String {
    item.get(key)
        .and_then(|one| one.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

// ------------------------------------------------------------------ 通讯录

fn row_to_json(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(json!({
        "id": row.get::<_, i64>(0)?,
        "name": row.get::<_, String>(1).unwrap_or_default(),
        "title": row.get::<_, String>(2).unwrap_or_default(),
        "email": row.get::<_, String>(3).unwrap_or_default(),
        "phone": row.get::<_, String>(4).unwrap_or_default(),
        "wechat": row.get::<_, String>(5).unwrap_or_default(),
        "qq": row.get::<_, String>(6).unwrap_or_default(),
        "extra": row.get::<_, String>(7).unwrap_or_default(),
        "note": row.get::<_, String>(8).unwrap_or_default(),
        "updated_at": row.get::<_, String>(9).unwrap_or_default(),
    }))
}

/// 通讯录列表；`keyword` 非空时按姓名 / 职位 / 邮箱 / 微信 / 手机号模糊搜
pub fn contacts(store: &Store, keyword: &str) -> Vec<Value> {
    ensure_schema(store);
    let conn = store.connection();
    let key = format!("%{}%", keyword.trim());
    let sql = "SELECT id, name, title, email, phone, wechat, qq, extra, note, updated_at
               FROM student_contacts
               WHERE ?1 = '%%' OR name LIKE ?1 OR title LIKE ?1 OR email LIKE ?1
                     OR wechat LIKE ?1 OR phone LIKE ?1 OR qq LIKE ?1
               ORDER BY name COLLATE NOCASE";
    let Ok(mut stmt) = conn.prepare(sql) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([key], row_to_json) else {
        return Vec::new();
    };
    rows.flatten().collect()
}

/// 新增 / 修改一个联系人（`id = 0` 表示新增）
pub fn save_contact(store: &Store, item: &Value) -> Result<i64, String> {
    ensure_schema(store);
    let id = item.get("id").and_then(|one| one.as_i64()).unwrap_or(0);
    let name = text_of(item, "name");
    if name.is_empty() {
        return Err("姓名不能为空".to_string());
    }
    let now = crate::now_str();
    let conn = store.connection();
    if id > 0 {
        conn.execute(
            "UPDATE student_contacts SET name=?1, title=?2, email=?3, phone=?4, wechat=?5, qq=?6,
                    extra=?7, note=?8, updated_at=?9 WHERE id=?10",
            rusqlite::params![
                name,
                text_of(item, "title"),
                text_of(item, "email"),
                text_of(item, "phone"),
                text_of(item, "wechat"),
                text_of(item, "qq"),
                text_of(item, "extra"),
                text_of(item, "note"),
                now,
                id
            ],
        )
        .map_err(|err| err.to_string())?;
        Ok(id)
    } else {
        conn.execute(
            "INSERT INTO student_contacts(name, title, email, phone, wechat, qq, extra, note,
                    created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?9)",
            rusqlite::params![
                name,
                text_of(item, "title"),
                text_of(item, "email"),
                text_of(item, "phone"),
                text_of(item, "wechat"),
                text_of(item, "qq"),
                text_of(item, "extra"),
                text_of(item, "note"),
                now
            ],
        )
        .map_err(|err| err.to_string())?;
        Ok(conn.last_insert_rowid())
    }
}

pub fn delete_contact(store: &Store, id: i64) -> Result<(), String> {
    ensure_schema(store);
    let conn = store.connection();
    conn.execute("DELETE FROM student_contacts WHERE id = ?1", [id])
        .map_err(|err| err.to_string())?;
    Ok(())
}

/// AI 能不能读通讯录（默认**不能**；勾选状态存在设置里，不用每次重设）
pub fn ai_allowed(store: &Store) -> bool {
    store
        .setting("ai_contacts_allowed")
        .map(|text| text.trim() == "1")
        .unwrap_or(false)
}

pub fn set_ai_allowed(store: &Store, on: bool) -> Result<bool, String> {
    store
        .set_setting("ai_contacts_allowed", if on { "1" } else { "0" })
        .map_err(|err| err.to_string())?;
    Ok(on)
}

/// 给 AI 工具用：按关键词找人（只在用户允许读通讯录时才会被调用）
pub fn search_for_ai(store: &Store, keyword: &str, limit: usize) -> Vec<Value> {
    let mut rows = contacts(store, keyword);
    rows.truncate(limit.max(1));
    rows
}

// ------------------------------------------------------------------ 定时通知

fn notice_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let targets: String = row.get::<_, String>(4).unwrap_or_default();
    Ok(json!({
        "id": row.get::<_, i64>(0)?,
        "title": row.get::<_, String>(1).unwrap_or_default(),
        "body": row.get::<_, String>(2).unwrap_or_default(),
        "channel": row.get::<_, String>(3).unwrap_or_default(),
        "targets": serde_json::from_str::<Value>(&targets).unwrap_or(json!([])),
        "send_at": row.get::<_, String>(5).unwrap_or_default(),
        "repeat": row.get::<_, String>(6).unwrap_or_default(),
        "status": row.get::<_, String>(7).unwrap_or_default(),
        "last_error": row.get::<_, String>(8).unwrap_or_default(),
        "last_sent_at": row.get::<_, String>(9).unwrap_or_default(),
    }))
}

pub fn notices(store: &Store) -> Vec<Value> {
    ensure_schema(store);
    let conn = store.connection();
    let Ok(mut stmt) = conn.prepare(
        "SELECT id, title, body, channel, targets, send_at, repeat_rule, status,
                last_error, last_sent_at
         FROM student_notices ORDER BY send_at, id",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([], notice_row) else {
        return Vec::new();
    };
    rows.flatten().collect()
}

/// 新增 / 修改一条定时通知
pub fn save_notice(store: &Store, item: &Value) -> Result<i64, String> {
    ensure_schema(store);
    let id = item.get("id").and_then(|one| one.as_i64()).unwrap_or(0);
    let title = text_of(item, "title");
    let body = text_of(item, "body");
    let channel = {
        let value = text_of(item, "channel");
        if value.is_empty() {
            "email".to_string()
        } else {
            value
        }
    };
    let send_at = text_of(item, "send_at");
    if title.is_empty() {
        return Err("标题不能为空".to_string());
    }
    if send_at.is_empty() {
        return Err("要填发送时间".to_string());
    }
    let repeat_rule = text_of(item, "repeat");
    let targets = item
        .get("targets")
        .cloned()
        .unwrap_or_else(|| json!([]))
        .to_string();
    let targets = if targets.trim().is_empty() {
        "[]".to_string()
    } else {
        targets
    };
    let now = crate::now_str();
    let conn = store.connection();
    if id > 0 {
        conn.execute(
            "UPDATE student_notices SET title=?1, body=?2, channel=?3, targets=?4, send_at=?5,
                    repeat_rule=?6, status='pending', last_error='', updated_at=?7 WHERE id=?8",
            rusqlite::params![title, body, channel, targets, send_at, repeat_rule, now, id],
        )
        .map_err(|err| err.to_string())?;
        Ok(id)
    } else {
        conn.execute(
            "INSERT INTO student_notices(title, body, channel, targets, send_at, repeat_rule,
                    status, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,'pending',?7,?7)",
            rusqlite::params![title, body, channel, targets, send_at, repeat_rule, now],
        )
        .map_err(|err| err.to_string())?;
        Ok(conn.last_insert_rowid())
    }
}

pub fn delete_notice(store: &Store, id: i64) -> Result<(), String> {
    ensure_schema(store);
    let conn = store.connection();
    conn.execute("DELETE FROM student_notices WHERE id = ?1", [id])
        .map_err(|err| err.to_string())?;
    Ok(())
}

/// 把「收件人」摊平成 `(显示名, 地址)`：
/// `{"type":"contact","id":3}` 去通讯录里查；`{"type":"raw","address":"a@b.com"}` 直接用
fn resolve_targets(store: &Store, targets: &Value) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let Some(rows) = targets.as_array() else {
        return out;
    };
    for row in rows {
        let kind = row.get("type").and_then(|one| one.as_str()).unwrap_or("raw");
        if kind == "contact" {
            let id = row.get("id").and_then(|one| one.as_i64()).unwrap_or(0);
            let conn = store.connection();
            let found: Option<(String, String, String, String)> = conn
                .query_row(
                    "SELECT name, email, wechat, extra FROM student_contacts WHERE id = ?1",
                    [id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0).unwrap_or_default(),
                            row.get::<_, String>(1).unwrap_or_default(),
                            row.get::<_, String>(2).unwrap_or_default(),
                            row.get::<_, String>(3).unwrap_or_default(),
                        ))
                    },
                )
                .ok();
            drop(conn);
            if let Some((name, email, wechat, extra)) = found {
                // 渠道不同取不同字段：邮箱渠道用 email，微信渠道用备注名（只用于显示），
                // 自定义渠道用「其它联系方式」里填的地址
                let address = if !email.trim().is_empty() {
                    email
                } else if !extra.trim().is_empty() {
                    extra
                } else {
                    wechat
                };
                if !address.trim().is_empty() {
                    out.push((name, address));
                }
            }
        } else {
            let address = row
                .get("address")
                .and_then(|one| one.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if !address.is_empty() {
                let label = row
                    .get("label")
                    .and_then(|one| one.as_str())
                    .unwrap_or(&address)
                    .to_string();
                out.push((label, address));
            }
        }
    }
    out
}

/// 真正发一条：邮箱走 SMTP，webhook 走 POST，微信只生成文本（等用户手动发）
fn deliver(store: &Store, notice: &Value) -> (usize, Vec<String>) {
    let channel = notice
        .get("channel")
        .and_then(|one| one.as_str())
        .unwrap_or("email")
        .to_string();
    let title = notice
        .get("title")
        .and_then(|one| one.as_str())
        .unwrap_or("")
        .to_string();
    let body = notice
        .get("body")
        .and_then(|one| one.as_str())
        .unwrap_or("")
        .to_string();
    let targets = resolve_targets(
        store,
        notice.get("targets").unwrap_or(&json!([])),
    );
    let mut sent = 0usize;
    let mut errors: Vec<String> = Vec::new();
    if targets.is_empty() {
        return (0, vec!["没有选收件人".to_string()]);
    }
    for (label, address) in targets {
        match channel.as_str() {
            "email" => {
                if !address.contains('@') {
                    errors.push(format!("{label} 没有邮箱地址"));
                    continue;
                }
                let html = body.replace('\n', "<br>");
                match crate::mail::send(store, &address, &title, &html, &[]) {
                    Ok(()) => sent += 1,
                    Err(err) => errors.push(format!("{label}：{err}")),
                }
            }
            "webhook" => {
                let payload = json!({ "title": title, "body": body });
                match post_json(&address, &payload) {
                    Ok(()) => sent += 1,
                    Err(err) => errors.push(format!("{label}：{err}")),
                }
            }
            // 微信：没有可靠的自动化接口，交给用户手动发（前端有「复制内容」）
            _ => {
                errors.push(format!("{label}：微信渠道需要你自己粘贴发送"));
            }
        }
    }
    (sent, errors)
}

/// 自定义渠道：往一个 URL 发 POST JSON（webhook）
fn post_json(url: &str, payload: &Value) -> Result<(), String> {
    let target = url.trim();
    if !(target.starts_with("http://") || target.starts_with("https://")) {
        return Err("webhook 地址要以 http(s):// 开头".to_string());
    }
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("DanLume/2.0")
        .build();
    match agent
        .post(target)
        .set("Content-Type", "application/json")
        .send_string(&payload.to_string())
    {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(code, _)) => Err(format!("服务器返回 {code}")),
        Err(err) => Err(format!("请求失败：{err}")),
    }
}

/// 到点该发的都发掉（后台每 30 秒调一次）；返回这一次发成功的条数
pub fn run_due(store: &Store) -> usize {
    ensure_schema(store);
    let now = crate::now_str();
    let due: Vec<Value> = {
        let conn = store.connection();
        let mut out = Vec::new();
        if let Ok(mut stmt) = conn.prepare(
            "SELECT id, title, body, channel, targets, send_at, repeat_rule, status,
                    last_error, last_sent_at
             FROM student_notices
             WHERE status = 'pending' AND send_at <> '' AND send_at <= ?1
             ORDER BY send_at LIMIT 20",
        ) {
            if let Ok(rows) = stmt.query_map([&now], notice_row) {
                out = rows.flatten().collect();
            }
        }
        out
    };
    let mut done = 0usize;
    for notice in due {
        done += send_one(store, &notice, true);
    }
    done
}

/// 发一条（`scheduled = true` 时按「重复规则」自动排下一次）
pub fn send_one(store: &Store, notice: &Value, scheduled: bool) -> usize {
    let id = notice.get("id").and_then(|one| one.as_i64()).unwrap_or(0);
    let (sent, errors) = deliver(store, notice);
    let now = crate::now_str();
    let status = if sent > 0 { "sent" } else { "failed" };
    let error = errors.join("；");
    let repeat = notice
        .get("repeat")
        .and_then(|one| one.as_str())
        .unwrap_or("")
        .to_string();
    let send_at = notice
        .get("send_at")
        .and_then(|one| one.as_str())
        .unwrap_or("")
        .to_string();
    let conn = store.connection();
    if scheduled && !repeat.is_empty() && sent > 0 {
        // 重复任务：把 send_at 往后推，状态留在 pending
        let next = next_time(&send_at, &repeat);
        let _ = conn.execute(
            "UPDATE student_notices SET status='pending', last_sent_at=?1, last_error=?2,
                    send_at=?3, updated_at=?1 WHERE id=?4",
            rusqlite::params![now, error, next, id],
        );
    } else {
        let _ = conn.execute(
            "UPDATE student_notices SET status=?1, last_sent_at=?2, last_error=?3,
                    updated_at=?2 WHERE id=?4",
            rusqlite::params![status, now, error, id],
        );
    }
    sent
}

/// 下一次的时间（按天 / 按周往后推，推过当前时间为止）
fn next_time(send_at: &str, repeat: &str) -> String {
    let Ok(base) = chrono::NaiveDateTime::parse_from_str(send_at, "%Y-%m-%d %H:%M") else {
        return send_at.to_string();
    };
    let step = match repeat {
        "daily" => chrono::Duration::days(1),
        "weekly" => chrono::Duration::days(7),
        _ => return send_at.to_string(),
    };
    let now = chrono::Local::now().naive_local();
    let mut next = base + step;
    while next <= now {
        next += step;
    }
    next.format("%Y-%m-%d %H:%M").to_string()
}

/// 手动「立刻发一次」：不看时间，直接把这条发出去
pub fn send_now(store: &Store, id: i64) -> Value {
    let notice = notices(store)
        .into_iter()
        .find(|one| one.get("id").and_then(|v| v.as_i64()) == Some(id));
    let Some(notice) = notice else {
        return json!({ "ok": false, "sent": 0, "error": "没找到这条通知" });
    };
    let sent = send_one(store, &notice, false);
    let after = notices(store)
        .into_iter()
        .find(|one| one.get("id").and_then(|v| v.as_i64()) == Some(id))
        .unwrap_or(json!({}));
    let error = after
        .get("last_error")
        .and_then(|one| one.as_str())
        .unwrap_or("")
        .to_string();
    json!({ "ok": sent > 0, "sent": sent, "error": error })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempData {
        previous: Option<std::ffi::OsString>,
        dir: std::path::PathBuf,
    }

    impl TempData {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "danlume-student-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let previous = std::env::var_os("LUMINA_DATA_DIR");
            std::env::set_var("LUMINA_DATA_DIR", &dir);
            Self { previous, dir }
        }
    }

    impl Drop for TempData {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var("LUMINA_DATA_DIR", value),
                None => std::env::remove_var("LUMINA_DATA_DIR"),
            }
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn contacts_crud_and_search() {
        let temp = TempData::new("contacts");
        let store = Store::open().expect("临时库");
        let id = save_contact(
            &store,
            &json!({ "name": "张老师", "title": "辅导员", "email": "zhang@fudan.edu.cn",
                      "phone": "13800000000", "wechat": "张老师（微信）", "qq": "123456" }),
        )
        .expect("存联系人");
        assert!(id > 0);
        let rows = contacts(&store, "张");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["title"], json!("辅导员"));
        // 按邮箱也能搜到
        assert_eq!(contacts(&store, "zhang@").len(), 1);
        // 改一次
        save_contact(&store, &json!({ "id": id, "name": "张老师", "title": "院系辅导员" }))
            .expect("改联系人");
        assert_eq!(contacts(&store, "张")[0]["title"], json!("院系辅导员"));
        // 删掉
        delete_contact(&store, id).expect("删联系人");
        assert!(contacts(&store, "").is_empty());
        drop(store);
    }

    #[test]
    fn ai_permission_defaults_off_and_is_remembered() {
        let temp = TempData::new("contacts-perm");
        let store = Store::open().expect("临时库");
        assert!(!ai_allowed(&store), "默认不允许 AI 读通讯录");
        set_ai_allowed(&store, true).expect("打开");
        assert!(ai_allowed(&store));
        // 重新打开库（模拟重启）后依然是开的
        drop(store);
        let again = Store::open().expect("再开一次");
        assert!(ai_allowed(&again), "勾选状态要记住，不能每次重设");
        drop(again);
    }

    #[test]
    fn notice_saves_and_marks_pending() {
        let temp = TempData::new("notices");
        let store = Store::open().expect("临时库");
        let id = save_notice(
            &store,
            &json!({ "title": "周五交材料", "body": "记得带学生证", "channel": "email",
                      "send_at": "2099-01-01 09:00",
                      "targets": [{ "type": "raw", "address": "a@b.com", "label": "测试" }] }),
        )
        .expect("存通知");
        let rows = notices(&store);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["status"], json!("pending"));
        assert_eq!(rows[0]["channel"], json!("email"));
        // 缺时间要报错
        assert!(save_notice(&store, &json!({ "title": "x" })).is_err());
        delete_notice(&store, id).expect("删通知");
        assert!(notices(&store).is_empty());
        drop(store);
    }

    #[test]
    fn repeat_advances_to_future() {
        let next = next_time("2020-01-01 09:00", "daily");
        assert!(next > "2026-01-01".to_string(), "重复任务要推到将来：{next}");
        assert_eq!(next_time("2020-01-01 09:00", ""), "2020-01-01 09:00");
    }
}
