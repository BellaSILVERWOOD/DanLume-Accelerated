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

/// 从一段文本里挑出发布日期（旧版 `notices.py` 会顺手把日期带上，v2 之前这一步漏了）
///
/// 收这几种写法（够覆盖教务 / eHall 的列表页）：
/// `2026-11-15`、`2026/11/15`、`2026.11.15`、`2026年11月15日`、`11月15日`。
/// 返回找到的那段原文（用 `-` 归一化），找不到给空串。
pub fn pick_date(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let digits_at = |from: usize, count: usize| -> Option<String> {
        if from + count > chars.len() {
            return None;
        }
        let slice: String = chars[from..from + count].iter().collect();
        if slice.chars().all(|ch| ch.is_ascii_digit()) {
            Some(slice)
        } else {
            None
        }
    };
    for start in 0..chars.len() {
        // ① 2026-11-15 / 2026/11/15 / 2026.11.15
        if let Some(year) = digits_at(start, 4) {
            let year_ok = year.starts_with("20");
            let sep = chars.get(start + 4).copied();
            if year_ok && matches!(sep, Some('-') | Some('/') | Some('.')) {
                let sep = sep.unwrap();
                if let (Some(month), Some(day_sep), Some(day)) = (
                    digits_at(start + 5, 2),
                    chars.get(start + 7).copied(),
                    digits_at(start + 8, 2),
                ) {
                    if day_sep == sep {
                        let month_num = month.parse::<u32>().unwrap_or(0);
                        let day_num = day.parse::<u32>().unwrap_or(0);
                        if (1..=12).contains(&month_num) && (1..=31).contains(&day_num) {
                            return format!("{year}-{month}-{day}");
                        }
                    }
                }
            }
        }
        // ② 2026年11月15日 / 11月15日
        if let Some(month) = digits_at(start, 2) {
            if chars.get(start + 2) == Some(&'月') {
                if let Some(day) = digits_at(start + 3, 2) {
                    if chars.get(start + 5) == Some(&'日') {
                        let month_num = month.parse::<u32>().unwrap_or(0);
                        let day_num = day.parse::<u32>().unwrap_or(0);
                        if (1..=12).contains(&month_num) && (1..=31).contains(&day_num) {
                            // 往前看有没有「YYYY年」（年字就在月份前面）
                            if start >= 5 && chars.get(start - 1) == Some(&'年') {
                                if let Some(year) = digits_at(start - 5, 4) {
                                    if year.starts_with("20") {
                                        return format!("{year}-{month}-{day}");
                                    }
                                }
                            }
                            return format!("{month}-{day}");
                        }
                    }
                }
            }
        }
    }
    String::new()
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
        // 发布日期：先看链接自己的文字，再看后面一小段（列表页通常把日期放在同一行）
        let tail = &html[close + 4..(close + 4 + 180).min(html.len())];
        let mut published = pick_date(&text);
        if published.is_empty() {
            published = pick_date(&strip_tags(tail));
        }
        out.push((text, absolute, published));
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

    /// J3-2：发布日期要一起带出来（旧版 `notices.py` 有这一步）
    #[test]
    fn picks_publish_date_in_common_formats() {
        assert_eq!(pick_date("2026-11-15 教务处"), "2026-11-15");
        assert_eq!(pick_date("2026/11/15"), "2026-11-15");
        assert_eq!(pick_date("2026.11.15"), "2026-11-15");
        assert_eq!(pick_date("2026年11月15日发布"), "2026-11-15");
        assert_eq!(pick_date("发布时间：11月15日"), "11-15");
        // 不像日期的数字不要乱认
        assert_eq!(pick_date("20261115"), "");
        assert_eq!(pick_date("学号 26303050173"), "");
        assert_eq!(pick_date("2026-13-45"), "");
        assert_eq!(pick_date(""), "");
    }

    /// 列表页常见形态：链接后面紧跟一个日期
    #[test]
    fn anchor_takes_date_from_next_to_it() {
        let html = r#"<li><a href="/n/9">关于开展 2026 年冬季安全检查的通知</a>
                      <span>2026-11-15</span></li>"#;
        let rows = parse_anchors(html, "https://ehall.fudan.edu.cn/list");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].2, "2026-11-15", "日期要抓出来：{:?}", rows[0]);
    }
}
