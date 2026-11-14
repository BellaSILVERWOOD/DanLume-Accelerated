//! 代理 / VPN：让后台请求和内嵌网页都真的走代理（对应旧版 `vpn.py`）。
//!
//! 旧版分两层：
//! 1. Qt 层 —— `QNetworkProxy.setApplicationProxy` 全局生效，内嵌网页跟着走；
//! 2. requests 层 —— 每次请求带 `proxies` 字典。
//!
//! Rust 这边一一对应：
//! 1. **后台请求**：所有 `ureq` 都从 `net::agent(store, url)` 拿 Agent，
//!    代理开没开、这个域名要不要绕过，都在这里判断（以前 20 处都是裸 `AgentBuilder::new()`，
//!    设置页填了代理也**根本没人读**，这是 J2-3 的由来）；
//! 2. **内嵌网页**：WebView2 认 `--proxy-server` / `--proxy-bypass-list`，
//!    启动时写进 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` 全局生效；代码里新建的 Webview
//!    再各自 `additional_browser_args()` 一次（改设置后这些窗口立刻跟着走，主窗口要重启）。
//!
//! 设置键（与旧版一致，另加一个 `proxy_bypass`）：`proxy_enabled`、`proxy_kind`
//! （http / socks5）、`proxy_host`、`proxy_port`、`proxy_user`、`proxy_password`（DPAPI 密文）、
//! `proxy_bypass`（逗号/空格分隔的域名后缀，支持 `*.` 前缀）。
use serde_json::{json, Value};

use crate::secrets;
use crate::store::Store;

/// 后台请求的默认超时（秒）；个别慢接口自己传更长的
const DEFAULT_TIMEOUT_SECS: u64 = 20;

#[derive(Debug, Clone, Default)]
pub struct ProxyCfg {
    pub enabled: bool,
    /// "http" 或 "socks5"
    pub kind: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    /// 不走代理的域名（后缀匹配）
    pub bypass: Vec<String>,
}

fn setting(store: &Store, key: &str) -> String {
    store
        .setting(key)
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// 读一遍设置（每次请求现读：改完设置立刻生效，不用重启后台部分）
pub fn config(store: &Store) -> ProxyCfg {
    let port = setting(store, "proxy_port")
        .parse::<f64>()
        .map(|value| value as u16)
        .ok()
        .filter(|value| *value > 0)
        .unwrap_or(8080);
    let password_raw = setting(store, "proxy_password");
    let password = if password_raw.is_empty() {
        String::new()
    } else {
        secrets::unprotect(&password_raw).unwrap_or_default()
    };
    ProxyCfg {
        enabled: setting(store, "proxy_enabled") == "1",
        kind: {
            let kind = setting(store, "proxy_kind").to_lowercase();
            if kind == "socks5" {
                "socks5".to_string()
            } else {
                "http".to_string()
            }
        },
        host: setting(store, "proxy_host"),
        port,
        user: setting(store, "proxy_user"),
        password,
        bypass: parse_bypass(&setting(store, "proxy_bypass")),
    }
}

/// `fudan.edu.cn, localhost *.fudan.edu.cn` → ["fudan.edu.cn", "localhost", "*.fudan.edu.cn"]
fn parse_bypass(raw: &str) -> Vec<String> {
    raw.replace(',', " ")
        .replace('，', " ")
        .split_whitespace()
        .map(|one| one.trim().trim_start_matches("@").to_lowercase())
        .filter(|one| !one.is_empty())
        .collect()
}

/// 这个主机要不要绕过代理（纯函数，方便单测）
///
/// 规则和旧版/常见约定一致：
/// * `localhost` / `127.0.0.1` / `[::1]` 永远直连；
/// * 后缀匹配：清单里写 `fudan.edu.cn` 时 `elearning.fudan.edu.cn` 也直连；
/// * 支持 `*.fudan.edu.cn` 这种写法（等价于后缀匹配）。
pub fn host_bypassed(host: &str, bypass: &[String]) -> bool {
    let host = host.trim().to_lowercase();
    if host.is_empty() {
        return true;
    }
    if host == "localhost" || host == "127.0.0.1" || host == "::1" || host == "[::1]" {
        return true;
    }
    bypass.iter().any(|rule| {
        let rule = rule.trim_start_matches("*.").trim_start_matches('.');
        !rule.is_empty()
            && (host == rule || host.ends_with(&format!(".{rule}")))
    })
}

/// 代理地址字符串（`http://user:pass@host:port`）；没启用作废
pub fn proxy_url(store: &Store) -> Option<String> {
    let cfg = config(store);
    proxy_url_of(&cfg)
}

fn proxy_url_of(cfg: &ProxyCfg) -> Option<String> {
    if !cfg.enabled || cfg.host.trim().is_empty() {
        return None;
    }
    let scheme = if cfg.kind == "socks5" { "socks5" } else { "http" };
    let host = cfg.host.trim();
    // IPv6 字面量要加方括号
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    let auth = if cfg.user.trim().is_empty() {
        String::new()
    } else if cfg.password.is_empty() {
        format!("{}@", cfg.user.trim())
    } else {
        format!("{}:{}@", cfg.user.trim(), cfg.password)
    };
    Some(format!("{scheme}://{auth}{host}:{}", cfg.port))
}

/// 从 URL 里取主机名（小写，去掉端口）
pub fn host_of(url: &str) -> String {
    let without_scheme = match url.split_once("://") {
        Some((_scheme, rest)) => rest,
        None => url,
    };
    let host_part = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .rsplit('@')
        .next()
        .unwrap_or("");
    // [::1]:8080 / host:443
    if let Some(rest) = host_part.strip_prefix('[') {
        return rest.split(']').next().unwrap_or("").to_lowercase();
    }
    host_part
        .split(':')
        .next()
        .unwrap_or("")
        .to_lowercase()
}

/// 给**已有**的 builder 挂上代理（按 URL 判断要不要绕过），调用方自己保留 UA / 超时 / 重定向设置
///
/// 各模块原来的写法都是 `ureq::AgentBuilder::new().timeout(..).user_agent(..).build()`，
/// 用这个包一层就最小改动地接上代理，不必把每个请求都改写成 `net::agent()`。
pub fn with_proxy(
    builder: ureq::AgentBuilder,
    store: &Store,
    url: &str,
) -> ureq::AgentBuilder {
    let cfg = config(store);
    if host_bypassed(&host_of(url), &cfg.bypass) {
        return builder;
    }
    let Some(proxy) = proxy_url_of(&cfg) else {
        return builder;
    };
    match ureq::Proxy::new(&proxy) {
        Ok(parsed) => builder.proxy(parsed),
        Err(exc) => {
            crate::uis_log(&format!("[net] 代理地址有问题，这次直连：{exc}"));
            builder
        }
    }
}

/// 给一批 `ureq` 请求用的 Agent（按目标地址决定要不要走代理）
pub fn agent(store: &Store, url: &str) -> ureq::Agent {
    agent_with_timeout(store, url, DEFAULT_TIMEOUT_SECS)
}

/// 同上，自定超时
pub fn agent_with_timeout(store: &Store, url: &str, timeout_secs: u64) -> ureq::Agent {
    let builder = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(timeout_secs.max(1)));
    with_proxy(builder, store, url).build()
}

/// 内嵌网页（WebView2）要的启动参数；没启用代理就返回空
pub fn webview_args(store: &Store) -> Vec<String> {
    let cfg = config(store);
    let Some(url) = proxy_url_of(&cfg) else {
        return Vec::new();
    };
    // Chromium 不接受 URL 里的账号密码：要认证的代理会由系统弹认证框
    let server = if cfg.user.is_empty() {
        url
    } else {
        let scheme = if cfg.kind == "socks5" { "socks5" } else { "http" };
        format!("{scheme}://{}:{}", cfg.host.trim(), cfg.port)
    };
    let mut bypass: Vec<String> = vec!["<local>".to_string()];
    bypass.extend(cfg.bypass.iter().map(|rule| {
        if rule.starts_with("*.") {
            rule.clone()
        } else {
            format!("*.{rule}")
        }
    }));
    vec![
        format!("--proxy-server={server}"),
        format!("--proxy-bypass-list={}", bypass.join(";")),
    ]
}

/// 给 WebView2 用的参数字符串（没启用代理就是 None）
///
/// 进程启动时写进 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`，
/// 之后创建的每个 webview 都会继承（改设置后重启生效）。
pub fn webview_args_string(store: &Store) -> Option<String> {
    let args = webview_args(store);
    if args.is_empty() {
        None
    } else {
        Some(args.join(" "))
    }
}

/// 一句话说清现在走什么（设置页那行小字）
pub fn summary(store: &Store) -> String {
    let cfg = config(store);
    if !cfg.enabled {
        return "当前是直连（没有启用代理）".to_string();
    }
    if cfg.host.trim().is_empty() {
        return "启用了代理，但没填地址（等于直连）".to_string();
    }
    let kind = if cfg.kind == "socks5" { "SOCKS5" } else { "HTTP" };
    let bypass = if cfg.bypass.is_empty() {
        String::new()
    } else {
        format!("；不走代理：{}", cfg.bypass.join(", "))
    };
    format!("当前走 {kind} 代理：{}:{}（内嵌网页需重启一次才生效）{bypass}", cfg.host.trim(), cfg.port)
}

/// 测一下现在能不能连到校内服务（对应旧版 `vpn.probe`）
pub fn probe(store: &Store) -> Value {
    let targets = [
        ("eLearning", "https://elearning.fudan.edu.cn/login"),
        ("教务处", "https://jwc.fudan.edu.cn/"),
    ];
    let mut results: Vec<Value> = Vec::new();
    let mut ok = true;
    for (name, url) in targets {
        let started = std::time::Instant::now();
        let agent = agent_with_timeout(store, url, 8);
        let outcome = agent.get(url).call();
        let cost = started.elapsed().as_millis() as i64;
        match outcome {
            Ok(response) => {
                let status = response.status() as i64;
                let good = status < 500;
                ok = ok && good;
                results.push(json!({ "name": name, "ok": good, "status": status, "ms": cost }));
            }
            Err(ureq::Error::Status(code, _)) => {
                let good = code < 500;
                ok = ok && good;
                results.push(json!({ "name": name, "ok": good, "status": code, "ms": cost }));
            }
            Err(exc) => {
                ok = false;
                results.push(json!({
                    "name": name, "ok": false, "status": 0, "ms": cost,
                    "error": exc.to_string().chars().take(120).collect::<String>(),
                }));
            }
        }
    }
    let cfg = config(store);
    json!({
        "ok": ok,
        "mode": if cfg.enabled && !cfg.host.trim().is_empty() { "代理" } else { "直连" },
        "results": results,
        "proxy": proxy_url_of(&cfg).unwrap_or_default(),
        "summary": summary(store),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Temp(Option<std::ffi::OsString>, std::path::PathBuf);

    impl Temp {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "danlume-net-{tag}-{}",
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
    fn parses_hosts_from_urls() {
        assert_eq!(host_of("https://elearning.fudan.edu.cn/login"), "elearning.fudan.edu.cn");
        assert_eq!(host_of("http://127.0.0.1:8443/api"), "127.0.0.1");
        assert_eq!(host_of("http://[::1]:8080/x"), "::1");
        assert_eq!(host_of("https://user:pw@host.example.com:8443/x"), "host.example.com");
        assert_eq!(host_of(""), "");
    }

    #[test]
    fn bypass_rules_work() {
        let rules = parse_bypass("fudan.edu.cn, localhost  *.qq.com，10.0.0.5");
        assert_eq!(rules, vec![
            "fudan.edu.cn".to_string(),
            "localhost".to_string(),
            "*.qq.com".to_string(),
            "10.0.0.5".to_string(),
        ]);
        // 本机永远直连
        assert!(host_bypassed("localhost", &[]));
        assert!(host_bypassed("127.0.0.1", &[]));
        // 后缀匹配
        assert!(host_bypassed("elearning.fudan.edu.cn", &rules));
        assert!(host_bypassed("fudan.edu.cn", &rules));
        assert!(host_bypassed("im.qq.com", &rules));
        // 不能误伤：evilfudan.edu.cn 不该被 fudan.edu.cn 放行
        assert!(!host_bypassed("evilfudan.edu.cn", &rules));
        assert!(!host_bypassed("fudan.edu.cn.evil.com", &rules));
        assert!(!host_bypassed("api.deepseek.com", &rules));
    }

    #[test]
    fn builds_proxy_urls() {
        let mut cfg = ProxyCfg {
            enabled: true,
            kind: "http".to_string(),
            host: "127.0.0.1".to_string(),
            port: 7890,
            ..Default::default()
        };
        assert_eq!(proxy_url_of(&cfg).unwrap(), "http://127.0.0.1:7890");
        cfg.kind = "socks5".to_string();
        assert_eq!(proxy_url_of(&cfg).unwrap(), "socks5://127.0.0.1:7890");
        cfg.user = "lena".to_string();
        cfg.password = "pw".to_string();
        assert_eq!(proxy_url_of(&cfg).unwrap(), "socks5://lena:pw@127.0.0.1:7890");
        // IPv6 要加方括号
        cfg.host = "::1".to_string();
        cfg.user = String::new();
        cfg.password = String::new();
        assert_eq!(proxy_url_of(&cfg).unwrap(), "socks5://[::1]:7890");
        // 关掉就是 None
        cfg.enabled = false;
        assert!(proxy_url_of(&cfg).is_none());
    }

    /// 设置页填进去的东西要能被读到（含 DPAPI 加密的口令）
    #[test]
    fn reads_settings_and_encrypts_password() {
        let _temp = Temp::new("cfg");
        let store = Store::open().expect("临时库");
        store.set_setting("proxy_enabled", "1").unwrap();
        store.set_setting("proxy_kind", "socks5").unwrap();
        store.set_setting("proxy_host", "10.0.0.9").unwrap();
        store.set_setting("proxy_port", "1080").unwrap();
        store.set_setting("proxy_user", "lena").unwrap();
        store.set_setting("proxy_bypass", "fudan.edu.cn, localhost").unwrap();
        let encrypted = crate::secrets::protect("secret-pass").unwrap();
        store.set_setting("proxy_password", &encrypted).unwrap();
        let cfg = config(&store);
        assert!(cfg.enabled);
        assert_eq!(cfg.kind, "socks5");
        assert_eq!(cfg.host, "10.0.0.9");
        assert_eq!(cfg.port, 1080);
        assert_eq!(cfg.password, "secret-pass", "密文要能解开");
        assert_eq!(
            proxy_url_of(&cfg).unwrap(),
            "socks5://lena:secret-pass@10.0.0.9:1080"
        );
        assert!(summary(&store).contains("SOCKS5"));
    }

    #[test]
    fn webview_args_follow_settings() {
        let _temp = Temp::new("webview");
        let store = Store::open().expect("临时库");
        assert!(webview_args(&store).is_empty(), "没启用代理时不该加参数");
        store.set_setting("proxy_enabled", "1").unwrap();
        store.set_setting("proxy_host", "127.0.0.1").unwrap();
        store.set_setting("proxy_port", "7890").unwrap();
        store.set_setting("proxy_bypass", "fudan.edu.cn").unwrap();
        let args = webview_args(&store);
        assert_eq!(args[0], "--proxy-server=http://127.0.0.1:7890");
        assert!(args[1].contains("<local>"));
        assert!(args[1].contains("*.fudan.edu.cn"));
        // 带账号时不能把口令塞进参数（Chromium 不认，且会泄漏）
        store.set_setting("proxy_user", "lena").unwrap();
        store.set_setting("proxy_password", "pw").unwrap();
        let args = webview_args(&store);
        assert!(!args[0].contains("lena"), "口令/账号不能写进启动参数：{}", args[0]);
        assert_eq!(args[0], "--proxy-server=http://127.0.0.1:7890");
    }
}
