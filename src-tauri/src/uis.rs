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

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(20))
        .redirects(5)
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/120.0 Safari/537.36",
        )
        .build()
}

/// 简化 cookie jar：只保存 name=value，不处理 Domain/Path/过期（内部站点够用）
#[derive(Debug, Default, Clone)]
pub struct Jar {
    pairs: Vec<(String, String)>,
}

impl Jar {
    pub fn from_header(text: &str) -> Self {
        let mut jar = Jar::default();
        for part in text.split(';') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if let Some((name, value)) = part.split_once('=') {
                jar.set(name.trim(), value.trim());
            }
        }
        jar
    }

    pub fn set(&mut self, name: &str, value: &str) {
        if let Some(slot) = self.pairs.iter_mut().find(|(key, _)| key == name) {
            slot.1 = value.to_string();
        } else {
            self.pairs.push((name.to_string(), value.to_string()));
        }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    /// 把另一个 jar 并进来（同名以对方为准）。抓会话时按站点分批收集，最后合并成一份。
    pub fn extend_from(&mut self, other: &Jar) {
        for (name, value) in &other.pairs {
            self.set(name, value);
        }
    }

    pub fn count(&self) -> usize {
        self.pairs.len()
    }

    pub fn names(&self) -> Vec<String> {
        self.pairs.iter().map(|(name, _)| name.clone()).collect()
    }

    /// 只读浏览（诊断日志用：打印名字和值的前几位）
    pub fn entries(&self) -> &[(String, String)] {
        &self.pairs
    }

    pub fn to_header(&self) -> String {
        self.pairs
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    pub fn merge_set_cookie(&mut self, header: &str) {
        // 一条 Set-Cookie 里第一个分号之前才是 name=value
        let first = header.split(';').next().unwrap_or("").trim();
        if let Some((name, value)) = first.split_once('=') {
            self.set(name.trim(), value.trim());
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
pub fn save_session(store: &Store, jar: &Jar, username: &str) -> Result<(), String> {
    let payload = json!({
        "cookies": jar.pairs,
        "username": username,
        "at": crate::now_str(),
    });
    let protected = crate::secrets::protect(&payload.to_string())?;
    store
        .set_setting(SESSION_KEY, &protected)
        .map_err(|exc| format!("存会话失败：{exc}"))
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
        let name = row.get(0)?.as_str()?.to_string();
        let value = row.get(1)?.as_str()?.to_string();
        jar.set(&name, &value);
    }
    Some(jar)
}

pub fn clear_session(store: &Store) {
    let _ = store.set_setting(SESSION_KEY, "");
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
        "cookies": jar.as_ref().map(|one| one.pairs.len()).unwrap_or(0),
        "has_castgc": jar.as_ref().and_then(|one| one.get("CASTGC")).is_some(),
        "has_service_session": has_service_session,
        "session_names": names,
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
    let page = agent()
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
    let response = agent()
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
    // 把返回的所有 Set-Cookie 都收下来
    for value in current.all("set-cookie") { jar.merge_set_cookie(value); }
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
        let follow = agent()
            .get(&next_url)
            .set("Cookie", &jar.to_header())
            .call();
        let next = match follow {
            Ok(res) => res,
            Err(ureq::Error::Status(_, res)) => res,
            Err(_) => break,
        };
        for value in next.all("set-cookie") { jar.merge_set_cookie(value); }
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
        "cookies": jar.pairs.len(),
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
    let mut current = agent()
        .get(url)
        .set("Cookie", &jar.to_header())
        .call()
        .map_err(|exc| format!("请求失败：{exc}"))?;
    for value in current.all("set-cookie") {
        jar.merge_set_cookie(value);
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
        let next = match agent()
            .get(&next_url)
            .set("Cookie", &jar.to_header())
            .call()
        {
            Ok(res) => res,
            Err(ureq::Error::Status(_, res)) => res,
            Err(exc) => return Err(format!("跟随跳转失败：{exc}")),
        };
        for value in next.all("set-cookie") {
            jar.merge_set_cookie(value);
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
    fn session_roundtrip_is_encrypted() {
        let store = temp_store("danlume-uis-test");
        let mut jar = Jar::default();
        jar.set("CASTGC", "TGT-xyz");
        jar.set("route", "r1");
        save_session(&store, &jar, "26303050178").unwrap();

        let raw = store.setting(SESSION_KEY).unwrap_or_default();
        println!("存的密文前缀：{}", &raw[..raw.len().min(12)]);
        assert!(raw.starts_with("dpapi:") || raw.starts_with("b64:"), "必须加密后存");
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
