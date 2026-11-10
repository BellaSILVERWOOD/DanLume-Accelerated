//! Tauri 后端入口：命令注册 + 数据层单例。
mod adminsign;
mod ai;
mod ai_tools;
mod ai_tools_net;
mod account;
mod backup;
mod canvas;
mod dashboard;
mod docs;
mod elearn;
mod ehall_grab;
mod ehall_remote;
mod focus;
mod geo;
mod ics;
mod latex_export;
mod mail;
mod mail_identity;
mod notices;
mod notices_auto;
mod notify;
mod papers;
mod paths;
mod registration;
mod reminders;
mod secrets;
mod store;
mod summary;
mod timetable_remote;
mod uis;
mod usage;
mod weather;
mod zipx;

use std::sync::OnceLock;

use chrono::{Duration, Local, NaiveTime};
use serde_json::{json, Value};
use tauri::{Manager, State};

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
    docs::read_text(&path, limit.unwrap_or(200_000))
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
    match mail::list_inbox(store, take) {
        Ok(items) => {
            let total = items.len();
            let items: Vec<_> = if unread_only {
                items
                    .into_iter()
                    .filter(|one| {
                        let value = serde_json::to_value(one).unwrap_or_default();
                        // 服务器没给 seen 字段时就当未读，别把信藏起来
                        value
                            .get("seen")
                            .and_then(|flag| flag.as_bool())
                            .map(|seen| !seen)
                            .unwrap_or(true)
                    })
                    .collect()
            } else {
                items
            };
            Ok(json!({ "ok": true, "items": items, "total": total,
                       "unread_only": unread_only, "error": "" }))
        }
        Err(err) => Ok(json!({ "ok": false, "items": [], "error": err })),
    }
}

#[tauri::command]
fn mail_body(uid: String, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    match mail::fetch_body(get_store(), &uid) {
        Ok(body) => json!({ "ok": true, "body": body, "error": "" }),
        Err(err) => json!({ "ok": false, "body": null, "error": err }),
    }
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

// ------------------------------------------------------------------ 文献库

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
        "ok": !rows.is_empty(),
        "maps": maps,
        "rooms": rooms,
        "building": cache.get("building").cloned().unwrap_or(Value::Null),
        "room": cache.get("room").cloned().unwrap_or(Value::Null),
        "value": cache.get("value").cloned().unwrap_or(Value::Null),
        "url": cache.get("url").cloned().unwrap_or(Value::Null),
        "ts": cache.get("ts").cloned().unwrap_or(Value::Null),
        "headers": headers,
        "rows": rows,
    })
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
#[tauri::command]
fn latex_pdf(body: String, name: Option<String>, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let stem = name.unwrap_or_else(|| {
        format!("danlume-{}", chrono::Local::now().format("%Y%m%d-%H%M%S"))
    });
    match latex_export::export_pdf(get_store(), &body, &stem) {
        Ok(path) => json!({ "ok": true, "path": path, "error": "" }),
        Err(err) => json!({ "ok": false, "path": "", "error": err }),
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

/// 带工具的对话（工具审核里开着的工具才会给模型）；返回 {text, rounds, log, tools_on}
#[tauri::command]
async fn ai_chat_tools(
    messages: Vec<Value>,
    temperature: Option<f64>,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let _ = &state;
    let store = get_store();
    let result = ai::chat_with_tools(store, &messages, temperature)?;
    Ok(result)
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

#[tauri::command]
fn ai_sessions(limit: Option<i64>, state: State<'_, AppState>) -> Vec<Value> {
    let _ = &state;
    ai::sessions(get_store(), limit.unwrap_or(30))
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

/// 到点该提醒的日程（remind_at <= 现在、还没提醒过、没完成）
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
    let _ = crate::reminders::auto_done(store);
    let mut out = if reminders_on {
        collect_event_reminders(store)
    } else {
        Vec::new()
    };
    // 「该出发了」单独一条链路（旧版 reminders.py::check_departure）
    out.extend(crate::reminders::due(store));
    out
}

fn collect_event_reminders(store: &Store) -> Vec<serde_json::Value> {
    let now = now_str();
    let conn = store.connection();
    let mut out = Vec::new();
    let Ok(mut stmt) = conn.prepare(
        "SELECT id, title, date, start_time, end_time, remind_at FROM events
         WHERE notified = 0 AND done = 0 AND remind_at IS NOT NULL AND remind_at <= ?1
         ORDER BY remind_at ASC LIMIT 5",
    ) else {
        return out;
    };
    let Ok(rows) = stmt.query_map([now], |row| {
        Ok(json!({
            "id": row.get::<_, i64>(0)?,
            "title": row.get::<_, String>(1).unwrap_or_default(),
            "date": row.get::<_, String>(2).unwrap_or_default(),
            "start_time": row.get::<_, String>(3).unwrap_or_default(),
            "end_time": row.get::<_, String>(4).unwrap_or_default(),
            "remind_at": row.get::<_, String>(5).unwrap_or_default(),
        }))
    }) else {
        return out;
    };
    for row in rows.flatten() {
        out.push(row);
    }
    out
}

#[tauri::command]
fn mark_notified(id: i64, state: State<'_, AppState>) -> Result<(), String> {
    let _ = &state;
    let store = get_store();
    let conn = store.connection();
    conn.execute("UPDATE events SET notified = 1 WHERE id = ?1", [id])
        .map_err(|err| err.to_string())?;
    Ok(())
}

/// 「稍后 10 分钟」：把提醒时间往后推，并把 notified 复位
#[tauri::command]
fn snooze_event(id: i64, minutes: Option<i64>, state: State<'_, AppState>) -> Result<String, String> {
    let _ = &state;
    let store = get_store();
    let shift = minutes.unwrap_or(10);
    let Some(event) = store.get_event(id).ok().flatten() else {
        return Err("这条日程不在了".to_string());
    };
    let base = event
        .remind_at
        .clone()
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| format!("{} {}", event.date, event.start_time));
    let parsed = chrono::NaiveDateTime::parse_from_str(&base, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(
                &format!("{} {}:00", event.date, event.start_time),
                "%Y-%m-%d %H:%M:%S",
            )
        })
        .map_err(|err| format!("时间看不懂：{err}"))?;
    let next = parsed + chrono::Duration::minutes(shift);
    let text = next.format("%Y-%m-%d %H:%M:%S").to_string();
    let conn = store.connection();
    conn.execute(
        "UPDATE events SET remind_at = ?1, notified = 0 WHERE id = ?2",
        rusqlite::params![text, id],
    )
    .map_err(|err| err.to_string())?;
    Ok(text)
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
fn open_uis_window(app: &tauri::AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("uis-login") {
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(());
    }
    let url: tauri::Url = "https://id.fudan.edu.cn/"
        .parse()
        .map_err(|exc| format!("URL parse failed: {exc}"))?;
    tauri::WebviewWindowBuilder::new(app, "uis-login", tauri::WebviewUrl::External(url))
        .title("登录统一身份认证")
        .inner_size(560.0, 760.0)
        .build()
        .map_err(|exc| format!("打不开登录窗口：{exc}"))?;
    Ok(())
}

#[tauri::command]
async fn uis_open_login(app: tauri::AppHandle) -> Result<(), String> {
    open_uis_window(&app)
}

#[allow(dead_code)]


/// 从登录窗口抓 cookie 存成会话（旧版读磁盘 cookie 的等价做法）
#[tauri::command]
fn uis_capture(app: tauri::AppHandle) -> serde_json::Value {
    let Some(win) = app.get_webview_window("uis-login") else {
        return json!({ "ok": false, "error": "登录窗口没开" });
    };
    let Ok(url) = "https://id.fudan.edu.cn/".parse::<tauri::Url>() else {
        return json!({ "ok": false, "error": "URL 解析失败" });
    };
    match win.cookies_for_url(url) {
        Ok(list) => {
            let mut jar = crate::uis::Jar::default();
            for cookie in &list {
                jar.set(cookie.name(), cookie.value());
            }
            if jar.is_empty() {
                return json!({ "ok": false, "error": "还没拿到 cookie：请先在窗口里完成登录" });
            }
            let store = get_store();
            let user = store.setting("uis_username").unwrap_or_default();
            if let Err(exc) = crate::uis::save_session(store, &jar, &user) {
                return json!({ "ok": false, "error": exc });
            }
            json!({ "ok": true, "cookies": jar.count(), "names": jar.names() })
        }
        Err(exc) => json!({ "ok": false, "error": format!("读 cookie 失败：{exc}") }),
    }
}

#[tauri::command]
fn jwgl_probe(semester: Option<String>, state: State<'_, AppState>) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    let sem = semester
        .filter(|one| !one.trim().is_empty())
        .unwrap_or_else(|| store.setting("jwgl_semester").unwrap_or_default());
    if sem.trim().is_empty() {
        return json!({ "ok": false, "error": "先填学期编号（例如 2025-2026-1）" });
    }
    let _ = store.set_setting("jwgl_semester", sem.trim());
    crate::timetable_remote::probe(store, sem.trim())
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

#[tauri::command]
fn jwgl_import(
    term_start: String,
    semester: Option<String>,
    state: State<'_, AppState>,
) -> serde_json::Value {
    let _ = &state;
    let store = get_store();
    let sem = semester
        .filter(|one| !one.trim().is_empty())
        .or_else(|| store.setting("jwgl_semester"))
        .unwrap_or_default();
    if sem.trim().is_empty() {
        return json!({ "ok": false, "error": "先填学期编号（例如 2025-2026-1）" });
    }
    let _ = store.set_setting("jwgl_semester", sem.trim());
    let body = match crate::timetable_remote::fetch_print_data(store, sem.trim()) {
        Ok((_, body)) => body,
        Err(exc) => return json!({ "ok": false, "error": exc }),
    };
    let slots = match crate::timetable_remote::parse_print_data(&body) {
        Ok(slots) => slots,
        Err(exc) => return json!({ "ok": false, "error": exc, "bytes": body.len() }),
    };
    let total = slots.len();
    match crate::timetable_remote::import_slots(store, &slots, term_start.trim()) {
        Ok((added, skipped)) => json!({
            "ok": true, "added": added, "skipped": skipped, "slots": total,
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
            // 关窗行为（旧版 tray_on_close）：设 1 时关窗只最小化、不退出
            {
                let handle = app.handle().clone();
                if let Some(window) = app.get_webview_window("main") {
                    window.on_window_event(move |event| {
                        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                            let keep = get_store()
                                .setting("tray_on_close")
                                .map(|text| text.trim() == "1")
                                .unwrap_or(false);
                            if keep {
                                api.prevent_close();
                                if let Some(win) = handle.get_webview_window("main") {
                                    let _ = win.minimize();
                                }
                            }
                        }
                    });
                }
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
            }            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            events_on,
            event_index,
            upcoming_events,
            week_number,
            add_event,
            update_event,
            delete_event,
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
            mail_body,
            mail_send,
            mail_test,
            mails,
            delete_mail,
            imap_test,
            settings_save,
            homework_items,
            homework_open_file,
            homework_write_deadlines,
            papers_list,
            paper_update,
            paper_citation,
            settings_all,
            ai_state,
            ai_profiles,
            ai_chat,
            ai_chat_tools,
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
            ai_profile_save,
            ai_profile_delete,
            classroom_view,
            ehall_cards,
            data_info,
            backup_db,
            data_export,
            data_import,
            app_relaunch,
            ehall_open,
            ehall_ingest,
            ehall_ingest_pasted,
            latex_pdf,
            due_reminders,
            mark_notified,
            snooze_event,
            account_health,
            account_request_email_code,
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
            account_notify_status,
        ])
        .plugin(tauri_plugin_dialog::init())
        .run(tauri::generate_context!())
        .expect("启动 Tauri 失败");
}
