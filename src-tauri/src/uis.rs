//! UIS 统一身份认证（CAS）登录 + 会话存取（对应旧版 `uis_login.py`）。
//!
//! 与旧版的关系：旧版是靠内嵌浏览器登录、再从磁盘读 cookie；v2 直接走 HTTP：
//! 1. GET `https://uis.fudan.edu.cn/authserver/login`（可带 `?service=`）拿隐藏字段 `execution`/`_eventId`/`lt`；
//! 2. POST 学号 + 密码，跟随 302 收 `Set-Cookie`；
//! 3. 会话（cookie 串）用 **DPAPI** 加密后存 settings 的 `uis_session`；
//! 4. 密码只在当次 POST 用，除非用户显式开「记住密码」（`uis_autofill=1`）才 DPAPI 存 `uis_password`。
use serde_json::{json, Value};

use crate::store::Store;

pub const UIS_LOGIN_URL: &str = "https://uis.fudan.edu.cn/authserver/login";
/// Canvas（eLearning）走 SAML，登录时把 service 带上，一次登录同时拿到两个服务的会话
pub const CANVAS_SERVICE: &str = "https%3A%2F%2Felearning.fudan.edu.cn%2Flogin%2Fsaml";
pub const SESSION_KEY: &str = "uis_session";
pub const USER_KEY: &str = "uis_username";
pub const PASS_KEY: &str = "uis_password";
/// 最近一次自检：HTTP 状态 / 时间 / 返回体前 200 字（界面显示「已验证 / Canvas 401」用）
pub const VERIFY_STATUS_KEY: &str = "uis_verify_status";
pub const VERIFY_AT_KEY: &str = "uis_verify_at";
pub const VERIFY_BODY_KEY: &str = "uis_verify_body";

fn agent(url: &str) -> ureq::Agent {
    crate::net::with_proxy(
        ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(20))
            .redirects(5)
            .user_agent(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                 (KHTML, like Gecko) Chrome/120.0 Safari/537.36",
            )
,
        crate::get_store(),
        url,
    )
    .build()
}

/// 一条 cookie：名字、值，以及它属于哪个域。
///
/// `domain` 为 `None` 表示「不知道域」——库里 2026-11-22 之前存的老会话就是这个形状
/// （只有 `[name, value]` 两元组），发送时按「哪个站都带」处理，老行为不变。
#[derive(Debug, Clone)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: Option<String>,
}

/// 简化 cookie jar：按 **(域, 名字)** 存 name=value，不处理 Path / 过期（内部站点够用）。
///
/// 2026-11-22 之前只按名字存：`lib.rs` 的 `collect_cookies()` 把四个校内域读出来
/// 用 `set(name, value)` 互相覆盖，`to_header()` 又把所有域的值发给每个站。
/// 后果是「登录前那份游客 `_normandy_session` 一旦先收进来，后面就再也换不掉」——
/// 这是作业同步 401 的一半原因（另一半是登录完成判定，见 `lib.rs` 的 `run_cookie_tour`）。
#[derive(Debug, Default, Clone)]
pub struct Jar {
    items: Vec<Cookie>,
}

/// 域归一化：去端口、去前导点、转小写（`".Fudan.edu.cn:443"` → `"fudan.edu.cn"`）
fn norm_host(raw: &str) -> String {
    raw.trim()
        .trim_start_matches('.')
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// 从 URL 里抠主机名（抠不出来就是空串）
fn host_of(url: &str) -> String {
    url.split("//")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .map(norm_host)
        .unwrap_or_default()
}

/// 这条 cookie 能不能发给 `host`：域未知（老会话）就都发，否则按「同域或父域」判断
fn domain_matches(domain: Option<&str>, host: &str) -> bool {
    match domain {
        None => true,
        Some(one) => {
            let one = norm_host(one);
            !one.is_empty() && (host == one || host.ends_with(&format!(".{one}")))
        }
    }
}

/// 域名越长说明越具体，同名冲突时用它决定留哪条（域未知记 0）
fn domain_weight(cookie: &Cookie) -> usize {
    cookie
        .domain
        .as_deref()
        .map(|one| norm_host(one).len())
        .unwrap_or(0)
}

impl Jar {
    pub fn from_header(text: &str) -> Self {
        let mut jar = Jar::default();
        jar.merge_from_header(None, text);
        jar
    }

    /// 从 `a=1; b=2` 这样的串里收 cookie；给了域就按域存
    pub fn merge_from_header(&mut self, domain: Option<&str>, text: &str) {
        for part in text.split(';') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if let Some((name, value)) = part.split_once('=') {
                match domain {
                    Some(host) => self.set_for(host, name.trim(), value.trim()),
                    None => self.set(name.trim(), value.trim()),
                }
            }
        }
    }

    /// 按名字存（域未知，老行为）
    pub fn set(&mut self, name: &str, value: &str) {
        self.set_domain(None, name, value);
    }

    /// 按域存：同一个域里同名覆盖，不同域互不干扰
    pub fn set_for(&mut self, domain: &str, name: &str, value: &str) {
        let host = norm_host(domain);
        if host.is_empty() {
            self.set(name, value);
            return;
        }
        self.set_domain(Some(host.as_str()), name, value);
    }

    fn set_domain(&mut self, domain: Option<&str>, name: &str, value: &str) {
        if let Some(slot) = self
            .items
            .iter_mut()
            .find(|one| one.name == name && one.domain.as_deref() == domain)
        {
            slot.value = value.to_string();
        } else {
            self.items.push(Cookie {
                name: name.to_string(),
                value: value.to_string(),
                domain: domain.map(|one| one.to_string()),
            });
        }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.items
            .iter()
            .find(|one| one.name == name)
            .map(|one| one.value.as_str())
    }

    /// 只在这个域下找（诊断用）
    pub fn get_for(&self, host: &str, name: &str) -> Option<&str> {
        let host = norm_host(host);
        self.items
            .iter()
            .find(|one| one.name == name && domain_matches(one.domain.as_deref(), &host))
            .map(|one| one.value.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 把另一个 jar 并进来（同域同名以对方为准）。抓会话时按站点分批收集，最后合并成一份。
    pub fn extend_from(&mut self, other: &Jar) {
        for one in &other.items {
            self.set_domain(one.domain.as_deref(), &one.name, &one.value);
        }
    }

    pub fn count(&self) -> usize {
        self.items.len()
    }

    pub fn names(&self) -> Vec<String> {
        self.items.iter().map(|one| one.name.clone()).collect()
    }

    /// jar 里出现过哪些域（诊断用；老会话是 `(未知)`）
    pub fn domains(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for one in &self.items {
            let key = one
                .domain
                .clone()
                .unwrap_or_else(|| "(未知)".to_string());
            if !out.contains(&key) {
                out.push(key);
            }
        }
        out
    }

    /// 只读浏览（诊断日志用：打印名字和值的前几位）
    pub fn entries(&self) -> Vec<(String, String)> {
        self.items
            .iter()
            .map(|one| (one.name.clone(), one.value.clone()))
            .collect()
    }

    /// 全都发（老接口；域已知时请用 `to_header_for`）
    pub fn to_header(&self) -> String {
        self.items
            .iter()
            .map(|one| format!("{}={}", one.name, one.value))
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// 只把这个域能用得上的 cookie 发出去。
    ///
    /// 同名冲突时取「域更长（更具体）」的那条 —— 浏览器的行为也是这样：
    /// `elearning.fudan.edu.cn` 上那份 `_normandy_session` 优先于 `.fudan.edu.cn` 上的。
    pub fn to_header_for(&self, host: &str) -> String {
        let host = norm_host(host);
        let mut chosen: Vec<&Cookie> = Vec::new();
        for one in &self.items {
            if !domain_matches(one.domain.as_deref(), &host) {
                continue;
            }
            let at = chosen.iter().position(|item| item.name == one.name);
            match at {
                Some(at) => {
                    if domain_weight(one) >= domain_weight(chosen[at]) {
                        chosen[at] = one;
                    }
                }
                None => chosen.push(one),
            }
        }
        chosen
            .iter()
            .map(|one| format!("{}={}", one.name, one.value))
            .collect::<Vec<_>>()
            .join("; ")
    }

    pub fn merge_set_cookie(&mut self, header: &str) {
        self.merge_set_cookie_for_host(None, header);
    }

    /// 从某个域回来的 `Set-Cookie`（推荐这条，能把域记下来）
    pub fn merge_set_cookie_for(&mut self, host: &str, header: &str) {
        self.merge_set_cookie_for_host(Some(host), header);
    }

    fn merge_set_cookie_for_host(&mut self, host: Option<&str>, header: &str) {
        // 一条 Set-Cookie 里第一个分号之前才是 name=value
        let first = header.split(';').next().unwrap_or("").trim();
        if let Some((name, value)) = first.split_once('=') {
            match host {
                Some(one) => self.set_for(one, name.trim(), value.trim()),
                None => self.set(name.trim(), value.trim()),
            }
        }
    }
}

/// 从登录页 HTML 里抠出表单要提交的隐藏字段
pub fn parse_login_form(html: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let lower = html.to_lowercase();
    let mut cursor = 0usize;
    while let Some(found) = lower[cursor..].find("<input") {
        let start = cursor + found;
        let Some(end_rel) = lower[start..].find('>') else {
            break;
        };
        let tag = &html[start..start + end_rel];
        cursor = start + end_rel;
        let attr = |name: &str| -> Option<String> {
            let lowered = tag.to_lowercase();
            let at = lowered.find(&format!("{name}="))?;
            let rest = &tag[at + name.len() + 1..];
            let quote = rest.chars().next()?;
            if quote == '"' || quote == '\'' {
                let body = &rest[1..];
                let close = body.find(quote)?;
                Some(body[..close].to_string())
            } else {
                let close = rest
                    .find(|ch: char| ch.is_whitespace() || ch == '>')
                    .unwrap_or(rest.len());
                Some(rest[..close].to_string())
            }
        };
        let kind = attr("type").unwrap_or_default().to_lowercase();
        if kind == "submit" || kind == "button" {
            continue;
        }
        if let Some(name) = attr("name") {
            let value = attr("value").unwrap_or_default();
            if let Some(slot) = out.iter_mut().find(|(key, _)| key == &name) {
                slot.1 = value;
            } else {
                out.push((name, value));
            }
        }
    }
    out
}

// ------------------------------------------------------------------ 会话存取
/// 把会话写进库（DPAPI 加密）。
///
/// `verify_status` = 落库前那次自检的 HTTP 状态（`Some(200)` = 真的能用）。
/// **只有 200 才该带 `Some(200)`**：401 的会话存下来等于把能用的旧会话覆盖掉。
pub fn save_session_with(
    store: &Store,
    jar: &Jar,
    username: &str,
    verify_status: Option<u16>,
) -> Result<(), String> {
    // 新格式：`[[名字, 值, 域], ...]`。第三项空串 = 域未知（`load_session` 认这个）
    let rows: Vec<Value> = jar
        .items
        .iter()
        .map(|one| {
            json!([
                one.name,
                one.value,
                one.domain.clone().unwrap_or_default()
            ])
        })
        .collect();
    let mut payload = json!({
        "cookies": rows,
        "username": username,
        "at": crate::now_str(),
        "format": 2,
    });
    if let Some(status) = verify_status {
        payload["verify"] = json!({ "status": status, "at": crate::now_str() });
    }
    let protected = crate::secrets::protect(&payload.to_string())?;
    store
        .set_setting(SESSION_KEY, &protected)
        .map_err(|exc| format!("存会话失败：{exc}"))?;
    if let Some(status) = verify_status {
        record_verdict(store, status, "");
    }
    Ok(())
}

pub fn save_session(store: &Store, jar: &Jar, username: &str) -> Result<(), String> {
    save_session_with(store, jar, username, None)
}

pub fn load_session(store: &Store) -> Option<Jar> {
    let encrypted = store.setting(SESSION_KEY)?;
    if encrypted.trim().is_empty() {
        return None;
    }
    let raw = crate::secrets::unprotect(&encrypted).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    let mut jar = Jar::default();
    for row in value.get("cookies")?.as_array()? {
        // 新格式 `[名字, 值, 域]`；老格式 `[名字, 值]`（第三项缺省 → 域未知）
        let name = row.get(0)?.as_str()?.to_string();
        let value = row.get(1)?.as_str()?.to_string();
        let domain = row
            .get(2)
            .and_then(|one| one.as_str())
            .map(|one| one.trim().to_string())
            .filter(|one| !one.is_empty());
        jar.set_domain(domain.as_deref(), &name, &value);
    }
    Some(jar)
}

pub fn clear_session(store: &Store) {
    let _ = store.set_setting(SESSION_KEY, "");
    let _ = store.set_setting(VERIFY_STATUS_KEY, "0");
    let _ = store.set_setting(VERIFY_AT_KEY, "");
    let _ = store.set_setting(VERIFY_BODY_KEY, "");
}

/// 最近一次「拿这份会话打一次 Canvas」的自检结果。`status == 0` 表示没验证过。
///
/// 界面上的「已验证 / Canvas 401」读的就是它 —— 光看 `has_session` 会把
/// 登录前的游客会话也显示成成功（2026-11-22 的 401 就是这么藏住的）。
pub fn verdict(store: &Store) -> Value {
    let status = store
        .setting(VERIFY_STATUS_KEY)
        .and_then(|text| text.trim().parse::<i64>().ok())
        .unwrap_or(0);
    let body = store.setting(VERIFY_BODY_KEY).unwrap_or_default();
    json!({
        "status": status,
        "at": store.setting(VERIFY_AT_KEY).unwrap_or_default(),
        "body_head": body,
    })
}

/// 记下这次自检的结果（200 = 真的能用）
pub fn record_verdict(store: &Store, status: u16, body_head: &str) {
    let _ = store.set_setting(VERIFY_STATUS_KEY, &status.to_string());
    let _ = store.set_setting(VERIFY_AT_KEY, &crate::now_str());
    // 空串也要写：200 之后不该还留着上一次 401 的返回体
    let head: String = body_head.chars().take(200).collect();
    let _ = store.set_setting(VERIFY_BODY_KEY, &head);
}

pub fn status(store: &Store) -> Value {
    let jar = load_session(store);
    let names: Vec<String> = jar.as_ref().map(|one| one.names()).unwrap_or_default();
    // 新 ID 系统（id.fudan.edu.cn）不发 CASTGC；会话有没有用要看业务站的 cookie
    let has_service_session = names.iter().any(|name| {
        matches!(
            name.as_str(),
            "CASTGC"
                | "_normandy_session"
                | "_legacy_normandy_session"
                | "log_session_id"
                | "SESS_ID"
        )
    });
    json!({
        "username": store.setting(USER_KEY).unwrap_or_default(),
        "has_session": jar.is_some(),
        "cookies": jar.as_ref().map(|one| one.count()).unwrap_or(0),
        "has_castgc": jar.as_ref().and_then(|one| one.get("CASTGC")).is_some(),
        "has_service_session": has_service_session,
        "session_names": names,
        // cookie 分在哪些域下（老会话是 `(未知)`）
        "cookie_domains": jar.as_ref().map(|one| one.domains()).unwrap_or_default(),
        // 最近一次「拿这份会话打 Canvas」的结果：0 = 没验过，200 = 已验证，401 = Canvas 401
        "verify": verdict(store),
        "remember_password": store
            .setting("uis_autofill")
            .map(|text| text.trim() != "0")
            .unwrap_or(true),
        "has_saved_password": store
            .setting(PASS_KEY)
            .map(|text| !text.trim().is_empty())
            .unwrap_or(false),
        // 会话是什么时候抓的（界面显示「抓取时间」用；解不开就当空）
        "saved_at": session_saved_at(store),
    })
}

/// 会话里记的抓取时间（`save_session` 写进去的 `at`）；没有会话时返回空串。
pub fn session_saved_at(store: &Store) -> String {
    let Some(encrypted) = store.setting(SESSION_KEY) else {
        return String::new();
    };
    if encrypted.trim().is_empty() {
        return String::new();
    }
    let Ok(raw) = crate::secrets::unprotect(&encrypted) else {
        return String::new();
    };
    serde_json::from_str::<Value>(&raw)
        .ok()
        .and_then(|value| value.get("at").and_then(|one| one.as_str()).map(|one| one.to_string()))
        .unwrap_or_default()
}

/// 密码：默认不落库；只有用户开了 uis_autofill 才 DPAPI 保存
pub fn remember_password(store: &Store, password: &str) -> Result<(), String> {
    let on = store
        .setting("uis_autofill")
        .map(|text| text.trim() != "0")
        .unwrap_or(true);
    if !on || password.is_empty() {
        return Ok(());
    }
    let protected = crate::secrets::protect(password)?;
    store
        .set_setting(PASS_KEY, &protected)
        .map_err(|exc| format!("存密码失败：{exc}"))
}

pub fn saved_password(store: &Store) -> String {
    store
        .setting(PASS_KEY)
        .filter(|text| !text.trim().is_empty())
        .and_then(|text| crate::secrets::unprotect(&text).ok())
        .unwrap_or_default()
}

/// 登录：GET 登录页 → 取隐藏字段 → POST → 收 cookie
pub fn login(store: &Store, username: &str, password: &str) -> Result<Value, String> {
    let username = username.trim();
    if username.is_empty() || password.is_empty() {
        return Err("要填学号和密码".to_string());
    }
    let page_url = format!("{UIS_LOGIN_URL}?service={CANVAS_SERVICE}");
    let page = agent(&page_url)
        .get(&page_url)
        .call()
        .map_err(|exc| format!("打不开登录页：{exc}"))?
        .into_string()
        .map_err(|exc| format!("登录页读不出来：{exc}"))?;
    let fields = parse_login_form(&page);
    if fields.is_empty() {
        return Err("登录页结构变了（一个隐藏字段都没找到）".to_string());
    }
    let mut jar = Jar::default();
    let form: String = fields
        .iter()
        .filter(|(name, _)| name != "username" && name != "password" && name != "submit")
        .map(|(name, value)| {
            format!(
                "{}={}",
                urlencode(name),
                urlencode(if value.is_empty() { "submit" } else { value })
            )
        })
        .chain(vec![
            format!("username={}", urlencode(username)),
            format!("password={}", urlencode(password)),
            "submit=LOGIN".to_string(),
        ])
        .collect::<Vec<_>>()
        .join("&");
    let response = agent(&page_url)
        .post(&page_url)
        .set("Content-Type", "application/x-www-form-urlencoded")
        .send_string(&form);
    let mut redirects = 0;
    let mut current = match response {
        Ok(res) => res,
        Err(ureq::Error::Status(code, res)) => {
            return Err(format!("登录请求被拒绝（HTTP {code}）"));
        }
        Err(exc) => return Err(format!("登录请求失败：{exc}")),
    };
    // 把返回的所有 Set-Cookie 都收下来（按域记，避免各站同名互相覆盖）
    for value in current.all("set-cookie") { jar.merge_set_cookie_for("uis.fudan.edu.cn", value); }
    // 有些 CAS 会在 302 链上分段种 cookie：手动跟几跳
    while redirects < 5 {
        let Some(location) = current.header("location").map(|v| v.to_string()) else {
            break;
        };
        let next_url = if location.starts_with("http") {
            location
        } else if location.starts_with('/') {
            format!("https://uis.fudan.edu.cn{location}")
        } else {
            break;
        };
        let hop_host = host_of(&next_url);
        let follow = agent(&next_url)
            .get(&next_url)
            .set("Cookie", &jar.to_header())
            .call();
        let next = match follow {
            Ok(res) => res,
            Err(ureq::Error::Status(_, res)) => res,
            Err(_) => break,
        };
        for value in next.all("set-cookie") {
            jar.merge_set_cookie_for(&hop_host, value);
        }
        current = next;
        redirects += 1;
    }
    if jar.is_empty() {
        return Err("登录没有拿到任何 cookie：多半是账号密码不对，或者需要验证码".to_string());
    }
    save_session(store, &jar, username)?;
    let _ = store.set_setting(USER_KEY, username);
    remember_password(store, password)?;
    Ok(json!({
        "ok": true,
        "username": username,
        "cookies": jar.count(),
        "has_castgc": jar.get("CASTGC").is_some(),
        "redirects": redirects,
    }))
}

fn urlencode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

// ------------------------------------------------------------------ 第二跳：ID 会话 → 服务会话
/// 带着已存会话请求目标站点，手动跟随重定向并收集新 cookie。
///
/// 为什么需要这一跳：教务 / Canvas / eHall 是各自独立的服务，登录 ID 系统后还要按
/// `entityId` 换一次服务票据。实测未登录访问教务会被重定向到
/// `id.fudan.edu.cn/ac/#/index?lck=context_CAS_..&entityId=https://fdjwgl.fudan.edu.cn`。
pub fn visit_with_session(store: &Store, url: &str) -> Result<(String, Jar), String> {
    let session = load_session(store)
        .ok_or_else(|| "还没有登录会话：先点「打开登录窗口」再「抓取会话」".to_string())?;
    let mut jar = session.clone();
    let first_host = host_of(url);
    let mut current = agent(url)
        .get(url)
        .set("Cookie", &jar.to_header_for(&first_host))
        .call()
        .map_err(|exc| format!("请求失败：{exc}"))?;
    for value in current.all("set-cookie") {
        jar.merge_set_cookie_for(&first_host, value);
    }
    let mut hops = 0;
    let mut final_url = current.get_url().to_string();
    while hops < 6 {
        let Some(location) = current.header("location").map(|one| one.to_string()) else {
            break;
        };
        let next_url = if location.starts_with("http") {
            location
        } else if location.starts_with('/') {
            let host = final_url
                .split("//")
                .nth(1)
                .and_then(|rest| rest.split('/').next())
                .unwrap_or("");
            format!("https://{host}{location}")
        } else {
            break;
        };
        let hop_host = host_of(&next_url);
        let next = match agent(url)
            .get(&next_url)
            .set("Cookie", &jar.to_header_for(&hop_host))
            .call()
        {
            Ok(res) => res,
            Err(ureq::Error::Status(_, res)) => res,
            Err(exc) => return Err(format!("跟随跳转失败：{exc}")),
        };
        for value in next.all("set-cookie") {
            jar.merge_set_cookie_for(&hop_host, value);
        }
        final_url = next.get_url().to_string();
        current = next;
        hops += 1;
    }
    Ok((final_url, jar))
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

    /// 真联网：用库里已存的学号 + DPAPI 密码跑一次真实 CAS 登录
    /// 跑法：cargo test --lib -- --ignored --nocapture uis_real_login
    #[test]
    #[ignore]
    fn uis_real_login() {
        let store = crate::testdata::real_store("uis");
        let user = store.setting(USER_KEY).unwrap_or_default();
        let pwd = saved_password(&store);
        println!("学号 = {user}，密码长度 = {}", pwd.len());
        if user.is_empty() || pwd.is_empty() {
            println!("缺少学号或密码，跳过真实登录");
            return;
        }
        match login(&store, &user, &pwd) {
            Ok(value) => println!("登录成功：{value}"),
            Err(exc) => println!("登录失败：{exc}"),
        }
        println!("登录后状态：{}", status(&store));
    }

    const SAMPLE: &str = r#"
      <form id="cas" method="post">
        <input type="hidden" name="execution" value="e1s1" />
        <input type="hidden" name="_eventId" value="submit" />
        <input type="hidden" name="lt" value="LT-123-abc" />
        <input type="text" name="username" />
        <input type="password" name="password" />
        <input type="submit" name="submit" value="登录" />
      </form>
    "#;

    #[test]
    fn parses_hidden_fields() {
        let fields = parse_login_form(SAMPLE);
        println!("字段：{fields:?}");
        assert!(fields.iter().any(|(k, v)| k == "execution" && v == "e1s1"));
        assert!(fields.iter().any(|(k, v)| k == "lt" && v == "LT-123-abc"));
        assert!(fields.iter().any(|(k, _)| k == "username"));
        assert!(!fields.iter().any(|(k, _)| k == "submit"), "提交按钮不该当字段");
    }

    #[test]
    fn jar_merges_set_cookie() {
        let mut jar = Jar::default();
        jar.merge_set_cookie("CASTGC=TGT-1; Path=/; Secure; HttpOnly");
        jar.merge_set_cookie("JSESSIONID=abc; Path=/authserver");
        println!("jar = {}", jar.to_header());
        assert_eq!(jar.get("CASTGC"), Some("TGT-1"));
        assert_eq!(jar.get("JSESSIONID"), Some("abc"));
        assert!(jar.to_header().contains("CASTGC=TGT-1; JSESSIONID=abc"));
    }

    #[test]
    fn to_header_for_filters_by_domain() {
        let mut jar = Jar::default();
        jar.set_for("id.fudan.edu.cn", "CASTGC", "TGT-1");
        jar.set_for("elearning.fudan.edu.cn", "_normandy_session", "canvas-login");
        jar.set_for("ehall.fudan.edu.cn", "SESS_ID", "ehall-1");
        jar.set("legacy", "no-domain");

        let canvas = jar.to_header_for("elearning.fudan.edu.cn");
        println!("发给 Canvas 的：{canvas}");
        assert!(canvas.contains("_normandy_session=canvas-login"));
        assert!(
            canvas.contains("legacy=no-domain"),
            "域未知的老 cookie 要继续发，否则老会话会立刻失效"
        );
        assert!(!canvas.contains("CASTGC"), "别的域的票不该发给 Canvas");
        assert!(!canvas.contains("SESS_ID"));

        // 父域下发的 cookie 要发给子域
        let mut parent = Jar::default();
        parent.set_for(".fudan.edu.cn", "shared", "yes");
        assert!(parent
            .to_header_for("elearning.fudan.edu.cn")
            .contains("shared=yes"));
    }

    #[test]
    fn same_name_in_two_domains_does_not_overwrite() {
        let mut jar = Jar::default();
        jar.set_for("elearning.fudan.edu.cn", "_normandy_session", "guest");
        jar.set_for("fdjwgl.fudan.edu.cn", "_normandy_session", "jwgl");
        // 登录后 Canvas 重发了一次：只替换它自己那一份
        jar.set_for("elearning.fudan.edu.cn", "_normandy_session", "logged-in");
        println!("domains = {:?}", jar.domains());
        assert_eq!(
            jar.get_for("elearning.fudan.edu.cn", "_normandy_session"),
            Some("logged-in")
        );
        assert_eq!(
            jar.get_for("fdjwgl.fudan.edu.cn", "_normandy_session"),
            Some("jwgl")
        );
        assert_eq!(jar.count(), 2, "同名不同域是两条，不能互相覆盖");
    }

    #[test]
    fn load_session_reads_old_two_tuple_format() {
        // 库里现存的是 2026-11-22 之前存的老格式：`cookies: [[名字, 值], …]`，没有域
        let store = temp_store("danlume-uis-old-format");
        let old = json!({
            "cookies": [
                ["CASTGC", "TGT-old"],
                ["_normandy_session", "guest-value"]
            ],
            "username": "26303050173",
            "at": "2026-11-17 11:23"
        });
        let protected = crate::secrets::protect(&old.to_string()).unwrap();
        store.set_setting(SESSION_KEY, &protected).unwrap();

        let jar = load_session(&store).expect("老格式要能读出来");
        assert_eq!(jar.get("CASTGC"), Some("TGT-old"));
        assert_eq!(jar.get("_normandy_session"), Some("guest-value"));
        // 老格式没有域 → 哪个站都发（老行为不变，不至于把用户的老会话弄失效）
        assert!(jar
            .to_header_for("elearning.fudan.edu.cn")
            .contains("CASTGC=TGT-old"));
        assert_eq!(session_saved_at(&store), "2026-11-17 11:23");
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-uis-old-format"));
    }

    #[test]
    fn verified_session_records_verdict() {
        let store = temp_store("danlume-uis-verdict");
        let mut jar = Jar::default();
        jar.set_for("elearning.fudan.edu.cn", "_normandy_session", "s1");
        save_session_with(&store, &jar, "26303050173", Some(200)).unwrap();
        let state = verdict(&store);
        println!("verdict = {state}");
        assert_eq!(state["status"], json!(200));
        clear_session(&store);
        assert_eq!(verdict(&store)["status"], json!(0), "退出 UIS 后不该还显示已验证");
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-uis-verdict"));
    }

    #[test]
    fn session_roundtrip_is_encrypted() {
        let store = temp_store("danlume-uis-test");
        let mut jar = Jar::default();
        jar.set("CASTGC", "TGT-xyz");
        jar.set("route", "r1");
        save_session(&store, &jar, "26303050178").unwrap();

        let raw = store.setting(SESSION_KEY).unwrap_or_default();
        println!("存的密文前缀：{}", &raw[..raw.len().min(12)]);
        // macOS 只保存 Keychain 引用；Windows 为 DPAPI，均不可从数据库直接还原。
        assert!(raw.starts_with("dpapi:") || raw.starts_with("keychain:") || raw.starts_with("b64:"), "必须加密后存");
        assert!(!raw.contains("TGT-xyz"), "明文不能出现在库里");

        let back = load_session(&store).expect("会话读不出来");
        assert_eq!(back.get("CASTGC"), Some("TGT-xyz"));
        let state = status(&store);
        println!("状态：{state}");
        assert_eq!(state["has_castgc"], true);

        clear_session(&store);
        assert!(load_session(&store).is_none());
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-uis-test"));
    }
}
