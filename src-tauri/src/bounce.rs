//! 退信识别（对应旧版 `lumina/bounce.py`）：把 PostMaster / Delivery Status 的退信，
//! 和发件箱里那封邮件对上，并把发件箱那条标成 `bounced`。
//!
//! 识别经验（与旧版一致）：
//! * 发件人常见 `postmaster@`、`MAILER-DAEMON`；
//! * 主题常见 `Undeliverable`、`退信`、`Delivery Status Notification`、`failure notice`；
//! * 正文里有 `550 5.5.0 …`、`mailbox unavailable`、`does not exist` 这类字样。

use serde_json::{json, Value};

use crate::store::Store;

const SUBJECT_HINTS: [&str; 7] = [
    "undeliverable",
    "退信",
    "delivery status",
    "failure notice",
    "delivery failure",
    "returned mail",
    "mail delivery failed",
];

/// 状态码 → 人话（旧版 `REASON_HINTS`）
fn reason_of(code: &str) -> Option<&'static str> {
    Some(match code {
        "5.5.0" => "对方邮箱不存在（mailbox unavailable）",
        "5.1.1" | "5.1.10" => "收件人地址不存在",
        "5.2.2" => "对方邮箱已满",
        "5.2.1" => "对方邮箱已满 / 被停用",
        "5.7.1" => "对方服务器拒绝接收（可能在黑名单 / 权限不足）",
        "5.7.60" => "发件地址与登录账号不一致",
        "5.4.7" => "投递超时",
        _ => return None,
    })
}

/// 退信正文里常抄着原始邮件头（含我们自己的发件地址），这几句提示词后面才是真正失败的地址
const FOCUS_HINTS: [&str; 20] = [
    "couldn't be delivered",
    "could not be delivered",
    "unable to deliver",
    "delivery has failed",
    "failed to these",
    "these recipients",
    "recipients or groups",
    "failed recipient",
    "following address",
    "delivery to",
    "failure occurred during the delivery",
    "diagnostic information",
    "final-recipient",
    "无法发送到",
    "收件人",
    "以下地址",
    "未送达",
    "退信原因",
    "被退回邮件",
    "无法投递",
];

/// 这封是不是退信（发件人或主题里带特征词）
pub fn is_bounce(sender: &str, subject: &str) -> bool {
    let text = format!("{sender} {subject}").to_lowercase();
    if text.contains("postmaster") || text.contains("mailer-daemon") {
        return true;
    }
    SUBJECT_HINTS.iter().any(|hint| text.contains(hint))
}

/// 从一行里挑出邮箱地址（够用即可：字母数字与 `._%+-` 组成本地部分，域名带点）
fn addresses_in(line: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut token = String::new();
    let allowed = |ch: char| ch.is_ascii_alphanumeric() || "._%+-@".contains(ch);
    let flush = |token: &mut String, out: &mut Vec<String>| {
        let text = std::mem::take(token);
        if !text.contains('@') {
            return;
        }
        let parts: Vec<&str> = text.split('@').collect();
        if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
            return;
        }
        let domain = parts[1];
        let has_dot = domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.');
        if !has_dot || !domain.chars().any(|ch| ch.is_ascii_alphabetic()) {
            return;
        }
        if !out.iter().any(|one| one.eq_ignore_ascii_case(&text)) {
            out.push(text);
        }
    };
    for ch in line.chars() {
        if allowed(ch) {
            token.push(ch);
        } else {
            flush(&mut token, &mut out);
        }
    }
    flush(&mut token, &mut out);
    out
}

/// 从正文里抠 `5xx a.b.c` 这种状态码（例如 `550 5.5.0` → `5.5.0`）
fn codes_in(text: &str) -> Vec<String> {
    let bytes: Vec<char> = text.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i + 4 < bytes.len() {
        // 形如 `550 5.5.0`
        if bytes[i] == '5'
            && bytes[i + 1].is_ascii_digit()
            && bytes[i + 2].is_ascii_digit()
            && (bytes[i + 3] == ' ' || bytes[i + 3] == '\t')
        {
            let mut j = i + 4;
            let mut code = String::new();
            // 收 `d.d.d`（中间允许 1~3 位）
            let mut groups = 0;
            let mut cur = String::new();
            while j < bytes.len() && groups < 3 {
                let ch = bytes[j];
                if ch.is_ascii_digit() {
                    cur.push(ch);
                    j += 1;
                    continue;
                }
                if ch == '.' && !cur.is_empty() && groups < 2 {
                    code.push_str(&cur);
                    code.push('.');
                    cur.clear();
                    groups += 1;
                    j += 1;
                    continue;
                }
                break;
            }
            if !cur.is_empty() {
                code.push_str(&cur);
                groups += 1;
            }
            if groups == 3 && !out.iter().any(|one| one == &code) {
                out.push(code);
            }
            i = j.max(i + 1);
            continue;
        }
        i += 1;
    }
    out
}

/// 解析退信：失败的收件地址 + 状态码 + 一句人话（旧版 `bounce.parse`）
pub fn parse(body: &str, subject: &str) -> (Vec<String>, Vec<String>, String) {
    let text = format!("{subject}\n{body}");
    let lines: Vec<&str> = text.lines().collect();
    let mut focused: Vec<String> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let low = line.to_lowercase();
        if !FOCUS_HINTS.iter().any(|hint| low.contains(hint)) {
            continue;
        }
        let mut window = (*line).to_string();
        if let Some(next) = lines.get(index + 1) {
            window.push('\n');
            window.push_str(next);
        }
        for found in addresses_in(&window) {
            if !focused.iter().any(|one| one.eq_ignore_ascii_case(&found)) {
                focused.push(found);
            }
        }
    }
    let addresses = if focused.is_empty() {
        addresses_in(&text)
    } else {
        focused
    };
    let codes = codes_in(&text);
    let mut reason = String::new();
    for code in &codes {
        if let Some(text) = reason_of(code) {
            reason = text.to_string();
            break;
        }
    }
    if reason.is_empty() {
        let low = text.to_lowercase();
        if low.contains("mailbox unavailable") || low.contains("does not exist") {
            reason = "对方邮箱不存在".to_string();
        } else if low.contains("mailbox full") || low.contains("over quota") {
            reason = "对方邮箱已满".to_string();
        } else if low.contains("blocked") || low.contains("blacklist") || low.contains("spam") {
            reason = "被对方当成垃圾邮件 / 拒收".to_string();
        }
    }
    (addresses, codes, reason)
}

/// 在发件箱里找「发给这些地址、且还没退信」的邮件（旧版 `bounce.match_outbox`）
pub fn match_outbox(store: &Store, addresses: &[String]) -> Vec<i64> {
    let wanted: Vec<String> = addresses
        .iter()
        .map(|one| one.trim().to_lowercase())
        .filter(|one| !one.is_empty())
        .collect();
    if wanted.is_empty() {
        return Vec::new();
    }
    let Ok(rows) = store.mails(300) else {
        return Vec::new();
    };
    let mut hits: Vec<i64> = Vec::new();
    for mail in rows {
        let status = mail.get("status").and_then(|one| one.as_str()).unwrap_or("");
        if status != "sent" && status != "failed" {
            continue;
        }
        let to_addr = mail
            .get("to_addr")
            .and_then(|one| one.as_str())
            .unwrap_or("")
            .trim()
            .to_lowercase();
        if to_addr.is_empty() {
            continue;
        }
        let candidates = addresses_in(&to_addr);
        let matched = if candidates.is_empty() {
            wanted.contains(&to_addr)
        } else {
            candidates
                .iter()
                .any(|one| wanted.contains(&one.to_lowercase()))
        };
        if !matched {
            continue;
        }
        if let Some(id) = mail.get("id").and_then(|one| one.as_i64()) {
            if !hits.contains(&id) {
                hits.push(id);
            }
        }
    }
    hits
}

/// 扫一遍收件箱里的退信，把发件箱对应的邮件标成「已退信」。
///
/// 返回 `{ok, marked:[{mail_id, to, reason, code}], scanned, error}`
pub fn sync(store: &Store) -> Value {
    let inbox = match crate::mail::list_inbox(store, 50) {
        Ok(rows) => rows,
        Err(exc) => return json!({ "ok": false, "error": exc, "marked": [] }),
    };
    let mut marked: Vec<Value> = Vec::new();
    let mut scanned = 0usize;
    for row in &inbox {
        if !is_bounce(&row.from, &row.subject) {
            continue;
        }
        scanned += 1;
        // 多读几封才有正文；取不到正文就只按主题判断
        let body = match crate::mail::fetch_body(store, &row.uid) {
            Ok(body) => format!("{}\n{}", body.text, body.html),
            Err(_) => String::new(),
        };
        let (addresses, codes, reason) = parse(&body, &row.subject);
        for mail_id in match_outbox(store, &addresses) {
            let code = codes.first().cloned().unwrap_or_default();
            let note = format!("{} {}", code, reason).trim().to_string();
            let note = if note.is_empty() {
                "对方退信".to_string()
            } else {
                note
            };
            if store.update_mail_status(mail_id, "bounced", &note).is_ok() {
                marked.push(json!({
                    "mail_id": mail_id,
                    "to": addresses.first().cloned().unwrap_or_default(),
                    "reason": reason,
                    "code": code,
                }));
            }
        }
    }
    json!({
        "ok": true,
        "scanned": scanned,
        "marked": marked,
        "error": "",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_bounce_senders_and_subjects() {
        assert!(is_bounce("postmaster@fudan.edu.cn", "随便什么"));
        assert!(is_bounce("MAILER-DAEMON@qq.com", "Undeliverable: 你好"));
        assert!(is_bounce("someone@x.com", "Delivery Status Notification (Failure)"));
        assert!(is_bounce("someone@x.com", "退信：邮件未送达"));
        assert!(!is_bounce("boss@x.com", "周报"));
    }

    #[test]
    fn parses_address_and_code() {
        let body = "The following address failed:\n  nobody@example.com\n\
                    550 5.5.0 mailbox unavailable\n";
        let (addresses, codes, reason) = parse(body, "Undeliverable");
        assert!(addresses.contains(&"nobody@example.com".to_string()), "{addresses:?}");
        assert_eq!(codes.first().map(|one| one.as_str()), Some("5.5.0"));
        assert!(reason.contains("不存在"), "{reason}");
    }

    #[test]
    fn ignores_our_own_address_in_quoted_headers() {
        // 引用头里有我们自己的地址，但提示词后面才是失败地址
        let body = "From: me@fudan.edu.cn\nTo: me@fudan.edu.cn\n\
                    The following recipients could not be delivered:\n other@qq.com\n";
        let (addresses, _, _) = parse(body, "failure notice");
        assert!(addresses.contains(&other()), "{addresses:?}");
    }

    fn other() -> String {
        "other@qq.com".to_string()
    }

    #[test]
    fn extracts_codes_without_false_positives() {
        assert_eq!(codes_in("550 5.1.1 user unknown"), vec!["5.1.1"]);
        assert_eq!(codes_in("552 5.2.2 mailbox full"), vec!["5.2.2"]);
        // 普通数字不该被当状态码
        assert!(codes_in("2026-11-11 12:30 有 550 个字").is_empty());
    }
}
