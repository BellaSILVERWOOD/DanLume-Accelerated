//! ehall 卡片实时抓取：在 WebView2 里打开目标页面 → DOM 文本回传 Rust → 解析 → 写缓存。
//!
//! 为什么这么做：
//! * 登录态就在这个 WebView 的 profile 里，不用去搬 Chromium 的 cookie 库；
//! * 回传缓存的形状与旧版 `ehall_cache_<key>` 一致，仪表板和空教室页不用改。
use serde_json::{json, Value};

use crate::store::Store;

/// DOM 文本 → 卡片内容。返回 (ok, value, lines, error)
pub fn parse(key: &str, url: &str, text: &str) -> (bool, String, Vec<String>, String) {
    let lower_url = url.to_lowercase();
    if lower_url.contains("id.fudan.edu.cn") || lower_url.contains("/login") {
        return (false, String::new(), Vec::new(),
                "页面被跳到登录页：请在 eHall 窗口里登录一次再同步".to_string());
    }
    let lines: Vec<String> = text
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        return (false, String::new(), Vec::new(), "页面是空的（可能还没加载完）".to_string());
    }
    if lower_url.contains("ehall.fudan.edu.cn") && text.contains("统一身份认证") {
        return (false, String::new(), Vec::new(),
                "还没登录 eHall：在窗口里登录后再同步".to_string());
    }

    let preview: Vec<String> = lines.iter().take(6).cloned().collect();
    let compact = lines.join(" ");

    let value = match key {
        "card" => find_money(&compact),
        "library_people" => find_count(&compact, &["人"]).unwrap_or_default(),
        "sports" => find_count(&compact, &["次", "公里", "km"]).unwrap_or_default(),
        _ => String::new(),
    };
    // 页面必须「像」这张卡。抓取窗口没填地址时会先落在 eHall 首页，
    // 那里也有文字甚至数字；不加这道闸，首页会被当成余额/在馆人数写进缓存，
    // 还会把首页地址记成来源地址（2026-11-23 的卡片就是这么跑偏的）。
    let label = match key {
        "card" => "校园卡余额",
        "bus" => "校车安排",
        "sports" => "体育刷锻",
        "library_people" => "图书馆人数",
        "ehall_todo" => "eHall 通知",
        "empty_room" => "空教室",
        _ => key,
    };
    let expects: &[&str] = match key {
        "card" => &["余额", "校园卡", "一卡通"],
        "bus" => &["校车", "班车", "时刻", "发车"],
        "sports" => &["刷锻", "锻炼", "跑步", "里程"],
        "library_people" => &["在馆", "图书馆", "座位"],
        "ehall_todo" => &["通知", "待办", "消息"],
        "empty_room" => &["教室", "空闲"],
        _ => &[],
    };
    let matched = expects.is_empty() || expects.iter().any(|word| compact.contains(word));
    if !value.is_empty() && matched {
        return (true, value, preview, String::new());
    }
    if !matched {
        return (
            false,
            String::new(),
            preview,
            format!("当前页面不像「{label}」的数据页（没找到关键词）；在窗口里点进对应服务再试"),
        );
    }
    // 通用兜底：取第一句像「结论」的文字（校车 / 通知这类没有固定数值的卡）
    let fallback = preview
        .iter()
        .find(|line| line.chars().count() <= 24 && !line.contains("登录"))
        .cloned()
        .unwrap_or_else(|| "已抓到页面".to_string());
    (true, fallback, preview, String::new())
}

/// 余额：`¥128.50` / `¥ 128.50` / `128.50 元`
fn find_money(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    // ① ¥ 后面跟数字
    for (index, ch) in chars.iter().enumerate() {
        if *ch == '¥' || *ch == '￥' {
            let mut digits = String::new();
            for next in chars.iter().skip(index + 1) {
                if next.is_ascii_digit() || *next == '.' {
                    digits.push(*next);
                } else if next.is_whitespace() && digits.is_empty() {
                    continue;
                } else {
                    break;
                }
            }
            if !digits.is_empty() && digits.trim_matches('.').parse::<f64>().is_ok() {
                return format!("¥{}", digits.trim_matches('.'));
            }
        }
    }
    // ② 数字后面跟「元」
    for (index, ch) in chars.iter().enumerate() {
        if *ch == '元' {
            let mut digits: Vec<char> = Vec::new();
            for before in chars[..index].iter().rev() {
                if before.is_ascii_digit() || *before == '.' {
                    digits.push(*before);
                } else if before.is_whitespace() && digits.is_empty() {
                    continue;
                } else {
                    break;
                }
            }
            digits.reverse();
            let value: String = digits.into_iter().collect();
            if !value.is_empty() && value.trim_matches('.').parse::<f64>().is_ok() {
                return format!("¥{}", value.trim_matches('.'));
            }
        }
    }
    String::new()
}

/// 数字 + 单位（允许数字和单位之间有空格）
fn find_count(text: &str, units: &[&str]) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    for unit in units {
        let mut start = 0usize;
        while start < chars.len() {
            let Some(found) = text[start..].find(unit) else {
                break;
            };
            let at = start + found;
            let char_index = text[..at].chars().count();
            let mut digits: Vec<char> = Vec::new();
            for before in chars[..char_index].iter().rev() {
                if before.is_ascii_digit() {
                    digits.push(*before);
                } else if before.is_whitespace() && digits.is_empty() {
                    continue;
                } else {
                    break;
                }
            }
            digits.reverse();
            let value: String = digits.into_iter().collect();
            if !value.is_empty() {
                return Some(format!("{value}{unit}"));
            }
            start = at + unit.len();
        }
    }
    None
}

/// 写缓存（形状与旧版一致）
///
/// 两条规矩（2026-11-23）：
/// * **解析失败不覆盖已经成功过的缓存**：抓取窗口可能还停在首页 / 登录页，那时的正文
///   解析不出余额，直接写会把上一次的好数据冲掉；
/// * **解析成功就记住来源地址**：抓取窗口里用户自己点进服务页后，把最终地址写回
///   `ehall_json_<key>`，以后「同步 ehall」就能直接会话直连刷新，不用再打开窗口。
pub fn store_cache(store: &Store, key: &str, url: &str, text: &str) -> Value {
    let (ok, value, lines, error) = parse(key, url, text);
    let existing_ok = store
        .setting(&format!("ehall_cache_{key}"))
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|cache| cache.get("ok").and_then(|one| one.as_bool()))
        .unwrap_or(false);
    if !ok && existing_ok {
        return json!({
            "ok": false,
            "value": "",
            "lines": [],
            "rows": [],
            "error": error,
            "url": url,
            "kept": true,
        });
    }
    if ok && !url.trim().is_empty() {
        let source_key = format!("ehall_json_{key}");
        let current = store.setting(&source_key).unwrap_or_default();
        if current.trim().is_empty() {
            let _ = store.set_setting(&source_key, url.trim());
        }
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|dur| dur.as_secs_f64())
        .unwrap_or(0.0);
    let payload = json!({
        "ok": ok,
        "value": value,
        "lines": lines,
        "rows": [],
        "text": text.chars().take(2000).collect::<String>(),
        "error": error,
        "url": url,
        "ts": now,
    });
    let _ = store.set_setting(&format!("ehall_cache_{key}"), &payload.to_string());
    let _ = store.set_setting(&format!("ehall_cache_at_{key}"), &now.to_string());
    payload
}

/// 注入到目标页面的脚本：等页面稳下来，把正文回传
pub fn grab_script(key: &str) -> String {
    format!(
        r#"(function(){{
  var key = {key:?};
  function mark(state) {{
    try {{ document.title = "grab-" + state; }} catch (e) {{}}
    try {{ location.hash = "grab-" + state; }} catch (e) {{}}
  }}
  function invoke(cmd, payload) {{
    var internals = window.__TAURI_INTERNALS__;
    if (internals && typeof internals.invoke === "function") {{
      return internals.invoke(cmd, payload);
    }}
    var api = window.__TAURI__;
    if (api && api.core && typeof api.core.invoke === "function") {{
      return api.core.invoke(cmd, payload);
    }}
    return null;
  }}
  function send() {{
    var text = (document.body ? document.body.innerText : "") || "";
    if (!text.trim()) {{ mark("empty"); return; }}
    var pending = invoke("ehall_ingest", {{
      key: key, url: String(location.href), text: text.slice(0, 200000)
    }});
    if (!pending) {{ mark("noipc"); return; }}
    pending.then(function () {{ mark("ok"); }})
           .catch(function (err) {{ mark("denied"); }});
  }}
  setTimeout(send, 3000);
  setTimeout(send, 8000);
  setTimeout(send, 14000);
  // SPA 里点进某个服务不会重新加载页面：盯着地址变化，变了就再回传一次。
  // 这样「打开 eHall 首页 → 登录 → 点进校园卡/图书馆…」也能抓到，而不是只在 3/8/14 秒各抓一次。
  var lastHref = String(location.href);
  function sendIfChanged() {{
    var href = String(location.href);
    if (href === lastHref) {{ return; }}
    lastHref = href;
    setTimeout(send, 1500);
  }}
  try {{
    var push = history.pushState;
    history.pushState = function () {{
      var out = push.apply(this, arguments);
      sendIfChanged();
      return out;
    }};
  }} catch (e) {{}}
  try {{
    var replace = history.replaceState;
    history.replaceState = function () {{
      var out = replace.apply(this, arguments);
      sendIfChanged();
      return out;
    }};
  }} catch (e) {{}}
  window.addEventListener("popstate", sendIfChanged);
  window.addEventListener("hashchange", sendIfChanged);
  setInterval(sendIfChanged, 2000);
}})();"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_redirect_is_reported() {
        let (ok, _, _, error) = parse("card", "https://id.fudan.edu.cn/ac/#/index", "登录 请选择");
        assert!(!ok);
        assert!(error.contains("登录"));
    }

    #[test]
    fn money_and_counts() {
        let (ok, value, _, _) = parse("card", "https://ecard.fudan.edu.cn/epay/",
                                      "校园卡余额 ¥ 128.50 充值");
        assert!(ok);
        assert_eq!(value, "¥128.50");

        let (_, people, _, _) = parse("library_people", "https://lib.fudan.edu.cn/",
                                      "当前在馆 1234 人 自习座位 320");
        assert_eq!(people, "1234人");
    }

    #[test]
    fn script_mentions_command() {
        let script = grab_script("card");
        assert!(script.contains("ehall_ingest"));
        assert!(script.contains("\"card\""));
    }

    /// 端到端：正文 → 写缓存（形状要和仪表板读的一致）
    #[test]
    fn ingest_writes_cache() {
        let dir = std::env::temp_dir().join(format!("lumina_grab_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::env::set_var("LUMINA_DATA_DIR", &dir);
        let store = crate::store::Store::open().expect("临时库打不开");
        let payload = store_cache(&store, "library_people",
                                  "https://lib.fudan.edu.cn/stat",
                                  "图书馆实时在馆 1234 人\n座位剩余 320");
        assert_eq!(payload["ok"], serde_json::json!(true));
        assert_eq!(payload["value"], serde_json::json!("1234人"));
        let cached = store.setting("ehall_cache_library_people").unwrap_or_default();
        assert!(cached.contains("1234人"), "缓存没写进去：{cached}");
        assert!(store.setting("ehall_cache_at_library_people").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 抓取窗口还停在首页/登录页时，解析失败不能把上一次的好数据冲掉
    #[test]
    fn failed_parse_keeps_previous_good_cache() {
        let dir = std::env::temp_dir().join(format!("lumina_grab_keep_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::env::set_var("LUMINA_DATA_DIR", &dir);
        let store = crate::store::Store::open().expect("临时库打不开");
        let good = store_cache(
            &store,
            "library_people",
            "https://lib.fudan.edu.cn/stat",
            "图书馆实时在馆 1234 人",
        );
        assert_eq!(good["ok"], serde_json::json!(true));

        let bad = store_cache(
            &store,
            "library_people",
            "https://ehall.fudan.edu.cn/",
            "欢迎使用 eHall 服务大厅",
        );
        assert_eq!(bad["ok"], serde_json::json!(false));
        assert_eq!(bad["kept"], serde_json::json!(true));
        let cached = store.setting("ehall_cache_library_people").unwrap_or_default();
        assert!(cached.contains("1234人"), "好缓存被失败结果覆盖了：{cached}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 抓取成功后自动记住来源地址；已经填过的地址不被后来的抓取覆盖
    #[test]
    fn successful_grab_learns_source_url() {
        let dir = std::env::temp_dir().join(format!("lumina_grab_learn_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::env::set_var("LUMINA_DATA_DIR", &dir);
        let store = crate::store::Store::open().expect("临时库打不开");
        assert!(store.setting("ehall_json_card").unwrap_or_default().is_empty());

        let payload = store_cache(
            &store,
            "card",
            "https://ecard.fudan.edu.cn/balance",
            "校园卡余额 ¥ 66.00",
        );
        assert_eq!(payload["ok"], serde_json::json!(true));
        assert_eq!(
            store.setting("ehall_json_card").unwrap_or_default(),
            "https://ecard.fudan.edu.cn/balance"
        );

        store_cache(&store, "card", "https://other.example/card", "校园卡余额 ¥ 1.00");
        assert_eq!(
            store.setting("ehall_json_card").unwrap_or_default(),
            "https://ecard.fudan.edu.cn/balance",
            "已经填好的来源地址不该被覆盖"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
