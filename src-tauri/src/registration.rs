//! 注册申请 / 批准 / 注册表（对应旧版 `registration.py` 里管理员那一半）。
//!
//! 流程：申请人在自己机器上用**管理员公钥**加密一封 `LUMINA-SEAL1.…` 发过来；
//! 这边解出来 → 批准（生成 6 位验证码 + 签回执）→ 把「验证码 + 回执」发回去。
//! 注册表存在 `reg_users` / `reg_devices`，与旧版同表同字段。
use chrono::{Duration, Local};
use serde_json::{json, Value};

use crate::adminsign;
use crate::store::Store;

/// 两代主题前缀都收（改名 DanLume 之后新客户端发的是第二个）
pub const REQUEST_PREFIXES: [&str; 2] = ["[LUMINA-REG", "[DANLUME-REG"];
pub const DEFAULT_VALID_DAYS: i64 = 180;
pub const DEFAULT_DEVICE_LIMIT: i64 = 3;
/// 审批人（回执里的 issuer 字段）：旧版取第一个本地账户名，v2 用设置里的用户名
fn issuer(store: &Store) -> String {
    store
        .setting("user_name")
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| "本机用户（管理员）".to_string())
}

fn stamp() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn date_after(days: i64) -> String {
    (Local::now() + Duration::days(days.max(1)))
        .format("%Y-%m-%d")
        .to_string()
}

/// 批准时的默认有效期天数（旧版 `registry_valid_days`，默认 180）
pub fn valid_days(store: &Store) -> i64 {
    store
        .setting("registry_valid_days")
        .and_then(|text| text.trim().parse::<i64>().ok())
        .unwrap_or(DEFAULT_VALID_DAYS)
        .max(1)
}

/// 一个账户最多登记几台设备（旧版 `registry_device_limit`，0 = 关闭自动放行）
pub fn device_limit(store: &Store) -> i64 {
    store
        .setting("registry_device_limit")
        .and_then(|text| text.trim().parse::<i64>().ok())
        .unwrap_or(DEFAULT_DEVICE_LIMIT)
}

// ------------------------------------------------------------------ 注册表
pub fn registry(store: &Store) -> Vec<Value> {
    let rows: Vec<Value> = {
        let conn = store.connection();
        let mut out = Vec::new();
        if let Ok(mut stmt) = conn.prepare(
            "SELECT id, username, email, status, issued_at, expires_at, note
             FROM reg_users ORDER BY id",
        ) {
            if let Ok(mapped) = stmt.query_map([], |row| {
                Ok(json!({
                    "id": row.get::<_, i64>(0)?,
                    "username": row.get::<_, String>(1)?,
                    "email": row.get::<_, String>(2)?,
                    "status": row.get::<_, String>(3)?,
                    "issued_at": row.get::<_, String>(4)?,
                    "expires_at": row.get::<_, String>(5)?,
                    "note": row.get::<_, String>(6)?,
                }))
            }) {
                for one in mapped.flatten() {
                    out.push(one);
                }
            }
        }
        out
    };
    // 每行补上「登记了几台设备」
    let mut enriched = Vec::new();
    for mut row in rows {
        let name = row["username"].as_str().unwrap_or_default().to_string();
        let devices: Vec<String> = {
            let conn = store.connection();
            let mut out = Vec::new();
            if let Ok(mut stmt) = conn.prepare(
                "SELECT device FROM reg_devices WHERE registry_id = ?1 ORDER BY id",
            ) {
                if let Ok(mapped) = stmt.query_map([row["id"].as_i64().unwrap_or(0)], |r| {
                    r.get::<_, String>(0)
                }) {
                    for one in mapped.flatten() {
                        out.push(one);
                    }
                }
            }
            out
        };
        row["username"] = json!(name);
        row["devices"] = json!(devices);
        enriched.push(row);
    }
    enriched
}

fn find_user(store: &Store, username: &str) -> Option<(i64, String, String)> {
    let conn = store.connection();
    let mut stmt = conn
        .prepare("SELECT id, email, expires_at FROM reg_users WHERE username = ?1")
        .ok()?;
    let mut rows = stmt.query_map([username], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })
    .ok()?;
    rows.next()?.ok()
}

fn register_device(store: &Store, registry_id: i64, device: &str, note: &str) {
    if device.trim().is_empty() {
        return;
    }
    let conn = store.connection();
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM reg_devices WHERE registry_id = ?1 AND device = ?2",
            rusqlite::params![registry_id, device],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if exists == 0 {
        let _ = conn.execute(
            "INSERT INTO reg_devices(registry_id, device, created_at, note) VALUES (?1,?2,?3,?4)",
            rusqlite::params![registry_id, device, stamp(), note],
        );
    }
}

/// 注册表的三个动作：停用/启用、注销、续期
pub fn set_disabled(store: &Store, username: &str, disabled: bool) -> Result<Value, String> {
    let Some((id, _, _)) = find_user(store, username) else {
        return Err(format!("注册表里没有「{username}」"));
    };
    let status = if disabled { "disabled" } else { "active" };
    {
        let conn = store.connection();
        conn.execute("UPDATE reg_users SET status = ?1 WHERE id = ?2", rusqlite::params![status, id])
            .map_err(|exc| exc.to_string())?;
    }
    Ok(json!({ "ok": true, "username": username, "status": status }))
}

pub fn remove(store: &Store, username: &str) -> Result<Value, String> {
    let Some((id, _, _)) = find_user(store, username) else {
        return Err(format!("注册表里没有「{username}」"));
    };
    {
        let conn = store.connection();
        conn.execute("DELETE FROM reg_users WHERE id = ?1", [id])
            .map_err(|exc| exc.to_string())?;
        let _ = conn.execute("DELETE FROM reg_devices WHERE registry_id = ?1", [id]);
    }
    Ok(json!({ "ok": true, "username": username, "removed": true }))
}

pub fn renew(store: &Store, username: &str, days: Option<i64>) -> Result<Value, String> {
    let Some((id, email, _)) = find_user(store, username) else {
        return Err(format!("注册表里没有「{username}」"));
    };
    if !adminsign::has_private_key(store) {
        return Err("这台机器还没有管理员私钥，没法签发回执".to_string());
    }
    let expires = date_after(days.unwrap_or_else(|| valid_days(store)));
    let (code, hash) = new_code();
    {
        let conn = store.connection();
        conn.execute(
            "UPDATE reg_users SET expires_at = ?1, code_hash = ?2 WHERE id = ?3",
            rusqlite::params![expires, hash, id],
        )
        .map_err(|exc| exc.to_string())?;
    }
    let payload = receipt_payload(store, username, &email, "", &code, &expires, "");
    let token = adminsign::sign_receipt(store, &payload)?;
    Ok(json!({
        "ok": true, "username": username, "code": code, "receipt": token,
        "expires_at": expires, "subject": APPROVED_SUBJECT, "body": body_text(username, &email, &expires, &code, &token),
    }))
}

pub const APPROVED_SUBJECT: &str = "[LUMINA-APPROVED] DanLume 注册通过（含验证码与回执）";

// ------------------------------------------------------------------ 验证码
fn new_code() -> (String, String) {
    let mut rng = rand::thread_rng();
    let code: String = (0..6)
        .map(|_| {
            let digit: u8 = rand::Rng::gen_range(&mut rng, 0..10);
            (b'0' + digit) as char
        })
        .collect();
    let hash = code_hash(&code);
    (code, hash)
}

/// 验证码在这台机器上只做登记用（真正的核对发生在申请人那边：回执里带了 code）
fn code_hash(code: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(b"danlume-reg-");
    hasher.update(code.as_bytes());
    format!("sha256${:x}", hasher.finalize())
}

fn receipt_payload(store: &Store, username: &str, email: &str, device: &str,
                    code: &str, expires: &str, nonce: &str) -> Value {
    // 字段名与旧版 `_approval_payload` 完全一致，否则申请人那边验签后取不到字段
    json!({
        "v": 1,
        "kind": "registration-receipt",
        "username": username,
        "email": email,
        "nonce": nonce,
        "device": device,
        "code": code,
        "expires_at": expires,
        "issued_at": stamp(),
        "issuer": issuer(store),
    })
}

fn body_text(username: &str, email: &str, expires: &str, code: &str, token: &str) -> String {
    format!(
        "你的 DanLume 注册申请已通过。\n\n账户：{username}\n绑定邮箱：{email}\n有效期到：{expires}\n\n\
         验证码：{code}\n\n请在注册窗口里填入验证码，并把下面这一整行回执粘进「回执」输入框：\n{token}\n\n\
         回执里有管理员私钥的签名，程序会用它确认这条批准没被篡改。\n"
    )
}

// ------------------------------------------------------------------ 收申请
fn subject_ok(subject: &str) -> bool {
    REQUEST_PREFIXES.iter().any(|one| subject.starts_with(one))
}

fn request_from_sealed(store: &Store, sealed: &str, mail: Value,
                       via: &str) -> Result<Value, String> {
    let payload = adminsign::unseal(store, sealed)?;
    let username = payload
        .get("username")
        .and_then(|one| one.as_str())
        .unwrap_or_default()
        .to_string();
    let email = payload
        .get("email")
        .and_then(|one| one.as_str())
        .unwrap_or_default()
        .to_string();
    let already = find_user(store, &username);
    Ok(json!({
        "username": username,
        "email": email,
        "device": payload.get("device").and_then(|one| one.as_str()).unwrap_or(""),
        "nonce": payload.get("nonce").and_then(|one| one.as_str()).unwrap_or(""),
        "ts": payload.get("ts").and_then(|one| one.as_str()).unwrap_or(""),
        "via": via,
        "mail": mail,
        "already": already.is_some(),
        "expires_at": already.as_ref().map(|one| one.2.clone()).unwrap_or_default(),
    }))
}

/// 网络直连收到的申请（`reg_inbox`，管理员程序开着时才落库）
fn network_requests(store: &Store) -> Vec<Value> {
    let conn = store.connection();
    let mut out = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT username, email, device, nonce, sealed, received_at FROM reg_inbox
         ORDER BY id DESC LIMIT 20",
    ) {
        if let Ok(mapped) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        }) {
            for (username, email, device, nonce, sealed, at) in mapped.flatten() {
                out.push(json!({
                    "username": username, "email": email, "device": device,
                    "nonce": nonce, "ts": at, "via": "网络", "sealed": sealed,
                }));
            }
        }
    }
    out
}

/// 收申请：邮件（两代前缀都收）+ 网络直连，解不开的记进 errors 而不是静默丢掉
pub fn requests(store: &Store, limit: usize) -> (Vec<Value>, Vec<String>) {
    let mut found: Vec<Value> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    // ① 邮件
    match crate::mail::list_inbox(store, limit.max(20)) {
        Ok(items) => {
            for summary in items {
                let value = serde_json::to_value(&summary).unwrap_or_default();
                let subject = value
                    .get("subject")
                    .and_then(|one| one.as_str())
                    .unwrap_or_default()
                    .to_string();
                if !subject_ok(&subject) {
                    continue;
                }
                let uid = value
                    .get("uid")
                    .and_then(|one| one.as_str())
                    .unwrap_or_default()
                    .to_string();
                let body = match crate::mail::fetch_body(store, &uid) {
                    Ok(body) => serde_json::to_value(&body).unwrap_or_default(),
                    Err(exc) => {
                        errors.push(format!("{subject}：取正文失败 {exc}"));
                        continue;
                    }
                };
                let mut text = String::new();
                if let Some(part) = body.get("text").and_then(|one| one.as_str()) {
                    text.push_str(part);
                }
                if let Some(part) = body.get("html").and_then(|one| one.as_str()) {
                    text.push('\n');
                    text.push_str(part);
                }
                let sealed = adminsign::extract_seal(&text);
                if sealed.is_empty() {
                    errors.push(format!("{subject}：正文里没找到密文"));
                    continue;
                }
                match request_from_sealed(
                    store,
                    &sealed,
                    json!({ "uid": uid, "subject": subject,
                            "from": value.get("from").cloned().unwrap_or_default(),
                            "date": value.get("date").cloned().unwrap_or_default() }),
                    "邮件",
                ) {
                    Ok(item) => found.push(item),
                    Err(exc) => errors.push(format!("{subject}：{exc}")),
                }
            }
        }
        Err(exc) => errors.push(format!("读收件箱失败：{exc}")),
    }
    // ② 网络直连
    for item in network_requests(store) {
        let sealed = item
            .get("sealed")
            .and_then(|one| one.as_str())
            .unwrap_or_default()
            .to_string();
        let username = item.get("username").cloned().unwrap_or_default();
        let already = username
            .as_str()
            .map(|name| find_user(store, name).is_some())
            .unwrap_or(false);
        if let Ok(mut one) = request_from_sealed(store, &sealed, json!({}), "网络") {
            one["already"] = json!(already);
            found.push(one);
        } else {
            let mut one = item.clone();
            one["already"] = json!(already);
            found.push(one);
        }
    }
    (found, errors)
}

/// 从 `.eml` 文件救一条申请（网页邮箱里「另存为」下来的那封）
pub fn request_from_eml(store: &Store, path: &str) -> Result<Value, String> {
    let raw = std::fs::read(path).map_err(|exc| format!("读不到文件：{exc}"))?;
    let parsed = mail_parser::MessageParser::default()
        .parse(&raw)
        .ok_or_else(|| "这个文件不是合法的邮件".to_string())?;
    let subject = parsed.subject().unwrap_or_default().to_string();
    if !subject_ok(&subject) {
        return Err(format!("这封邮件不是注册申请（主题：{subject}）"));
    }
    let mut body = String::new();
    for part in parsed.text_bodies() {
        if let Some(text) = part.text_contents() {
            body.push_str(text);
            body.push('\n');
        }
    }
    let sealed = adminsign::extract_seal(&body);
    if sealed.is_empty() {
        return Err("这封邮件里没有找到密文".to_string());
    }
    request_from_sealed(
        store,
        &sealed,
        json!({ "uid": "", "subject": subject, "from": path, "date": "" }),
        "文件",
    )
}

// ------------------------------------------------------------------ 批准
/// 批准一条申请：写注册表 + 生成验证码 + 签回执（不负责发邮件）
pub fn approve(store: &Store, request: &Value, days: Option<i64>) -> Result<Value, String> {
    let payload = request.get("payload").unwrap_or(request);
    let username = payload
        .get("username")
        .and_then(|one| one.as_str())
        .unwrap_or_default()
        .trim()
        .to_string();
    let email = payload
        .get("email")
        .and_then(|one| one.as_str())
        .unwrap_or_default()
        .trim()
        .to_string();
    let device = payload
        .get("device")
        .and_then(|one| one.as_str())
        .unwrap_or_default()
        .to_string();
    let nonce = payload
        .get("nonce")
        .and_then(|one| one.as_str())
        .unwrap_or_default()
        .to_string();
    if username.is_empty() || !email.contains('@') {
        return Err("这条申请缺少用户名或邮箱".to_string());
    }
    if !adminsign::has_private_key(store) {
        return Err("这台机器还没有管理员私钥，没法签发回执".to_string());
    }
    // 老用户：沿用原有效期，只补登记设备
    if let Some((id, _, expires_at)) = find_user(store, &username) {
        let (code, hash) = new_code();
        {
            let conn = store.connection();
            conn.execute(
                "UPDATE reg_users SET code_hash = ?1, note = TRIM(note || '；管理员批准新设备')
                 WHERE id = ?2",
                rusqlite::params![hash, id],
            )
            .map_err(|exc| exc.to_string())?;
        }
        register_device(store, id, &device, "管理员批准新设备");
        let payload_json = receipt_payload(store, &username, &email, &device, &code, &expires_at, &nonce);
        let token = adminsign::sign_receipt(store, &payload_json)?;
        return Ok(json!({
            "ok": true, "reused": true, "username": username, "email": email,
            "code": code, "receipt": token, "expires_at": expires_at,
            "subject": APPROVED_SUBJECT,
            "body": body_text(&username, &email, &expires_at, &code, &token),
        }));
    }
    let expires = date_after(days.unwrap_or_else(|| valid_days(store)));
    let (code, hash) = new_code();
    let registry_id = {
        let conn = store.connection();
        conn.execute(
            "INSERT INTO reg_users(username, email, status, issued_at, expires_at, code_hash, note)
             VALUES (?1, ?2, 'active', ?3, ?4, ?5, '')",
            rusqlite::params![username, email, stamp(), expires, hash],
        )
        .map_err(|exc| format!("写注册表失败：{exc}"))?;
        conn.last_insert_rowid()
    };
    register_device(store, registry_id, &device, "首次批准");
    let payload_json = receipt_payload(store, &username, &email, &device, &code, &expires, &nonce);
    let token = adminsign::sign_receipt(store, &payload_json)?;
    Ok(json!({
        "ok": true, "reused": false, "username": username, "email": email,
        "code": code, "receipt": token, "expires_at": expires,
        "subject": APPROVED_SUBJECT,
        "body": body_text(&username, &email, &expires, &code, &token),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real_store() -> Store {
        std::env::set_var(
            "LUMINA_DATA_DIR",
            r"E:\FIles\Lena‘s Files\Work\znw\Agent\data",
        );
        Store::open().expect("打不开数据库")
    }

    #[test]
    fn registry_reads_real_rows() {
        let store = real_store();
        let rows = registry(&store);
        println!("注册表 {} 个账户", rows.len());
        for row in &rows {
            println!("  {} {}", row["username"], row["expires_at"]);
        }
        assert!(!rows.is_empty(), "旧库里已经有批准过的账户");
        println!("默认有效期 = {} 天，设备上限 = {}", valid_days(&store), device_limit(&store));
    }

    #[test]
    fn approve_existing_user_reuses_expiry() {
        let store = real_store();
        if !adminsign::has_private_key(&store) {
            println!("没有私钥，跳过");
            return;
        }
        let rows = registry(&store);
        let Some(first) = rows.first() else {
            println!("注册表空的，跳过");
            return;
        };
        let username = first["username"].as_str().unwrap_or_default().to_string();
        let email = first["email"].as_str().unwrap_or_default().to_string();
        let before = first["expires_at"].as_str().unwrap_or_default().to_string();
        let request = json!({ "username": username, "email": email,
                              "device": "test-device-x", "nonce": "n-test" });
        let approval = approve(&store, &request, None).expect("批准失败");
        println!("批准结果：{}", approval["code"]);
        assert_eq!(approval["reused"], true, "老用户应该沿用原有效期");
        assert_eq!(approval["expires_at"], before);
        let token = approval["receipt"].as_str().unwrap();
        let back = adminsign::verify_receipt(&store, token).expect("回执验不过");
        assert_eq!(back["username"], username);
        assert_eq!(back["nonce"], "n-test");
        println!("回执校验通过：{}", back["kind"]);
    }
}
