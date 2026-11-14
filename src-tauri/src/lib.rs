//! Tauri 后端入口：命令注册 + 数据层单例。
mod adminsign;
mod ai;
mod ai_files;
mod ai_tools;
mod ai_tools_net;
mod account;
mod backup;
mod bounce;
mod canvas;
mod classroom;
mod dashboard;
mod docs;
mod elearn;
mod ehall_grab;
mod embed;
mod exams;
mod endnote;
mod ehall_remote;
mod focus;
mod geo;
mod ics;
mod latex_export;
mod mail;
mod mail_identity;
mod memory;
mod net;
mod notices;
mod notices_auto;
mod notify;
mod papers;
mod paths;
mod prompts;
mod refs;
mod registration;
mod reminders;
mod sandbox;
mod secrets;
mod semantic;
mod store;
mod student;
mod summary;
#[cfg(test)]
mod testdata;
mod timetable_remote;
mod uis;
mod uis_autofill;
mod usage;
mod weather;
mod wechat;
mod zipx;

use std::sync::OnceLock;

use chrono::{Duration, Local, NaiveTime};
use serde_json::{json, Value};
use tauri::{Emitter, Manager, State};

use store::{EventInput, EventRow, Store};

static STORE: OnceLock<Store> = OnceLock::new();

fn get_store() -> &'static Store {
    STORE.get_or_init(|| Store::open().expect("打不开数据库（data/lumina.db）"))
}

/// 当前时间字符串（旧版 now_str 的格式）
pub fn now_str() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

pub fn today_str() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

/// 提醒时间 = 开始时间 - 提前分钟（与旧版 store.compute_remind_at 一致）
pub fn compute_remind_at(date: &str, start: &str, minutes: i64) -> String {
    let base = format!("{} {}", date, start);
    let parsed = chrono::NaiveDateTime::parse_from_str(&base, "%Y-%m-%d %H:%M")
        .or_else(|_| {
            let fallback = format!("{} 00:00", date);
            chrono::NaiveDateTime::parse_from_str(&fallback, "%Y-%m-%d %H:%M")
        })
        .unwrap_or_else(|_| Local::now().naive_local());
    (parsed - Duration::minutes(minutes))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

// ------------------------------------------------------------------ 命令

#[tauri::command]
fn events_on(date: String, state: State<'_, AppState>) -> Result<Vec<EventRow>, String> {
    let _ = &state;
    get_store().events_on(&date).map_err(|err| err.to_string())
}

#[tauri::command]
fn event_index(
    start: String,
    end: String,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let _ = &state;
    let index = get_store()
        .event_index(&start, &end)
        .map_err(|err| err.to_string())?;
    Ok(serde_json::to_value(index).unwrap_or(json!({})))
}

#[tauri::command]
fn upcoming_events(
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<Vec<dashboard::UpcomingItem>, String> {
    let _ = &state;
    Ok(dashboard::upcoming_events(
        get_store(),
        limit.unwrap_or(5),
        3,
    ))
}

#[tauri::command]
fn week_number(state: State<'_, AppState>) -> i64 {
    let _ = &state;
    dashboard::week_number(get_store())
}

#[tauri::command]
fn add_event(payload: EventInput, state: State<'_, AppState>) -> Result<i64, String> {
    let _ = &state;
    let store = get_store();
    // 旧版 group_color / default_event_group：没指定颜色时用「分组颜色」
    let mut payload = payload;
    let empty_color = payload
        .color
        .as_deref()
        .map(|text| text.trim().is_empty())
        .unwrap_or(true);
    if empty_color {
        if let Some(color) = store
            .setting("group_color")
            .filter(|text| !text.trim().is_empty())
        {
            payload.color = Some(color.trim().to_string());
        }
    }
    store.add_event(&payload).map_err(|err| err.to_string())
}

#[tauri::command]
fn update_event(
    id: i64,
    patch: EventInput,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let _ = &state;
    get_store()
        .update_event(id, &patch)
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn delete_event(id: i64, state: State<'_, AppState>) -> Result<bool, String> {
    let _ = &state;
    get_store().delete_event(id).map_err(|err| err.to_string())
}

/// 标记完成 / 取消完成。`occurrence_date` 给的是「哪一天的那一次」，
/// 重复日程只影响那一天，不会把整条系列都标成已完成。
#[tauri::command]
fn event_set_done(
    id: i64,
    done: bool,
    occurrence_date: Option<String>,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let _ = &state;
    get_store()
        .set_done(id, done, occurrence_date.as_deref())
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn get_settings(
    state: State<'_, AppState>,
) -> Result<std::collections::BTreeMap<String, String>, String> {
    let _ = &state;
    get_store().settings().map_err(|err| err.to_string())
}

#[tauri::command]
fn set_setting(key: String, value: String, state: State<'_, AppState>) -> Result<(), String> {
    let _ = &state;
    get_store()
        .set_setting(&key, &value)
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn recent_notices(
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    let _ = &state;
    get_store()
        .notices(limit.unwrap_or(5))
        .map_err(|err| err.to_string())
}

/// 同步「最近作业」：拉一次 eLearning 的 To Do，按旧版格式写回缓存。
///
/// 旧版是 `elearning.todo_items()` + `dashboard.save_assignments()`；
/// 缓存键 `homework_assignments`（JSON 数组）+ `homework_assignments_at`（epoch 秒），
/// 作业页和仪表板「最近作业」卡片共用这一份。
#[tauri::command]
async fn homework_sync() -> Result<serde_json::Value, String> {
    let handle = tauri::async_runtime::spawn_blocking(move || {
        let store = get_store();
        match crate::canvas::todo_items(store) {
            Ok(items) => {
                let text = serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string());
                let _ = store.set_setting("homework_assignments", &text);
                let stamp = chrono::Utc::now().timestamp().to_string();
                let _ = store.set_setting("homework_assignments_at", &stamp);
                json!({ "ok": true, "items": items, "at": stamp })
            }
            Err(exc) => json!({ "ok": false, "error": exc, "items": [] }),
        }
    })
    .await;
    match handle {
        Ok(value) => Ok(value),
        Err(exc) => Ok(json!({ "ok": false, "error": format!("同步失败：{exc}"), "items": [] })),
    }
}

// ------------------------------------------------------------------ 重要日（倒计时）
//
// 对应旧版 `important.py`：表 `important_dates` 早就在 schema 里（AI 工具也能写），
// 但 Rust 一直**没有界面**能看 / 加 / 删。这里补上三个命令，前端挂在日历页右侧。

/// 距离今天还有几天（负数=已经过了）
fn days_left(date_text: &str) -> i64 {
    let Ok(target) = chrono::NaiveDate::parse_from_str(date_text.trim(), "%Y-%m-%d") else {
        return 0;
    };
    (target - chrono::Local::now().date_naive()).num_days()
}

/// 旧版 `important.label` 的中文说法
fn days_label(days: i64) -> String {
    match days {
        0 => "今天".to_string(),
        1 => "明天".to_string(),
        2 => "后天".to_string(),
        d if d > 0 => format!("{d} 天后"),
        d => format!("已过 {} 天", d.abs()),
    }
}

/// 解析用户填的日期（支持「今天 / 明天 / 后天 / +7」这种说法，与旧版 `parse_day` 一致）
fn parse_important_day(text: &str) -> String {
    use chrono::Datelike;
    let raw = text.trim();
    let today = chrono::Local::now().date_naive();
    let relative = match raw {
        "今天" | "today" => Some(0),
        "明天" | "tomorrow" => Some(1),
        "后天" => Some(2),
        _ => raw
            .strip_prefix('+')
            .and_then(|rest| rest.trim().parse::<i64>().ok()),
    };
    if let Some(offset) = relative {
        return (today + chrono::Duration::days(offset))
            .format("%Y-%m-%d")
            .to_string();
    }
    for fmt in ["%Y-%m-%d", "%Y/%m/%d", "%Y%m%d"] {
        if let Ok(day) = chrono::NaiveDate::parse_from_str(raw, fmt) {
            return day.format("%Y-%m-%d").to_string();
        }
    }
    // 「6-20」这种只写月日的：补上今年
    if let Ok(day) = chrono::NaiveDate::parse_from_str(&format!("{}-{raw}", today.year()), "%Y-%m-%d")
    {
        return day.format("%Y-%m-%d").to_string();
    }
    raw.to_string()
}

fn important_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let date: String = row.get("date").unwrap_or_default();
    let days = days_left(&date);
    Ok(json!({
        "id": row.get::<_, i64>("id").unwrap_or(0),
        "title": row.get::<_, String>("title").unwrap_or_default(),
        "date": date,
        "note": row.get::<_, String>("note").unwrap_or_default(),
        "color": row.get::<_, String>("color").unwrap_or_else(|_| "rose".to_string()),
        "kind": row.get::<_, String>("kind").unwrap_or_else(|_| "custom".to_string()),
        "days": days,
        "left": days_label(days),
    }))
}

#[tauri::command]
fn important_list(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    let conn = store.connection();
    let mut out: Vec<Value> = Vec::new();
    if let Ok(mut stmt) = conn.prepare("SELECT * FROM important_dates ORDER BY date ASC, id ASC") {
        if let Ok(rows) = stmt.query_map([], important_row) {
            out = rows.flatten().collect();
        }
    }
    json!({ "ok": true, "items": out })
}

#[tauri::command]
fn important_save(
    id: Option<i64>,
    title: String,
    date: String,
    note: Option<String>,
    color: Option<String>,
    state: State<'_, AppState>,
) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    let title = if title.trim().is_empty() {
        "重要日".to_string()
    } else {
        title.trim().to_string()
    };
    let day = parse_important_day(&date);
    if chrono::NaiveDate::parse_from_str(&day, "%Y-%m-%d").is_err() {
        return json!({ "ok": false, "error": format!("日期看不懂：{date}（支持 2026-06-20 / 明天 / +7）") });
    }
    let note = note.unwrap_or_default();
    let color = match color.as_deref() {
        Some("rose" | "violet" | "teal" | "amber" | "sky") => color.unwrap(),
        _ => "rose".to_string(),
    };
    let conn = store.connection();
    let result = match id {
        Some(found) if found > 0 => conn.execute(
            "UPDATE important_dates SET title=?1, date=?2, note=?3, color=?4 WHERE id=?5",
            rusqlite::params![title, day, note, color, found],
        ),
        _ => {
            let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
            conn.execute(
                "INSERT INTO important_dates(title, date, note, color, kind, created_at)
                 VALUES (?1, ?2, ?3, ?4, 'custom', ?5)",
                rusqlite::params![title, day, note, color, stamp],
            )
        }
    };
    match result {
        Ok(_) => json!({ "ok": true, "date": day }),
        Err(exc) => json!({ "ok": false, "error": format!("保存失败：{exc}") }),
    }
}

#[tauri::command]
fn important_delete(id: i64, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    let conn = store.connection();
    match conn.execute("DELETE FROM important_dates WHERE id = ?1", [id]) {
        Ok(_) => json!({ "ok": true }),
        Err(exc) => json!({ "ok": false, "error": format!("删除失败：{exc}") }),
    }
}

#[tauri::command]
fn recent_assignments(limit: Option<usize>, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    // 与旧版一致：先读缓存（作业页 / 仪表板写进 settings 的那份）
    let cached = get_store().setting("homework_assignments").unwrap_or_default();
    let items: Vec<serde_json::Value> = serde_json::from_str(&cached).unwrap_or_default();
    let take = limit.unwrap_or(4);
    if items.is_empty() {
        return json!({
            "ok": false,
            "error": "还没有读到作业：点「同步」读一次 eLearning 的 To Do",
            "items": [],
        });
    }
    json!({
        "ok": true,
        "error": "",
        "items": items.into_iter().take(take).collect::<Vec<_>>(),
    })
}

/// 天气：与旧版一致，走 wttr.in（上海），缓存 10 分钟，取不到就用缓存
#[tauri::command]
fn weather(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    weather::weather_info(get_store())
}

#[tauri::command]
fn boot_info() -> serde_json::Value {
    json!({
        "data_dir": paths::data_dir().display().to_string(),
        "db": paths::db_path().display().to_string(),
        "version": env!("CARGO_PKG_VERSION"),
        // 验收脚本用：DANLUME_START_PAGE=settings 可以直接开在某一页
        "start_page": std::env::var("DANLUME_START_PAGE").unwrap_or_default(),
    })
}

/// 密码：前端只传明文，密文由 Rust 用 DPAPI 处理（永远不回传明文）
/// 密码：前端只传明文，密文由 Rust 用 DPAPI 处理（永远不回传明文）。
/// 键名与旧版一致（`smtp_password` / `imap_password_1` / `llm_api_key` …），
/// 所以两个版本能共用同一份密文。
#[tauri::command]
fn save_secret(key: String, value: String, state: State<'_, AppState>) -> Result<(), String> {
    let _ = &state;
    let encrypted = secrets::protect(&value)?;
    get_store()
        .set_setting(&key, &encrypted)
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn has_secret(key: String, state: State<'_, AppState>) -> bool {
    let _ = &state;
    get_store()
        .setting(&key)
        .map(|text| !text.is_empty())
        .unwrap_or(false)
}

// ------------------------------------------------------------------ 通知

#[tauri::command]
fn notice_list(
    limit: Option<i64>,
    unread_only: Option<bool>,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let _ = &state;
    let store = get_store();
    let items = store
        .notice_list(limit.unwrap_or(200), unread_only.unwrap_or(false))
        .map_err(|err| err.to_string())?;
    Ok(json!({
        "items": items,
        "total": items.len(),
        "unread": store.notice_unread_count(),
    }))
}

#[tauri::command]
fn mark_notice_read(id: i64, state: State<'_, AppState>) -> Result<(), String> {
    let _ = &state;
    get_store()
        .mark_notice_read(id)
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn mark_all_notices_read(state: State<'_, AppState>) -> Result<i64, String> {
    let _ = &state;
    get_store()
        .mark_all_notices_read()
        .map_err(|err| err.to_string())
}

/// 抓一次通知：拿 设置 → 仪表板 → 教务通知 里配的地址，抽页面里的链接
#[tauri::command]
fn refresh_notices(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    let url = store
        .setting("dashsrc:notice")
        .or_else(|| store.setting("notice_url"))
        .unwrap_or_default();
    if url.trim().is_empty() {
        return json!({
            "ok": false,
            "added": 0,
            "error": "还没配通知来源：设置 → 仪表板 → 教务通知 里填一次地址",
        });
    }
    match crate::notices::fetch(&url) {
        Ok(rows) => {
            let mut added = 0;
            for (title, link, published) in rows {
                if store.add_notice("notice", &title, &link, &published).unwrap_or(false) {
                    added += 1;
                }
            }
            json!({ "ok": true, "added": added, "error": "", "url": url })
        }
        Err(err) => json!({ "ok": false, "added": 0, "error": err, "url": url }),
    }
}

// ------------------------------------------------------------------ 备忘

#[tauri::command]
fn memos(state: State<'_, AppState>) -> Result<Vec<serde_json::Value>, String> {
    let _ = &state;
    get_store().memos().map_err(|err| err.to_string())
}

#[tauri::command]
fn add_memo(
    title: String,
    html: String,
    state: State<'_, AppState>,
) -> Result<i64, String> {
    let _ = &state;
    get_store()
        .add_memo(&title, &html)
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn update_memo(
    id: i64,
    title: String,
    html: String,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let _ = &state;
    get_store()
        .update_memo(id, &title, &html)
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn delete_memo(id: i64, state: State<'_, AppState>) -> Result<bool, String> {
    let _ = &state;
    get_store()
        .delete_memo(id)
        .map_err(|err| err.to_string())
}

// ------------------------------------------------------------------ 联系人

#[tauri::command]
fn contacts(state: State<'_, AppState>) -> Result<Vec<serde_json::Value>, String> {
    let _ = &state;
    get_store().contacts().map_err(|err| err.to_string())
}

#[tauri::command]
fn add_contact(
    name: String,
    email: String,
    state: State<'_, AppState>,
) -> Result<i64, String> {
    let _ = &state;
    get_store()
        .add_contact(&name, &email)
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn delete_contact(id: i64, state: State<'_, AppState>) -> Result<bool, String> {
    let _ = &state;
    get_store()
        .delete_contact(id)
        .map_err(|err| err.to_string())
}

// -------------------------------------------------------------------- 文档

/// 下载目录里还没归类的文件 + 候选课程目录（用户手动归类用）
#[tauri::command]
fn homework_unclassified(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    crate::canvas::unclassified(get_store())
}

/// 把下载目录根下的某个文件挪进指定课程目录
#[tauri::command]
fn homework_classify(path: String, course: String) -> Result<serde_json::Value, String> {
    crate::canvas::classify(get_store(), &path, &course)
}

#[tauri::command]
fn list_dir(dir: String, state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let _ = &state;
    let entries = docs::list_dir(&dir)?;
    Ok(json!({ "dir": dir, "entries": entries }))
}

#[tauri::command]
fn read_text_file(
    path: String,
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let _ = &state;
    // DOCX / PPTX / XLSX 会自动抽正文（旧版 docs_reader.py 的那套），其它按文本读
    docs::preview_text(&path, limit.unwrap_or(200_000))
}

/// 读二进制文件（PDF 预览用）：返回 base64，前端转成 Blob 交给 pdf.js 渲染。
#[tauri::command]
fn read_file_base64(path: String, limit: Option<usize>) -> Result<serde_json::Value, String> {
    let cap = limit.unwrap_or(64 * 1024 * 1024);
    let bytes = std::fs::read(&path).map_err(|err| format!("读不到文件：{err}"))?;
    if bytes.len() > cap {
        return Err(format!(
            "文件太大（{} MB），超过预览上限 {} MB",
            bytes.len() / 1024 / 1024,
            cap / 1024 / 1024
        ));
    }
    let name = std::path::Path::new(&path)
        .file_name()
        .map(|one| one.to_string_lossy().to_string())
        .unwrap_or_default();
    Ok(json!({
        "name": name,
        "size": bytes.len(),
        "data": crate::secrets::base64_encode(&bytes),
    }))
}

#[tauri::command]
fn open_path(path: String) -> Result<(), String> {
    docs::open_with_system(&path)
}

#[tauri::command]
fn reveal_path(path: String) -> Result<(), String> {
    docs::reveal_in_explorer(&path)
}

/// 文档页默认打开的目录：下载目录（作业）→ 数据目录
#[tauri::command]
fn default_docs_dir(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    let downloads = store
        .setting("dir_downloads")
        .filter(|text| !text.trim().is_empty())
        .map(|text| {
            let path = std::path::PathBuf::from(&text);
            if path.is_absolute() {
                path
            } else {
                paths::data_dir().join(text)
            }
        })
        .unwrap_or_else(|| paths::data_dir().join("elearning"));
    json!({
        "downloads": downloads.display().to_string(),
        "data": paths::data_dir().display().to_string(),
    })
}

// -------------------------------------------------------------------- 邮箱

#[tauri::command]
fn mail_account_state(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    let imap_ok = mail::config(store).is_ok();
    let smtp_ok = mail::smtp_config(store).is_ok();
    json!({
        "imap": imap_ok,
        "smtp": smtp_ok,
        "user": store.setting("imap_user").unwrap_or_default(),
        "from": store.setting("smtp_from")
            .or_else(|| store.setting("smtp_user"))
            .unwrap_or_default(),
        "imap_error": mail::config(store).err().unwrap_or_default(),
        "smtp_error": mail::smtp_config(store).err().unwrap_or_default(),
    })
}

#[tauri::command]
/// 退信识别：扫一遍收件箱里的 PostMaster / Delivery Status，把发件箱对应的邮件标成 bounced
/// （对应旧版 `bounce.py::sync`）
async fn bounce_sync() -> Result<serde_json::Value, String> {
    let handle = tauri::async_runtime::spawn_blocking(move || crate::bounce::sync(get_store())).await;
    match handle {
        Ok(value) => Ok(value),
        Err(exc) => Ok(json!({ "ok": false, "error": format!("检查退信失败：{exc}"), "marked": [] })),
    }
}

// ------------------------------------------------------------------ 参考文献（BibTeX / RIS / DOI）
//
// 对应旧版 `refs.py`：文献库页以前只能看正文里的 `papers` 表，**没有参考文献这一层**。

#[tauri::command]
fn refs_list(keyword: Option<String>, limit: Option<usize>, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    let items = crate::refs::list_refs(store, keyword.as_deref().unwrap_or(""), limit.unwrap_or(200));
    json!({ "ok": true, "items": items, "total": items.len() })
}

/// 导入：`format` = auto / bibtex / ris，`text` 或 `path` 二选一
#[tauri::command]
fn refs_import(
    text: Option<String>,
    path: Option<String>,
    format: Option<String>,
    state: State<'_, AppState>,
) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    let content = match (text, path) {
        (Some(text), _) if !text.trim().is_empty() => text,
        (_, Some(path)) => match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(exc) => return json!({ "ok": false, "error": format!("读不到 {path}：{exc}") }),
        },
        _ => return json!({ "ok": false, "error": "没有内容可导入（要么给 text，要么给 path）" }),
    };
    let kind = format.as_deref().unwrap_or("auto").to_lowercase();
    let entries = match kind.as_str() {
        "bibtex" | "bib" => crate::refs::parse_bibtex(&content),
        "ris" => crate::refs::parse_ris(&content),
        _ => {
            // auto：先按 RIS 试（有 `TY  - ` 就是它），否则按 BibTeX
            if content.contains("TY  - ") {
                crate::refs::parse_ris(&content)
            } else {
                crate::refs::parse_bibtex(&content)
            }
        }
    };
    if entries.is_empty() {
        return json!({ "ok": false, "error": "没解析出任何条目（确认是 BibTeX 或 RIS 文件）" });
    }
    let parsed = entries.len();
    let added = crate::refs::add_many(store, &entries);
    json!({ "ok": true, "parsed": parsed, "added": added, "skipped": parsed - added })
}

/// 用 DOI 加一条（Crossref）
#[tauri::command]
async fn refs_add_doi(doi: String) -> Result<serde_json::Value, String> {
    let handle = tauri::async_runtime::spawn_blocking(move || {
        let store = get_store();
        match crate::refs::fetch_doi(&doi) {
            Ok(entry) => {
                let added = crate::refs::add_many(store, std::slice::from_ref(&entry));
                Ok(json!({
                    "ok": true,
                    "added": added,
                    "entry": crate::refs::entry_json(&entry),
                }))
            }
            Err(exc) => Ok(json!({ "ok": false, "error": exc })),
        }
    })
    .await;
    match handle {
        Ok(value) => value,
        Err(exc) => Ok(json!({ "ok": false, "error": format!("请求失败：{exc}") })),
    }
}

#[tauri::command]
fn refs_delete(id: i64, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    crate::refs::delete_ref(get_store(), id);
    json!({ "ok": true })
}

/// 导出：不传 path 就写到数据目录的 `参考文献.ris` / `参考文献.bib`
#[tauri::command]
fn refs_export(path: Option<String>, format: Option<String>, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    let kind = format.unwrap_or_else(|| "ris".to_string());
    let ext = if kind.eq_ignore_ascii_case("bibtex") || kind.eq_ignore_ascii_case("bib") {
        "bib"
    } else if kind.eq_ignore_ascii_case("enw") || kind.eq_ignore_ascii_case("endnote") {
        "enw"
    } else {
        "ris"
    };
    let target = path
        .filter(|one| !one.trim().is_empty())
        .unwrap_or_else(|| {
            crate::paths::data_dir()
                .join(format!("参考文献.{ext}"))
                .display()
                .to_string()
        });
    match crate::refs::export(store, &target, &kind) {
        Ok(path) => {
            let _ = store.set_setting("refs_export_path", &path);
            json!({ "ok": true, "path": path })
        }
        Err(exc) => json!({ "ok": false, "error": exc }),
    }
}

#[tauri::command]
fn endnote_detect() -> serde_json::Value {
    crate::endnote::detect()
}

/// 用 EndNote 打开一个导出的 .enw / .ris；没装就返回 `{ok:false, found:false}`
#[tauri::command]
fn endnote_open(path: String) -> serde_json::Value {
    match crate::endnote::open_with(&path) {
        Ok(true) => json!({ "ok": true, "found": true, "error": "" }),
        Ok(false) => json!({
            "ok": false,
            "found": false,
            "error": "这台机器没装 EndNote：双击文件可以交给 Zotero / Mendeley，或者去 eHall 软件中心装一个",
        }),
        Err(exc) => json!({ "ok": false, "found": true, "error": exc }),
    }
}

#[tauri::command]
fn mail_list(
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let _ = &state;
    let store = get_store();
    // 旧版三个收信设置：imap_max（每页封数）、imap_unread_only（只看未读）、imap_on_start（前端用）
    let cap = store
        .setting("imap_max")
        .and_then(|text| text.trim().parse::<usize>().ok())
        .filter(|value| *value > 0 && *value <= 500)
        .unwrap_or(30);
    let take = limit.unwrap_or(cap).min(cap);
    let unread_only = store
        .setting("imap_unread_only")
        .map(|text| text.trim() == "1")
        .unwrap_or(false);
    // 先读本地缓存（秒开、离线也能看）；缓存空的时候才现场拉一次并落库
    let mut cached = mail::cached_inbox(store, take);
    if cached.is_empty() {
        if let Ok(items) = mail::list_inbox(store, take) {
            let _ = mail::save_summaries(store, &items);
            cached = mail::cached_inbox(store, take);
        }
    }
    if cached.is_empty() {
        return Ok(json!({
            "ok": false,
            "items": [],
            "total": 0,
            "unread_only": unread_only,
            "cached": 0,
            "error": "收件箱还没缓存：检查一下收件设置，或者点「刷新」同步一次",
        }));
    }
    let total = cached.len();
    let items: Vec<_> = if unread_only {
        cached.into_iter().filter(|one| one.unread).collect()
    } else {
        cached
    };
    Ok(json!({
        "ok": true,
        "items": items,
        "total": total,
        "unread_only": unread_only,
        "cached": mail::cached_count(store),
        "error": "",
    }))
}

/// 增量同步收件箱：只取服务器上 UID 更大的新邮件（旧版 `imap_inbox` 的 uid 缓存做法）
#[tauri::command]
async fn mail_sync(limit: Option<usize>) -> Result<serde_json::Value, String> {
    let handle = tauri::async_runtime::spawn_blocking(move || {
        let store = get_store();
        crate::mail::sync_inbox(store, limit.unwrap_or(50))
    })
    .await;
    match handle {
        Ok(value) => Ok(value.unwrap_or_else(|exc| json!({ "ok": false, "error": exc, "added": 0 }))),
        Err(exc) => Ok(json!({ "ok": false, "error": format!("同步失败：{exc}"), "added": 0 })),
    }
}

#[tauri::command]
fn mail_body(uid: String, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    // 读缓存：点开过的邮件不再联网（旧版 `inbox_messages.body`）
    // 主题 / 发件人 / 日期也一起带出来——以前这四项给的是空串，预览头就一片空白
    if let Some((html, text, subject, from, date)) = mail::cached_body(store, &uid) {
        let (html, text) = pick_body(html, text);
        // 附件清单以前没缓存，重开这封邮件就看不见附件了；现在从库里补回来
        let saved = mail::cached_attachments(store, &uid);
        let body = json!({
            "uid": uid,
            "subject": subject,
            "from": from,
            "date": date,
            "html": html,
            "text": text,
            "attachments": saved,
        });
        return json!({ "ok": true, "body": body, "cached": true, "error": "" });
    }
    match mail::fetch_body(store, &uid) {
        Ok(body) => {
            mail::save_body(store, &uid, &body.html, &body.text);
            let names: Vec<serde_json::Value> = body
                .attachments
                .iter()
                .map(|one| json!({ "name": one.name, "size": one.size }))
                .collect();
            mail::save_attachments(store, &uid, &names);
            let (html, text) = pick_body(body.html.clone(), body.text.clone());
            json!({
                "ok": true,
                "cached": false,
                "error": "",
                "body": json!({
                    "uid": body.uid,
                    "subject": body.subject,
                    "from": body.from,
                    "date": body.date,
                    "html": html,
                    "text": text,
                    "attachments": body.attachments,
                }),
            })
        }
        Err(err) => json!({ "ok": false, "body": null, "error": err }),
    }
}

/// 选正文：HTML 里连一个标签都没有（等于纯文本）时就用 text，
/// 否则 `<iframe>` 会把换行全吃掉，几段话连成一坨。
fn pick_body(html: String, text: String) -> (String, String) {
    let looks_plain = !html.contains('<') || html.contains("&lt;");
    if html.trim().is_empty() || looks_plain {
        let fallback = if text.trim().is_empty() { html } else { text };
        return (String::new(), fallback);
    }
    (html, text)
}

/// 把邮件 HTML 转成纯文本，喂给摘要接口。
///
/// 只做「去杂质」：挖掉 script/style、去掉标签、还原实体、按段落换行。
/// **不裁剪、不丢引用、不砍长度**——用户 2026-11-11 明确说过「不要压缩邮件正文内容」。
fn mail_plain_text(html: &str) -> String {
    // 先把 script / style 整段挖掉（连内容），否则 CSS 会被当成正文喂给模型
    let mut lower = html.to_lowercase();
    loop {
        let mut cut = false;
        for tag in ["script", "style"] {
            let open = format!("<{tag}");
            if let Some(start) = lower.find(&open) {
                let end = lower[start..]
                    .find(&format!("</{tag}>"))
                    .map(|pos| start + pos + tag.len() + 3)
                    .unwrap_or(lower.len());
                lower.replace_range(start..end, " ");
                cut = true;
                break;
            }
        }
        if !cut {
            break;
        }
    }
    // 块级标签先换成换行：不然几段话会被粘成一行，模型读起来更费劲
    for tag in [
        "<br>", "<br/>", "<br />", "</p>", "</div>", "</tr>", "</li>", "</h1>", "</h2>", "</h3>",
        "</blockquote>", "</td>",
    ] {
        lower = lower.replace(tag, "\n");
    }
    let mut out = String::with_capacity(lower.len());
    let mut inside = false;
    for ch in lower.chars() {
        match ch {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => out.push(ch),
            _ => {}
        }
    }
    let cleaned = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    // 每行内部压空白，行之间保留换行；连续空行压成一行（这不算压缩内容）
    let mut lines: Vec<String> = Vec::new();
    let mut blank = 0;
    for line in cleaned.lines() {
        let text = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            blank += 1;
            if blank > 1 || lines.is_empty() {
                continue;
            }
            lines.push(String::new());
        } else {
            blank = 0;
            lines.push(text);
        }
    }
    lines.join("\n").trim().to_string()
}

/// 附件清单 → 提示词里的一行（**只有名字、类型和大小，没有内容**）
fn mail_attachment_line(items: &[serde_json::Value]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let rows: Vec<String> = items
        .iter()
        .filter_map(|one| {
            let name = one.get("name").and_then(|v| v.as_str())?.trim();
            if name.is_empty() {
                return None;
            }
            let size = one.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
            // 后缀当类型用：PDF / DOCX / XLSX / ZIP…
            let kind = name
                .rsplit_once('.')
                .map(|(_, ext)| ext.to_uppercase())
                .filter(|ext| !ext.is_empty() && ext.len() <= 5)
                .unwrap_or_else(|| "文件".to_string());
            let size_text = if size >= 1024 * 1024 {
                format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
            } else if size > 0 {
                format!("{} KB", (size as f64 / 1024.0).round().max(1.0) as u64)
            } else {
                "大小未知".to_string()
            };
            Some(format!("{name}（{kind}，{size_text}）"))
        })
        .collect();
    if rows.is_empty() {
        return String::new();
    }
    format!(
        "\n附件（只给了名字和格式，内容没有上传）：{}",
        rows.join("、")
    )
}

/// 收拾模型输出：去代码围栏、去 Markdown 标记、压掉空行
fn mail_clean_summary(raw: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for line in raw.lines() {
        let mut one = line.trim().to_string();
        if one.starts_with("```") || one.starts_with("---") {
            continue;
        }
        while one.starts_with('#') {
            one = one.trim_start_matches('#').trim().to_string();
        }
        one = one.replace("**", "").replace('`', "");
        if one.is_empty() {
            continue;
        }
        lines.push(one);
    }
    lines.join("\n").trim().to_string()
}

/// 摘要提示词：单独拿出来，真联网的测试能用同一份文字（不然测的和跑的不是一回事）
///
/// `attachments` 只有名字和大小（内容不上传）；正文**不做任何裁剪**。
fn mail_summary_messages(
    subject: &str,
    from: &str,
    date: &str,
    body: &str,
    attachments: &[serde_json::Value],
) -> Vec<serde_json::Value> {
    let attach = mail_attachment_line(attachments);
    vec![
        json!({
            "role": "system",
            "content": "你是邮件摘要助手。用简体中文，只写正文里确实有的信息，不编造时间、金额和人名。",
        }),
        json!({
            "role": "user",
            "content": format!(
                "请给下面这封邮件写一段摘要，让用户扫一眼就知道要不要细读。\n\
                 要求：\n\
                 1. 2–3 行，每行以「· 」开头；第一行先说清这是什么邮件（谁发的、关于什么事）；\n\
                 2. 正文里出现的截止时间、会议时间、金额、需要点的链接或要回复的事，必须写进去；没有就别编；\n\
                 3. 如果确实需要用户处理，最后单独一行用「要做：」开头；纯通知就不写这一行；\n\
                 4. 不要复述称呼、签名、页脚，不要用 Markdown 标题 / 加粗 / 代码块 / 分隔线；\n\
                 5. 全篇不超过 120 个汉字。\n\
                 \n\
                 主题：{subject}\n发件人：{from}\n时间：{date}{attach}\n\n正文（完整，未删减）：\n{body}"
            ),
        }),
    ]
}

/// 从邮件里抽「可能需要加进日程的事」的提示词
///
/// 单独一次请求（结果也单独缓存）：摘要要短，抽取要结构化，混在一段自然语言里
/// 解析起来很脆；分开还能在解析失败时只丢这一半，不影响摘要。
fn mail_events_messages(
    subject: &str,
    from: &str,
    date: &str,
    body: &str,
    today: &str,
) -> Vec<serde_json::Value> {
    vec![
        json!({
            "role": "system",
            "content": "你是日程抽取助手。只输出 JSON 数组，不解释、不加代码块。",
        }),
        json!({
            "role": "user",
            "content": format!(
                "下面是一封邮件。找出**需要加到日历里的事**（提交截止、考试、会议、讲座、报名、答辩等）。\n\
                 今天是 {today}；这封邮件的时间是 {date}。请据此把「下周三」「7 个工作日后」\
                 「11月18日（周三）」这类说法换算成具体日期。\n\
                 要求：\n\
                 1. **只输出 JSON 数组**，不要 Markdown 代码块、不要任何解释；\n\
                 2. 每项字段：\n\
                 {{\"title\": \"日程标题，不超过 20 字，动宾短语\",\n\
                  \"date\": \"YYYY-MM-DD\",\n\
                  \"start_time\": \"HH:MM，不知道就空串\",\n\
                  \"end_time\": \"HH:MM，不知道就空串\",\n\
                  \"note\": \"一句话备注（关键要求 / 地点），没有就空串\"}}\n\
                 3. **逐行读完全文，一条都不要漏**：正文里的表格、列表、每一场宣讲会 / 讲座\n\
                 （哪怕在同一张表里）都要各算一条；最多 10 条；\n\
                 4. 日期必须是 `YYYY-MM-DD` 数字格式（不要写「11月12日」）；\n\
                 5. 正文里没有明确日期就输出 []，**不要编日期**；纯通知也输出 []。\n\
                 \n主题：{subject}\n发件人：{from}\n\n正文：\n{body}"
            ),
        }),
    ]
}

/// 单封邮件最多抽几条日程（再多就该让用户自己建了）
const MAIL_EVENTS_MAX: usize = 10;

/// 把模型给的日期掰成 `YYYY-MM-DD`。
///
/// 以前只认标准格式，别的**直接丢掉**——用户 2026-11-11 报「怎么少一个」，
/// 就是表格里那条写成了「11月12日」被扔了。现在常见写法都收。
fn normalize_mail_date(raw: &str, today: &str) -> Option<String> {
    let text = raw.trim().replace('／', "/").replace(['年', '月'], "-").replace('日', "");
    if text.is_empty() {
        return None;
    }
    // 已经是标准格式
    if chrono::NaiveDate::parse_from_str(&text, "%Y-%m-%d").is_ok() {
        return Some(text);
    }
    let cleaned: String = text.replace(['/', '.'], "-").trim().to_string();
    // 2026-11-12
    if let Ok(date) = chrono::NaiveDate::parse_from_str(&cleaned, "%Y-%m-%d") {
        return Some(date.format("%Y-%m-%d").to_string());
    }
    let year = chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d")
        .map(|one| one.format("%Y").to_string())
        .unwrap_or_default();
    // 2026-11-12 00:00 之类的尾巴去掉
    let head = cleaned.split_whitespace().next().unwrap_or("");
    // 11-12（补上今天的年份）
    if let Ok(date) = chrono::NaiveDate::parse_from_str(&format!("{year}-{head}"), "%Y-%m-%d") {
        return Some(date.format("%Y-%m-%d").to_string());
    }
    None
}

/// `10:00-11:30` / `9:00` / `10点` → `(start, end)`，都规整成 HH:MM
fn normalize_mail_time(raw: &str) -> (String, String) {
    let text = raw.trim().replace('：', ":").replace('～', "-").replace('~', "-");
    if text.is_empty() {
        return (String::new(), String::new());
    }
    let parts: Vec<&str> = text.split('-').map(|one| one.trim()).collect();
    let fix = |one: &str| {
        let text = one.trim().replace("点", ":").replace("时", ":");
        let text = text.trim_end_matches(':').to_string();
        let candidate = if text.matches(':').count() == 0 && !text.is_empty() {
            format!("{text}:00")
        } else if text.len() == 4 && text.contains(':') {
            format!("0{text}")
        } else {
            text
        };
        chrono::NaiveTime::parse_from_str(&candidate, "%H:%M")
            .map(|_| candidate)
            .unwrap_or_default()
    };
    let start = fix(parts.first().copied().unwrap_or(""));
    let end = parts.get(1).map(|one| fix(one)).unwrap_or_default();
    (start, end)
}

/// 把模型吐的东西解析成日程条目：去掉代码块、找最外层数组、逐条校验
fn parse_mail_events(raw: &str, today: &str) -> Vec<serde_json::Value> {
    let text = raw.trim();
    let start = text.find('[');
    let end = text.rfind(']');
    let (Some(start), Some(end)) = (start, end) else {
        return Vec::new();
    };
    if end <= start {
        return Vec::new();
    }
    let Ok(rows) = serde_json::from_str::<Value>(&text[start..=end]) else {
        return Vec::new();
    };
    let Some(items) = rows.as_array() else {
        return Vec::new();
    };
    let mut out: Vec<Value> = Vec::new();
    for item in items.iter().take(MAIL_EVENTS_MAX) {
        let title = item
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let raw_date = item.get("date").and_then(|v| v.as_str()).unwrap_or("");
        // 日期先尽量掰成 YYYY-MM-DD；真掰不出来才丢（往日历里塞「11月12日」会直接建不出来）
        let Some(date) = normalize_mail_date(raw_date, today) else {
            continue;
        };
        if title.is_empty() {
            continue;
        }
        // 时间也可能是 "10:00-11:30" 挤在一个字段里
        let raw_start = item.get("start_time").and_then(|v| v.as_str()).unwrap_or("");
        let raw_end = item.get("end_time").and_then(|v| v.as_str()).unwrap_or("");
        let (mut start_time, mut end_time) = normalize_mail_time(raw_start);
        let (end2, end3) = normalize_mail_time(raw_end);
        if start_time.is_empty() {
            start_time = end2;
        }
        if end_time.is_empty() {
            end_time = end3;
        }
        out.push(json!({
            "title": title,
            "date": date,
            "start_time": start_time,
            "end_time": end_time,
            "note": item.get("note").and_then(|v| v.as_str()).unwrap_or("").trim(),
            "from_mail": true,
            "today": today,
        }));
    }
    out
}

/// 每封邮件顶端的 AI 摘要：调用户配置好的接口，把正文压成两三行
///
/// 摘要缓存在 `inbox_messages.summary_zh`，同一封邮件只花一次 token；
/// `force = true` 是前端「重新生成」。没配接口时返回 `ok:false` + 提示语，不报错。
#[tauri::command]
async fn mail_summarize(
    uid: String,
    force: Option<bool>,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let _ = &state;
    let store = get_store();
    if force != Some(true) {
        let cached = mail::cached_summary(store, &uid);
        let (events, events_done) = mail::cached_events(store, &uid);
        if !cached.trim().is_empty() && events_done {
            return Ok(json!({
                "ok": true, "summary": cached, "cached": true, "error": "",
                "events": events,
            }));
        }
        // 摘要有了、但「可能要加的日程」还没抽过（老缓存）：只补抽日程，不重新生成摘要
        if !cached.trim().is_empty() {
            if let Some((html, text, subject, from, date)) = mail::cached_body(store, &uid) {
                let (html_body, text_body) = pick_body(html, text);
                let raw = if html_body.trim().is_empty() {
                    text_body
                } else {
                    html_body
                };
                let body = mail_plain_text(&raw);
                let events = mail_events_for(store, &uid, &subject, &from, &date, &body, false);
                return Ok(json!({
                    "ok": true, "summary": cached, "cached": true, "error": "",
                    "events": events,
                }));
            }
        }
    }
    let meta = ai::config_state(store);
    if meta.get("ok").and_then(|one| one.as_bool()) != Some(true) {
        let error = meta
            .get("error")
            .and_then(|one| one.as_str())
            .unwrap_or("还没配置 AI 接口");
        return Ok(json!({
            "ok": false, "summary": "", "cached": false,
            "error": format!("{error}（没配接口就出不了摘要）"),
        }));
    }
    // 正文可能还没落库（用户是直接点开的这封），顺手补一次
    let payload = match mail::cached_body(store, &uid) {
        Some(one) => Some(one),
        None => {
            let want = uid.clone();
            let handle = tauri::async_runtime::spawn_blocking(move || {
                let store = get_store();
                match mail::fetch_body(store, &want) {
                    Ok(body) => {
                        mail::save_body(store, &want, &body.html, &body.text);
                        // 附件只记名字和大小（内容不落库、也不上传给模型）
                        let names: Vec<serde_json::Value> = body
                            .attachments
                            .iter()
                            .map(|one| json!({ "name": one.name, "size": one.size }))
                            .collect();
                        mail::save_attachments(store, &want, &names);
                        Some((body.html, body.text, body.subject, body.from, body.date))
                    }
                    Err(_) => None,
                }
            })
            .await;
            handle.ok().flatten()
        }
    };
    let Some((html, text, subject, from, date)) = payload else {
        return Ok(json!({
            "ok": false, "summary": "", "cached": false,
            "error": "这封邮件的正文还没下载下来，等有网了再试",
        }));
    };
    // 以前这里取了 `pick_body` 的**第二个**返回值（纯文本），而纯文本那列经常是空的
    // （老邮件只缓存了 HTML），于是正文被当成「空」→ 用户看到「这封邮件没有正文」。
    // 现在：HTML 有就用 HTML，没有才退回纯文本。
    let (html_body, text_body) = pick_body(html, text);
    let raw = if html_body.trim().is_empty() {
        text_body
    } else {
        html_body
    };
    // 用户 2026-11-11：「不要压缩邮件正文内容」——整篇都给模型，不再截 4000 字。
    let body = mail_plain_text(&raw);
    if body.trim().is_empty() {
        return Ok(json!({
            "ok": false, "summary": "", "cached": false,
            "error": "这封邮件没有正文，摘要无从下手",
        }));
    }
    // 附件只把**名字和格式**写进提示词，内容不上传（用户要求）
    let attachments = mail::cached_attachments(store, &uid);
    let messages = mail_summary_messages(&subject, &from, &date, &body, &attachments);
    let summary = match ai::chat(store, &messages, Some(0.3)) {
        Ok(reply) => {
            let summary = mail_clean_summary(&reply);
            if summary.is_empty() {
                return Ok(json!({
                    "ok": false, "summary": "", "cached": false, "events": [],
                    "error": "接口返回的是空的，重试一次看看",
                }));
            }
            mail::save_summary(store, &uid, &summary);
            summary
        }
        Err(err) => {
            return Ok(json!({
                "ok": false, "summary": "", "cached": false, "events": [],
                "error": format!("摘要生成失败：{err}"),
            }));
        }
    };
    // 顺手抽「可能需要加进日程的事」（单独一次请求 + 单独缓存）
    let events = mail_events_for(store, &uid, &subject, &from, &date, &body, force == Some(true));
    Ok(json!({
        "ok": true, "summary": summary, "cached": false, "error": "",
        "events": events,
        "model": meta.get("model").and_then(|one| one.as_str()).unwrap_or(""),
    }))
}

/// 取这封邮件「可能要加进日程的事」：有缓存就用缓存，没有就抽一次
fn mail_events_for(
    store: &'static Store,
    uid: &str,
    subject: &str,
    from: &str,
    date: &str,
    body: &str,
    force: bool,
) -> Vec<serde_json::Value> {
    if !force {
        // 抽取规则升级过（版本号变了）就先作废旧缓存，否则用户看到的还是老结果
        mail::invalidate_old_event_cache(store);
        let (items, done) = mail::cached_events(store, uid);
        if done {
            return items;
        }
    }
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let messages = mail_events_messages(subject, from, date, body, &today);
    match ai::chat(store, &messages, Some(0.1)) {
        Ok(reply) => {
            let items = parse_mail_events(&reply, &today);
            mail::save_events(store, uid, &items);
            items
        }
        Err(_) => {
            // 抽失败就先不缓存，下次打开再试；摘要照常显示
            Vec::new()
        }
    }
}

/// 把用户勾选的「可能要做的事」真的加进日程（走 `schedule_add` 那个工具，校验规则一致）
#[tauri::command]
fn mail_add_events(items: Vec<Value>, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    let mut added = 0;
    let mut results: Vec<Value> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for item in items {
        let title = item
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if title.is_empty() {
            continue;
        }
        let mut args = json!({
            "title": title,
            "date": item.get("date").cloned().unwrap_or(json!("")),
        });
        for key in ["start_time", "end_time"] {
            if let Some(text) = item.get(key).and_then(|v| v.as_str()) {
                if !text.trim().is_empty() {
                    args[key] = json!(text.trim());
                }
            }
        }
        if let Some(note) = item.get("note").and_then(|v| v.as_str()) {
            let note = note.trim();
            if !note.is_empty() {
                args["description"] = json!(note);
            }
        }
        let result = ai_tools::dispatch(store, "schedule_add", &args);
        let ok = result.get("ok").and_then(|one| one.as_bool()).unwrap_or(false);
        if ok {
            added += 1;
        } else {
            let reason = result
                .get("summary")
                .or_else(|| result.get("error"))
                .and_then(|one| one.as_str())
                .unwrap_or("加不进去")
                .to_string();
            errors.push(format!("{}：{reason}", args["title"].as_str().unwrap_or("")));
        }
        results.push(json!({
            "title": args["title"],
            "date": args["date"],
            "ok": ok,
            "summary": result.get("summary").and_then(|one| one.as_str()).unwrap_or(""),
        }));
    }
    json!({
        "ok": errors.is_empty(),
        "added": added,
        "results": results,
        "error": errors.join("；"),
    })
}

#[tauri::command]
fn mail_send(
    to: String,
    subject: String,
    html: String,
    attachments: Option<Vec<String>>,
    state: State<'_, AppState>,
) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    // 旧版 default_to：收件人留空时用默认收件人
    let mut to = to;
    if to.trim().is_empty() {
        if let Some(fallback) = store
            .setting("default_to")
            .filter(|text| !text.trim().is_empty())
        {
            to = fallback.trim().to_string();
        }
    }
    // 发信前：OAuth 明确拒绝、smtp_security 映射；正文自动追加签名（旧版 mail_signature）
    if let Err(exc) = crate::mail_identity::prepare(store) {
        return json!({ "ok": false, "error": exc });
    }
    let html = crate::mail_identity::with_signature(store, &html);
    let files = attachments.unwrap_or_default();
    let plain = html
        .replace("<br>", "\n")
        .replace("</p>", "\n")
        .replace(|ch: char| ch == '<' || ch == '>', " ");
    let record = store
        .save_mail(&to, &subject, &html, &plain, &json!(files).to_string(), "sending", "")
        .unwrap_or(0);
    match mail::send(store, &to, &subject, &html, &files) {
        Ok(()) => {
            let _ = store.update_mail_status(record, "sent", "");
            json!({ "ok": true, "id": record, "error": "" })
        }
        Err(err) => {
            let _ = store.update_mail_status(record, "failed", &err);
            json!({ "ok": false, "id": record, "error": err })
        }
    }
}

#[tauri::command]
fn mail_test(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    match mail::smtp_config(store) {
        Ok(_) => json!({ "ok": true, "error": "" }),
        Err(err) => json!({ "ok": false, "error": err }),
    }
}

/// 收件箱连通性自检（只验证配置齐不齐，不真连服务器，避免卡住界面）
#[tauri::command]
fn imap_test(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    match mail::config(store) {
        Ok(config) => json!({
            "ok": true,
            "error": "",
            "server": format!("{}:{}", config.host, config.port),
            "user": config.user,
        }),
        Err(err) => json!({ "ok": false, "error": err }),
    }
}

/// 设置页批量保存（等价于逐条 set_setting）
#[tauri::command]
fn settings_save(
    values: std::collections::BTreeMap<String, String>,
    state: State<'_, AppState>,
) -> Result<i64, String> {
    let _ = &state;
    let store = get_store();
    let mut saved = 0;
    for (key, value) in values {
        if store.set_setting(&key, &value).is_ok() {
            saved += 1;
        }
    }
    Ok(saved)
}

#[tauri::command]
fn mails(limit: Option<i64>, state: State<'_, AppState>) -> Result<Vec<serde_json::Value>, String> {
    let _ = &state;
    get_store().mails(limit.unwrap_or(100)).map_err(|err| err.to_string())
}

#[tauri::command]
fn delete_mail(id: i64, state: State<'_, AppState>) -> Result<bool, String> {
    let _ = &state;
    get_store().delete_mail(id).map_err(|err| err.to_string())
}

// -------------------------------------------------------------------- 作业

#[tauri::command]
fn homework_items(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    let items = elearn::cached_items(store);
    let mut courses: serde_json::Map<String, Value> = serde_json::Map::new();
    for item in &items {
        let course = item
            .get("course")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        let title = item
            .get("title")
            .or_else(|| item.get("name"))
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        let due = item
            .get("due")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        let files = elearn::local_files(store, item);
        let entry = courses.entry(course.clone()).or_insert_with(|| {
            json!({ "course": course, "items": [] })
        });
        if let Some(list) = entry.get_mut("items").and_then(|value| value.as_array_mut()) {
            list.push(json!({
                "id": item.get("id").cloned().unwrap_or(Value::Null),
                "title": title,
                "due": due,
                "url": item.get("url").cloned().unwrap_or(Value::Null),
                "files": files,
            }));
        }
    }
    json!({
        "ok": true,
        "total": items.len(),
        "courses": courses.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
        "downloads": get_store().setting("dir_downloads").unwrap_or_default(),
    })
}

#[tauri::command]
fn homework_open_file(path: String) -> Result<(), String> {
    docs::open_with_system(&path)
}

#[tauri::command]
fn homework_write_deadlines(force: Option<bool>, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    elearn::write_deadlines(get_store(), force.unwrap_or(true))
}

/// 导入考试安排：在已登录的窗口里打开教务「我的考试」，把表格行读出来（J2-1）
///
/// 和课程表一样：教务的票是页面自己换的，进程内复现会被弹回登录页，
/// 所以在**登录窗口**里落到考试页，用 JS 把页面已经渲染出来的表格读回来
/// （对应旧版 `exam_dialog.py` + `exams.py` 的 `PAGE_JS`）。
///
/// * `commit = false`：只把读到的条目返回（前端先摆给人看，确认后再写）；
/// * `commit = true`：写进日历（提前 60 分钟提醒）+ 建「重要日」倒计时。
#[tauri::command]
async fn exams_import(
    app: tauri::AppHandle,
    commit: Option<bool>,
    as_important: Option<bool>,
) -> Result<serde_json::Value, String> {
    let handle = app.clone();
    let handle_for_task = app.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        open_uis_window(&handle_for_task)?;
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let Some(window) = handle_for_task.get_webview_window("uis-login") else {
            return Err("登录窗口没起来".to_string());
        };
        let mut last_error = String::new();
        for url in crate::exams::EXAM_URLS {
            let target: tauri::Url = match url.parse() {
                Ok(value) => value,
                Err(_) => continue,
            };
            if window.navigate(target).is_err() {
                continue;
            }
            std::thread::sleep(std::time::Duration::from_millis(4500));
            let landed = window
                .url()
                .map(|one| one.to_string())
                .unwrap_or_default();
            uis_log(&format!("[exams] 考试页落地：{}", shorten_url(&landed)));
            if landed.contains("id.fudan.edu.cn") {
                last_error =
                    "教务又跳到统一身份认证了：先在弹出的登录窗口里登录一次，再点「导入考试」"
                        .to_string();
                break;
            }
            let Some(raw) = eval_json(&window, crate::exams::PAGE_JS) else {
                last_error = format!("考试页没响应（{url}）");
                continue;
            };
            let items = crate::exams::parse_exams(&raw);
            if items.is_empty() {
                last_error = format!("{url} 上没读到考试行（这个学期可能还没排考试）");
                continue;
            }
            if !commit.unwrap_or(false) {
                return Ok(json!({
                    "ok": true,
                    "items": items,
                    "committed": false,
                    "url": landed,
                    "title": raw.get("title").and_then(|one| one.as_str()).unwrap_or(""),
                }));
            }
            let written = crate::exams::add_exams(get_store(), &items, as_important.unwrap_or(true));
            return Ok(json!({
                "ok": true,
                "items": items,
                "committed": true,
                "result": written,
                "url": landed,
            }));
        }
        Err(last_error)
    })
    .await;
    let _ = handle;
    match outcome {
        // 内层已经是 Result<Value, String>：读不到考试页就把那句人话原样交给前端
        Ok(inner) => inner,
        Err(exc) => Ok(json!({ "ok": false, "error": format!("导入考试的任务没跑起来：{exc}") })),
    }
}

/// 日历页右侧「作业截止」卡：未来 N 天内的截止时间（数据是缓存里的，不联网）
///
/// 用户 2026-11-12：eLearning 的 `/dash` 上有作业截止时间，想直接摆在日历页。
#[tauri::command]
fn homework_upcoming(days: Option<i64>, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    elearn::upcoming(get_store(), days.unwrap_or(14))
}

/// 代理现在是什么状态（设置页那行小字）
#[tauri::command]
fn proxy_summary(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    json!({
        "ok": true,
        "summary": net::summary(store),
        "webview_args": net::webview_args_string(store).unwrap_or_default(),
    })
}

/// 测一下走当前设置能不能连到校内服务（对应旧版 `vpn.probe`）
#[tauri::command]
async fn proxy_probe() -> Result<serde_json::Value, String> {
    let handle = tauri::async_runtime::spawn_blocking(|| net::probe(get_store())).await;
    match handle {
        Ok(value) => Ok(value),
        Err(exc) => Ok(json!({ "ok": false, "error": format!("测试没跑起来：{exc}") })),
    }
}

/// 把一条作业标成「已完成 / 已提交」（`done = false` 取消）
///
/// 用户 2026-11-12：有时候其实已经交了，不想再被提醒 —— 标掉就不再出现在卡片里、也不再提醒。
#[tauri::command]
fn homework_mark_done(key: String, done: Option<bool>, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let keys = elearn::mark_done(get_store(), &key, done.unwrap_or(true));
    json!({ "ok": true, "done_keys": keys })
}

/// 快到截止时间的作业（`mark = true` 就记成「这条提过了」）
///
/// 判定在 `elearn::due_reminders` 里：进入「截止前 24 小时」的窗口就返回，每条只提醒一次。
#[tauri::command]
fn elearn_due_reminders(mark: Option<bool>, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let rows = elearn::due_reminders(get_store(), mark.unwrap_or(false));
    json!({ "ok": true, "items": rows, "ahead_minutes": 24 * 60 })
}

// ------------------------------------------------------------------ 文献库

/// 文献在线检索：arXiv + Crossref + Semantic Scholar（都不用 Key），按 DOI / 标题去重
/// （对应旧版 `lit.search_all`；Rust 以前只能读本地 papers 表）
///
/// `limit` 是每个来源这一页取多少（默认 20），`offset` 用来翻页。
#[tauri::command]
async fn papers_search(
    query: String,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<serde_json::Value, String> {
    let handle = tauri::async_runtime::spawn_blocking(move || {
        crate::papers::search_online(&query, limit.unwrap_or(20), offset.unwrap_or(0))
    })
    .await;
    match handle {
        Ok(value) => Ok(value),
        Err(exc) => Ok(json!({ "ok": false, "results": [], "errors": [format!("检索失败：{exc}")] })),
    }
}

/// 把一条检索结果写进文献库（同来源同 id 就更新）
#[tauri::command]
fn papers_add(item: serde_json::Value, project: Option<String>, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    match crate::papers::add_paper(get_store(), &item, project.as_deref().unwrap_or("")) {
        Some(id) => json!({ "ok": true, "id": id }),
        None => json!({ "ok": false, "error": "这条没有可用的标识（DOI / 标题 / 链接）" }),
    }
}

/// 下载这篇文献的 PDF 到文献库（用户 2026-11-12：J1-2）
///
/// 落盘位置照旧版：`library/<项目或未分类>/<年份>/<标题>.pdf`；
/// 下载要联网，所以是 async + spawn_blocking（同步命令会冻住窗口）。
#[tauri::command]
async fn papers_download(id: i64) -> Result<serde_json::Value, String> {
    let handle = tauri::async_runtime::spawn_blocking(move || {
        crate::papers::download_pdf(get_store(), id)
    })
    .await;
    match handle {
        Ok(value) => Ok(value),
        Err(exc) => Ok(json!({ "ok": false, "summary": format!("下载任务没跑起来：{exc}") })),
    }
}

/// 语义检索：本地有多少篇建了向量、还差多少
#[tauri::command]
fn papers_semantic_status(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    crate::semantic::status(get_store())
}

/// 语义检索：给还没建向量的文献补上（要联网，所以 async + spawn_blocking）
#[tauri::command]
async fn papers_semantic_index(force: Option<bool>) -> Result<serde_json::Value, String> {
    let handle = tauri::async_runtime::spawn_blocking(move || {
        crate::semantic::index(get_store(), force.unwrap_or(false))
    })
    .await;
    match handle {
        Ok(value) => Ok(value),
        Err(exc) => Ok(json!({ "ok": false, "error": format!("建索引的任务没跑起来：{exc}") })),
    }
}

/// 文献库「按意思搜」：问句走一次服务器算向量，其余在本地算
#[tauri::command]
async fn papers_semantic_search(
    query: String,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    let handle = tauri::async_runtime::spawn_blocking(move || {
        crate::semantic::search(get_store(), &query, limit.unwrap_or(10))
    })
    .await;
    match handle {
        Ok(value) => Ok(value),
        Err(exc) => Ok(json!({ "ok": false, "error": format!("检索任务没跑起来：{exc}") })),
    }
}

#[tauri::command]
fn papers_list(
    project: Option<String>,
    stars_min: Option<i64>,
    keyword: Option<String>,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let _ = &state;
    let store = get_store();
    let items = papers::papers(
        store,
        &project.unwrap_or_default(),
        stars_min.unwrap_or(0),
        &keyword.unwrap_or_default(),
    )
    .unwrap_or_default();
    let projects = papers::projects(store).unwrap_or_default();
    Ok(json!({
        "ok": true,
        "items": items,
        "projects": projects.into_iter()
            .map(|(name, total)| json!({ "name": name, "total": total }))
            .collect::<Vec<_>>(),
    }))
}

#[tauri::command]
fn paper_update(
    id: i64,
    stars: Option<i64>,
    note: Option<String>,
    summary: Option<String>,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let _ = &state;
    papers::set_meta(
        get_store(),
        id,
        stars,
        note.as_deref(),
        summary.as_deref(),
    )
    .map_err(|err| err.to_string())
}

#[tauri::command]
fn paper_citation(id: i64, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    let rows = papers::papers(store, "", 0, "").unwrap_or_default();
    let found = rows.into_iter().find(|item| item["id"].as_i64() == Some(id));
    match found {
        Some(paper) => json!({ "ok": true, "text": papers::citation(&paper) }),
        None => json!({ "ok": false, "text": "" }),
    }
}

// -------------------------------------------------------------------- 设置

#[tauri::command]
fn settings_all(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    let values = store.settings().unwrap_or_default();
    // `secret:` 前缀是老约定；账号服务器的两个 token 是 DPAPI 密文，同样不该出现在 values 里
    let is_secret = |key: &str| {
        key.starts_with("secret:")
            || key == account::TOKEN_KEY
            || key == account::REFRESH_KEY
    };
    let secrets: Vec<String> = values
        .keys()
        .filter(|key| is_secret(key.as_str()))
        .cloned()
        .collect();
    json!({
        "values": values.into_iter()
            .filter(|(key, _)| !is_secret(key))
            .collect::<std::collections::BTreeMap<String, String>>(),
        "secrets": secrets,
    })
}

// ------------------------------------------------------------------ 空教室

/// 空教室页要的三样东西：校区→教学楼、教学楼→教室、最近一次抓到的空教室表格
#[tauri::command]
fn classroom_view(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let store = get_store();

    let mut maps: Value = json!({});
    if let Some(raw) = store.setting("ehall_campus_buildings") {
        if let Ok(parsed) = serde_json::from_str::<Value>(&raw) {
            maps = parsed;
        }
    }
    let mut rooms: serde_json::Map<String, Value> = serde_json::Map::new();
    for (key, value) in store.settings().unwrap_or_default() {
        if let Some(building) = key.strip_prefix("ehall_room_rooms_") {
            if let Ok(list) = serde_json::from_str::<Value>(&value) {
                rooms.insert(building.to_string(), list);
            }
        }
    }

    let cache: Value = store
        .setting("ehall_cache_empty_room")
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .unwrap_or_else(|| json!({}));

    // 新版缓存带 `rooms`（抓取时解析好的「教室 × 节次」），旧版只有 `rows`（原表）
    let grid: Vec<Value> = cache
        .get("rooms")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let periods: Vec<Value> = cache
        .get("periods")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    let headers: Vec<String> = cache
        .get("headers")
        .and_then(|value| value.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|item| item.as_str().map(|text| text.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let rows: Vec<Value> = cache
        .get("rows")
        .and_then(|value| value.as_array())
        .map(|rows| rows.iter().skip(2).cloned().collect())
        .unwrap_or_default();

    json!({
        "ok": !grid.is_empty() || !rows.is_empty(),
        "maps": maps,
        "rooms": rooms,
        "grid": grid,
        "periods": periods,
        "building": cache.get("building").cloned().unwrap_or(Value::Null),
        "room": cache.get("room").cloned().unwrap_or(Value::Null),
        "value": cache.get("value").cloned().unwrap_or(Value::Null),
        "url": cache.get("url").cloned().unwrap_or(Value::Null),
        "ts": cache.get("ts").cloned().unwrap_or(Value::Null),
        "headers": headers,
        "rows": rows,
    })
}

/// 空教室「抓取当前页面」：在内嵌网页里跑 JS 把表格读回来，解析后写缓存。
///
/// 对应旧版 `page_classroom.scrape()` → `ehall_bridge.CLASSROOM_JS` → `parse_classroom_grid`。
/// 走 `eval_with_callback`：网页把 DOM 表格序列化成 JSON 交给这里，解析全在 Rust 侧。
/// 空教室「查询」：在一个**看不见的**内嵌页面里选校区/教学楼/教室 → 点「查询」→ 抓表格。
///
/// 对应旧版 `page_classroom` → `ehall_bridge.classroom_query`：旧版把学校页面藏在左边
/// （默认 `classroom_show_web=0`，抓取照样在后台跑）。用户 2026-11-11 明确说
/// 「空教室页面就是空教室，ehall 的网页只放在 ehall 页面里」，所以这个页面完全看不见：
/// 网页开在窗口右侧之外（被窗口裁掉），只用来取数。页面若掉登录态，
/// 注入的自动填表脚本会自己登录（和 ehall 页面用的是同一段脚本）。
#[tauri::command]
async fn classroom_query(
    app: tauri::AppHandle,
    campus: Option<String>,
    building: Option<String>,
    room: Option<String>,
    url: Option<String>,
) -> Result<serde_json::Value, String> {
    let campus = campus.unwrap_or_default();
    let building = building.unwrap_or_default();
    let room = room.unwrap_or_default();
    let handle = tauri::async_runtime::spawn_blocking(move || {
        let store = get_store();
        let target = url
            .unwrap_or_default()
            .trim()
            .to_string();
        let target = if target.is_empty() {
            store
                .setting("ehall_empty_room_url")
                .map(|text| text.trim().to_string())
                .filter(|text| !text.is_empty())
                .or_else(|| {
                    // 旧版 `ehall_bridge._source_url("empty_room")` 读的就是这个键
                    store
                        .setting("ehall_json_empty_room")
                        .map(|text| text.trim().to_string())
                        .filter(|text| !text.is_empty())
                })
                .or_else(|| {
                    store
                        .setting("ehall_url")
                        .map(|text| text.trim().to_string())
                        .filter(|text| !text.is_empty())
                })
                .unwrap_or_else(|| "https://ehall.fudan.edu.cn/".to_string())
        } else {
            target
        };
        crate::embed::open_hidden(&app, "classroom", &target)?;
        std::thread::sleep(std::time::Duration::from_millis(1500));
        if !campus.is_empty() {
            let _ = crate::embed::eval_callback_collect(
                "classroom",
                &crate::classroom::pick_script(&campus),
                std::time::Duration::from_secs(10),
            );
            std::thread::sleep(std::time::Duration::from_millis(700));
        }
        if !building.is_empty() {
            let _ = crate::embed::eval_callback_collect(
                "classroom",
                &crate::classroom::pick_script(&building),
                std::time::Duration::from_secs(10),
            );
            std::thread::sleep(std::time::Duration::from_millis(900));
        }
        if !room.is_empty() && room != "全部教室" {
            let _ = crate::embed::eval_callback_collect(
                "classroom",
                &crate::classroom::pick_script(&room),
                std::time::Duration::from_secs(10),
            );
            std::thread::sleep(std::time::Duration::from_millis(600));
        }
        let _ = crate::embed::eval_callback_collect(
            "classroom",
            crate::classroom::click_query_script(),
            std::time::Duration::from_secs(10),
        );
        std::thread::sleep(std::time::Duration::from_millis(2600));
        let raw = crate::embed::eval_callback_collect(
            "classroom",
            crate::classroom::scrape_script(),
            std::time::Duration::from_secs(20),
        )?;
        let payload = crate::classroom::parse(&raw, &building, &room);
        cache_classroom_payload(&payload);
        Ok::<serde_json::Value, String>(payload)
    })
    .await;
    match handle {
        Ok(payload) => payload,
        Err(exc) => Err(format!("查询失败：{exc}")),
    }
}

/// 只把「真从表格里解出来、且像一次完整查询」的结果写进缓存
fn cache_classroom_payload(payload: &serde_json::Value) {
    let ok = payload
        .get("ok")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let from_table = payload
        .get("from_table")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let count = payload
        .get("rooms")
        .and_then(|value| value.as_array())
        .map(|rooms| rooms.len())
        .unwrap_or(0);
    if ok && from_table && count >= 3 {
        if let Ok(text) = serde_json::to_string(payload) {
            let _ = get_store().set_setting("ehall_cache_empty_room", &text);
        }
    }
}

#[tauri::command]
async fn classroom_scrape(
    building: Option<String>,
    room: Option<String>,
) -> Result<serde_json::Value, String> {
    let building = building.unwrap_or_default();
    let room = room.unwrap_or_default();
    let handle = tauri::async_runtime::spawn_blocking(move || {
        let raw = crate::embed::eval_callback_collect(
            "classroom",
            crate::classroom::scrape_script(),
            std::time::Duration::from_secs(20),
        )?;
        let payload = crate::classroom::parse(&raw, &building, &room);
        cache_classroom_payload(&payload);
        Ok::<serde_json::Value, String>(payload)
    })
    .await;
    match handle {
        Ok(payload) => payload,
        Err(exc) => Err(format!("抓取失败：{exc}")),
    }
}

// --------------------------------------------------- ehall 实时抓取（WebView）

/// 真正干活的部分（命令与验收钩子共用）
fn open_grab(app: &tauri::AppHandle, key: &str) -> Result<serde_json::Value, String> {
    let store = get_store();
    let source_key = format!("ehall_json_{key}");
    let url_text = store.setting(&source_key).unwrap_or_default();
    if url_text.trim().is_empty() {
        return Ok(json!({ "ok": false, "error": "这张卡还没填数据地址（设置 → 仪表板）" }));
    }
    let url: tauri::Url = url_text
        .parse()
        .map_err(|err| format!("地址不合法：{err}"))?;
    let label = format!("ehall-grab-{key}");
    if let Some(existing) = app.get_webview_window(&label) {
        let _ = existing.close();
    }
    let title = format!("抓取：{key}");
    tauri::WebviewWindowBuilder::new(app, &label, tauri::WebviewUrl::External(url))
        .title(title)
        .inner_size(1180.0, 860.0)
        .initialization_script(ehall_grab::grab_script(key))
        .build()
        .map_err(|err| format!("打不开抓取窗口：{err}"))?;
    Ok(json!({ "ok": true, "error": "", "url": url_text }))
}

/// 内嵌网页：在主窗口里贴一个子 Webview（位置/大小用 CSS 像素）
#[tauri::command]
async fn embed_open(
    app: tauri::AppHandle,
    kind: String,
    url: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<serde_json::Value, String> {
    let handle = app.clone();
    match tauri::async_runtime::spawn_blocking(move || {
        crate::embed::open(&handle, &kind, &url, x, y, width, height)
    })
    .await
    {
        Ok(Ok(label)) => Ok(json!({ "ok": true, "label": label })),
        Ok(Err(exc)) => Ok(json!({ "ok": false, "error": exc })),
        Err(exc) => Ok(json!({ "ok": false, "error": format!("内嵌网页失败：{exc}") })),
    }
}

#[tauri::command]
fn embed_bounds(kind: String, x: f64, y: f64, width: f64, height: f64) -> serde_json::Value {
    match crate::embed::bounds(&kind, x, y, width, height) {
        Ok(()) => json!({ "ok": true }),
        Err(exc) => json!({ "ok": false, "error": exc }),
    }
}

#[tauri::command]
fn embed_close(kind: String) -> serde_json::Value {
    match crate::embed::close(&kind) {
        Ok(()) => json!({ "ok": true }),
        Err(exc) => json!({ "ok": false, "error": exc }),
    }
}

#[tauri::command]
fn embed_reload(kind: String) -> serde_json::Value {
    match crate::embed::reload(&kind) {
        Ok(()) => json!({ "ok": true }),
        Err(exc) => json!({ "ok": false, "error": exc }),
    }
}

#[tauri::command]
fn embed_navigate(kind: String, url: String) -> serde_json::Value {
    match crate::embed::navigate(&kind, &url) {
        Ok(()) => json!({ "ok": true }),
        Err(exc) => json!({ "ok": false, "error": exc }),
    }
}

#[tauri::command]
fn embed_status(kind: String) -> serde_json::Value {
    crate::embed::status(&kind)
}

/// 在 WebView2 里打开某张卡的数据页，页面加载完由注入脚本把正文回传（见 `ehall_ingest`）
#[tauri::command]
fn ehall_open(app: tauri::AppHandle, key: String) -> Result<serde_json::Value, String> {
    open_grab(&app, &key)
}

/// 注入脚本的回传口：解析正文并写缓存
#[tauri::command]
fn ehall_ingest(
    key: String,
    url: String,
    text: String,
    state: State<'_, AppState>,
) -> serde_json::Value {
    let _ = &state;
    ehall_grab::store_cache(get_store(), &key, &url, &text)
}

/// 兜底路径：用户把 eHall 页面原文粘进来，走同一套解析并写缓存
/// （远程页面不允许注入脚本，所以这条不依赖 IPC 的路必须有）
#[tauri::command]
fn ehall_ingest_pasted(
    key: String,
    text: String,
    state: State<'_, AppState>,
) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    let url = store
        .setting(&format!("ehall_json_{key}"))
        .unwrap_or_default();
    ehall_grab::store_cache(store, &key, &url, &text)
}

// ------------------------------------------------------------- 公式导出

/// 把公式页的 LaTeX 编成 PDF（调本机 XeLaTeX）；返回 PDF 路径
///
/// **必须是 async + spawn_blocking**：编译一次要 5–15 秒，以前是同步命令（跑在主线程），
/// 整窗会跟着卡死，用户以为程序崩了就关掉了（「一编译 pdf 就退出了」）。
#[tauri::command]
async fn latex_pdf(body: String, name: Option<String>) -> Result<serde_json::Value, String> {
    let stem = name
        .unwrap_or_else(|| format!("danlume-{}", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    let handle = tauri::async_runtime::spawn_blocking(move || {
        latex_export::export_pdf(get_store(), &body, &stem)
    })
    .await;
    match handle {
        Ok(Ok(path)) => Ok(json!({ "ok": true, "path": path, "error": "" })),
        Ok(Err(err)) => Ok(json!({ "ok": false, "path": "", "error": err })),
        Err(exc) => Ok(json!({ "ok": false, "path": "", "error": format!("编译任务失败：{exc}") })),
    }
}
/// 把选中文字里的公式转成 LaTeX（用户 2026-11-11：选中一段文字让 AI 转成公式并给建议）
///
/// 返回 `{ok, latex, advice, error}`：`latex` 是要替换选中内容的那段 LaTeX（含 `$…$` 或
/// `\[…\]`），`advice` 是给人看的一两句提示（哪里写错了、建议用什么写法）。
#[tauri::command]
async fn latex_formula_from_text(
    text: String,
    display: Option<bool>,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let _ = &state;
    let store = get_store();
    let raw = text.trim().to_string();
    if raw.is_empty() {
        return Ok(json!({ "ok": false, "latex": "", "advice": "", "error": "先选中一段文字" }));
    }
    if ai::config_state(store).get("ok").and_then(|one| one.as_bool()) != Some(true) {
        return Ok(json!({
            "ok": false, "latex": "", "advice": "",
            "error": "还没配置 AI 接口（设置 → AI 把 API Key 填上）",
        }));
    }
    let block = display.unwrap_or(false);
    let shape = if block {
        "行间公式：用 \\[ … \\] 包起来"
    } else {
        "行内公式：用 $ … $ 包起来"
    };
    let messages = vec![
        json!({
            "role": "system",
            "content": "你是 LaTeX 公式助手。只输出 JSON 对象，不解释、不加代码块。",
        }),
        json!({
            "role": "user",
            "content": format!(
                "把下面这段文字里的公式改写成 LaTeX。\n\
                 要求：\n\
                 1. **只输出 JSON**：{{\"latex\": \"…\", \"advice\": \"…\"}}；\n\
                 2. `latex` 是替换选中内容后的完整文本——普通文字保持原样，只把公式部分换成 LaTeX，\
                 公式用 {shape}，例如 `设 $x^2+y^2=1$`；\n\
                 3. `advice` 用一句中文说明你改了什么、或者用户怎么写会更规范（不超过 60 字，没有就空串）；\n\
                 4. 用 \\\\ 表示换行、不要用 Markdown；不要编造原文没有的内容。\n\
                 \n原文：\n{raw}"
            ),
        }),
    ];
    match ai::chat(store, &messages, Some(0.2)) {
        Ok(reply) => {
            let (latex, advice) = parse_formula_reply(&reply);
            if latex.trim().is_empty() {
                return Ok(json!({
                    "ok": false, "latex": "", "advice": "",
                    "error": "模型没给出可用的 LaTeX，换个说法再试一次",
                }));
            }
            Ok(json!({ "ok": true, "latex": latex, "advice": advice, "error": "" }))
        }
        Err(err) => Ok(json!({
            "ok": false, "latex": "", "advice": "",
            "error": format!("接口没回话：{err}"),
        })),
    }
}

/// 解析「转公式」的回复：抠出 `latex` / `advice`；模型不听话时退化成整段当公式
fn parse_formula_reply(raw: &str) -> (String, String) {
    let text = raw.trim();
    let start = text.find('{');
    let end = text.rfind('}');
    if let (Some(start), Some(end)) = (start, end) {
        if end > start {
            if let Ok(value) = serde_json::from_str::<Value>(&text[start..=end]) {
                let latex = value
                    .get("latex")
                    .and_then(|one| one.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let advice = value
                    .get("advice")
                    .and_then(|one| one.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let latex = dedupe_lines(&latex);
                if !latex.is_empty() {
                    return (latex, advice);
                }
            }
        }
    }
    // 没按 JSON 回：把代码块去掉、整段当公式（至少不丢用户的东西）
    let cleaned: String = text
        .lines()
        .filter(|line| !line.trim().starts_with("```"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    (dedupe_lines(&cleaned), String::new())
}

/// 模型偶尔把同一个公式返回两遍（行内一遍、行间一遍）：只剩一份
///
/// 2026-11-11 实测：`$x^{2}+y^{2}=1$` 连着来了两行，粘上去就是两个公式块。
fn dedupe_lines(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect();
    if lines.len() > 1 && lines.iter().all(|line| *line == lines[0]) {
        return lines[0].to_string();
    }
    text.trim().to_string()
}

// ---------------------------------------------------------------- 学生工作（通讯录 / 定时通知）

/// 通讯录列表（`keyword` 可空）
#[tauri::command]
fn contacts_list(keyword: Option<String>, state: State<'_, AppState>) -> Vec<Value> {
    let _ = &state;
    student::contacts(get_store(), keyword.as_deref().unwrap_or(""))
}

/// 新增 / 修改联系人（`id = 0` 是新增）
#[tauri::command]
fn contacts_save(item: Value, state: State<'_, AppState>) -> Result<i64, String> {
    let _ = &state;
    student::save_contact(get_store(), &item)
}

#[tauri::command]
fn contacts_delete(id: i64, state: State<'_, AppState>) -> Result<(), String> {
    let _ = &state;
    student::delete_contact(get_store(), id)
}

/// AI 能不能读通讯录（默认关，状态存在设置里）
#[tauri::command]
fn contacts_ai_allowed(state: State<'_, AppState>) -> bool {
    let _ = &state;
    student::ai_allowed(get_store())
}

#[tauri::command]
fn contacts_set_ai_allowed(on: bool, state: State<'_, AppState>) -> Result<bool, String> {
    let _ = &state;
    student::set_ai_allowed(get_store(), on)
}

#[tauri::command]
fn student_notices(state: State<'_, AppState>) -> Vec<Value> {
    let _ = &state;
    student::notices(get_store())
}

#[tauri::command]
fn student_notice_save(item: Value, state: State<'_, AppState>) -> Result<i64, String> {
    let _ = &state;
    student::save_notice(get_store(), &item)
}

#[tauri::command]
fn student_notice_delete(id: i64, state: State<'_, AppState>) -> Result<(), String> {
    let _ = &state;
    student::delete_notice(get_store(), id)
}

/// 立刻发一次（不等时间）
#[tauri::command]
async fn student_notice_send(id: i64, state: State<'_, AppState>) -> Result<Value, String> {
    let _ = &state;
    let handle = tauri::async_runtime::spawn_blocking(move || {
        student::send_now(get_store(), id)
    })
    .await;
    match handle {
        Ok(value) => Ok(value),
        Err(exc) => Err(format!("发送任务没跑起来：{exc}")),
    }
}

// ---------------------------------------------------------------- 看板娘悬浮球（T2）

/// 打开/显示桌面的看板娘小窗（置顶、无边框、不占任务栏）
///
/// 用户 2026-11-12：要一个悬浮球，把文件或文字拖到上面就能直接发给 AI。
#[tauri::command]
fn mascot_open(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("mascot") {
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(());
    }
    // 位置不能写死：写死 1500×520 在小屏（1366×768、或 125% 缩放）上直接跑到屏幕外面，
    // 表现就是「窗口开了但一片白、找不到」。改成按主显示器的工作区算，贴右边、略靠下。
    let (width, height) = (200.0_f64, 216.0_f64);
    let mut spot = (900.0_f64, 400.0_f64);
    if let Ok(Some(monitor)) = app.primary_monitor() {
        let scale = monitor.scale_factor();
        let size = monitor.size().to_logical::<f64>(scale);
        let origin = monitor.position().to_logical::<f64>(scale);
        let x = origin.x + size.width - width - 24.0;
        let y = origin.y + (size.height - height) * 0.62;
        spot = (x.max(origin.x), y.max(origin.y));
    }
    let builder = tauri::WebviewWindowBuilder::new(
        &app,
        "mascot",
        tauri::WebviewUrl::App("mascot.html".into()),
    )
    .title("DanLume 看板娘")
    .inner_size(width, height)
    .min_inner_size(140.0, 200.0)
    .resizable(true)
    .decorations(false)
    // 不透明：透明窗口在小窗里既难截图也容易出现「看不到内容」的怪状态，
    // 圆角由页面里的卡片自己画（好看的部分不受影响）
    .transparent(false)
    .shadow(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .position(spot.0, spot.1);
    builder.build().map_err(|err| format!("开不了看板娘窗口：{err}"))?;
    Ok(())
}

/// 关掉看板娘小窗
#[tauri::command]
fn mascot_close(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("mascot") {
        let _ = win.close();
    }
    Ok(())
}

/// 把悬浮球上收到的文字/文件交给主窗口的 AI 面板
///
/// 主窗口如果藏在托盘里就先亮出来，然后 emit `mascot:send`，由 AiPanel 接住
/// （它会切到 AI 标签、填好输入框、把文件放进附件）。
#[tauri::command]
fn mascot_send(
    app: tauri::AppHandle,
    text: String,
    paths: Option<Vec<String>>,
) -> Result<serde_json::Value, String> {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
    let payload = json!({
        "text": text,
        "paths": paths.unwrap_or_default(),
    });
    let _ = app.emit_to("main", "mascot:send", payload.clone());
    Ok(payload)
}

// ---------------------------------------------------------------- 桌面通知

/// 弹一条系统通知（最小化到托盘时也能看到）
///
/// 失败不报错：有的 Windows 机器把通知权限关掉了，这时还有前端的卡片兜底。
fn notify_desktop(app: &tauri::AppHandle, title: &str, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    if let Err(exc) = app
        .notification()
        .builder()
        .title(title)
        .body(body)
        .show()
    {
        eprintln!("[notify] 系统通知没弹出来：{exc}");
    }
}

// ---------------------------------------------------------------- 读微信（J8-4）

/// 直接读当前微信聊天（截图 + 系统 OCR，见 `wechat.rs`），失败退回剪贴板
///
/// 用户 2026-11-12：「读取微信消息」，先只做读，不做自动发送。
#[tauri::command]
async fn wechat_read(use_clipboard: Option<bool>) -> Result<serde_json::Value, String> {
    let handle = tauri::async_runtime::spawn_blocking(move || {
        if use_clipboard.unwrap_or(false) {
            return match crate::wechat::clipboard_text() {
                Ok(text) => json!({ "ok": true, "how": "剪贴板", "text": text }),
                Err(exc) => json!({ "ok": false, "error": exc }),
            };
        }
        match crate::wechat::grab_current_chat() {
            Ok(text) => json!({ "ok": true, "how": "直接读取的当前聊天", "text": text }),
            Err(exc) => {
                // 直读不行就试剪贴板（用户可能已经右键复制了消息）
                match crate::wechat::clipboard_text() {
                    Ok(text) => json!({
                        "ok": true,
                        "how": "剪贴板",
                        "text": text,
                        "warning": format!("直接读微信没成功（{exc}），用的是剪贴板内容"),
                    }),
                    Err(_) => json!({ "ok": false, "error": exc }),
                }
            }
        }
    })
    .await;
    match handle {
        Ok(value) => Ok(value),
        Err(exc) => Ok(json!({ "ok": false, "error": format!("读微信的任务没跑起来：{exc}") })),
    }
}

// ------------------------------------------------------------- ehall 卡片

/// 仪表板那几张卡：直接读旧版抓好的缓存（`ehall_cache_*`），不搬 cookie。
#[tauri::command]
fn ehall_cards(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    let specs = [
        ("card", "校园卡余额", "ehall_json_card"),
        ("bus", "校车安排", "ehall_json_bus"),
        ("sports", "体育刷锻", "ehall_json_sports"),
        ("library_people", "图书馆人数", "ehall_json_library_people"),
        ("ehall_todo", "我的 ehall 通知", "ehall_json_ehall_todo"),
        ("empty_room", "空教室", "ehall_json_empty_room"),
    ];
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0);
    let mut out = Vec::new();
    for (key, title, source_key) in specs {
        // 旧版只有一个总地址 ehall_url；v2 每张卡各有自己的键。这里做兜底：
        // 某张卡没填地址时用 ehall_url 顶上，不覆盖已经填好的。
        let source = store
            .setting(source_key)
            .filter(|text| !text.trim().is_empty())
            .or_else(|| {
                store
                    .setting("ehall_url")
                    .filter(|text| !text.trim().is_empty())
            })
            .unwrap_or_default();
        let cache: Value = store
            .setting(&format!("ehall_cache_{key}"))
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            .unwrap_or_else(|| json!({}));
        let ok = cache
            .get("ok")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let value_text = cache
            .get("value")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        let lines: Vec<String> = cache
            .get("lines")
            .and_then(|value| value.as_array())
            .map(|rows| {
                rows.iter()
                    .filter_map(|item| item.as_str().map(|text| text.to_string()))
                    .filter(|text| !text.trim().is_empty())
                    .take(6)
                    .collect()
            })
            .unwrap_or_default();
        let error = cache
            .get("error")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        let at = cache
            .get("ts")
            .and_then(|value| value.as_f64())
            .map(|value| value as u64)
            .unwrap_or(0);
        out.push(json!({
            "key": key,
            "title": title,
            "ok": ok,
            "value": value_text,
            "lines": lines,
            "error": if !ok && error.is_empty() { "还没同步过".to_string() } else { error },
            "source": source,
            "age_minutes": if at > 0 && now > at { ((now - at) / 60) as i64 } else { -1 },
        }));
    }
    json!({ "cards": out })
}

// ---------------------------------------------------------------------- AI

#[tauri::command]
fn ai_state(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    ai::config_state(get_store())
}

#[tauri::command]
fn ai_profiles(state: State<'_, AppState>) -> Vec<Value> {
    let _ = &state;
    ai::profiles(get_store())
}

#[tauri::command]
async fn ai_chat(
    messages: Vec<Value>,
    temperature: Option<f64>,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let _ = &state;
    let store = get_store();
    let reply = ai::chat(store, &messages, temperature)?;
    Ok(json!({ "ok": true, "reply": reply }))
}

// ---------------------------------------------------------------- AI 工具审核（J8-3）
//
// 以前「危险操作要确认」只是让模型自己带 `confirm=true` 再调一次——模型当然会点头，
// 等于没有审核。现在改成**真的问用户**：后端发 `ai:confirm` 事件后在 Condvar 上等，
// 前端把「允许一次 / 本次会话都允许 / 拒绝」用 `ai_confirm` 送回来。

/// 一个等着用户点头的工具调用
type WaitSlot = std::sync::Arc<(std::sync::Mutex<Option<String>>, std::sync::Condvar)>;

fn confirm_waiters() -> &'static std::sync::Mutex<std::collections::HashMap<String, WaitSlot>> {
    static SLOTS: OnceLock<std::sync::Mutex<std::collections::HashMap<String, WaitSlot>>> =
        OnceLock::new();
    SLOTS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// 本次会话里已经点过「都允许」的「工具 + 目标」（重启就清空，与旧版 `_SESSION_OK` 一致）
fn confirm_session_ok() -> &'static std::sync::Mutex<Vec<String>> {
    static OK: OnceLock<std::sync::Mutex<Vec<String>>> = OnceLock::new();
    OK.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

fn confirm_target(name: &str, args: &Value) -> String {
    args.get("path")
        .or_else(|| args.get("to"))
        .or_else(|| args.get("command"))
        .and_then(|one| one.as_str())
        .unwrap_or("")
        .to_string()
        .trim()
        .to_string()
}

/// 问用户一句「AI 要用这个工具，行不行」；返回 true = 允许
fn ask_tool_permission(
    app: &tauri::AppHandle,
    req_id: &str,
    seq: usize,
    name: &str,
    args: &Value,
) -> bool {
    let reason = crate::ai_tools::confirm_reason(name, args).unwrap_or_default();
    let target = confirm_target(name, args);
    let key = format!("{name}|{target}");
    // 用户之前点过「本次会话都允许」
    if confirm_session_ok()
        .lock()
        .map(|list| list.iter().any(|one| one == &key))
        .unwrap_or(false)
    {
        return true;
    }
    // 设置里开了「工具自动执行」就不再问
    if get_store()
        .setting("ai_auto_tools")
        .map(|text| text.trim() == "1")
        .unwrap_or(false)
    {
        return true;
    }
    let id = format!("{req_id}#{seq}");
    let slot: WaitSlot = std::sync::Arc::new((
        std::sync::Mutex::new(None),
        std::sync::Condvar::new(),
    ));
    if let Ok(mut map) = confirm_waiters().lock() {
        map.insert(id.clone(), slot.clone());
    }
    let _ = app.emit(
        "ai:confirm",
        json!({
            "id": id,
            "tool": name,
            "label": crate::ai_tools::label_of(name),
            "target": target,
            "reason": reason,
            "risk": if crate::ai_tools::confirm_reason(name, args).is_some() { "high" } else { "low" },
            "args": args,
        }),
    );
    let (lock, cvar) = &*slot;
    let mut guard = lock.lock().unwrap_or_else(|err| err.into_inner());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while guard.is_none() {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            break;
        }
        let (next, _timeout) = cvar
            .wait_timeout(guard, left)
            .unwrap_or_else(|err| err.into_inner());
        guard = next;
    }
    let choice = guard.clone();
    drop(guard);
    if let Ok(mut map) = confirm_waiters().lock() {
        map.remove(&id);
    }
    match choice.as_deref() {
        Some("allow") => true,
        Some("session") => {
            if let Ok(mut list) = confirm_session_ok().lock() {
                list.push(key);
            }
            true
        }
        // 拒绝，或者 120 秒没人理（面板关掉 / 用户走开了）都当拒绝
        _ => false,
    }
}

/// 前端把审核结果送回来（`choice` = allow / session / deny）
#[tauri::command]
fn ai_confirm(id: String, choice: String) -> bool {
    let slot = confirm_waiters()
        .lock()
        .ok()
        .and_then(|map| map.get(&id).cloned());
    let Some(slot) = slot else {
        return false;
    };
    let (lock, cvar) = &*slot;
    if let Ok(mut guard) = lock.lock() {
        *guard = Some(choice);
    }
    cvar.notify_all();
    true
}

/// 清掉「本次会话都允许」的记录（换场景 / 新对话时前端可以调）
#[tauri::command]
fn ai_confirm_reset() -> bool {
    if let Ok(mut list) = confirm_session_ok().lock() {
        list.clear();
    }
    true
}

/// 带工具的对话（工具审核里开着的工具才会给模型）；返回 {text, rounds, log, tools_on}
///
/// `images` 是图片附件的 `data:` URL（旧版 `ai_files.image_parts`），
/// 非空时最后一条用户消息会变成多模态内容（文字 + 图片）。
#[tauri::command]
async fn ai_chat_tools(
    app: tauri::AppHandle,
    messages: Vec<Value>,
    temperature: Option<f64>,
    images: Option<Vec<String>>,
    scene: Option<String>,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let _ = &state;
    let req_id = format!("sync-{}", std::process::id());
    let handle = tauri::async_runtime::spawn_blocking(move || {
        let store = get_store();
        let sink = app.clone();
        let base = req_id.clone();
        let mut seq = 0usize;
        let mut confirm = move |name: &str, args: &Value| {
            seq += 1;
            ask_tool_permission(&sink, &base, seq, name, args)
        };
        ai::chat_with_tools_confirm(
            store,
            &messages,
            temperature,
            &images.unwrap_or_default(),
            scene.as_deref(),
            &mut confirm,
        )
    })
    .await;
    match handle {
        Ok(value) => value,
        Err(exc) => Err(format!("对话没能跑起来：{exc}")),
    }
}

/// Agent 沙箱：根目录 / 是否允许跑命令 / 里面现有的文件
#[tauri::command]
fn agent_sandbox_state(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    crate::sandbox::overview(get_store())
}

/// 打开沙箱目录（资源管理器）
#[tauri::command]
fn agent_sandbox_open() -> Result<(), String> {
    docs::open_with_system(&crate::sandbox::root().display().to_string())
}

/// 只允许跑命令的开关（默认关，界面上点开才生效）
#[tauri::command]
fn agent_sandbox_set_run(on: bool) -> Result<serde_json::Value, String> {
    crate::sandbox::set_run_enabled(get_store(), on)
}

/// 面板上手动跑一条命令（调试 / 演示用，同样受开关约束）
#[tauri::command]
async fn agent_sandbox_run(command: String) -> Result<serde_json::Value, String> {
    let handle = tauri::async_runtime::spawn_blocking(move || {
        crate::sandbox::run(get_store(), &command)
    })
    .await;
    match handle {
        Ok(result) => result,
        Err(exc) => Err(format!("命令执行失败：{exc}")),
    }
}

/// 场景提示词清单（旧版 `prompts.load()["scenes"]`）：给 AI 面板的「场景」下拉用
#[tauri::command]
fn ai_scenes(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    let doc = crate::prompts::load(store);
    let mut scenes: Vec<Value> = Vec::new();
    if let Some(map) = doc.get("scenes").and_then(|value| value.as_object()) {
        for (key, item) in map {
            if item.get("enabled").and_then(|one| one.as_bool()) == Some(false) {
                continue;
            }
            scenes.push(json!({
                "key": key,
                "title": item.get("title").and_then(|one| one.as_str()).unwrap_or(key),
            }));
        }
    }
    scenes.sort_by(|left, right| {
        left.get("title")
            .and_then(|one| one.as_str())
            .unwrap_or("")
            .cmp(right.get("title").and_then(|one| one.as_str()).unwrap_or(""))
    });
    json!({ "ok": true, "scenes": scenes })
}

/// 把「拖进来的一串路径」展开成可当附件的文件：文件夹递归、按后缀过滤、去重。
///
/// 对应旧版 `ai_files.expand_paths`；具体规则与单测在 `ai_files.rs`。
#[tauri::command]
fn ai_expand_paths(paths: Vec<String>) -> serde_json::Value {
    let (files, notes) = crate::ai_files::expand(&paths);
    json!({ "ok": true, "files": files, "notes": notes })
}

/// 工具清单（设置页「工具审核」用：名字 / 说明 / 风险 / 默认 / 当前开关）
#[tauri::command]
fn ai_tool_catalog(state: State<'_, AppState>) -> Vec<Value> {
    let _ = &state;
    let store = get_store();
    ai_tools::catalog()
        .into_iter()
        .map(|one| {
            let name = one["name"].as_str().unwrap_or_default().to_string();
            json!({
                "name": name,
                "label": one["label"],
                "description": one["description"],
                "risk": one["risk"],
                "default": one["default"],
                "on": ai_tools::tool_on(store, &name),
            })
        })
        .collect()
}

/// 手动跑一个工具（调试 / 验收用；同样受开关约束）
#[tauri::command]
fn ai_tool_run(
    name: String,
    args: Option<Value>,
    state: State<'_, AppState>,
) -> Value {
    let _ = &state;
    ai_tools::dispatch(get_store(), &name, &args.unwrap_or_else(|| json!({})))
}

/// 流式对话：边收边推（事件 `ai:delta`），`req_id` 用来区分是哪个请求的增量
///
/// 前端点「停止生成」时调 `ai_cancel_stream(req_id)`，这里读流的循环看到就收手，
/// 返回里带 `stopped: true`，已经收到的正文不丢。
#[tauri::command]
async fn ai_chat_stream(
    app: tauri::AppHandle,
    req_id: String,
    messages: Vec<Value>,
    temperature: Option<f64>,
    images: Option<Vec<String>>,
    scene: Option<String>,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let _ = &state;
    let emit_id = req_id.clone();
    let cancel_id = req_id.clone();
    let confirm_base = req_id.clone();
    let handle = tauri::async_runtime::spawn_blocking(move || {
        let store = get_store();
        let sink = app.clone();
        let confirm_sink = app.clone();
        let mut seq = 0usize;
        let mut confirm = move |name: &str, args: &Value| {
            seq += 1;
            ask_tool_permission(&confirm_sink, &confirm_base, seq, name, args)
        };
        ai::stream_with_tools(
            store,
            &messages,
            temperature,
            &images.unwrap_or_default(),
            scene.as_deref(),
            move |kind, text| {
                // 前端没在听也无所谓，最后还有一次性返回兜底
                let _ = sink.emit(
                    "ai:delta",
                    json!({ "id": emit_id, "kind": kind, "text": text }),
                );
            },
            move || crate::ai::is_cancelled(&cancel_id),
            &mut confirm,
        )
    })
    .await;
    match handle {
        Ok(value) => value,
        Err(exc) => Err(format!("流式请求没能跑起来：{exc}")),
    }
}

/// 请停止某个还在跑的流式请求
#[tauri::command]
fn ai_cancel_stream(req_id: String) -> bool {
    ai::cancel_stream(&req_id);
    true
}

#[tauri::command]
fn ai_sessions(limit: Option<i64>, state: State<'_, AppState>) -> Vec<Value> {
    let _ = &state;
    ai::sessions(get_store(), limit.unwrap_or(30))
}

/// 改历史会话的标题
#[tauri::command]
fn ai_session_rename(id: i64, title: String, state: State<'_, AppState>) -> Result<(), String> {
    let _ = &state;
    ai::rename_session(get_store(), id, &title)
}

/// 删掉一条历史会话
#[tauri::command]
fn ai_session_delete(id: i64, state: State<'_, AppState>) -> Result<(), String> {
    let _ = &state;
    ai::delete_session(get_store(), id)
}

#[tauri::command]
fn ai_save_session(
    title: String,
    model: String,
    payload: String,
    state: State<'_, AppState>,
) -> Result<i64, String> {
    let _ = &state;
    ai::save_session(get_store(), &title, &model, &payload)
}

// ---------------------------------------------------------------- 提醒

/// 补提醒窗口（小时）：设置里叫「开机后补提醒」，默认 12，限制在 0–168
fn catchup_hours(store: &Store) -> i64 {
    store
        .setting("remind_catchup")
        .and_then(|text| text.trim().parse::<i64>().ok())
        .unwrap_or(12)
        .clamp(0, 168)
}

/// 到点该提醒的日程
#[tauri::command]
fn due_reminders(state: State<'_, AppState>) -> Vec<serde_json::Value> {
    let _ = &state;
    let store = get_store();
    // 全局开关：旧版 `reminders_enabled`（默认开）
    let reminders_on = store
        .setting("reminders_enabled")
        .map(|text| text.trim() != "0")
        .unwrap_or(true);
    // 旧版 auto_done_enabled：过期日程自动完成（每 30 秒轮询时顺手跑一次）
    let _ = crate::reminders::auto_done(&store);
    // 一次性清扫：把「窗口以外、还挂着提醒」的老记录标成已提醒，否则它们会被
    // 反复翻出来（用户反馈的那个问题）。
    let _ = sweep_stale_reminders(store);
    let mut out = if reminders_on {
        collect_event_reminders(store)
    } else {
        Vec::new()
    };
    // 「该出发了」单独一条链路（旧版 reminders.py::check_departure）
    out.extend(crate::reminders::due(store));
    out
}

fn parse_clock(text: &str) -> Option<chrono::NaiveTime> {
    chrono::NaiveTime::parse_from_str(&crate::store::clock5(text), "%H:%M").ok()
}

/// 「开始时间 - 提前分钟」= 该提醒的时刻
fn reminder_due(
    date: &str,
    start_time: &str,
    minutes: i64,
) -> Option<chrono::NaiveDateTime> {
    let day = chrono::NaiveDate::parse_from_str(date.get(..10).unwrap_or(date), "%Y-%m-%d").ok()?;
    let start = parse_clock(start_time)?;
    Some(chrono::NaiveDateTime::new(day, start) - chrono::Duration::minutes(minutes))
}

/// 这条日程「什么时候结束」（跨夜会算到第二天）
fn event_end(date: &str, start_time: &str, end_time: &str) -> Option<chrono::NaiveDateTime> {
    let day = chrono::NaiveDate::parse_from_str(date.get(..10).unwrap_or(date), "%Y-%m-%d").ok()?;
    let begin = parse_clock(start_time)?;
    let finish = if end_time.trim().is_empty() {
        chrono::NaiveTime::from_hms_opt(23, 59, 0)?
    } else {
        parse_clock(end_time)?
    };
    let mut end = chrono::NaiveDateTime::new(day, finish);
    if end < chrono::NaiveDateTime::new(day, begin) {
        end += chrono::Duration::days(1);
    }
    Some(end)
}

/// 把「窗口以外、还挂着提醒」的老记录标成已提醒。
/// 幂等，每次轮询跑一次都不心疼（只碰 notified = 0 的非重复日程）。
fn sweep_stale_reminders(store: &Store) -> usize {
    let now = chrono::Local::now().naive_local();
    let cutoff = now - chrono::Duration::hours(catchup_hours(store));
    let candidates: Vec<(i64, String, String, String)> = {
        let conn = store.connection();
        let mut out = Vec::new();
        if let Ok(mut stmt) = conn.prepare(
            "SELECT id, date, start_time, end_time FROM events
             WHERE notified = 0 AND (repeat IS NULL OR repeat = '')",
        ) {
            if let Ok(mapped) = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3).unwrap_or_default(),
                ))
            }) {
                for one in mapped.flatten() {
                    out.push(one);
                }
            }
        }
        out
    };
    let mut changed = 0usize;
    for (id, date, start, end) in candidates {
        let Some(finish) = event_end(&date, &start, &end) else {
            continue;
        };
        if finish >= cutoff {
            continue;
        }
        let conn = store.connection();
        if conn
            .execute(
                "UPDATE events SET notified = 1 WHERE id = ?1",
                rusqlite::params![id],
            )
            .is_ok()
        {
            changed += 1;
        }
    }
    changed
}

/// 到点该弹的提醒，**含重复日程展开出来的那一天**。
///
/// 规则（与旧版对齐并按用户反馈收紧）：
/// * 只看「补提醒窗口」内还没结束的（默认 12 小时）→ 两个月前的过期日程不会被翻出来；
/// * 窗口内已经结束的仍然补一次（旧版设置项 `remind_catchup` 就是这个意思）；
/// * 重复日程按天算「提醒过没有」（`notified_dates`），一次提醒不会把以后都吃掉；
/// * 普通日程仍用库里的 `remind_at`（这样「稍后 10 分钟」直接生效）。
fn collect_event_reminders(store: &Store) -> Vec<serde_json::Value> {
    let now = chrono::Local::now().naive_local();
    let cutoff = now - chrono::Duration::hours(catchup_hours(store));
    let start = (now.date() - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    let end = now.date().format("%Y-%m-%d").to_string();
    let rows = match store.events_between(&start, &end) {
        Ok(items) => items,
        Err(_) => return Vec::new(),
    };
    let mut out: Vec<serde_json::Value> = Vec::new();
    for row in rows {
        if row.done || row.notified {
            continue;
        }
        let Some(minutes) = row.remind_minutes else {
            continue;
        };
        let Some(finish) = event_end(&row.date, &row.start_time, &row.end_time) else {
            continue;
        };
        // 已经结束的日程不再补提醒：用户反馈「15:10 结束的会在 16:10 又弹一次」，
        // 补提醒只对「还没结束」的有意义（错过了但正在进行，提醒仍然有用）。
        if finish < cutoff || finish < now {
            continue;
        }
        let mut due = match reminder_due(&row.date, &row.start_time, minutes) {
            Some(value) => value,
            None => continue,
        };
        if row.repeat.is_empty() {
            if let Some(stored) = row
                .remind_at
                .as_deref()
                .filter(|text| !text.trim().is_empty())
                .and_then(|text| {
                    chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").ok()
                })
            {
                due = stored;
            }
        } else if let Ok(snooze) =
            chrono::NaiveDateTime::parse_from_str(&row.snooze_until, "%Y-%m-%d %H:%M:%S")
        {
            // 「稍后 10 分钟」：同一天、比原定更晚就按它算
            if snooze.date() == due.date() && snooze > due {
                due = snooze;
            }
        }
        if now < due {
            continue;
        }
        out.push(json!({
            "id": row.id,
            "title": row.title,
            "date": row.date,
            "start_time": row.start_time,
            "end_time": row.end_time,
            "remind_at": due.format("%Y-%m-%d %H:%M:%S").to_string(),
            "remind_minutes": minutes,
            "repeat": row.repeat,
        }));
        if out.len() >= 5 {
            break;
        }
    }
    out
}

/// 记「这一次提醒过了」。`occurrence_date` 给了就按天记（重复日程）。
#[tauri::command]
fn mark_notified(
    id: i64,
    occurrence_date: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let _ = &state;
    get_store()
        .mark_notified(id, occurrence_date.as_deref())
        .map(|_| ())
        .map_err(|err| err.to_string())
}

/// 「稍后 10 分钟」：普通日程把 remind_at 往后推；重复日程只推迟那一天。
#[tauri::command]
fn snooze_event(
    id: i64,
    minutes: Option<i64>,
    occurrence_date: Option<String>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let _ = &state;
    get_store()
        .set_snooze(id, minutes.unwrap_or(10), occurrence_date.as_deref())
        .map_err(|err| err.to_string())
}

/// 整条重复系列一起标完成 / 取消（对应「整串完成」按钮）
#[tauri::command]
fn event_series_done(
    id: i64,
    done: bool,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let _ = &state;
    get_store()
        .set_series_done(id, done)
        .map_err(|err| err.to_string())
}

/// 新增 / 修改一套 AI 配置（id = 0 = 新增）
#[tauri::command]
fn ai_profile_save(
    id: Option<i64>,
    name: String,
    base_url: String,
    model: String,
    state: State<'_, AppState>,
) -> Result<i64, String> {
    let _ = &state;
    ai::save_profile(get_store(), id.unwrap_or(0), &name, &base_url, &model)
}

#[tauri::command]
fn ai_profile_delete(id: i64, state: State<'_, AppState>) -> Result<bool, String> {
    let _ = &state;
    ai::delete_profile(get_store(), id)
}

// -------------------------------------------------  AI 记忆库 / 提示词（新增）
//
// 记忆库：`memory.rs`，对应旧版 `lumina/memory.py`；注入在 `ai.rs` 的 chat_with_tools 里。
// 提示词：`prompts.rs`，对应旧版 `lumina/prompts.py` + `lumina/prompt_gen.py`。
// 出错一律用 `{ok:false, error}` 返回，不往前端抛异常（旧版这两块也是「错误不影响使用」）。

#[tauri::command]
fn memory_list(limit: Option<i64>, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    let items = memory::list(store, limit.unwrap_or(200).max(1) as usize);
    json!({ "ok": true, "items": items, "total": memory::count(store) })
}

#[tauri::command]
fn memory_add(
    content: String,
    source: Option<String>,
    tags: Option<String>,
    state: State<'_, AppState>,
) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    let source = source
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "手动".to_string());
    match memory::add(store, &content, &source, &tags.unwrap_or_default()) {
        Ok((id, duplicated)) => json!({
            "ok": true,
            "id": id,
            "duplicated": duplicated,
            "total": memory::count(store),
            "message": if duplicated { "这条已经记过了，刷新了时间" } else { "已记下" },
        }),
        Err(err) => json!({ "ok": false, "error": err }),
    }
}

#[tauri::command]
fn memory_search(keyword: String, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    let items = memory::search(store, &keyword);
    json!({ "ok": true, "items": items, "total": items.len() })
}

#[tauri::command]
fn memory_update(
    id: i64,
    content: Option<String>,
    source: Option<String>,
    tags: Option<String>,
    state: State<'_, AppState>,
) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    match memory::update(
        store,
        id,
        content.as_deref(),
        source.as_deref(),
        tags.as_deref(),
    ) {
        Ok(true) => json!({ "ok": true, "id": id }),
        Ok(false) => json!({ "ok": false, "error": "这条记忆不在了（可能刚被删掉）" }),
        Err(err) => json!({ "ok": false, "error": err }),
    }
}

#[tauri::command]
fn memory_delete(id: i64, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    match memory::delete(store, id) {
        Ok(true) => json!({ "ok": true, "total": memory::count(store) }),
        Ok(false) => json!({ "ok": false, "error": "这条记忆不在了" }),
        Err(err) => json!({ "ok": false, "error": err }),
    }
}

#[tauri::command]
fn memory_clear(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    match memory::clear(get_store()) {
        Ok(removed) => json!({ "ok": true, "removed": removed }),
        Err(err) => json!({ "ok": false, "error": err }),
    }
}

#[tauri::command]
fn prompt_templates(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    let doc = prompts::load(store);
    let items = prompts::templates(store);
    json!({
        "ok": true,
        "items": items,
        "version": doc["version"].clone(),
        "updated_at": doc["updated_at"].clone(),
        "note": doc["note"].clone(),
    })
}

#[tauri::command]
fn prompt_template_save(
    kind: String,
    key: String,
    title: Option<String>,
    text: String,
    state: State<'_, AppState>,
) -> serde_json::Value {
    let _ = &state;
    match prompts::template_save(
        get_store(),
        &kind,
        &key,
        &title.unwrap_or_default(),
        &text,
    ) {
        Ok(value) => value,
        Err(err) => json!({ "ok": false, "error": err }),
    }
}

#[tauri::command]
fn prompt_template_delete(
    kind: String,
    key: String,
    state: State<'_, AppState>,
) -> serde_json::Value {
    let _ = &state;
    match prompts::template_delete(get_store(), &kind, &key) {
        Ok(value) => value,
        Err(err) => json!({ "ok": false, "error": err }),
    }
}

/// 提示词生成器：一段需求描述 → 一份结构化提示词（没配 AI 时给友好提示）
#[tauri::command]
fn prompt_generate(requirement: String, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    prompts::generate(get_store(), &requirement)
}

// ------------------------------------------------------------- 数据与关于

/// 数据与关于：数据目录、数据库大小、各类条数、版本
#[tauri::command]
fn data_info(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    let db = paths::db_path();
    let size = std::fs::metadata(&db).map(|meta| meta.len()).unwrap_or(0);
    let mut counts = serde_json::Map::new();
    {
        let conn = store.connection();
        for (label, sql) in [
            ("events", "SELECT COUNT(*) FROM events"),
            ("memos", "SELECT COUNT(*) FROM memos"),
            ("notices", "SELECT COUNT(*) FROM notices"),
            ("papers", "SELECT COUNT(*) FROM papers"),
            ("mails", "SELECT COUNT(*) FROM mails"),
            ("chat_sessions", "SELECT COUNT(*) FROM chat_sessions"),
        ] {
            let value: i64 = conn.query_row(sql, [], |row| row.get(0)).unwrap_or(0);
            counts.insert(label.to_string(), json!(value));
        }
    }
    json!({
        "data_dir": paths::data_dir().display().to_string(),
        "db_path": db.display().to_string(),
        "db_size": size,
        "counts": counts,
        "version": env!("CARGO_PKG_VERSION"),
        "kernel": "Rust + Tauri 2 + WebView2",
    })
}

/// 备份数据库：复制一份到 data/backups/lumina-<时间>.db，返回路径
#[tauri::command]
fn backup_db(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let source = paths::db_path();
    let folder = paths::data_dir().join("backups");
    if std::fs::create_dir_all(&folder).is_err() {
        return json!({ "ok": false, "path": "", "error": "建不了 backups 目录" });
    }
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let target = folder.join(format!("lumina-{stamp}.db"));
    match std::fs::copy(&source, &target) {
        Ok(bytes) => json!({ "ok": true, "path": target.display().to_string(), "bytes": bytes, "error": "" }),
        Err(err) => json!({ "ok": false, "path": "", "error": format!("备份失败：{err}") }),
    }
}

// ------------------------------------------------------------- 数据迁移（.dlpk）

/// 导出数据包：把 lumina.db 的一致性快照 + manifest + README 打成 .dlpk（其实是 zip）
#[tauri::command]
fn data_export(path: String, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    match backup::export(&paths::db_path(), std::path::Path::new(&path)) {
        Ok(value) => value,
        Err(err) => json!({
            "ok": false,
            "path": path,
            "bytes": 0,
            "tables": {},
            "stripped_keys": [],
            "error": err,
        }),
    }
}

/// 从数据包导入：先备份、再把包里的库展开成 lumina.db.imported，下次启动才换库
#[tauri::command]
fn data_import(path: String, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let data_dir = paths::data_dir();
    match backup::import(&paths::db_path(), std::path::Path::new(&path), &data_dir) {
        Ok(value) => value,
        Err(err) => json!({
            "ok": false,
            "needs_restart": false,
            "backup": "",
            "tables": {},
            "stripped_keys": [],
            "error": err,
        }),
    }
}

/// 重新启动本程序（导入数据后「立即重启」用；没装 plugin-process，自己 spawn 自己）
#[tauri::command]
fn app_relaunch(app: tauri::AppHandle) -> serde_json::Value {
    let exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(err) => {
            return json!({ "ok": false, "error": format!("找不到程序自身：{err}（请手动关闭后重新打开）") })
        }
    };
    match std::process::Command::new(&exe)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(_) => {
            // 留一点时间把「已重启」的返回送到前端，再退出（新进程会等旧进程放开数据库）
            let handle = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(500));
                handle.exit(0);
            });
            json!({ "ok": true, "error": "" })
        }
        Err(err) => json!({
            "ok": false,
            "error": format!("自动重启失败：{err}（请手动关闭后重新打开）"),
        }),
    }
}

// ------------------------------------------------------------------ 专注（离开检测）
#[tauri::command]
fn focus_start(
    minutes: Option<i64>,
    label: Option<String>,
    state: State<'_, AppState>,
) -> serde_json::Value {
    let _ = &state;
    crate::focus::start(get_store(), minutes, &label.unwrap_or_default())
}

#[tauri::command]
fn focus_stop(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    crate::focus::stop(get_store())
}

/// 查状态；会话进行中就顺带做一次心跳（前端每 15 秒轮一次即可）
#[tauri::command]
fn focus_status(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    crate::focus::tick(get_store())
}

// ------------------------------------------------------------------ 通知自动抓取
/// 立刻抓一轮（不管间隔），返回新增条数等
#[tauri::command]
fn notices_auto_run(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    crate::notices_auto::run_once(get_store())
}

/// 自动抓取的状态：开关、间隔、上次时间与条数、是否在静默时段、来源数
#[tauri::command]
fn notices_auto_status(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    json!({
        "enabled": crate::notices_auto::enabled(store),
        "interval_hours": crate::notices_auto::interval_hours(store),
        "due": crate::notices_auto::due(store),
        "sources": crate::notices_auto::sources(store),
        "last_run": store.setting("notice_last_run").unwrap_or_default(),
        "last_count": store.setting("notice_last_count").unwrap_or_default(),
        "quiet": crate::notices_auto::in_quiet_hours(store, chrono::Local::now().time()),
    })
}

/// 后台每 10 分钟看一次，到点（默认 6 小时）才真的联网抓
fn spawn_notice_watcher() {
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(45));
        loop {
            {
                let store = get_store();
                if crate::notices_auto::due(store) {
                    let result = crate::notices_auto::run_once(store);
                    if result["added"].as_i64().unwrap_or(0) > 0 {
                        println!("[通知] {}", result);
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(600));
        }
    });
}

// ------------------------------------------------------------------ 使用上报（旧版 usage_report.py 的客户端半边）
#[tauri::command]
fn usage_status(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    crate::usage::status(get_store())
}

/// 立刻上报一次（设置页的手动按钮）
#[tauri::command]
fn usage_report_now(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    crate::usage::send(get_store())
}

/// 后台每 5 分钟看一次，到点（默认 10 分钟）才真的发
fn spawn_usage_watcher() {
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(90));
        loop {
            {
                let store = get_store();
                if crate::usage::due(store) {
                    let result = crate::usage::send(store);
                    if result["ok"] != true {
                        println!("[上报] {}", result["message"]);
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(300));
        }
    });
}

// ------------------------------------------------------------------ 账号服务器（自家 FastAPI）
//
// 老的「客户端 RSA 信封 + 邮件申请 + 管理员签发」那条路（registration.rs / adminsign.rs）
// 原样保留，只是降级成兜底；主路径改成直接连账号服务器。

/// 探测服务器（设置页「测试连接」用）：返回 `{ok,online,version,latency_ms,...}`
#[tauri::command]
fn account_health() -> Result<serde_json::Value, account::ApiError> {
    account::health()
}

/// 注册前先让服务器给邮箱发验证码（`POST /api/v1/register/request-code`）。
///
/// 成功回 `{ok, email, expires_in, cooldown}`；失败按 `ApiError.kind` 分类，
/// 429 带 `retry_after`、400 invalid_code 带 `attempts_left`。
#[tauri::command]
fn account_request_email_code(email: String) -> Result<serde_json::Value, account::ApiError> {
    account::request_email_code(&email)
}

/// 忘记密码第一步：发重置验证码
#[tauri::command]
fn account_request_reset_code(email: String) -> Result<serde_json::Value, account::ApiError> {
    account::request_reset_code(&email)
}

/// 忘记密码第二步：验证码 + 新口令
#[tauri::command]
fn account_reset_password(
    email: String,
    code: String,
    password: String,
) -> Result<serde_json::Value, account::ApiError> {
    account::reset_password(&email, &code, &password)
}

#[tauri::command]
fn account_register(
    username: String,
    email: String,
    password: String,
    invite_code: Option<String>,
    code: Option<String>,
) -> Result<serde_json::Value, account::ApiError> {
    let result = account::register(
        &username,
        &email,
        &password,
        invite_code.as_deref(),
        code.as_deref(),
    )?;
    // 登录态变了：让公告轮询立刻按新 token 拉一次
    crate::notify::kick();
    Ok(result)
}

#[tauri::command]
fn account_login(
    username_or_email: String,
    password: String,
) -> Result<serde_json::Value, account::ApiError> {
    let result = account::login(&username_or_email, &password)?;
    crate::notify::kick();
    Ok(result)
}

#[tauri::command]
fn account_logout() -> serde_json::Value {
    account::logout()
}

#[tauri::command]
fn account_me() -> Result<serde_json::Value, account::ApiError> {
    account::me()
}

#[tauri::command]
fn account_devices() -> Result<serde_json::Value, account::ApiError> {
    account::devices()
}

#[tauri::command]
fn account_kick_device(device_id: i64) -> Result<serde_json::Value, account::ApiError> {
    account::kick_device(device_id)
}

#[tauri::command]
fn account_delete_me() -> Result<serde_json::Value, account::ApiError> {
    account::delete_me()
}

/// 账号相关的全部状态（设置页 / 标题栏一次拿全）
#[tauri::command]
fn account_state() -> serde_json::Value {
    account::state()
}

/// 保存服务器地址（空字符串 = 不用服务器），并让公告轮询按新地址重建
#[tauri::command]
fn account_save_server(url: String, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let saved = account::save_server_url(get_store(), &url);
    crate::notify::kick();
    saved
}

/// 本地 SQLite 里存着的服务器公告（新→旧，置顶在前）
#[tauri::command]
fn account_announcements(limit: Option<i64>, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    let items = store.announcements(limit.unwrap_or(100)).unwrap_or_default();
    json!({
        "ok": true,
        "items": items,
        "total": items.len(),
        "unread": store.announcement_unread_count(),
        "last_id": store.setting("last_announcement_id").unwrap_or_default(),
        "notify": crate::notify::status(),
    })
}

#[tauri::command]
fn account_mark_announcements_read(
    id: Option<i64>,
    state: State<'_, AppState>,
) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    let changed = store.mark_announcements_read(id).unwrap_or(0);
    json!({ "ok": true, "changed": changed, "unread": store.announcement_unread_count() })
}

/// 立刻拉一次公告（不等 20 秒的轮询）
#[tauri::command]
fn account_check_announcements(app: tauri::AppHandle) -> serde_json::Value {
    crate::notify::check_now(&app)
}

/// 公告通道状态（模式 / 上次成功 / 上次错误 / 退避）
#[tauri::command]
fn account_notify_status() -> serde_json::Value {
    crate::notify::status()
}

// ------------------------------------------------------------------ UIS / CAS login
#[tauri::command]
fn uis_status(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    crate::uis::status(get_store())
}

#[tauri::command]
fn uis_login(
    username: String,
    password: String,
    state: State<'_, AppState>,
) -> serde_json::Value {
    let _ = &state;
    match crate::uis::login(get_store(), &username, &password) {
        Ok(value) => value,
        Err(err) => json!({ "ok": false, "error": err }),
    }
}

#[tauri::command]
fn uis_logout(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    crate::uis::clear_session(get_store());
    json!({ "ok": true })
}

// ------------------------------------------------------------------ UIS: embedded login window (ID system is an SPA)
//
// 与 Python 版（`uis_login.autofill_script`）保持同一套做法：
// 打开内嵌登录页 → 注入脚本把学号密码填好并点登录 → 登录成功后从窗口里把会话抓下来存 DPAPI。
// 用户只需要「打开登录页」一步（有二次验证时手动过一次），不再需要单独点「抓取会话」。
/// Python 版语义：登录之后把业务站各走一遍，让它们各自把 cookie 发下来，再合并成一份会话。
///
/// 关键点（2026-11-10 实测踩到）：**只访问站点首页只会拿到游客会话**（Canvas 会回 401）。
/// 必须走各站的 CAS 登录入口（eLearning 是 `/login/saml`，教务是 course-table 触发跳转），
/// 让浏览器完成一次真正的 SSO，页面才会带上登录态。

/// 用合并后的会话直接请求一次 Canvas API：200 才算「真的登录了」。
fn verify_canvas_session(jar: &crate::uis::Jar) -> String {
    let url = "https://elearning.fudan.edu.cn/api/v1/courses?enrollment_state=active&per_page=1";
    match crate::net::with_proxy(
        ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(30))
            .redirects(3)
            .user_agent(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                 (KHTML, like Gecko) Chrome/120.0 Safari/537.36",
            ),
        get_store(),
        url,
    )
    .build()
    .get(url)
    .set("Cookie", &jar.to_header())
    .set("Accept", "application/json")
    .call()
    {
        Ok(response) => format!("Canvas API {}", response.status()),
        Err(ureq::Error::Status(code, _)) => format!("Canvas API {code}（会话没被接受）"),
        Err(exc) => format!("Canvas API 请求失败：{exc}"),
    }
}

static TOUR_DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 读一段 JS 的返回值（失败/超时给一句说明，不 panic）
fn eval_text(window: &tauri::WebviewWindow, js: &str) -> String {
    let (tx, rx) = std::sync::mpsc::channel();
    if window
        .eval_with_callback(js.to_string(), move |value| {
            let _ = tx.send(value);
        })
        .is_err()
    {
        return "(eval 调用失败)".to_string();
    }
    rx.recv_timeout(std::time::Duration::from_secs(4))
        .unwrap_or_else(|_| "(eval 超时)".to_string())
}

/// 同上，但只接受能解析成 JSON 的返回值（脚本里返回对象/数组时用）
fn eval_json(window: &tauri::WebviewWindow, js: &str) -> Option<serde_json::Value> {
    let raw = eval_text(window, js);
    serde_json::from_str(&raw).ok()
}

/// 登录窗口的入口：**与 Python 版一致，先开业务页**（`elearning_login.py` 是
/// `BASE_URL + "/"`），由业务站跳到统一身份认证登录页。
///
/// 实测（2026-11-10）：`https://elearning.fudan.edu.cn/login` 会 302 到
/// `https://id.fudan.edu.cn/ac/#/index?lck=context_CAS_…&entityId=https://elearning.fudan.edu.cn`，
/// 那才是新版登录页（带密码框，自动填表才有地方填）；而 `/login/saml` 现在直接 404，
/// 光开 `id.fudan.edu.cn` 会落到「用户中心」也没有登录框。
const UIS_ENTRY: &str = "https://elearning.fudan.edu.cn/login";

/// 手动「重新抓取会话」时先走一遍 CAS 入口：SSO 还有效就直接弹回来，
/// 失效就把登录页带出来，等自动填表把它填完。
const CAS_ENTRY: &str = "https://elearning.fudan.edu.cn/login";

/// 业务站顺序：先 eLearning（把 SSO 会话换成 Canvas 会话），再教务、eHall。
const TOUR_SITES: [(&str, &str); 3] = [
    ("eLearning", "https://elearning.fudan.edu.cn/"),
    ("教务", "https://fdjwgl.fudan.edu.cn/student/for-std/course-table"),
    ("eHall", "https://ehall.fudan.edu.cn/"),
];

/// 登录页现在的样子：有没有密码框、框里填了没有、标题是什么。
/// 只看长度，不把密码写进日志。
const PROBE_JS: &str = "JSON.stringify({url: location.href, hasPwd: !!document.querySelector('input[type=password]'), pwdLen: (function(){var e=document.querySelector('input[type=password]');return e?e.value.length:0;})(), title: document.title})";

/// 等自动填表把登录走完：密码框消失（或跳回业务页）就算走完，最多等 26 秒。
/// 第一次登录要加载 SPA + RSA 加密提交，慢的时候十几秒。
fn wait_for_login(window: &tauri::WebviewWindow) -> String {
    let mut last = String::new();
    for round in 1..=12 {
        std::thread::sleep(std::time::Duration::from_millis(2000));
        last = eval_text(window, PROBE_JS);
        uis_log(&format!("[tour] 登录页第 {round} 次探测：{last}"));
        let done = last.contains("\"hasPwd\":false")
            || window
                .url()
                .map(|one| {
                    let host = one.host_str().unwrap_or("").to_ascii_lowercase();
                    !crate::uis_autofill::is_login_host(&host) && host.ends_with("fudan.edu.cn")
                })
                .unwrap_or(false);
        if done {
            uis_log("[tour] 已经离开登录页，当作登录完成");
            break;
        }
    }
    last
}

/// 手动刷新会话：走一遍 CAS 入口把登录页带出来（不主动清 cookie —— Python 版也不清，
/// 用户想换账号就在窗口里自己退出）。
fn start_cas_login(window: &tauri::WebviewWindow) {
    uis_log("[tour] 走一遍 CAS 入口拿登录态");
    match CAS_ENTRY.parse::<tauri::Url>() {
        Ok(url) => {
            if let Err(exc) = window.navigate(url) {
                uis_log(&format!("[tour] 打开 CAS 入口失败：{exc}"));
                return;
            }
        }
        Err(exc) => {
            uis_log(&format!("[tour] CAS 入口地址不对：{exc}"));
            return;
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(3000));
    let state = wait_for_login(window);
    if state.contains("\"hasPwd\":true") {
        uis_log("[tour] 登录页还有密码框：可能需要短信/扫码二次验证，或者自动填表没成功");
    }
}

fn run_cookie_tour(window: &tauri::WebviewWindow, reset_login: bool) {
    use std::sync::atomic::Ordering;
    if TOUR_DONE.swap(true, Ordering::SeqCst) {
        return; // 每次登录只走一遍，免得来回跳
    }
    uis_log("[tour] 开始逐个访问业务站，收集 cookie");
    // 手动点「重新抓取会话」时先走一遍 CAS 入口，把登录页带出来（与 Python 版一致：
    // 登录页由业务站跳过去，不自己去开 id.fudan 首页）
    if reset_login {
        start_cas_login(window);
    }
    let mut merged = crate::uis::Jar::default();
    for (label, raw) in TOUR_SITES {
        let Ok(url) = raw.parse::<tauri::Url>() else {
            continue;
        };
        if let Err(exc) = window.navigate(url) {
            uis_log(&format!("[tour] 打开{label}失败：{exc}"));
            continue;
        }
        // 等页面把 cookie 设下来；CAS 走一趟可能要好几秒
        std::thread::sleep(std::time::Duration::from_millis(3500));
        // 落地地址能看出有没有被弹回登录页（弹回去就先等自动填表把登录走完）
        let landed = window
            .url()
            .map(|u| u.to_string())
            .unwrap_or_else(|exc| format!("(取不到 URL：{exc})"));
        let host = window
            .url()
            .ok()
            .and_then(|one| one.host_str().map(|text| text.to_ascii_lowercase()))
            .unwrap_or_default();
        if crate::uis_autofill::is_login_host(&host) {
            uis_log(&format!("[tour] {label} 被弹到统一身份认证登录页，等自动填表"));
            wait_for_login(window);
        }
        let jar = collect_cookies(window);
        uis_log(&format!(
            "[tour] {label} 之后：落地={} cookie {} 个",
            shorten_url(&landed),
            jar.count()
        ));
        // 值只打印前 8 位，够判断是空值 / 占位值，又不会把会话写进日志
        for (name, value) in jar.entries() {
            let head: String = value.chars().take(8).collect();
            uis_log(&format!(
                "[tour]   {name} = {head}…（{} 字符）",
                value.chars().count()
            ));
        }
        merged.extend_from(&jar);
    }
    // 把窗口带回业务入口，别把用户留在教务页上
    if let Ok(url) = UIS_ENTRY.parse::<tauri::Url>() {
        let _ = window.navigate(url);
    }
    if merged.is_empty() {
        uis_log("[tour] 一圈下来一个 cookie 都没拿到");
        return;
    }
    // 关键：验证会话是不是真的能用（只访问了首页的话 Canvas 会回 401）
    let verdict = verify_canvas_session(&merged);
    uis_log(&format!("[tour] 会话自检：{verdict}"));
    let store = get_store();
    let user = store.setting("uis_username").unwrap_or_default();
    match crate::uis::save_session(store, &merged, &user) {
        Ok(()) => uis_log(&format!(
            "[tour] 已保存会话：{} 个 cookie，CASTGC={}，{}",
            merged.count(),
            merged.get("CASTGC").is_some(),
            verdict
        )),
        Err(exc) => uis_log(&format!("[tour] 保存会话失败：{exc}")),
    }
}

/// 抓会话时要合并的域：只读 id.fudan.edu.cn 会漏掉业务站的 cookie（实测只有 1 个、没有 CASTGC）
const COOKIE_HOSTS: [&str; 4] = [
    "https://id.fudan.edu.cn/",
    "https://fdjwgl.fudan.edu.cn/",
    "https://elearning.fudan.edu.cn/",
    "https://ehall.fudan.edu.cn/",
];

/// 日志里只留「主机 + 路径前 60 字」，避免把长查询串写进去
fn shorten_url(url: &str) -> String {
    let trimmed = url.split('?').next().unwrap_or(url);
    if trimmed.chars().count() <= 80 {
        trimmed.to_string()
    } else {
        let head: String = trimmed.chars().take(80).collect();
        format!("{head}…")
    }
}

fn uis_log(message: &str) {    use std::io::Write;
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let line = format!("[{stamp}] {message}\n");
    println!("[uis] {message}");
    let path = std::env::temp_dir().join("danlume-uis.log");
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
    }
}

/// 把几个校内域的 cookie 合并成一个 jar（Python 版是直接读整个 WebEngine profile，等价）
fn collect_cookies(window: &tauri::WebviewWindow) -> crate::uis::Jar {
    let mut jar = crate::uis::Jar::default();
    for raw in COOKIE_HOSTS {
        let Ok(url) = raw.parse::<tauri::Url>() else {
            continue;
        };
        match window.cookies_for_url(url) {
            Ok(list) => {
                for cookie in &list {
                    jar.set(cookie.name(), cookie.value());
                }
            }
            Err(exc) => uis_log(&format!("读 {raw} 的 cookie 失败：{exc}")),
        }
    }
    jar
}

/// 注入到登录窗口：把页面自己发的 `print-data` 请求抄一份到 `window.__danlumePrint`。
///
/// 为什么要这样：教务那跳的 CAS 票据是**页面自己**换的，进程里拿 cookie 复现会给 403/重定向；
/// 而页面切学期时自己就会请求 `…/semester/<数字>/print-data`，我们只要在旁边抄下来，
/// 票和学期都不必猜（与 Python 版 `fdu.py` 的请求拦截等价）。
const PRINT_INTERCEPT_JS: &str = r#"(function(){
  if (window.__danlumePrintHook) { return; }
  window.__danlumePrintHook = true;
  window.__danlumePrint = null;
  function keep(url, body){
    try {
      if (!url || String(url).indexOf('print-data') < 0) { return; }
      if (!body || body.length <= 4) { return; }
      window.__danlumePrint = {url: String(url), body: String(body)};
    } catch (e) {}
  }
  var origFetch = window.fetch;
  if (origFetch) {
    window.fetch = function(input, init){
      var url = (typeof input === 'string') ? input : ((input && input.url) || '');
      var promise = origFetch.apply(this, arguments);
      try {
        promise.then(function(resp){
          try { resp.clone().text().then(function(text){ keep(url, text); }); } catch (e) {}
          return resp;
        });
      } catch (e) {}
      return promise;
    };
  }
  var open = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function(method, url){
    try {
      this.addEventListener('load', function(){
        try { keep(url, this.responseText); } catch (e) {}
      });
    } catch (e) {}
    return open.apply(this, arguments);
  };
})()"#;

/// 读拦截到的 print-data：没抓到就是 null
const PRINT_CAPTURED_JS: &str =
    "(function(){ var hit = window.__danlumePrint; return hit ? {url: hit.url, body: hit.body} : null; })()";

/// 页面上点一下「课表」之类的入口，让页面自己去请求 print-data
const CLICK_TIMETABLE_JS: &str = r#"(function(){
  function norm(t){ return String(t || '').replace(/\s+/g, ''); }
  var wanted = ['课表', '打印课表', '查看课表', '课表打印', '全部课程'];
  var nodes = document.querySelectorAll('a, button, div[role="button"], li, span');
  for (var w = 0; w < wanted.length; w++) {
    for (var i = 0; i < nodes.length; i++) {
      if (norm(nodes[i].textContent) === wanted[w]) {
        try { nodes[i].click(); return wanted[w]; } catch (e) {}
      }
    }
  }
  return '';
})()"#;

fn open_uis_window(app: &tauri::AppHandle) -> Result<(), String> {
    // 每次打开都允许重新走一遍「业务站收 cookie」
    TOUR_DONE.store(false, std::sync::atomic::Ordering::SeqCst);
    if let Some(win) = app.get_webview_window("uis-login") {
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(());
    }
    let url: tauri::Url = UIS_ENTRY
        .parse()
        .map_err(|exc| format!("URL parse failed: {exc}"))?;
    let script = crate::uis_autofill::script(get_store());
    let mut builder =
        tauri::WebviewWindowBuilder::new(app, "uis-login", tauri::WebviewUrl::External(url))
            .title("登录 eLearning / 统一身份认证（学号密码会自动填好）")
            .inner_size(560.0, 800.0)
            // 顺手把「页面自己请求的 print-data」抄一份：教务的票是浏览器现换的，
            // 进程内复现不了（见 `fetch_print_in_window`）
            .initialization_script(PRINT_INTERCEPT_JS);
    if !script.is_empty() {
        builder = builder.initialization_script(script);
    }
    builder
        .on_page_load(|window, payload| {
            let host = payload.url().host_str().unwrap_or("").to_ascii_lowercase();
            // 还在登录页：等用户（或脚本）把登录走完
            if crate::uis_autofill::is_login_host(&host) {
                return;
            }
            // 跳到别的站点了（外链之类）：不管
            if !host.ends_with("fudan.edu.cn") {
                return;
            }
            // 回到校内业务页 = 登录成功，自动把会话存下来。
            //
            // **必须放到子线程里做**：`cookies_for_url` 是通过 dispatcher 同步等主线程的，
            // 而 on_page_load 本身就跑在主线程上，直接调用会自锁（表现为界面卡死/看起来像退出）。
            let win = window.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(800));
                run_cookie_tour(&win, false);
            });
        })
        .build()
        .map_err(|exc| format!("打不开登录窗口：{exc}"))?;
    Ok(())
}

#[tauri::command]
async fn uis_open_login(app: tauri::AppHandle) -> Result<(), String> {
    open_uis_window(&app)
}

#[allow(dead_code)]


/// 从**主窗口**的 cookie 里抓一次会话（内嵌网页登录成功后用）。
///
/// 内嵌的 eHall / eLearning 和主窗口共用同一个 WebView2 profile，所以登录完成后
/// 直接读主窗口的 cookie 就能拿到会话，不用再开登录窗口。
#[tauri::command]
async fn uis_grab_profile(app: tauri::AppHandle) -> serde_json::Value {
    let handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let Some(win) = handle.get_webview_window("main") else {
            return json!({ "ok": false, "error": "找不到主窗口" });
        };
        let jar = collect_cookies(&win);
        if jar.is_empty() {
            return json!({ "ok": false, "error": "还没拿到 cookie：先在网页里登录一次" });
        }
        let verdict = verify_canvas_session(&jar);
        let store = get_store();
        let user = store.setting("uis_username").unwrap_or_default();
        let has_service = jar.names().iter().any(|name| {
            matches!(
                name.as_str(),
                "CASTGC" | "_normandy_session" | "_legacy_normandy_session" | "log_session_id" | "SESS_ID"
            )
        });
        match crate::uis::save_session(store, &jar, &user) {
            Ok(()) => json!({
                "ok": true,
                "cookies": jar.count(),
                "has_service_session": has_service,
                "verdict": verdict,
            }),
            Err(exc) => json!({ "ok": false, "error": exc }),
        }
    })
    .await;
    match result {
        Ok(value) => value,
        Err(exc) => json!({ "ok": false, "error": format!("抓取会话失败：{exc}") }),
    }
}

/// 从登录窗口抓 cookie 存成会话（旧版读磁盘 cookie 的等价做法）。
///
/// 做成 **async + spawn_blocking**：`cookies_for_url` 走 dispatcher 同步等主线程，
/// 如果这个命令在主线程上执行（同步命令就是），就会自锁 —— 用户看到的是「点了没反应 /
/// 程序像是退出」。放到线程池里跑，主线程才能把结果回给它。
#[tauri::command]
async fn uis_capture(app: tauri::AppHandle) -> serde_json::Value {
    let handle = app.clone();
    match tauri::async_runtime::spawn_blocking(move || capture_session_now(&handle)).await {
        Ok(value) => value,
        Err(exc) => json!({ "ok": false, "error": format!("读会话失败：{exc}") }),
    }
}

fn capture_session_now(app: &tauri::AppHandle) -> serde_json::Value {
    let Some(win) = app.get_webview_window("uis-login") else {
        return json!({ "ok": false, "error": "登录窗口没开：先点「打开登录页」" });
    };
    // 手动点「重新抓取会话」= 用户想刷新会话，允许再走一遍业务站
    TOUR_DONE.store(false, std::sync::atomic::Ordering::SeqCst);
    run_cookie_tour(&win, true);
    let jar = collect_cookies(&win);
    if jar.is_empty() {
        return json!({ "ok": false, "error": "还没拿到 cookie：请先在窗口里完成登录" });
    }
    let saved = crate::uis::load_session(get_store());
    let verdict = saved
        .as_ref()
        .map(verify_canvas_session)
        .unwrap_or_else(|| "(没存下会话)".to_string());
    let has_service = jar.names().iter().any(|name| {
        matches!(
            name.as_str(),
            "CASTGC" | "_normandy_session" | "_legacy_normandy_session" | "log_session_id" | "SESS_ID"
        )
    });
    json!({
        "ok": true,
        "cookies": jar.count(),
        "has_castgc": jar.get("CASTGC").is_some(),
        "has_service_session": has_service,
        "verdict": verdict,
        "names": jar.names(),
    })
}

/// 教务诊断。
///
/// 先用 ureq + 抓来的 cookie 试（快、无界面）；**失败再退回网页里同源 fetch**：
/// 教务那跳要在浏览器里现换服务票据，拿 cookie 在进程里复现不了（实测被弹回
/// `id.fudan.edu.cn/ac/#/index?lck=context_CAS_…&entityId=fdjwgl.fudan.edu.cn`）。
#[tauri::command]
async fn jwgl_probe(app: tauri::AppHandle, semester: Option<String>) -> serde_json::Value {
    let handle = app.clone();
    let sem = semester.unwrap_or_default();
    match tauri::async_runtime::spawn_blocking(move || probe_timetable(&handle, &sem)).await {
        Ok(value) => value,
        Err(exc) => json!({ "ok": false, "error": format!("诊断失败：{exc}") }),
    }
}

/// 一次 print-data 抓取（进程内直连或网页里现抓，两条路都归到这个结构）
struct FetchedPrint {
    body: String,
    url: String,
    semester: String,
    sniffed: bool,
    /// 教务课表页上写的「学年起始日期」（浏览器里才读得到）
    term_start: String,
}

/// 读页面正文（连 iframe），交给 Rust 侧认日期——不在 JS 里写正则，省得两边不一致
fn read_page_text(window: &tauri::WebviewWindow) -> String {
    let js = r#"(function(){
  var out = (document.body && document.body.innerText) || '';
  try {
    var frames = document.querySelectorAll('iframe, frame');
    for (var i = 0; i < frames.length; i++) {
      try {
        var d = frames[i].contentDocument;
        if (d && d.body && d.body.innerText) { out += '\n' + d.body.innerText; }
      } catch (e) {}
    }
  } catch (e) {}
  return out;
})()"#;
    let raw = eval_text(window, js);
    if raw.starts_with("(eval ") {
        return String::new();
    }
    serde_json::from_str::<String>(&raw).unwrap_or(raw)
}

/// 读页面上的学期/学年起始日期（读不到返回空串，不影响主流程）
///
/// 页面是 SPA：print-data 一返回我们就来了，正文可能还没渲染完，所以轮询几轮；
/// 都不行就把正文前 300 字打进日志，方便判断教务改版后长什么样。
fn read_term_start(window: &tauri::WebviewWindow) -> String {
    let mut last = String::new();
    for attempt in 0..6 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(1500));
        }
        last = read_page_text(window);
        if let Some(found) = crate::timetable_remote::sniff_term_start(&last) {
            uis_log(&format!("[jwgl] 页面上的学期起始日期：{found}"));
            return found;
        }
    }
    let preview: String = last.chars().take(300).collect();
    uis_log(&format!("[jwgl] 没读到学期起始日期，页面正文：{preview}"));
    String::new()
}

/// 取 print-data 原文：先进程内直连，不行再让登录窗口同源抓。
fn fetch_print_body(app: &tauri::AppHandle, semester: &str) -> Result<FetchedPrint, String> {
    let store = get_store();
    let sem = if semester.trim().is_empty() {
        store.setting("jwgl_semester").unwrap_or_default()
    } else {
        semester.to_string()
    };
    uis_log(&format!("[jwgl] 传入学期：{sem:?}"));
    let direct = crate::timetable_remote::fetch_print_data(store, sem.trim());
    match direct {
        Ok(data) => Ok(FetchedPrint {
            body: data.body,
            url: data.url,
            semester: data.semester,
            sniffed: data.sniffed,
            term_start: String::new(),
        }),
        Err(direct_exc) => {
            let Some(win) = app.get_webview_window("uis-login") else {
                return Err(direct_exc);
            };
            uis_log("[jwgl] 进程内直连走不通，改在登录窗口里同源抓 print-data");
            fetch_print_in_window(&win, sem.trim()).map_err(|exc| {
                format!("{exc}（进程内直连也失败：{direct_exc}）")
            })
        }
    }
}

/// 等页面的 print-data 被拦到（最多约 20 秒），拿到就把学期号从 URL 里认出来。
fn take_captured_print(window: &tauri::WebviewWindow) -> Option<FetchedPrint> {
    for _ in 0..10 {
        std::thread::sleep(std::time::Duration::from_millis(2000));
        let Some(value) = eval_json(window, PRINT_CAPTURED_JS) else {
            continue;
        };
        let Some(body) = value.get("body").and_then(|one| one.as_str()) else {
            continue;
        };
        if body.trim().len() <= 4 {
            continue;
        }
        let url = value
            .get("url")
            .and_then(|one| one.as_str())
            .unwrap_or("")
            .to_string();
        let semester = crate::timetable_remote::sniff_semester(&url).unwrap_or_default();
        uis_log(&format!(
            "[jwgl] 拦到页面自己发的 print-data：{} 字节{}",
            body.len(),
            if semester.is_empty() {
                String::new()
            } else {
                format!("，学期 {semester}")
            }
        ));
        return Some(FetchedPrint {
            body: body.to_string(),
            url,
            semester,
            sniffed: true,
            term_start: read_term_start(window),
        });
    }
    None
}

fn probe_timetable(app: &tauri::AppHandle, semester: &str) -> serde_json::Value {
    let store = get_store();
    match fetch_print_body(app, semester) {
        Ok(data) => {
            let _ = store.set_setting("jwgl_semester", data.semester.trim());
            // 页面上读到「学年起始日期」就换算成第一周周一存起来，界面顺手回填
            let first_monday = crate::timetable_remote::first_monday_from(&data.term_start)
                .unwrap_or_default();
            if !first_monday.is_empty() {
                let _ = store.set_setting("term_start", &first_monday);
            }
            crate::timetable_remote::probe_body_with(
                &data.body,
                &data.url,
                &data.semester,
                data.sniffed,
                &data.term_start,
                &first_monday,
            )
        }
        Err(exc) => json!({ "ok": false, "error": exc }),
    }
}

/// 在课表页里找学期号：URL / 学期下拉 / 链接 / `data-value` 里的纯数字。
/// 返回 `{url, items:[{value,label,selected}]}`（value 是纯数字串）。
const SEMESTER_SCAN_JS: &str = r#"(function(){
  function num(v){ return /^\d{2,4}$/.test(v || '') ? v : ''; }
  var items = [], seen = {};
  function push(v, label, selected) {
    if (!v || seen[v]) { return; }
    seen[v] = 1;
    items.push({value: v, label: (label || '').slice(0, 40), selected: !!selected});
  }
  var m = (location.href || '').match(/semester\/(\d+)/);
  if (m) { push(m[1], 'url', true); }
  var nodes = document.querySelectorAll('option, [data-value], a, li');
  for (var i = 0; i < nodes.length; i++) {
    var node = nodes[i];
    var v = num(node.value || node.getAttribute('data-value') || '');
    var t = (node.textContent || '').trim();
    if (!v) {
      var mm = t.match(/^(\d{2,4})\s*[|｜]/);
      if (mm) { v = mm[1]; }
    }
    if (v && /20\d{2}/.test(t)) {
      var cls = String(node.className || '');
      var on = node.selected || node.getAttribute('aria-selected') === 'true' ||
               /current|selected|active/i.test(cls);
      push(v, t, on);
    }
  }
  return {url: location.href, items: items.slice(0, 15)};
})()"#;

/// 在已经登录的窗口里：先落到课表页，再同源 `fetch` print-data（浏览器自己带票）。
///
/// 学期号按页面给的候选逐个试：实测第一次拿到的 527 返回的是空对象 `{}`，
/// 说明那学期没有排课数据；换成下拉里的其它学期才有内容，所以这里不赌单个学期号。
fn fetch_print_in_window(
    window: &tauri::WebviewWindow,
    semester: &str,
) -> Result<FetchedPrint, String> {
    let url: tauri::Url = crate::timetable_remote::COURSE_TABLE
        .parse()
        .map_err(|exc| format!("课表地址不对：{exc}"))?;
    window
        .navigate(url)
        .map_err(|exc| format!("打不开课表页：{exc}"))?;
    std::thread::sleep(std::time::Duration::from_millis(4500));
    let landed = window
        .url()
        .map(|one| one.to_string())
        .unwrap_or_default();
    uis_log(&format!("[jwgl] 课表页落地：{}", shorten_url(&landed)));
    if landed.contains("id.fudan.edu.cn") {
        return Err(format!(
            "教务还是跳到了统一身份认证，先在「打开登录页」里登录一次：{landed}"
        ));
    }
    // ① 首选：页面自己请求 print-data 时被拦截脚本抄下来的那一份
    if let Some(captured) = take_captured_print(window) {
        return Ok(captured);
    }
    // ② 次选：点一下页面上的「课表」入口，让它自己去请求
    let clicked = eval_text(window, CLICK_TIMETABLE_JS);
    if !clicked.trim_matches('"').is_empty() && !clicked.starts_with("(eval ") {
        uis_log(&format!("[jwgl] 点了课表入口：{clicked}"));
        std::thread::sleep(std::time::Duration::from_millis(3000));
        if let Some(captured) = take_captured_print(window) {
            return Ok(captured);
        }
    }
    // 候选学期：用户填的 > 落地地址里的 > 页面下拉/链接里的（选中的排前面）
    let mut candidates: Vec<String> = Vec::new();
    if crate::timetable_remote::is_numeric_semester(semester) {
        candidates.push(semester.trim().to_string());
    }
    let mut scan_text = String::new();
    for _ in 0..6 {
        let Some(value) = eval_json(window, SEMESTER_SCAN_JS) else {
            std::thread::sleep(std::time::Duration::from_millis(2000));
            continue;
        };
        scan_text = value.to_string();
        let items = value
            .get("items")
            .and_then(|one| one.as_array())
            .cloned()
            .unwrap_or_default();
        if items.is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(2000));
            continue;
        }
        let mut selected: Vec<String> = Vec::new();
        let mut rest: Vec<String> = Vec::new();
        for item in items {
            let Some(found) = item.get("value").and_then(|one| one.as_str()) else {
                continue;
            };
            let found = found.trim().to_string();
            if found.is_empty() {
                continue;
            }
            if item.get("selected").and_then(|one| one.as_bool()).unwrap_or(false) {
                selected.push(found);
            } else {
                rest.push(found);
            }
        }
        candidates.extend(selected);
        candidates.extend(rest);
        break;
    }
    candidates.dedup();
    if candidates.is_empty() {
        let preview: String = scan_text.chars().take(240).collect();
        return Err(format!(
            "认不出学期号（课表页里没找到学期下拉/链接）。页面探测：{preview}"
        ));
    }
    for sem in &candidates {
        let target = crate::timetable_remote::PRINT_DATA.replace("{sem}", sem);
        let script = format!(
            "fetch('{target}', {{credentials: 'include'}}).then(function(r){{ return r.text(); }})"
        );
        let raw = eval_text(window, &script);
        if raw.starts_with("(eval ") {
            return Err(format!("网页里抓课表失败：{raw}"));
        }
        // eval 回来的是 JSON 字符串（带引号/转义），解回原文
        let body = serde_json::from_str::<String>(&raw).unwrap_or(raw);
        uis_log(&format!(
            "[jwgl] 学期 {sem} 的 print-data：{} 字节",
            body.len()
        ));
        // `{}`（2 字节）就是「这学期没数据」，换下一个学期
        if body.trim().len() <= 4 {
            continue;
        }
        return Ok(FetchedPrint {
            body,
            url: target,
            semester: sem.clone(),
            sniffed: true,
            term_start: read_term_start(window),
        });
    }
    Err(format!(
        "试了 {} 个学期（{}）都是空数据：可能这学期还没排课，或者课表页需要先在网页里选一次学期",
        candidates.len(),
        candidates.join("、")
    ))
}

#[tauri::command]
fn canvas_probe(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    crate::canvas::probe(get_store())
}

#[tauri::command]
fn canvas_download(course_id: String, limit: Option<usize>, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    crate::canvas::download_course(get_store(), &course_id, limit.unwrap_or(40))
}

#[tauri::command]
fn canvas_migrate(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    crate::canvas::migrate_downloads(get_store())
}

#[tauri::command]
fn ehall_refresh_online(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    crate::ehall_remote::refresh_all(get_store())
}

#[tauri::command]
fn papers_auto_summary(limit: Option<usize>, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    crate::summary::auto_summarize(get_store(), limit.unwrap_or(5))
}

/// 从教务导入课表（走和「教务诊断」同一条抓取路径：进程内直连失败就改在登录窗口里同源抓）。
///
/// `term_start` 允许留空：留空时用教务页面上的「学年起始日期」换算第一周周一
/// （与 Python `_on_fdu_data` 一致），设置里也没有时才报错。
#[tauri::command]
async fn jwgl_import(
    app: tauri::AppHandle,
    term_start: String,
    semester: Option<String>,
) -> serde_json::Value {
    let handle = app.clone();
    let sem = semester.unwrap_or_default();
    match tauri::async_runtime::spawn_blocking(move || import_timetable(&handle, &term_start, &sem))
        .await
    {
        Ok(value) => value,
        Err(exc) => json!({ "ok": false, "error": format!("导入失败：{exc}") }),
    }
}

fn import_timetable(app: &tauri::AppHandle, term_start: &str, semester: &str) -> serde_json::Value {
    let store = get_store();
    let data = match fetch_print_body(app, semester) {
        Ok(data) => data,
        Err(exc) => return json!({ "ok": false, "error": exc }),
    };
    let _ = store.set_setting("jwgl_semester", data.semester.trim());
    // 第一周周一：这条顺序与 Python 版一致，但「上次自动填进去的值」不算用户输入，
    // 否则一次自动填错（比如页面还没渲染出来）就会一直带错下去。
    let auto_filled = store.setting("term_start_auto").unwrap_or_default() == "1";
    let given = if auto_filled { "" } else { term_start };
    let picked = crate::timetable_remote::resolve_term_start(
        given,
        &data.term_start,
        &store.setting("term_start").unwrap_or_default(),
    );
    let _ = store.set_setting("term_start", &picked);
    // 这次是自动认出来的（页面给了日期）就记一笔，下次允许被页面覆盖
    if given.trim().is_empty()
        && !crate::timetable_remote::sniff_term_start(&data.term_start).unwrap_or_default().is_empty()
    {
        let _ = store.set_setting("term_start_auto", "1");
    }
    let slots = match crate::timetable_remote::parse_print_data(&data.body) {
        Ok(slots) => slots,
        Err(exc) => return json!({ "ok": false, "error": exc, "bytes": data.body.len() }),
    };
    let total = slots.len();
    match crate::timetable_remote::import_slots(store, &slots, &picked) {
        Ok((added, skipped)) => json!({
            "ok": true, "added": added, "skipped": skipped, "slots": total,
            "semester": data.semester, "semester_sniffed": data.sniffed,
            "term_start": picked,
            "term_start_sniffed": term_start.trim().is_empty(),
        }),
        Err(exc) => json!({ "ok": false, "error": exc }),
    }
}

pub struct AppState;

// ------------------------------------------------------------------ 账户审核（管理员）
/// 密钥状态：有没有私钥、公钥指纹、默认有效期、设备上限
#[tauri::command]
fn admin_key_state(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    json!({
        "has_private_key": adminsign::has_private_key(store),
        "fingerprint": adminsign::fingerprint(store),
        "public_pem": adminsign::public_pem(store),
        "valid_days": registration::valid_days(store),
        "device_limit": registration::device_limit(store),
    })
}

/// 生成一对新的管理员密钥（旧库里已有就不建议动）
#[tauri::command]
fn admin_generate_key(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    match adminsign::generate_keypair(get_store()) {
        Ok(fingerprint) => json!({ "ok": true, "fingerprint": fingerprint }),
        Err(err) => json!({ "ok": false, "error": err }),
    }
}

/// 收申请（邮件 + 网络直连），返回 `{requests, errors}`
#[tauri::command]
fn registration_requests(limit: Option<usize>, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let (requests, errors) = registration::requests(get_store(), limit.unwrap_or(60));
    json!({ "requests": requests, "errors": errors })
}

/// 批准一条申请（days 传空就用 registry_valid_days）
#[tauri::command]
fn registration_approve(
    request: Value,
    days: Option<i64>,
    state: State<'_, AppState>,
) -> serde_json::Value {
    let _ = &state;
    match registration::approve(get_store(), &request, days) {
        Ok(approval) => approval,
        Err(err) => json!({ "ok": false, "error": err }),
    }
}

/// 从 .eml 文件读一条申请
#[tauri::command]
fn registration_import_eml(path: String, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    match registration::request_from_eml(get_store(), &path) {
        Ok(request) => json!({ "ok": true, "request": request }),
        Err(err) => json!({ "ok": false, "error": err }),
    }
}

#[tauri::command]
fn registration_registry(state: State<'_, AppState>) -> Vec<Value> {
    let _ = &state;
    registration::registry(get_store())
}

/// 注册表动作：disable / enable / delete / renew
#[tauri::command]
fn registration_action(
    action: String,
    username: String,
    days: Option<i64>,
    state: State<'_, AppState>,
) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    let result = match action.as_str() {
        "disable" => registration::set_disabled(store, &username, true),
        "enable" => registration::set_disabled(store, &username, false),
        "delete" => registration::remove(store, &username),
        "renew" => registration::renew(store, &username, days),
        other => Err(format!("不认识的动作：{other}")),
    };
    match result {
        Ok(value) => value,
        Err(err) => json!({ "ok": false, "error": err }),
    }
}

// ------------------------------------------------------------------ 课表：每天自动同步
/// 课表 .ics 的位置：设置里能改，默认 data/timetable.ics
fn timetable_path(store: &Store) -> String {
    match store.setting("timetable_ics_path") {
        Some(text) if !text.trim().is_empty() => text.trim().to_string(),
        _ => paths::data_dir().join("timetable.ics").display().to_string(),
    }
}

/// 距离上次同步够久了没有（默认 20 小时，避免同一天反复同步）
fn timetable_due(store: &Store) -> bool {
    let stale = store
        .setting("timetable_synced_at")
        .and_then(|text| {
            chrono::NaiveDateTime::parse_from_str(text.trim(), "%Y-%m-%d %H:%M:%S").ok()
        })
        .map(|last| (Local::now().naive_local() - last).num_hours() >= 20)
        .unwrap_or(true);
    stale
}

fn timetable_auto(store: &Store) -> bool {
    // 旧版有两个开关：timetable_enabled（要不要课表这套功能）+ timetable_auto_sync（要不要每天自动同步）
    let enabled = store
        .setting("timetable_enabled")
        .map(|text| text.trim() != "0")
        .unwrap_or(true);
    if !enabled {
        return false;
    }
    store
        .setting("timetable_auto_sync")
        .map(|text| text.trim() != "0")
        .unwrap_or(true)
}

/// 跑一次同步（不改变「开关 / 文件」状态，只报结果）
fn timetable_try_sync(store: &Store) -> serde_json::Value {
    let path = timetable_path(store);
    if !std::path::Path::new(&path).exists() {
        return json!({ "ok": false, "error": format!("没有课表文件：{path}"), "path": path });
    }
    match ics::sync(store, &path) {
        Ok(report) => json!({
            "ok": true,
            "path": path,
            "added": report.added,
            "updated": report.updated,
            "removed": report.removed,
            "kept": report.kept,
            "summary": report.summary(),
            "synced_at": now_str(),
        }),
        Err(err) => json!({ "ok": false, "error": err, "path": path }),
    }
}

/// 后台线程：启动 20 秒后查一次，之后每 3 小时查一次；到 20 小时才真的同步
fn spawn_timetable_watcher() {
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(20));
        loop {
            {
                let store = get_store();
                if timetable_auto(store) && timetable_due(store) {
                    let result = timetable_try_sync(store);
                    if result["ok"] == true {
                        println!("[课表] {}", result["summary"]);
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(3 * 3600));
        }
    });
}

/// 手动同步一次（前端 / 验收脚本用）
#[tauri::command]
fn timetable_sync_now(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    timetable_try_sync(get_store())
}

/// 课表同步状态（路径、文件在不在、上次同步时间、自动开关）
#[tauri::command]
fn timetable_status(state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    let path = timetable_path(store);
    json!({
        "path": path,
        "exists": std::path::Path::new(&path).exists(),
        "auto": timetable_auto(store),
        "due": timetable_due(store),
        "synced_at": store.setting("timetable_synced_at").unwrap_or_default(),
        "status": store.setting("timetable_status").unwrap_or_default(),
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 内嵌网页（WebView2）的代理：这个环境变量要在**任何 webview 创建之前**设好，
    // 之后创建的所有 webview（主窗口、登录窗、看板娘、内嵌 eHall/eLearning…）都会继承。
    // 改设置后要重启才生效（后台请求是每次现读设置的，不用重启）。
    if let Some(args) = crate::net::webview_args_string(crate::get_store()) {
        std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", args);
    }
        // 崩溃可查：把 panic 写到 %TEMP%\danlume-panic.log（release 版没有控制台，
        // 用户只会看到「程序退出了」，有这个文件才能知道是哪一行）。
        std::panic::set_hook(Box::new(|info| {
            use std::io::Write;
            let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            let text = format!("[{stamp}] {info}\n");
            eprintln!("{text}");
            let path = std::env::temp_dir().join("danlume-panic.log");
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = file.write_all(text.as_bytes());
            }
        }));

    // 导入数据包后换库：必须发生在**任何 sqlite 连接建立之前**，
    // 所以放在 get_store() 前面（run() 是最早的入口）。
    match backup::apply_pending(&paths::data_dir()) {
        Ok(Some(db)) => println!("[导入] 已换成数据包里的库：{db}"),
        Ok(None) => {}
        Err(err) => eprintln!("[导入] 换库没成功：{err}"),
    }
    // 提前打开数据库：路径/权限有问题时启动就报错，而不是第一次点页面才炸
    let _ = get_store();
    // 标记「库已初始化」：旧版用 seeded 判断要不要塞示例数据，v2 不塞，但要替它把这个标记补上，
    // 免得两个版本交替打开时旧版又往库里灌一遍样例。
    {
        let store = get_store();
        if store.setting("seeded").is_none() {
            let _ = store.set_setting("seeded", "1");
        }
        // 设备指纹：第一次启动就生成一次，之后一直是同一个（账号服务器按它认设备）
        let _ = account::device_fingerprint(store);
    }
    spawn_timetable_watcher();
    spawn_notice_watcher();
    spawn_usage_watcher();
    let _ = NaiveTime::from_hms_opt(9, 0, 0);

    tauri::Builder::default()
        .manage(AppState)
        .setup(|app| {
            // 服务器公告：登录后按 20 秒长轮询拉 /announcements，新公告走 danlume://announcement 事件
            crate::notify::spawn(app.handle().clone());
            // 验收钩子：DANLUME_GRAB_TEST=<key> 时启动就抓一次，验证整条回传链路
            if let Ok(key) = std::env::var("DANLUME_GRAB_TEST") {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    match open_grab(&handle, &key) {
                        Ok(value) => eprintln!("[grab-test] 打开 {}：{}", key, value),
                        Err(err) => eprintln!("[grab-test] 失败：{}", err),
                    }
                    // 等脚本跑完，把窗口 URL（脚本会写 hash）打出来做诊断
                    for round in 1..=4 {
                        std::thread::sleep(std::time::Duration::from_secs(4));
                        let label = format!("ehall-grab-{key}");
                        if let Some(window) = handle.get_webview_window(&label) {
                            let url = window
                                .url()
                                .map(|value| value.to_string())
                                .unwrap_or_else(|err| format!("读不到：{err}"));
                            eprintln!("[grab-test] 第 {round} 次窗口地址：{url}");
                        } else {
                            eprintln!("[grab-test] 窗口不在了");
                            break;
                        }
                    }
                });
            }
            // 关窗行为（用户 2026-11-12 要求）：点右上角 × 只**缩回系统托盘**，不退出
            //
            // 想真退出：托盘右键菜单里「退出」，或者把设置 `tray_on_close` 设成 0
            // （老版本里这条默认是「关」，现在默认「缩到托盘」）。
            {
                let handle = app.handle().clone();
                // 托盘图标（点一下 = 显示主窗口）
                let icon = app.default_window_icon().cloned();
                if let Some(icon) = icon {
                    let menu_handle = handle.clone();
                    let click_handle = handle.clone();
                    let quit_handle = handle.clone();
                    let menu = tauri::menu::MenuBuilder::new(app)
                        .text("show", "显示主界面")
                        .text("quit", "退出 DanLume")
                        .build();
                    let mut builder = tauri::tray::TrayIconBuilder::with_id("danlume-tray")
                        .tooltip("DanLume 旦光")
                        .icon(icon);
                    if let Ok(menu) = menu {
                        builder = builder.menu(&menu).show_menu_on_left_click(false);
                    }
                    let _ = builder
                        .on_menu_event(move |_tray, event| match event.id.as_ref() {
                            "quit" => quit_handle.exit(0),
                            _ => {
                                if let Some(win) = menu_handle.get_webview_window("main") {
                                    let _ = win.show();
                                    let _ = win.unminimize();
                                    let _ = win.set_focus();
                                }
                            }
                        })
                        .on_tray_icon_event(move |_tray, event| {
                            if let tauri::tray::TrayIconEvent::Click {
                                button: tauri::tray::MouseButton::Left,
                                button_state: tauri::tray::MouseButtonState::Up,
                                ..
                            } = event
                            {
                                if let Some(win) = click_handle.get_webview_window("main") {
                                    let _ = win.show();
                                    let _ = win.unminimize();
                                    let _ = win.set_focus();
                                }
                            }
                        })
                        .build(app);
                }
                if let Some(window) = app.get_webview_window("main") {
                    window.on_window_event(move |event| {
                        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                            // 默认缩到托盘；只有用户明确关掉这条设置才真退出
                            let keep = get_store()
                                .setting("tray_on_close")
                                .map(|text| text.trim() != "0")
                                .unwrap_or(true);
                            if keep {
                                api.prevent_close();
                                if let Some(win) = handle.get_webview_window("main") {
                                    let _ = win.hide();
                                }
                            }
                        }
                    });
                }
            }
            // 验收/调试钩子：DANLUME_OPEN_MASCOT=1 时启动 2 秒后自动弹出看板娘小窗
            // （小窗没有侧边栏，靠界面设置脚本找不到入口时就靠它）
            if std::env::var("DANLUME_OPEN_MASCOT").is_ok() {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    match mascot_open(handle) {
                        Ok(()) => eprintln!("[mascot] 看板娘窗口已打开"),
                        Err(err) => eprintln!("[mascot] 打开失败：{err}"),
                    }
                });
            }
            // 验收/调试钩子：DANLUME_OPEN_UIS=1 时启动 2 秒后自动弹出 UIS 登录窗
            if std::env::var("DANLUME_OPEN_UIS").is_ok() {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    match open_uis_window(&handle) {
                        Ok(()) => eprintln!("[uis] 登录窗口已打开"),
                        Err(err) => eprintln!("[uis] 打开失败：{err}"),
                    }
                });
            }
            // 作业截止的当天提醒（用户 2026-11-12）：早于设置里的小时不吵，
            // 到点以后每条每天只提一次；系统通知 + 事件（前端还能弹卡片）。
            {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    // 启动 25 秒后再看（别和开机时的一堆同步抢资源）
                    std::thread::sleep(std::time::Duration::from_secs(25));
                    loop {
                        let rows = std::panic::catch_unwind(|| {
                            elearn::due_reminders(get_store(), true)
                        })
                        .unwrap_or_default();
                        if !rows.is_empty() {
                            let count = rows.len();
                            let body = rows
                                .iter()
                                .map(|one| {
                                    one.get("text")
                                        .and_then(|value| value.as_str())
                                        .unwrap_or("作业今天截止")
                                        .to_string()
                                })
                                .collect::<Vec<String>>()
                                .join("\n");
                            notify_desktop(
                                &handle,
                                &format!("今天有 {count} 个作业要交"),
                                &body,
                            );
                            let _ = handle.emit("elearn:due", json!({ "items": rows }));
                            eprintln!("[elearn] 今天有 {count} 个作业截止，已提醒");
                        }
                        std::thread::sleep(std::time::Duration::from_secs(300));
                    }
                });
            }
            // 定时通知的后台闹钟：每 30 秒看一眼有没有到点的（旧版没有这个功能，是新加的）
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_secs(8));
                loop {
                    let sent =
                        std::panic::catch_unwind(|| student::run_due(get_store())).unwrap_or(0);
                    if sent > 0 {
                        eprintln!("[student] 定时通知已发送 {sent} 条");
                    }
                    std::thread::sleep(std::time::Duration::from_secs(30));
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            events_on,
            event_index,
            upcoming_events,
            week_number,
            add_event,
            update_event,
            delete_event,
            event_set_done,
            get_settings,
            set_setting,
            recent_notices,
            recent_assignments,
            weather,
            boot_info,
            save_secret,
            has_secret,
            notice_list,
            mark_notice_read,
            mark_all_notices_read,
            refresh_notices,
            memos,
            add_memo,
            update_memo,
            delete_memo,
            contacts,
            add_contact,
            delete_contact,
            list_dir,
            read_text_file,
            read_file_base64,
            open_path,
            reveal_path,
            default_docs_dir,
            mail_account_state,
            mail_list,
            mail_sync,
            bounce_sync,
            mail_body,
            mail_summarize,
            mail_add_events,
            mail_send,
            mail_test,
            mails,
            delete_mail,
            imap_test,
            settings_save,
            homework_items,
            homework_sync,
            important_list,
            important_save,
            important_delete,
            homework_open_file,
            homework_write_deadlines,
            papers_list,
            papers_search,
            papers_add,
            papers_download,
            papers_semantic_status,
            papers_semantic_index,
            papers_semantic_search,
            homework_upcoming,
            homework_mark_done,
            proxy_summary,
            proxy_probe,
            elearn_due_reminders,
            exams_import,
            refs_list,
            refs_import,
            refs_add_doi,
            refs_delete,
            refs_export,
            contacts_list,
            mascot_open,
            mascot_close,
            mascot_send,
            wechat_read,
            contacts_save,
            contacts_delete,
            contacts_ai_allowed,
            contacts_set_ai_allowed,
            student_notices,
            student_notice_save,
            student_notice_delete,
            student_notice_send,
            endnote_detect,
            endnote_open,
            paper_update,
            paper_citation,
            settings_all,
            ai_state,
            ai_profiles,
            ai_chat,
            ai_chat_tools,
            ai_expand_paths,
            ai_scenes,
            agent_sandbox_state,
            agent_sandbox_open,
            agent_sandbox_set_run,
            agent_sandbox_run,
            ai_tool_catalog,
            ai_tool_run,
            focus_start,
            focus_stop,
            focus_status,
            usage_status,
            usage_report_now,
            uis_status,
            uis_login,
            uis_logout,
            uis_open_login,
            uis_capture,
            uis_grab_profile,
            jwgl_probe,
            jwgl_import,
            canvas_probe,
            canvas_download,
            canvas_migrate,
            ehall_refresh_online,
            papers_auto_summary,
            notices_auto_run,
            notices_auto_status,
            timetable_sync_now,
            timetable_status,
            admin_key_state,
            admin_generate_key,
            registration_requests,
            registration_approve,
            registration_import_eml,
            registration_registry,
            registration_action,
            ai_sessions,
            ai_save_session,
            ai_session_rename,
            ai_session_delete,
            ai_chat_stream,
            ai_cancel_stream,
            ai_confirm,
            ai_confirm_reset,
            ai_profile_save,
            ai_profile_delete,
            classroom_view,
            classroom_scrape,
            classroom_query,
            homework_unclassified,
            homework_classify,
            ehall_cards,
            data_info,
            backup_db,
            data_export,
            data_import,
            lit_flow_generate,
            lit_flow_load,
            lit_flow_delete,
            web_open_url,
            app_relaunch,
            embed_open,
            embed_bounds,
            embed_close,
            embed_reload,
            embed_navigate,
            embed_status,
            ehall_open,
            ehall_ingest,
            ehall_ingest_pasted,
            latex_pdf,
            latex_formula_from_text,
            due_reminders,
            mark_notified,
            snooze_event,
            event_series_done,
            account_health,
            account_request_email_code,
            account_request_reset_code,
            account_reset_password,
            account_register,
            account_login,
            account_logout,
            account_me,
            account_devices,
            account_kick_device,
            account_delete_me,
            account_state,
            account_save_server,
            account_announcements,
            account_mark_announcements_read,
            account_check_announcements,
            memory_list,
            memory_add,
            memory_search,
            memory_update,
            memory_delete,
            memory_clear,
            prompt_templates,
            prompt_template_save,
            prompt_template_delete,
            prompt_generate,
            account_notify_status,
        ])
        .plugin(tauri_plugin_dialog::init())
        // 系统通知：作业到期当天早上的提醒（见 `elearn::due_reminders`）
        .plugin(tauri_plugin_notification::init())
        .run(tauri::generate_context!())
        .expect("启动 Tauri 失败");
}

// ------------------------------------------------------------------ 文献方法流程图
// 对应旧版 lit_flow.py / lit_flow_dialog.py：后端只产出结构化节点 + 边（并落盘），
// 画图交给前端 CSS/SVG。命令追加在这里，注册处只动了 invoke_handler 里 data_import 后面那几行。
mod lit_flow;
mod webui;

/// 生成一篇文献的方法流程图
///
/// * `paper_id`：文献库里的 id（>0 才会按 id 存取 data/lit_flows/flow-<id>.json）
/// * `text`：已解析的正文 / 摘要（给了就不查库；论文 PDF 正文由前端解析后递进来）
/// * `title`：标题（不给就用库里的）
/// * `force`：忽略上次存下来的结果，让模型重新拆一次
#[tauri::command]
fn lit_flow_generate(
    paper_id: Option<i64>,
    text: Option<String>,
    title: Option<String>,
    force: Option<bool>,
    state: State<'_, AppState>,
) -> serde_json::Value {
    let _ = &state;
    lit_flow::generate(
        get_store(),
        paper_id.unwrap_or(0),
        &text.unwrap_or_default(),
        &title.unwrap_or_default(),
        force.unwrap_or(false),
    )
}

/// 读回这篇文献上次生成的流程图（重启后还能看）
#[tauri::command]
fn lit_flow_load(paper_id: i64, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    lit_flow::load(paper_id)
}

/// 删掉这篇文献存下来的流程图
#[tauri::command]
fn lit_flow_delete(paper_id: i64, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    lit_flow::delete(paper_id)
}

/// 用系统默认浏览器打开一个 http(s) 地址（网页版面板「打不开时」的出口）
#[tauri::command]
fn web_open_url(url: String) -> Result<(), String> {
    webui::open_url(&url)
}

// ---------------------------------------------------------------- 提醒回归测试
#[cfg(test)]
mod reminder_tests {
    use super::*;
    use crate::store::{EventInput, Store};

    struct EnvGuard {
        previous: Option<std::ffi::OsString>,
        dir: std::path::PathBuf,
    }

    impl EnvGuard {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("danlume-{tag}-{}", std::process::id()));
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

    fn day(offset: i64) -> String {
        (chrono::Local::now().date_naive() + chrono::Duration::days(offset))
            .format("%Y-%m-%d")
            .to_string()
    }

    fn clock(offset_minutes: i64) -> String {
        (chrono::Local::now().naive_local() + chrono::Duration::minutes(offset_minutes))
            .format("%H:%M")
            .to_string()
    }

    /// 现在离午夜还有足够空间吗（这些用例要造「今天稍后开始」的日程）
    ///
    /// 00:00–01:30 和 22:30–24:00 这两个窗口里，「现在 +20 分钟」会滚到另一天，
    /// 用例本身的前提就不成立——直接跳过，别把时间问题当代码 bug。
    fn soon_enough_for_today() -> bool {
        let now = chrono::Local::now().naive_local().time();
        let early = chrono::NaiveTime::from_hms_opt(1, 30, 0).unwrap();
        let late = chrono::NaiveTime::from_hms_opt(22, 30, 0).unwrap();
        now > early && now < late
    }

    /// 用户反馈的那个问题：一开程序就把两个月前那批日程全提醒一遍
    #[test]
    fn old_events_never_remind() {
        let guard = EnvGuard::new("reminder-window");
        let store = guard.store();
        // 这个用例要造一条「今天马上开始」的日程：贴近午夜时 +20 分钟会滚到明天，
        // 于是它看起来像「今天已经结束」——那不是 bug，是时间不合适，跳过即可。
        if !soon_enough_for_today() {
            println!("跳过：当前时间贴近午夜，造不出「今天还没结束」的日程");
            return;
        }
        let old = store
            .add_event(&EventInput {
                title: Some("两个月前的课".to_string()),
                date: Some(day(-60)),
                start_time: Some("08:00".to_string()),
                end_time: Some("09:40".to_string()),
                remind_minutes: Some(10),
                ..Default::default()
            })
            .unwrap();
        // 昨天凌晨那一小段：结束时间早于「补提醒窗口」（12 小时），所以不该提醒
        let ended_long_ago = store
            .add_event(&EventInput {
                title: Some("早就上完的课".to_string()),
                date: Some(day(-1)),
                start_time: Some("00:00".to_string()),
                end_time: Some("00:10".to_string()),
                remind_minutes: Some(0),
                ..Default::default()
            })
            .unwrap();
        let upcoming = store
            .add_event(&EventInput {
                title: Some("马上要上的课".to_string()),
                date: Some(day(0)),
                start_time: Some(clock(20)),
                end_time: Some(clock(80)),
                remind_minutes: Some(30),
                ..Default::default()
            })
            .unwrap();

        let rows = collect_event_reminders(&store);
        let ids: Vec<i64> = rows
            .iter()
            .filter_map(|row| row.get("id").and_then(|one| one.as_i64()))
            .collect();
        println!("提醒出来的：{ids:?}（旧={old} 早就结束={ended_long_ago} 马上开始={upcoming}）");
        assert!(!ids.contains(&old), "两个月前的日程不该再提醒");
        assert!(!ids.contains(&ended_long_ago), "结束超过补提醒窗口的不该提醒");
        assert!(ids.contains(&upcoming), "马上要上的课应该提醒");

        // 清扫之后旧记录被标成已提醒，下次不会再被翻出来
        let changed = sweep_stale_reminders(&store);
        println!("清扫了 {changed} 条");
        assert!(changed >= 2, "过去的两条都该被清扫");
        let rows = collect_event_reminders(&store);
        let ids: Vec<i64> = rows
            .iter()
            .filter_map(|row| row.get("id").and_then(|one| one.as_i64()))
            .collect();
        assert!(!ids.contains(&old));
    }

    /// 刚结束的日程不再补提醒（用户反馈：15:10 结束的会在 16:10 又弹一次）；
    /// 「正在进行中」的仍然补一次（错过了但还在进行，提醒仍然有用）。
    #[test]
    fn ended_event_is_not_caught_up() {
        let guard = EnvGuard::new("reminder-ended");
        let store = guard.store();
        let start = clock(-180);
        let finish = clock(-120);
        // 跨午夜的样本会失真（00:30 算出「23:29」这种），直接跳过
        {
            let now = chrono::Local::now();
            let hour = chrono::Timelike::hour(&now);
            let minute = chrono::Timelike::minute(&now);
            if hour < 2 || hour >= 23 {
                println!("跳过：当前时间贴近午夜（{hour}:{minute:02}），样本会跨天失真");
                return;
            }
        }
        if start > finish {
            println!("跳过：当前时间贴近午夜，跨夜样本会失真");
            return;
        }
        let ended = store
            .add_event(&EventInput {
                title: Some("已经结束的会".to_string()),
                date: Some(day(0)),
                start_time: Some(start),
                end_time: Some(finish),
                remind_minutes: Some(10),
                ..Default::default()
            })
            .unwrap();
        let ongoing = store
            .add_event(&EventInput {
                title: Some("正在进行的会".to_string()),
                date: Some(day(0)),
                start_time: Some(clock(-60)),
                end_time: Some(clock(60)),
                remind_minutes: Some(10),
                ..Default::default()
            })
            .unwrap();

        let ids: Vec<i64> = collect_event_reminders(&store)
            .iter()
            .filter_map(|row| row.get("id").and_then(|one| one.as_i64()))
            .collect();
        println!("提醒出来的：{ids:?}（已结束={ended} 进行中={ongoing}）");
        assert!(!ids.contains(&ended), "已经结束的不该再补提醒");
        assert!(ids.contains(&ongoing), "正在进行的应该补一次");
    }

    /// 过期自动完成不能把重复日程整串标成已完成
    #[test]
    fn auto_done_skips_repeat_master() {
        let guard = EnvGuard::new("reminder-autodone");
        let store = guard.store();
        let repeat = store
            .add_event(&EventInput {
                title: Some("每周的课".to_string()),
                date: Some(day(-28)),
                start_time: Some("08:00".to_string()),
                end_time: Some("09:40".to_string()),
                repeat: Some("weekly".to_string()),
                ..Default::default()
            })
            .unwrap();
        let plain = store
            .add_event(&EventInput {
                title: Some("一次性的事".to_string()),
                date: Some(day(-3)),
                start_time: Some("08:00".to_string()),
                end_time: Some("09:40".to_string()),
                ..Default::default()
            })
            .unwrap();
        let _ = store.set_setting("auto_done_enabled", "1");
        let changed = crate::reminders::auto_done(&store);
        println!("自动完成改了 {changed} 条");
        assert_eq!(store.get_event(plain).unwrap().unwrap().done, true);
        let master = store.get_event(repeat).unwrap().unwrap();
        assert!(!master.done, "重复日程的母行不能被自动完成");
        // 过去的那几次记进 done_dates，今天以后的不受影响
        let rows = store.events_between(&day(-35), &day(30)).unwrap();
        let future: Vec<&crate::store::EventRow> =
            rows.iter().filter(|one| one.date.as_str() > day(0).as_str()).collect();
        assert!(!future.is_empty(), "以后还应该有几次");
        assert!(
            future.iter().all(|one| !one.done),
            "以后的每一次都不能是已完成：{:?}",
            future.iter().map(|one| (one.date.as_str(), one.done)).collect::<Vec<_>>()
        );
        assert!(
            rows.iter().any(|one| one.date.as_str() < day(0).as_str() && one.done),
            "过去的那几次应该被自动完成"
        );
    }

    /// 重复日程要「每一次都能提醒」，而不是只提醒母事件那一天
    #[test]
    fn repeat_occurrence_can_remind() {
        let guard = EnvGuard::new("reminder-repeat");
        let store = guard.store();
        if !soon_enough_for_today() {
            println!("跳过：当前时间贴近午夜，造不出「今天还没结束」的日程");
            return;
        }
        // 从上周的同一天开始，每周一次 → 今天正好是其中一次
        let id = store
            .add_event(&EventInput {
                title: Some("每周的课".to_string()),
                date: Some(day(-7)),
                start_time: Some(clock(20)),
                end_time: Some(clock(80)),
                remind_minutes: Some(30),
                repeat: Some("weekly".to_string()),
                ..Default::default()
            })
            .unwrap();
        let rows = collect_event_reminders(&store);
        let hit = rows
            .iter()
            .find(|row| row.get("id").and_then(|one| one.as_i64()) == Some(id))
            .cloned();
        println!("今天的重复日程提醒：{hit:?}");
        let hit = hit.expect("今天这一次应该要提醒");
        assert_eq!(hit["date"], serde_json::json!(day(0)));

        // 标记「今天提醒过了」：母行不能被标掉，也不能把下一周吃掉
        store.mark_notified(id, Some(&day(0))).unwrap();
        let master = store.get_event(id).unwrap().unwrap();
        assert!(!master.notified, "母行的 notified 不该被改");
        assert_eq!(master.notified_dates, day(0));
        let again = collect_event_reminders(&store);
        assert!(
            !again
                .iter()
                .any(|row| row.get("id").and_then(|one| one.as_i64()) == Some(id)),
            "今天已经提醒过，不该再弹"
        );
    }

    /// 今天已经过了结束时间的日程也要自动完成（旧版 `auto_complete_past` 的语义）。
    /// 以前只判 `date < today`，于是「08:00–09:40 的课 09:45 还挂着未完成」。
    #[test]
    fn auto_done_catches_today_finished_events() {
        let guard = EnvGuard::new("reminder-today-done");
        let store = guard.store();
        let now = chrono::Local::now().format("%H:%M").to_string();
        if now <= "00:02".to_string() || now >= "23:57".to_string() {
            println!("跳过：当前时间 {now} 贴近午夜");
            return;
        }
        let finished = store
            .add_event(&EventInput {
                title: Some("刚上完的课".to_string()),
                date: Some(day(0)),
                start_time: Some("00:00".to_string()),
                end_time: Some("00:01".to_string()),
                ..Default::default()
            })
            .unwrap();
        let upcoming = store
            .add_event(&EventInput {
                title: Some("还没结束的课".to_string()),
                date: Some(day(0)),
                start_time: Some("23:56".to_string()),
                end_time: Some("23:59".to_string()),
                ..Default::default()
            })
            .unwrap();
        let _ = store.set_setting("auto_done_enabled", "1");
        crate::reminders::auto_done(&store);
        let finished_done = store.get_event(finished).unwrap().map(|one| one.done).unwrap_or(false);
        let upcoming_done = store.get_event(upcoming).unwrap().map(|one| one.done).unwrap_or(false);
        assert!(finished_done, "今天已经结束的日程应当自动完成");
        assert!(!upcoming_done, "今天还没结束的不该被勾");
    }
}

/// 邮件顶部 AI 摘要用到的三段纯本地逻辑（不发网络请求，改坏了会直接喂垃圾给模型）
#[cfg(test)]
mod mail_summary_tests {
    use super::*;

    #[test]
    fn plain_text_drops_style_and_scripts() {
        let html = "<html><head><style>p{color:red}</style></head>\
                    <body><script>alert('x')</script>\
                    <p>本周六 23:59 截止</p><p>&amp; 记得回复</p></body></html>";
        let text = mail_plain_text(html);
        assert!(!text.contains("color:red"), "CSS 不该进正文：{text}");
        assert!(!text.contains("alert"), "脚本不该进正文：{text}");
        assert!(text.contains("本周六 23:59 截止"), "{text}");
        assert!(text.contains("& 记得回复"), "实体要还原：{text}");
    }

    /// 正文**原样**给模型：不删引用、不删签名、不截断（用户 2026-11-11 要求）
    #[test]
    fn plain_text_keeps_everything() {
        let raw = "同学你好：\n\n请于周五前交作业。\n\n> 上封邮件写的\n--\n张老师";
        let text = mail_plain_text(raw);
        assert!(text.contains("请于周五前交作业"), "{text}");
        assert!(text.contains("上封邮件写的"), "引用行也应当保留：{text}");
        assert!(text.contains("张老师"), "{text}");
        // 长正文不截断
        let long = "字".repeat(5000);
        assert_eq!(mail_plain_text(&long).chars().count(), 5000);
    }

    /// HTML 的段落不能被粘成一行
    #[test]
    fn plain_text_keeps_paragraph_breaks() {
        let html = "<p>第一段</p><p>第二段</p><div>第三段</div>";
        let text = mail_plain_text(html);
        assert!(text.contains("第一段\n第二段"), "段落要换行：{text:?}");
        assert!(text.contains("第二段\n第三段"), "{text:?}");
    }

    /// 附件只给名字和格式，不给内容
    #[test]
    fn attachment_line_has_only_names_and_kinds() {
        let items = vec![
            json!({ "name": "通知.pdf", "size": 118784 }),
            json!({ "name": "名单.xlsx", "size": 2048000 }),
        ];
        let line = mail_attachment_line(&items);
        assert!(line.contains("通知.pdf（PDF"), "{line}");
        assert!(line.contains("116 KB"), "大小要换算：{line}");
        assert!(line.contains("名单.xlsx（XLSX，2.0 MB）"), "{line}");
        assert!(line.contains("内容没有上传"), "{line}");
        // 没有附件就一个字都不加
        assert_eq!(mail_attachment_line(&[]), "");
    }

    #[test]
    fn clean_summary_strips_markdown() {
        let raw = "```\n## 摘要\n· 教务处通知选课\n**要做：** 周五前选课\n---\n```";
        let text = mail_clean_summary(raw);
        assert_eq!(text, "摘要\n· 教务处通知选课\n要做： 周五前选课", "{text}");
    }

    /// 模型给的日程 JSON：能从代码块里抠出来，字段也对得上
    #[test]
    fn parses_events_from_model_output() {
        let raw = "```json\n[{\"title\":\"提交复议材料\",\"date\":\"2026-11-18\",\
                   \"start_time\":\"12:00\",\"end_time\":\"\",\"note\":\"系统里提交\"}]\n```";
        let items = parse_mail_events(raw, "2026-11-11");
        assert_eq!(items.len(), 1, "{items:?}");
        assert_eq!(items[0]["title"], json!("提交复议材料"));
        assert_eq!(items[0]["date"], json!("2026-11-18"));
        assert_eq!(items[0]["start_time"], json!("12:00"));
        assert_eq!(items[0]["end_time"], json!(""), "没给结束时间就留空");
        assert_eq!(items[0]["note"], json!("系统里提交"));
    }

    /// 坏数据要丢掉：日期不是 YYYY-MM-DD、标题为空、时间不合法
    #[test]
    fn parse_events_drops_bad_items() {
        let raw = r#"[
            {"title":"好日期","date":"2026-11-18"},
            {"title":"坏日期","date":"下周三"},
            {"title":"","date":"2026-11-18"},
            {"title":"坏时间","date":"2026-11-18","start_time":"晚上"}
        ]"#;
        let items = parse_mail_events(raw, "2026-11-11");
        assert_eq!(items.len(), 2, "只该留「好日期」和「坏时间」两条：{items:?}");
        assert_eq!(items[0]["title"], json!("好日期"));
        assert_eq!(items[1]["title"], json!("坏时间"));
        assert_eq!(items[1]["start_time"], json!(""), "不合法的时间要丢掉，不能塞进日历");
    }

    #[test]
    fn parse_events_survives_garbage() {
        assert!(parse_mail_events("我不想输出 JSON", "2026-11-11").is_empty());
        assert!(parse_mail_events("[]", "2026-11-11").is_empty());
        assert!(parse_mail_events("```\n[]\n```", "2026-11-11").is_empty());
    }

    /// 常见的中文/斜杠日期都要收（用户 2026-11-11：「怎么少一个」就是被这条扔掉的）
    #[test]
    fn parses_chinese_and_slash_dates() {
        assert_eq!(normalize_mail_date("2026-11-12", "2026-11-11").as_deref(), Some("2026-11-12"));
        assert_eq!(normalize_mail_date("2026/11/12", "2026-11-11").as_deref(), Some("2026-11-12"));
        assert_eq!(
            normalize_mail_date("2026年11月12日", "2026-11-11").as_deref(),
            Some("2026-11-12")
        );
        assert_eq!(
            normalize_mail_date("11月12日", "2026-11-11").as_deref(),
            Some("2026-11-12"),
            "没写年份就补今天的年份"
        );
        assert_eq!(normalize_mail_date("11-12", "2026-11-11").as_deref(), Some("2026-11-12"));
        assert_eq!(normalize_mail_date("随便写的", "2026-11-11"), None);
    }

    /// 时间可能是范围挤在一个字段里，也可能是「10点」
    #[test]
    fn normalizes_time_ranges() {
        assert_eq!(
            normalize_mail_time("10:00-11:30"),
            ("10:00".to_string(), "11:30".to_string())
        );
        assert_eq!(normalize_mail_time("9:30"), ("09:30".to_string(), String::new()));
        assert_eq!(normalize_mail_time("14点"), ("14:00".to_string(), String::new()));
        assert_eq!(normalize_mail_time(""), (String::new(), String::new()));
    }

    /// 表格里那种一条不缺：模型给 8 条就出 8 条（以前写死最多 6 条）
    #[test]
    fn keeps_up_to_ten_events() {
        let rows: Vec<String> = (1..=8)
            .map(|index| {
                format!(
                    "{{\"title\":\"第{index}场宣讲会\",\"date\":\"2026-11-1{index}\",\"start_time\":\"10:00-11:30\"}}"
                )
            })
            .collect();
        let raw = format!("[{}]", rows.join(","));
        let items = parse_mail_events(&raw, "2026-11-11");
        assert_eq!(items.len(), 8, "{items:?}");
        assert_eq!(items[7]["start_time"], json!("10:00"));
        assert_eq!(items[7]["end_time"], json!("11:30"));
    }

    /// 诊断 + 真联网：把一封「网页表格型」邮件的正文打出来，确认**全文**都给了模型，
    /// 再看抽出几条（用户 2026-11-11 两次报「怎么少一个」）
    ///
    /// `cargo test real_api_events_table_mail -- --ignored --test-threads=1 --nocapture`
    #[test]
    #[ignore]
    fn real_api_events_table_mail() {
        let store = crate::testdata::real_store("mailtable");
        let mut picked = None;
        for row in crate::mail::cached_inbox(&store, 40) {
            if let Some((html, text, subject, from, date)) = crate::mail::cached_body(&store, &row.uid)
            {
                let (html_body, text_body) = pick_body(html, text);
                let raw = if html_body.trim().is_empty() {
                    text_body
                } else {
                    html_body
                };
                let body = mail_plain_text(&raw);
                // 这封「宣讲会表格」邮件的标题里没有关键词，按**正文**找
                if !body.contains("宣讲会") && !body.contains("雄安") {
                    continue;
                }
                picked = Some((subject, from, date, body, row.uid));
                break;
            }
        }
        let Some((subject, from, date, body, uid)) = picked else {
            println!("缓存里没有「宣讲会」这封邮件，跳过");
            return;
        };
        println!("邮件：{subject}");
        println!("正文全长：{} 字，{} 行", body.chars().count(), body.lines().count());
        println!("含「雄安」：{}", body.contains("雄安"));
        for line in body.lines().filter(|line| line.contains("雄安") || line.contains("11月12")) {
            println!("  · {line}");
        }
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let messages = mail_events_messages(&subject, &from, &date, &body, &today);
        let reply = ai::chat(&store, &messages, Some(0.1)).expect("接口没回话");
        let items = parse_mail_events(&reply, &today);
        println!("抽出 {} 条：", items.len());
        for one in &items {
            println!(
                "  {} {} {}",
                one["date"].as_str().unwrap_or(""),
                one["start_time"].as_str().unwrap_or(""),
                one["title"].as_str().unwrap_or("")
            );
        }
        // 顺手把结果写回缓存，界面再打开就是新的（省得用户还得点「重新生成」）
        mail::save_events(&store, &uid, &items);
        assert!(!items.is_empty(), "一条都没抽出来：{reply}");
    }

    /// 真联网：抽一次「可能要加进日程的事」，看提示词灵不灵（花一次 token）
    ///
    /// `cargo test real_api_events_on_real_mail -- --ignored --test-threads=1 --nocapture`
    #[test]
    #[ignore]
    fn real_api_events_on_real_mail() {
        let store = crate::testdata::real_store("mailevents");
        let mut picked = None;
        for row in crate::mail::cached_inbox(&store, 10) {
            if let Some((html, text, subject, from, date)) = crate::mail::cached_body(&store, &row.uid)
            {
                let (html_body, text_body) = pick_body(html, text);
                let raw = if html_body.trim().is_empty() {
                    text_body
                } else {
                    html_body
                };
                let body = mail_plain_text(&raw);
                if body.contains("截止") && body.chars().count() > 100 {
                    picked = Some((subject, from, date, body));
                    break;
                }
            }
        }
        let Some((subject, from, date, body)) = picked else {
            println!("缓存里没有带「截止」的邮件，这个用例跳过");
            return;
        };
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        println!("抽日程的邮件：{subject}");
        let messages = mail_events_messages(&subject, &from, &date, &body, &today);
        let reply = ai::chat(&store, &messages, Some(0.1)).expect("接口没回话");
        println!("原始回复：{reply}");
        let items = parse_mail_events(&reply, &today);
        for one in &items {
            println!(
                "  {} {} {}（{}）",
                one["date"].as_str().unwrap_or(""),
                one["start_time"].as_str().unwrap_or(""),
                one["title"].as_str().unwrap_or(""),
                one["note"].as_str().unwrap_or("")
            );
        }
        assert!(!items.is_empty(), "这封邮件有截止日期，应该抽得出来：{reply}");
        // 日期必须落在合理区间（今天前后一年内），防止模型乱编年份
        for one in &items {
            let text = one["date"].as_str().unwrap_or("");
            let parsed = chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").expect("日期要合法");
            let today = chrono::Local::now().date_naive();
            let gap = (parsed - today).num_days();
            assert!(
                (-400..=400).contains(&gap),
                "日期离谱：{text}（距今天 {gap} 天）"
            );
        }
    }

    /// 真联网 + 真库副本：拿一封真有正文的邮件，真问一次用户配的接口
    ///
    /// 会花一次 token（几百字进、一百字出），默认 `#[ignore]`，要验的时候手动跑：
    /// `cargo test real_api_summary_on_real_mail -- --ignored --test-threads=1 --nocapture`
    #[test]
    #[ignore]
    fn real_api_summary_on_real_mail() {
        let store = crate::testdata::real_store("mailsum");
        let meta = ai::config_state(&store);
        assert_eq!(
            meta.get("ok").and_then(|one| one.as_bool()),
            Some(true),
            "这台机器上没有可用的 AI 配置：{meta}"
        );
        let mut picked = None;
        for row in crate::mail::cached_inbox(&store, 8) {
            if let Some((html, text, subject, from, date)) = crate::mail::cached_body(&store, &row.uid)
            {
                // 和命令里一样的取法：HTML 优先，其次纯文本
                let (html_body, text_body) = pick_body(html, text);
                let raw = if html_body.trim().is_empty() {
                    text_body
                } else {
                    html_body
                };
                let body = mail_plain_text(&raw);
                if body.chars().count() > 200 {
                    picked = Some((subject, from, date, body));
                    break;
                }
            }
        }
        let Some((subject, from, date, body)) = picked else {
            println!("缓存里没有够长的正文，这个用例跳过");
            return;
        };
        println!("摘要对象：{subject}（{from}）正文 {} 字", body.chars().count());
        let messages = mail_summary_messages(&subject, &from, &date, &body, &[]);
        let reply = ai::chat(&store, &messages, Some(0.3)).expect("接口没回话");
        let summary = mail_clean_summary(&reply);
        println!("原始回复：\n{reply}\n----\n清洗后：\n{summary}");
        assert!(!summary.is_empty(), "摘要是空的");
        assert!(
            summary.chars().count() <= 200,
            "摘要太长了（{} 字），提示词的「不超过 120 汉字」没被遵守",
            summary.chars().count()
        );
        assert!(
            summary.contains("·"),
            "摘要没有按「· 」分行：{summary}"
        );
    }
}

/// 「选中文字转公式」的回复解析（纯本地）
#[cfg(test)]
mod formula_reply_tests {
    use super::*;

    #[test]
    fn parses_json_reply() {
        let raw = r#"{"latex":"设 $x^2+y^2=1$，求 $y$","advice":"根号建议写成 \\sqrt{}"}"#;
        let (latex, advice) = parse_formula_reply(raw);
        assert_eq!(latex, "设 $x^2+y^2=1$，求 $y$");
        assert!(advice.contains("根号"), "{advice}");
    }

    #[test]
    fn falls_back_to_plain_text() {
        // 模型没按 JSON 回：去掉代码块，整段当公式用，至少不丢内容
        let (latex, advice) = parse_formula_reply("```latex\n$E=mc^2$\n```");
        assert_eq!(latex, "$E=mc^2$");
        assert!(advice.is_empty());
    }

    #[test]
    fn empty_reply_gives_empty() {
        let (latex, advice) = parse_formula_reply("   ");
        assert!(latex.is_empty());
        assert!(advice.is_empty());
    }

    /// 模型把同一个公式回了行内 + 行间两遍：只留一份
    #[test]
    fn duplicate_lines_are_collapsed() {
        let raw = "{\"latex\":\"$x^{2}+y^{2}=1$\\n$x^{2}+y^{2}=1$\",\"advice\":\"\"}";
        let (latex, _advice) = parse_formula_reply(raw);
        assert_eq!(latex, "$x^{2}+y^{2}=1$");
        // 不一样的几行要原样保留（是真正的多段文字）
        let raw = "{\"latex\":\"第一段 $x$\n第二段 $y$\",\"advice\":\"\"}";
        let (latex, _advice) = parse_formula_reply(raw);
        assert!(latex.contains("第一段") && latex.contains("第二段"), "{latex}");
    }
}
