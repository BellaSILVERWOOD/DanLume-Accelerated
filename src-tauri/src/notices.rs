//! 通知抓取：从配置的页面里抽出「像通知」的链接（旧版 `notices.py` 的简化版）。
use std::collections::HashSet;

/// 返回 (标题, 绝对链接, 发布时间) —— 发布时间一般抓不到，给空串
pub fn fetch(url: &str) -> Result<Vec<(String, String, String)>, String> {
    let agent = crate::net::with_proxy(
     ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(12))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) DanLume/2.0"),
     crate::get_store(),
     url,
 )
 .build();
    let response = agent
        .get(url)
        .call()
        .map_err(|err| format!("抓取失败：{err}"))?;
    let body = response
        .into_string()
        .map_err(|err| format!("读页面失败：{err}"))?;
    Ok(parse_anchors(&body, url))
}

fn find_from(text: &str, needle: &str, from: usize) -> Option<usize> {
    if from >= text.len() {
        return None;
    }
    text[from..].find(needle).map(|offset| from + offset)
}

fn extract_attr(attrs: &str, name: &str) -> Option<String> {
    let lower = attrs.to_lowercase();
    let mut search = 0usize;
    while let Some(offset) = lower[search..].find(name) {
        let at = search + offset;
        let after = &attrs[at + name.len()..];
        let trimmed = after.trim_start();
        if let Some(rest) = trimmed.strip_prefix('=') {
            let value = rest.trim_start();
            let quote = value.chars().next()?;
            if quote == '"' || quote == '\'' {
                let body = &value[1..];
                let end = body.find(quote)?;
                return Some(body[..end].trim().to_string());
            }
            let end = value
                .find(|ch: char| ch.is_whitespace() || ch == '>')
                .unwrap_or(value.len());
            return Some(value[..end].trim().to_string());
        }
        search = at + name.len();
    }
    None
}

fn strip_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut inside = false;
    for ch in text.chars() {
        match ch {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => out.push(ch),
            _ => {}
        }
    }
    let cleaned = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"");
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 把相对链接补成绝对链接（只处理常见的 http(s) / 站内 / 协议相对三种）
pub fn absolutize(base: &str, href: &str) -> String {
    let link = href.trim();
    if link.is_empty() {
        return String::new();
    }
    let lower = link.to_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return link.to_string();
    }
    let (scheme, rest) = match base.split_once("://") {
        Some(pair) => pair,
        None => return link.to_string(),
    };
    let host = rest.split('/').next().unwrap_or(rest);
    if link.starts_with("//") {
        return format!("{scheme}:{link}");
    }
    if link.starts_with('/') {
        return format!("{scheme}://{host}{link}");
    }
    // 相对当前目录
    let mut parts: Vec<&str> = rest.split('/').collect();
    parts.pop();
    parts.push(link);
    format!("{scheme}://{}", parts.join("/"))
}

/// 从 HTML 里抽出「像一条通知」的锚点
pub fn parse_anchors(html: &str, base: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut index = 0usize;

    while index < html.len() {
        let Some(start) = find_from(html, "<a", index) else {
            break;
        };
        let Some(open_end) = html[start..].find('>').map(|offset| start + offset) else {
            break;
        };
        let attrs = &html[start + 2..open_end];
        let Some(href) = extract_attr(attrs, "href") else {
            index = open_end + 1;
            continue;
        };
        let Some(close) = find_from(html, "</a>", open_end) else {
            break;
        };
        let text = strip_tags(&html[open_end + 1..close]);
        index = close + 4;
        let lower = href.to_lowercase();
        if lower.starts_with("javascript:") || lower.starts_with('#') {
            continue;
        }
        if text.chars().count() < 6 {
            continue;
        }
        let absolute = absolutize(base, &href);
        if absolute.is_empty() || !seen.insert(format!("{absolute}|{text}")) {
            continue;
        }
        out.push((text, absolute, String::new()));
        if out.len() >= 120 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_links() {
        assert_eq!(
            absolutize("https://ehall.fudan.edu.cn/a/b", "https://x.com/y"),
            "https://x.com/y"
        );
        assert_eq!(
            absolutize("https://ehall.fudan.edu.cn/a/b", "/notice/1"),
            "https://ehall.fudan.edu.cn/notice/1"
        );
        assert_eq!(
            absolutize("https://ehall.fudan.edu.cn/a/b", "notice/1"),
            "https://ehall.fudan.edu.cn/a/notice/1"
        );
        assert_eq!(
            absolutize("https://ehall.fudan.edu.cn/a/b", "//cdn.x.com/z"),
            "https://cdn.x.com/z"
        );
    }

    #[test]
    fn anchors_are_clean() {
        let html = r#"<div><a href="/n/1">关于 2026 年国庆节放假安排的通知</a>
                      <a href="javascript:void(0)">忽略</a>
                      <a href="/n/2"> 短 </a></div>"#;
        let rows = parse_anchors(html, "https://ehall.fudan.edu.cn/list");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1, "https://ehall.fudan.edu.cn/n/1");
        assert_eq!(rows[0].0, "关于 2026 年国庆节放假安排的通知");
    }
}
