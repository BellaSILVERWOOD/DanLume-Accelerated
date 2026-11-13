//! 自家账号服务器（FastAPI）的客户端。
//!
//! 接口契约（服务器 `https://43.108.99.38.sslip.io`，全部在 `/api/v1` 下）：
//!
//! ```text
//! GET    /health                                  -> {ok, version, uptime_seconds}
//! POST   /register/request-code {email}           -> {ok, expires_in, cooldown}
//! POST   /register  {username,email,password,device_fingerprint,device_name,platform,invite_code?,code}
//! POST   /login     {username_or_email,password,device_fingerprint,device_name,platform}
//! POST   /refresh   {refresh_token}
//! GET    /me        (Bearer)
//! DELETE /me        (Bearer)
//! GET    /devices   (Bearer) -> {devices:[{id,name,platform,last_seen_at}]}
//! POST   /devices/kick {device_id} (Bearer)
//! GET    /announcements?since=<id> -> {items:[...]}   （公告本身是公开接口，见 notify.rs）
//! ```
//!
//! 设计要点：
//! * 只用项目里已有的 `ureq`，不新增 crate；每个请求 5 秒超时（`TIMEOUT_SECS`）。
//! * token / refresh_token 用 `secrets.rs` 那套 DPAPI 加密（键名带 `secret:` 前缀，
//!   所以 `settings_all` 不会把它们当明文吐给前端）。
//! * 出错一律返回 [`ApiError`]，里面带 `kind` 机器码：`offline`（网络不可达）、
//!   `unauthorized`（401 凭据错）、`device_limit`（409，原样带回设备列表）、
//!   `unsupported`（404，服务器没这个接口）、`server`（其它 4xx/5xx）、`config`（本地没配好）。
//!   比返回一个字符串好用：前端能直接按 `kind` / `offline` 分支，409 还能把设备列表画出来。
//! * 邮箱验证码相关的错误另外分了类，并且把服务器给的 `retry_after` / `attempts_left`
//!   一起带进错误体（前端要把它写进文案里）：
//!   * `cooldown`      429 `email_cooldown` / `too_many_requests` -> 还有 `retry_after` 秒；
//!   * `email`         503 `email_send_failed` / `email_not_configured` -> 服务器发不出信；
//!   * `invalid_email` 400 `invalid_email` / `email_domain_not_allowed`；
//!   * `invalid_code`  400 `invalid_code` / `code_expired` -> 还有 `attempts_left` 次机会
//!     （服务端 v1.1.0 实际发的是 `remaining_attempts`，这里两个字段名都认，统一成 `attempts_left`）；
//!   * `code` 字段则是服务器原始的机器码，`kind` 只是归并后的大类。
//! * 带 Bearer 的请求遇到 401 会自动用 refresh_token 换一次 token 再重试**一次**；
//!   refresh 也失败就把原来的 401 错误抛给前端（不静默吞掉）。
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::secrets;
use crate::store::Store;

/// 默认账号服务器（设置里 `server_url` 没填过时用它；填成空字符串 = 不用服务器）
pub const DEFAULT_SERVER: &str = "https://43.108.99.38.sslip.io";
/// 所有请求的超时（秒）
pub const TIMEOUT_SECS: u64 = 5;
/// 存取 token 的设置键（值一定是 DPAPI 密文；`settings_all` 里会按密钥处理，不吐明文）
pub const TOKEN_KEY: &str = "server_token";
pub const REFRESH_KEY: &str = "server_refresh_token";

// ---------------------------------------------------------------- 设置项

/// 服务器地址：没设过 → 默认值；设成空串 → 明确表示「不用服务器」
pub fn server_url(store: &Store) -> String {
    match store.setting("server_url") {
        None => DEFAULT_SERVER.to_string(),
        Some(text) => text.trim().trim_end_matches('/').to_string(),
    }
}

pub fn server_enabled(store: &Store) -> bool {
    !server_url(store).is_empty()
}

/// 读一个 DPAPI 密文设置（解不开就当没有）
fn secret_setting(store: &Store, key: &str) -> String {
    store
        .setting(key)
        .map(|raw| secrets::unprotect(&raw).unwrap_or_default())
        .unwrap_or_default()
}

pub fn token(store: &Store) -> String {
    secret_setting(store, TOKEN_KEY)
}

pub fn refresh_token(store: &Store) -> String {
    secret_setting(store, REFRESH_KEY)
}

pub fn logged_in(store: &Store) -> bool {
    !token(store).is_empty()
}

/// 写 token（DPAPI 加密后存 settings 表）；空字符串表示清掉
pub fn set_token(store: &Store, key: &str, value: &str) -> Result<(), String> {
    let payload = if value.trim().is_empty() {
        String::new()
    } else {
        secrets::protect(value)?
    };
    store.set_setting(key, &payload).map_err(|err| err.to_string())
}

pub fn clear_tokens(store: &Store) {
    let _ = set_token(store, TOKEN_KEY, "");
    let _ = set_token(store, REFRESH_KEY, "");
    let _ = store.set_setting("server_token_expires_at", "");
    let _ = store.set_setting("server_devices_used", "0");
    let _ = store.set_setting("server_device_limit", "0");
}

/// 设备指纹：第一次用到时生成一次，之后一直用同一个（uuid v4 风格）
pub fn device_fingerprint(store: &Store) -> String {
    if let Some(existing) = store.setting("device_fingerprint") {
        let trimmed = existing.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }
    let fresh = uuid_v4();
    let _ = store.set_setting("device_fingerprint", &fresh);
    fresh
}

pub fn device_name() -> String {
    std::env::var("COMPUTERNAME")
        .ok()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .map(|name| format!("DanLume @ {name}"))
        .unwrap_or_else(|| "DanLume 桌面端".to_string())
}

pub fn platform() -> &'static str {
    std::env::consts::OS
}

/// 随机 uuid（v4 格式，不引新 crate：rand 已经在依赖里）
fn uuid_v4() -> String {
    use rand::Rng;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant 10xx
    let hex: Vec<String> = bytes.iter().map(|one| format!("{one:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        hex[0..4].concat(),
        hex[4..6].concat(),
        hex[6..8].concat(),
        hex[8..10].concat(),
        hex[10..16].concat()
    )
}

// ---------------------------------------------------------------- 错误

/// 账号服务器的错误。序列化后直接当 Tauri 的 rejection 传给前端。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    /// 恒为 false，方便前端 `if (!result.ok)` 统一处理
    pub ok: bool,
    /// 机器码：offline / unauthorized / device_limit / unsupported / server / config / other
    /// 以及邮箱验证码那几类：cooldown / email / invalid_email / invalid_code / invalid
    pub kind: String,
    /// 服务器原始的 `error` 机器码（`email_cooldown`、`invalid_code`…）；本地校验失败时为空
    #[serde(default)]
    pub code: String,
    /// 给人看的中文说明
    pub error: String,
    pub http_status: u16,
    /// 网络不可达（连不上 / 超时 / DNS 失败）
    pub offline: bool,
    /// 409 设备超限时原样带回的设备列表
    pub devices: Vec<Value>,
    pub devices_used: i64,
    pub device_limit: i64,
    /// 429 冷却：还要等多少秒才能再发（`retry_after`，没有就是 0）
    #[serde(default)]
    pub retry_after: i64,
    /// 400 验证码错 / 过期：还能再试几次（`attempts_left`，没有就是 0）
    #[serde(default)]
    pub attempts_left: i64,
    /// 服务器原始响应体（截断到 500 字），排查用
    pub body: String,
}

impl ApiError {
    fn base(kind: &str, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            kind: kind.to_string(),
            code: String::new(),
            error: message.into(),
            http_status: 0,
            offline: false,
            devices: Vec::new(),
            devices_used: 0,
            device_limit: 0,
            retry_after: 0,
            attempts_left: 0,
            body: String::new(),
        }
    }

    /// 网络不可达（前端看到 `offline=true` 就显示「服务器暂不可用」）
    pub fn offline(message: impl Into<String>) -> Self {
        let mut err = Self::base("offline", message);
        err.offline = true;
        err
    }

    /// 本地没配好（服务器地址为空之类）
    pub fn config(message: impl Into<String>) -> Self {
        Self::base("config", message)
    }

    pub fn other(message: impl Into<String>) -> Self {
        Self::base("other", message)
    }

    pub fn is_offline(&self) -> bool {
        self.offline || self.kind == "offline"
    }

    /// 把 HTTP 状态码 + 响应体归成上面的几类
    fn from_response(status: u16, body_text: &str) -> Self {
        let body: Value = serde_json::from_str(body_text).unwrap_or(Value::Null);
        let code = body
            .get("error")
            .and_then(|one| one.as_str())
            .unwrap_or_default()
            .to_string();
        let message = body
            .get("message")
            .and_then(|one| one.as_str())
            .or_else(|| body.get("error").and_then(|one| one.as_str()))
            .unwrap_or_default()
            .to_string();
        let truncated: String = body_text.chars().take(500).collect();
        let retry_after = body
            .get("retry_after")
            .and_then(|one| one.as_i64())
            .unwrap_or(0);
        let attempts_left = body
            .get("attempts_left")
            // 服务端实际字段叫 remaining_attempts（v1.1.0），两个都认，别因为改名就不显示次数
            .or_else(|| body.get("remaining_attempts"))
            .and_then(|one| one.as_i64())
            .unwrap_or(0);

        // 409 + error=device_limit：设备超限，设备列表原样带回
        if status == 409 || code == "device_limit" {
            let devices: Vec<Value> = body
                .get("devices")
                .and_then(|one| one.as_array())
                .cloned()
                .unwrap_or_default();
            let mut err = Self::base(
                "device_limit",
                if message.is_empty() {
                    "设备数量已达上限，请先在设备管理里踢掉一台".to_string()
                } else {
                    message
                },
            );
            err.code = code;
            err.http_status = status;
            err.body = truncated;
            err.devices = devices;
            err.devices_used = body
                .get("devices_used")
                .and_then(|one| one.as_i64())
                .unwrap_or(0);
            err.device_limit = body
                .get("device_limit")
                .and_then(|one| one.as_i64())
                .unwrap_or(0);
            err.retry_after = retry_after;
            err.attempts_left = attempts_left;
            return err;
        }

        // 邮箱验证码那几个错误码归并成大类（前端按 kind 分支），原始机器码留在 `code` 里
        let kind = match (status, code.as_str()) {
            (401 | 403, _) => "unauthorized",
            (404, _) => "unsupported",
            (429, _) => "cooldown",
            (503, "email_send_failed" | "email_not_configured") => "email",
            (400, "invalid_email" | "email_domain_not_allowed") => "invalid_email",
            (400, "invalid_code" | "code_expired") => "invalid_code",
            (400, _) => "invalid",
            _ => "server",
        };
        let error = if !message.is_empty() {
            message
        } else {
            match (status, kind) {
                (401, _) => "账号或口令不对".to_string(),
                (403, _) => "服务器拒绝了这个请求".to_string(),
                (404, _) => "服务器上没有这个接口（可能是旧版服务器）".to_string(),
                (429, _) => "操作太频繁了，请稍后再试".to_string(),
                (_, "email") => "服务器暂时发不出邮件（SMTP 没配好？）".to_string(),
                (_, "invalid_email") => "这个邮箱服务器不接受".to_string(),
                (_, "invalid_code") => "邮箱验证码不对或已经过期".to_string(),
                (_, "invalid") => "服务器不接受这个请求参数".to_string(),
                (_, _) => format!("服务器返回 HTTP {status}"),
            }
        };
        let mut err = Self::base(kind, error);
        err.code = code;
        err.http_status = status;
        err.retry_after = retry_after;
        err.attempts_left = attempts_left;
        err.body = truncated;
        err
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.error)
    }
}

impl std::error::Error for ApiError {}

// ---------------------------------------------------------------- HTTP 底层

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(TIMEOUT_SECS))
        .timeout_connect(Duration::from_secs(TIMEOUT_SECS))
        .user_agent(concat!("DanLume/", env!("CARGO_PKG_VERSION")))
        .build()
}

/// 发一个请求，拿到 (状态码, 响应体)。4xx/5xx 也算成功返回，只有连不上才是 Err。
fn send(
    method: &str,
    url: &str,
    body: Option<&Value>,
    bearer: Option<&str>,
) -> Result<(u16, String), ApiError> {
    let agent = agent();
    let mut request = match method {
        "GET" => agent.get(url),
        "POST" => agent.post(url),
        "DELETE" => agent.request("DELETE", url),
        "PUT" => agent.put(url),
        other => return Err(ApiError::other(format!("不支持的请求方法：{other}"))),
    };
    if let Some(value) = bearer {
        if !value.is_empty() {
            request = request.set("Authorization", &format!("Bearer {value}"));
        }
    }
    request = request.set("Accept", "application/json");
    let result = match body {
        Some(payload) => request
            .set("Content-Type", "application/json")
            .send_string(&payload.to_string()),
        None => request.call(),
    };
    match result {
        Ok(response) => Ok((response.status(), response.into_string().unwrap_or_default())),
        Err(ureq::Error::Status(status, response)) => {
            Ok((status, response.into_string().unwrap_or_default()))
        }
        Err(ureq::Error::Transport(exc)) => Err(ApiError::offline(format!(
            "连不上服务器：{exc}"
        ))),
    }
}

/// 发请求 + 解析 JSON + 归类错误
fn request_json(
    method: &str,
    url: &str,
    body: Option<&Value>,
    bearer: Option<&str>,
) -> Result<Value, ApiError> {
    let (status, text) = send(method, url, body, bearer)?;
    if !(200..300).contains(&status) {
        return Err(ApiError::from_response(status, &text));
    }
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str::<Value>(&text).map_err(|err| {
        let mut bad = ApiError::other(format!("服务器返回的不是 JSON：{err}"));
        bad.http_status = status;
        bad.body = text.chars().take(500).collect();
        bad
    })
}

fn endpoint(store: &Store, path: &str) -> Result<String, ApiError> {
    let base = server_url(store);
    if base.is_empty() {
        return Err(ApiError::config("服务器地址为空（留空表示不用服务器）"));
    }
    Ok(format!("{base}{path}"))
}

// ---------------------------------------------------------------- token 处理

/// 把 register / login / refresh 返回的 token 存下来，并记下账号信息
fn apply_session(store: &Store, value: &Value) -> Result<(), ApiError> {
    let access = value
        .get("token")
        .and_then(|one| one.as_str())
        .unwrap_or_default()
        .to_string();
    if access.is_empty() {
        return Err(ApiError::other("服务器没有返回 token"));
    }
    set_token(store, TOKEN_KEY, &access).map_err(ApiError::other)?;
    if let Some(refresh) = value.get("refresh_token").and_then(|one| one.as_str()) {
        set_token(store, REFRESH_KEY, refresh).map_err(ApiError::other)?;
    }
    if let Some(expires) = value.get("expires_in").and_then(|one| one.as_i64()) {
        let at = chrono::Local::now() + chrono::Duration::seconds(expires);
        let _ = store.set_setting("server_token_expires_at", &at.format("%Y-%m-%d %H:%M:%S").to_string());
    }
    if let Some(account) = value.get("account") {
        if let Some(name) = account.get("username").and_then(|one| one.as_str()) {
            let _ = store.set_setting("server_username", name);
        }
        // 角色（user / admin）：登录响应里可能没有，没有就等 `/me` 补上
        if let Some(role) = account.get("role").and_then(|one| one.as_str()) {
            let _ = store.set_setting("server_role", role);
        }
        if let Some(id) = account.get("id") {
            let text = id
                .as_i64()
                .map(|one| one.to_string())
                .or_else(|| id.as_str().map(|one| one.to_string()))
                .unwrap_or_default();
            let _ = store.set_setting("server_account_id", &text);
        }
    }
    // 设备数 / 设备上限：登录响应里带着，存下来给设置页显示
    if let Some(used) = value.get("devices_used").and_then(|one| one.as_i64()) {
        let _ = store.set_setting("server_devices_used", &used.to_string());
    }
    if let Some(limit) = value.get("device_limit").and_then(|one| one.as_i64()) {
        let _ = store.set_setting("server_device_limit", &limit.to_string());
    }
    Ok(())
}

/// 用 refresh_token 换一套新 token（成功返回服务器响应）
pub fn refresh(store: &Store) -> Result<Value, ApiError> {
    let url = endpoint(store, "/api/v1/refresh")?;
    let token = refresh_token(store);
    if token.is_empty() {
        let mut err = ApiError::base("unauthorized", "没有 refresh_token，需要重新登录");
        err.http_status = 401;
        return Err(err);
    }
    let value = request_json(
        "POST",
        &url,
        Some(&json!({ "refresh_token": token })),
        None,
    )?;
    apply_session(store, &value)?;
    Ok(value)
}

/// 带 Bearer 的请求：401 时自动 refresh 一次再重试一次
fn authed(store: &Store, method: &str, path: &str, body: Option<&Value>) -> Result<Value, ApiError> {
    let url = endpoint(store, path)?;
    let current = token(store);
    if current.is_empty() {
        let mut err = ApiError::base("unauthorized", "还没有登录");
        err.http_status = 401;
        return Err(err);
    }
    match request_json(method, &url, body, Some(&current)) {
        Ok(value) => Ok(value),
        Err(err) if err.kind == "unauthorized" => {
            // token 过期：换一次再重试一次；换不到就把原来的 401 抛出去
            if refresh(store).is_err() {
                return Err(err);
            }
            let fresh = token(store);
            request_json(method, &url, body, Some(&fresh))
        }
        Err(err) => Err(err),
    }
}

// ---------------------------------------------------------------- 对外的几个调用

/// 探测服务器：`GET /health`（5 秒超时），返回 `{ok,online,version,uptime_seconds,latency_ms}`
pub fn health() -> Result<Value, ApiError> {
    let store = crate::get_store();
    let url = endpoint(&store, "/api/v1/health")?;
    let started = std::time::Instant::now();
    let value = request_json("GET", &url, None, None)?;
    let latency = started.elapsed().as_millis() as i64;
    Ok(json!({
        "ok": true,
        "online": true,
        "version": value.get("version").and_then(|one| one.as_str()).unwrap_or(""),
        "uptime_seconds": value.get("uptime_seconds").and_then(|one| one.as_i64()).unwrap_or(0),
        "latency_ms": latency,
        "url": server_url(&store),
        "raw": value,
    }))
}

/// 粗略的邮箱格式检查：只有一个 `@`、两边都不空、不带空白。
///
/// 不求符合 RFC（真正的校验在服务器那边），只为了让本地能立刻给一句人话，
/// 而不是白跑一趟网络再显示服务器错误。
fn email_looks_valid(email: &str) -> bool {
    let email = email.trim();
    if email.chars().any(char::is_whitespace) {
        return false;
    }
    let mut parts = email.split('@');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(local), Some(domain), None) => !local.is_empty() && !domain.is_empty(),
        _ => false,
    }
}

/// 注册前先让服务器给邮箱发一封验证码：`POST /api/v1/register/request-code {"email"}`。
///
/// 成功返回 `{ok, email, expires_in, cooldown, raw}`：
/// * `expires_in` 验证码有效期（秒，服务器默认 600）；
/// * `cooldown` 多少秒后能再点「获取验证码」（服务器默认 60，前端拿它做倒计时）。
///
/// 失败分类见模块头注释：429 是 `cooldown`（错误体带 `retry_after`），
/// 503 是 `email`（`email_send_failed` / `email_not_configured`），
/// 400 是 `invalid_email`（`email_domain_not_allowed` 也归这一类）。
pub fn request_email_code(email: &str) -> Result<Value, ApiError> {
    let email = email.trim();
    if email.is_empty() {
        return Err(ApiError::config("邮箱不能为空"));
    }
    if !email_looks_valid(email) {
        return Err(ApiError::config("邮箱格式不对"));
    }
    let store = crate::get_store();
    let url = endpoint(&store, "/api/v1/register/request-code")?;
    let value = request_json("POST", &url, Some(&json!({ "email": email })), None)?;
    Ok(json!({
        "ok": true,
        "email": email,
        "expires_in": value.get("expires_in").and_then(|one| one.as_i64()).unwrap_or(600),
        "cooldown": value.get("cooldown").and_then(|one| one.as_i64()).unwrap_or(60),
        "raw": value,
    }))
}

/// 忘记密码第一步：请求重置验证码（`purpose=reset` 那套，和注册码互不干扰）
pub fn request_reset_code(email: &str) -> Result<Value, ApiError> {
    let email = email.trim();
    if email.is_empty() {
        return Err(ApiError::config("邮箱不能为空"));
    }
    if !email_looks_valid(email) {
        return Err(ApiError::config("邮箱格式不对"));
    }
    let store = crate::get_store();
    let url = endpoint(&store, "/api/v1/password/request-code")?;
    let value = request_json("POST", &url, Some(&json!({ "email": email })), None)?;
    Ok(json!({
        "ok": true,
        "email": email,
        "message": value.get("message").and_then(|one| one.as_str()).unwrap_or(""),
        "expires_in": value.get("expires_in").and_then(|one| one.as_i64()).unwrap_or(600),
        "cooldown": value.get("cooldown").and_then(|one| one.as_i64()).unwrap_or(60),
        "raw": value,
    }))
}

/// 忘记密码第二步：验证码 + 新口令，成功后用新口令登录
pub fn reset_password(email: &str, code: &str, password: &str) -> Result<Value, ApiError> {
    let email = email.trim();
    let code = code.trim();
    if email.is_empty() || code.is_empty() || password.is_empty() {
        return Err(ApiError::config("邮箱、验证码、新口令都要填"));
    }
    if password.chars().count() < 8 {
        return Err(ApiError::config("新口令至少 8 位"));
    }
    let store = crate::get_store();
    let url = endpoint(&store, "/api/v1/password/reset")?;
    let value = request_json(
        "POST",
        &url,
        Some(&json!({ "email": email, "code": code, "password": password })),
        None,
    )?;
    Ok(json!({
        "ok": true,
        "message": value.get("message").and_then(|one| one.as_str()).unwrap_or("密码已重置"),
    }))
}

/// 拼 `/register` 的请求体。`invite_code` / `code` 传 None 或空白都不带这个字段
/// （老服务器不认识 `code`，但空着传也不会出错；这里干净一点）。
fn register_body(
    store: &Store,
    username: &str,
    email: &str,
    password: &str,
    invite_code: Option<&str>,
    code: Option<&str>,
) -> Value {
    let mut body = json!({
        "username": username,
        "email": email,
        "password": password,
        "device_fingerprint": device_fingerprint(store),
        "device_name": device_name(),
        "platform": platform(),
    });
    if let Some(invite) = invite_code.map(|one| one.trim()).filter(|one| !one.is_empty()) {
        body["invite_code"] = json!(invite);
    }
    if let Some(code) = code.map(|one| one.trim()).filter(|one| !one.is_empty()) {
        body["code"] = json!(code);
    }
    body
}

/// 注册。`invite_code` 传 None / 空串都表示不带邀请码；
/// `code` 是邮箱验证码（服务器开了邮箱验证时必填，None / 空串时不带这个字段）。
pub fn register(
    username: &str,
    email: &str,
    password: &str,
    invite_code: Option<&str>,
    code: Option<&str>,
) -> Result<Value, ApiError> {
    let store = crate::get_store();
    let username = username.trim();
    let email = email.trim();
    if username.is_empty() || email.is_empty() || password.is_empty() {
        return Err(ApiError::config("用户名、邮箱、口令都不能为空"));
    }
    if !email_looks_valid(email) {
        return Err(ApiError::config("邮箱格式不对"));
    }
    if password.chars().count() < 6 {
        return Err(ApiError::config("口令至少 6 位"));
    }
    let url = endpoint(&store, "/api/v1/register")?;
    let body = register_body(&store, username, email, password, invite_code, code);
    let value = request_json("POST", &url, Some(&body), None)?;
    apply_session(&store, &value)?;
    Ok(with_session_echo(&store, value))
}

/// 登录（用户名或邮箱都行）
pub fn login(username_or_email: &str, password: &str) -> Result<Value, ApiError> {
    let store = crate::get_store();
    let who = username_or_email.trim();
    if who.is_empty() || password.is_empty() {
        return Err(ApiError::config("账号和口令都要填"));
    }
    let url = endpoint(&store, "/api/v1/login")?;
    let body = json!({
        "username_or_email": who,
        "password": password,
        "device_fingerprint": device_fingerprint(&store),
        "device_name": device_name(),
        "platform": platform(),
    });
    let value = request_json("POST", &url, Some(&body), None)?;
    apply_session(&store, &value)?;
    Ok(with_session_echo(&store, value))
}

/// 注册 / 登录成功后顺手把「我是谁」也回给前端，省一次 /me
fn with_session_echo(store: &Store, mut value: Value) -> Value {
    if let Some(map) = value.as_object_mut() {
        map.insert("ok".to_string(), json!(true));
        map.insert("logged_in".to_string(), json!(true));
        map.insert("server_url".to_string(), json!(server_url(store)));
        map.insert(
            "device_fingerprint".to_string(),
            json!(device_fingerprint(store)),
        );
        map.insert(
            "username".to_string(),
            json!(store.setting("server_username").unwrap_or_default()),
        );
        map.insert(
            "account_id".to_string(),
            json!(store.setting("server_account_id").unwrap_or_default()),
        );
    }
    value
}

/// 退出登录：只清本地 token（服务器那侧的 session 让它自己过期）
pub fn logout() -> Value {
    let store = crate::get_store();
    let username = store.setting("server_username").unwrap_or_default();
    clear_tokens(&store);
    let _ = store.set_setting("server_username", "");
    let _ = store.set_setting("server_account_id", "");
    crate::notify::kick();
    json!({ "ok": true, "logged_in": false, "username": username })
}

pub fn me() -> Result<Value, ApiError> {
    let store = crate::get_store();
    let value = authed(&store, "GET", "/api/v1/me", None)?;
    // 顺手把角色记下来：客户端要靠它决定「管理员入口」显不显示
    if let Some(account) = value.get("account").or_else(|| value.get("user")) {
        if let Some(role) = account.get("role").and_then(|one| one.as_str()) {
            let _ = store.set_setting("server_role", role);
        }
        if let Some(name) = account.get("username").and_then(|one| one.as_str()) {
            let _ = store.set_setting("server_username", name);
        }
    } else if let Some(role) = value.get("role").and_then(|one| one.as_str()) {
        let _ = store.set_setting("server_role", role);
    }
    Ok(value)
}

/// 这台机器上登录的服务器账号是不是管理员（`/me` 里那个 role；没登录就是普通用户）
pub fn is_admin(store: &Store) -> bool {
    logged_in(store)
        && store
            .setting("server_role")
            .map(|text| text.trim().eq_ignore_ascii_case("admin"))
            .unwrap_or(false)
}

pub fn devices() -> Result<Value, ApiError> {
    let store = crate::get_store();
    let path = format!(
        "/api/v1/devices?device_fingerprint={}",
        device_fingerprint(&store)
    );
    authed(&store, "GET", &path, None)
}

pub fn kick_device(device_id: i64) -> Result<Value, ApiError> {
    let store = crate::get_store();
    authed(
        &store,
        "POST",
        "/api/v1/devices/kick",
        Some(&json!({ "device_id": device_id })),
    )
}

/// 注销账号（真删）：成功后清本地 token
pub fn delete_me() -> Result<Value, ApiError> {
    let store = crate::get_store();
    let value = authed(&store, "DELETE", "/api/v1/me", None)?;
    clear_tokens(&store);
    let _ = store.set_setting("server_username", "");
    let _ = store.set_setting("server_account_id", "");
    crate::notify::kick();
    Ok(value)
}

/// 拉公告（公开接口，不需要 token）。`notify.rs` 轮询时也用它。
///
/// 服务器那边 `since` 只返回 `id > since`（id 倒序、默认上限 50），这里显式要 200，
/// 免得第一次同步时旧公告被 limit 截掉。
pub fn announcements(store: &Store, since: i64) -> Result<Value, ApiError> {
    let url = endpoint(
        store,
        &format!("/api/v1/announcements?since={since}&limit=200"),
    )?;
    request_json("GET", &url, None, None)
}

/// 设置页 / 标题栏要的一份汇总
pub fn state() -> Value {
    let store = crate::get_store();
    let url = server_url(&store);
    json!({
        "server_url": url,
        "default_server": DEFAULT_SERVER,
        "enabled": server_enabled(&store),
        "logged_in": logged_in(&store),
        "username": store.setting("server_username").unwrap_or_default(),
        "role": store.setting("server_role").unwrap_or_default(),
        "is_admin": is_admin(&store),
        "account_id": store.setting("server_account_id").unwrap_or_default(),
        "device_fingerprint": device_fingerprint(&store),
        "token_expires_at": store.setting("server_token_expires_at").unwrap_or_default(),
        "has_refresh_token": !refresh_token(&store).is_empty(),
        "devices_used": number_setting(&store, "server_devices_used"),
        "device_limit": number_setting(&store, "server_device_limit"),
        "last_announcement_id": store.setting("last_announcement_id").unwrap_or_default(),
        "announcements_unread": store.announcement_unread_count(),
        "notify": crate::notify::status(),
    })
}

fn number_setting(store: &Store, key: &str) -> i64 {
    store
        .setting(key)
        .and_then(|text| text.trim().parse::<i64>().ok())
        .unwrap_or(0)
}

/// 保存服务器地址（顺便踢一下后台轮询，让它按新地址重建连接）
pub fn save_server_url(store: &Store, url: &str) -> Value {
    let normalized = url.trim().trim_end_matches('/').to_string();
    let _ = store.set_setting("server_url", &normalized);
    crate::notify::kick();
    json!({ "ok": true, "server_url": normalized })
}

// ---------------------------------------------------------------- 测试

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
    fn server_url_default_empty_and_trailing_slash() {
        let store = temp_store("danlume-account-url-test");
        // 没设过 → 默认服务器
        assert_eq!(server_url(&store), DEFAULT_SERVER);
        assert!(server_enabled(&store));
        // 设成空串 → 明确表示不用服务器
        store.set_setting("server_url", "").unwrap();
        assert_eq!(server_url(&store), "");
        assert!(!server_enabled(&store));
        // 结尾的斜杠会被去掉，避免拼出 //api/v1
        store.set_setting("server_url", "https://example.com/").unwrap();
        assert_eq!(server_url(&store), "https://example.com");
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-account-url-test"));
    }

    #[test]
    fn tokens_are_encrypted_and_cleared() {
        let store = temp_store("danlume-account-token-test");
        assert!(!logged_in(&store));
        set_token(&store, TOKEN_KEY, "access-abc").unwrap();
        set_token(&store, REFRESH_KEY, "refresh-xyz").unwrap();
        // 明文不能出现在 settings；macOS 只应有 Keychain 引用，Windows 使用 DPAPI。
        let raw = store.setting(TOKEN_KEY).unwrap();
        assert!(!raw.contains("access-abc"), "token 不该明文存：{raw}");
        assert!(raw.starts_with("dpapi:") || raw.starts_with("keychain:") || raw.starts_with("b64:") || raw.starts_with("plain:"));
        assert_eq!(token(&store), "access-abc");
        assert_eq!(refresh_token(&store), "refresh-xyz");
        assert!(logged_in(&store));
        clear_tokens(&store);
        assert_eq!(token(&store), "");
        assert!(!logged_in(&store));
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-account-token-test"));
    }

    #[test]
    fn device_fingerprint_is_stable_uuid() {
        let store = temp_store("danlume-account-fp-test");
        let first = device_fingerprint(&store);
        let second = device_fingerprint(&store);
        assert_eq!(first, second, "同一台机器只生成一次");
        assert_eq!(first.len(), 36, "uuid 风格：{first}");
        let parts: Vec<&str> = first.split('-').collect();
        assert_eq!(parts.iter().map(|one| one.len()).collect::<Vec<_>>(), vec![8, 4, 4, 4, 12]);
        assert!(first.chars().all(|one| one.is_ascii_hexdigit() || one == '-'));
        assert_eq!(parts[2].chars().next(), Some('4'), "version 4");
        assert_ne!(first, uuid_v4(), "两次随机结果不该一样");
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-account-fp-test"));
    }

    #[test]
    fn error_classification() {
        // 409 设备超限：设备列表要原样带回
        let limit = ApiError::from_response(
            409,
            r#"{"error":"device_limit","devices":[{"id":3,"name":"旧笔记本","platform":"windows","last_seen_at":"2026-01-01"}],"devices_used":3,"device_limit":3}"#,
        );
        assert_eq!(limit.kind, "device_limit");
        assert_eq!(limit.devices_used, 3);
        assert_eq!(limit.device_limit, 3);
        assert_eq!(limit.devices.len(), 1);
        assert_eq!(limit.devices[0]["name"], "旧笔记本");
        assert!(!limit.offline);

        // 401 凭据错
        let bad = ApiError::from_response(401, r#"{"error":"unauthorized","message":"用户名或密码错误"}"#);
        assert_eq!(bad.kind, "unauthorized");
        assert_eq!(bad.error, "用户名或密码错误");

        // 403（注册关闭）
        let closed = ApiError::from_response(403, r#"{"error":"register_closed","message":"当前未开放注册"}"#);
        assert_eq!(closed.kind, "unauthorized");
        assert_eq!(closed.error, "当前未开放注册");

        // 404 = 服务器没这个接口（老版本服务器没有 /announcements）
        let missing = ApiError::from_response(404, r#"{"error":"not_found","message":"Not Found"}"#);
        assert_eq!(missing.kind, "unsupported");

        // 500 之类
        let boom = ApiError::from_response(500, "oops");
        assert_eq!(boom.kind, "server");
        assert_eq!(boom.http_status, 500);

        // 网络不可达
        let offline = ApiError::offline("连不上服务器：连接被拒绝");
        assert!(offline.is_offline());
        assert!(offline.offline);
        assert!(offline.to_string().contains("连不上"));
    }

    #[test]
    fn email_code_error_classification() {
        // 429 冷却：retry_after 要带出来
        let cooldown = ApiError::from_response(
            429,
            r#"{"error":"email_cooldown","message":"请 45 秒后再试","retry_after":45}"#,
        );
        assert_eq!(cooldown.kind, "cooldown");
        assert_eq!(cooldown.code, "email_cooldown");
        assert_eq!(cooldown.http_status, 429);
        assert_eq!(cooldown.retry_after, 45);
        assert_eq!(cooldown.error, "请 45 秒后再试");
        assert!(!cooldown.offline);

        // 429 频率限制：同一类，也是 cooldown
        let too_many = ApiError::from_response(
            429,
            r#"{"error":"too_many_requests","message":"请求太频繁","retry_after":600}"#,
        );
        assert_eq!(too_many.kind, "cooldown");
        assert_eq!(too_many.code, "too_many_requests");
        assert_eq!(too_many.retry_after, 600);

        // 503 SMTP 没配 / 发信失败：都是「服务器发不出信」
        let no_smtp = ApiError::from_response(
            503,
            r#"{"error":"email_not_configured","message":"服务器没有配置发信邮箱"}"#,
        );
        assert_eq!(no_smtp.kind, "email");
        assert_eq!(no_smtp.code, "email_not_configured");
        assert_eq!(no_smtp.http_status, 503);
        assert_eq!(no_smtp.error, "服务器没有配置发信邮箱");

        let send_failed = ApiError::from_response(
            503,
            r#"{"error":"email_send_failed","message":"发信失败"}"#,
        );
        assert_eq!(send_failed.kind, "email");
        assert_eq!(send_failed.code, "email_send_failed");

        // 400 邮箱不被接受（白名单为空时服务器不会拦，那是服务器的事，这里只管分类）
        let bad_email = ApiError::from_response(
            400,
            r#"{"error":"invalid_email","message":"邮箱格式不对"}"#,
        );
        assert_eq!(bad_email.kind, "invalid_email");
        assert_eq!(bad_email.code, "invalid_email");
        assert_eq!(bad_email.http_status, 400);

        let domain = ApiError::from_response(
            400,
            r#"{"error":"email_domain_not_allowed","message":"只允许复旦邮箱"}"#,
        );
        assert_eq!(domain.kind, "invalid_email");
        assert_eq!(domain.code, "email_domain_not_allowed");

        // 400 验证码错 / 过期：attempts_left 要带出来
        let wrong = ApiError::from_response(
            400,
            r#"{"error":"invalid_code","message":"验证码不对","attempts_left":3}"#,
        );
        assert_eq!(wrong.kind, "invalid_code");
        assert_eq!(wrong.code, "invalid_code");
        assert_eq!(wrong.attempts_left, 3);

        // 服务端 v1.1.0 实际用的是 remaining_attempts，字段名不一样也要认
        let wrong_alt = ApiError::from_response(
            400,
            r#"{"error":"invalid_code","message":"验证码错误","remaining_attempts":4}"#,
        );
        assert_eq!(wrong_alt.kind, "invalid_code");
        assert_eq!(wrong_alt.attempts_left, 4, "remaining_attempts 要映射成 attempts_left");

        let expired = ApiError::from_response(
            400,
            r#"{"error":"code_expired","message":"验证码过期了","attempts_left":0}"#,
        );
        assert_eq!(expired.kind, "invalid_code");
        assert_eq!(expired.code, "code_expired");
        assert_eq!(expired.attempts_left, 0);

        // 没见过的 400：归到 invalid，但别把原来 401/403/404 的分类带歪
        let vague = ApiError::from_response(400, r#"{"error":"bad_request"}"#);
        assert_eq!(vague.kind, "invalid");
        assert_eq!(vague.code, "bad_request");
        assert!(!vague.error.is_empty(), "空 message 要有中文兜底");
        let closed = ApiError::from_response(403, r#"{"error":"register_closed","message":"当前未开放注册"}"#);
        assert_eq!(closed.kind, "unauthorized");
        assert_eq!(closed.code, "register_closed");
    }

    #[test]
    fn register_body_only_carries_present_fields() {
        let store = temp_store("danlume-account-body-test");
        // 有验证码、没邀请码
        let with_code = register_body(&store, "alice", "a@b.com", "secret-1", None, Some(" 123456 "));
        assert_eq!(with_code["code"], "123456");
        assert!(with_code.get("invite_code").is_none(), "空邀请码不该带字段");
        assert_eq!(with_code["username"], "alice");
        assert_eq!(with_code["email"], "a@b.com");
        assert!(!with_code["device_fingerprint"].as_str().unwrap_or_default().is_empty());

        // 有邀请码、验证码是空串（None / 空串都不该带 code 字段）
        let without_code = register_body(&store, "bob", "c@d.com", "secret-2", Some(" INV-1 "), Some(""));
        assert_eq!(without_code["invite_code"], "INV-1");
        assert!(without_code.get("code").is_none(), "空验证码不该带字段");
        assert!(without_code.get("invite_code").is_some());

        // 两个都不带
        let bare = register_body(&store, "carol", "e@f.com", "secret-3", None, None);
        assert!(bare.get("invite_code").is_none());
        assert!(bare.get("code").is_none());
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-account-body-test"));
    }

    #[test]
    fn email_check_is_loose_but_catches_typos() {
        assert!(email_looks_valid("name@example.com"));
        assert!(email_looks_valid("  name@example.com  "), "两边空白先 trim");
        assert!(email_looks_valid("a@localhost"), "不强制域名带点（服务器说了算）");
        assert!(!email_looks_valid(""));
        assert!(!email_looks_valid("   "));
        assert!(!email_looks_valid("no-at-sign"));
        assert!(!email_looks_valid("@example.com"));
        assert!(!email_looks_valid("name@"));
        assert!(!email_looks_valid("a@b@c.com"), "两个 @ 不算邮箱");
        assert!(!email_looks_valid("a b@example.com"), "带空格不算邮箱");
    }

    #[test]
    fn local_validation_short_circuits_before_network() {
        // 邮箱格式不对时直接 config 报错（不该去连服务器）
        let err = request_email_code("   ").expect_err("空邮箱应当本地就拒");
        assert_eq!(err.kind, "config");
        assert_eq!(err.error, "邮箱不能为空");
        let err = request_email_code("not-an-email").expect_err("坏邮箱应当本地就拒");
        assert_eq!(err.kind, "config");
        assert_eq!(err.error, "邮箱格式不对");
    }

    #[test]
    fn endpoint_requires_server_url() {
        let store = temp_store("danlume-account-endpoint-test");
        assert_eq!(
            endpoint(&store, "/api/v1/health").unwrap(),
            format!("{DEFAULT_SERVER}/api/v1/health")
        );
        store.set_setting("server_url", "").unwrap();
        let err = endpoint(&store, "/api/v1/health").unwrap_err();
        assert_eq!(err.kind, "config");
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-account-endpoint-test"));
    }

    /// 真连服务器验 `request-code`（`cargo test live_request_email_code -- --ignored --nocapture`）
    ///
    /// 服务器 SMTP 没配时这里会打印 `email`（`email_not_configured`），那是**预期**结果，
    /// 所以这个测试只打印、不断言成功：它验的是错误码分类真能跑通。
    #[test]
    #[ignore]
    fn live_request_email_code() {
        let store = temp_store("danlume-account-code-live-test");
        store.set_setting("server_url", DEFAULT_SERVER).unwrap();
        println!("== 服务器 = {}", server_url(&crate::get_store()));

        match request_email_code("probe_nobody@example.com") {
            Ok(value) => println!("== POST /register/request-code -> {value}"),
            Err(err) => println!(
                "== POST /register/request-code -> 失败 kind={} code={} http={} retry_after={}：{}",
                err.kind, err.code, err.http_status, err.retry_after, err.error
            ),
        }

        // 带 code 去注册：服务器 register_mode=closed 时应该原样回 register_closed（403 / unauthorized）
        let name = format!("probe_code_{}", &uuid_v4().replace('-', "")[..8]);
        match register(
            &name,
            &format!("{name}@example.com"),
            "Str0ng-Pass-123",
            None,
            Some("000000"),
        ) {
            Ok(value) => println!("== POST /register（带 code）-> {value}"),
            Err(err) => println!(
                "== POST /register（带 code）-> 失败 kind={} code={} http={}：{}",
                err.kind, err.code, err.http_status, err.error
            ),
        }

        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-account-code-live-test"));
    }

    /// 真连服务器的验收测试（`cargo test live_ -- --ignored --nocapture --test-threads=1`）
    ///
    /// 2026-02 实测：服务器 `register_mode=closed`（`/register` 回 403 register_closed），
    /// 所以这里不改注册模式、也不留测试账号：注册分支会自动走「注册关闭」的断言。
    /// health / announcements 是公开接口，可以真验证；错账号登录验证 401 分类。
    #[test]
    #[ignore]
    fn live_probe_against_real_server() {
        let store = temp_store("danlume-account-live-test");
        store.set_setting("server_url", DEFAULT_SERVER).unwrap();
        println!("== 服务器 = {}", server_url(&crate::get_store()));

        let health = health().expect("health 应该成功");
        println!("== GET /health -> {health}");
        assert_eq!(health["online"], true);
        assert!(!health["version"].as_str().unwrap_or_default().is_empty());

        match announcements(&crate::get_store(), 0) {
            Ok(value) => println!("== GET /announcements -> {value}"),
            Err(err) => {
                println!("== GET /announcements -> 失败 kind={} http={} {}", err.kind, err.http_status, err.error);
                assert_eq!(err.kind, "unsupported", "服务器没有公告接口时应当是 unsupported");
            }
        }

        let name = format!("probe_rust_{}", &uuid_v4().replace('-', "")[..8]);
        match register(
            &name,
            &format!("{name}@example.com"),
            "Str0ng-Pass-123",
            None,
            Some("000000"),
        ) {
            Ok(value) => {
                println!("== POST /register -> {value}");
                // 注册成功能登录、能读 /me，最后把测试账号注销
                let session = login(&name, "Str0ng-Pass-123").expect("注册后应当能登录");
                assert_eq!(session["ok"].as_bool(), Some(true));
                println!("== POST /login -> ok");
                let who = me().expect("/me 应该成功");
                println!("== GET /me -> {who}");
                let listed = devices().expect("/devices 应该成功");
                println!("== GET /devices -> {listed}");
                let gone = delete_me().expect("注销测试账号失败");
                println!("== DELETE /me -> {gone}");
            }
            Err(err) => {
                println!("== POST /register -> 失败 kind={} http={} {}", err.kind, err.http_status, err.error);
                assert!(err.http_status == 403 || err.kind == "offline", "注册关闭时应当是 403");
            }
        }

        // 错口令必须归类成 unauthorized（不要被当成服务器故障）
        let bad = login(&format!("probe_missing_{}", &uuid_v4()[..8]), "wrong-password")
            .expect_err("不存在的账号不该登录成功");
        println!("== POST /login（错账号）-> kind={} http={} {}", bad.kind, bad.http_status, bad.error);
        assert_eq!(bad.kind, "unauthorized");

        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-account-live-test"));
    }
}
