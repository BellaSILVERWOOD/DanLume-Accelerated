//! 邮箱：IMAP 收件（正文 / 内联图片 / 附件）+ SMTP 发件。
//!
//! 配置全部沿用旧版的设置键，所以旧库里配好的账户能直接用：
//! `imap_host / imap_port / imap_user`、密码在 `secret:imap_pass`（DPAPI 密文）；
//! `smtp_host / smtp_port / smtp_enc / smtp_user / smtp_from`、密码在 `secret:smtp_pass`。
use std::io::Write;
use std::path::PathBuf;

use imap::types::Fetch;
use mail_parser::MimeHeaders;
use native_tls::TlsConnector;

use serde_json::json;
use crate::store::Store;

/// 收件箱里一封邮件的概要
#[derive(Debug, Clone, serde::Serialize)]
pub struct MailSummary {
    pub uid: String,
    pub subject: String,
    pub from: String,
    pub date: String,
    pub unread: bool,
    pub has_attachments: bool,
}

/// 正文 + 附件（附件已落盘）
#[derive(Debug, Clone, serde::Serialize)]
pub struct MailBody {
    pub uid: String,
    pub subject: String,
    pub from: String,
    pub date: String,
    pub html: String,
    pub text: String,
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Attachment {
    pub name: String,
    pub size: u64,
    pub path: String,
}

fn setting(store: &Store, key: &str) -> String {
    store.setting(key).unwrap_or_default()
}

/// 读账户配置：先用 v2 自己的设置键，没有就回退读旧版的 `imap_accounts` 表
pub fn config(store: &Store) -> Result<ImapConfig, String> {
    let mut host = setting(store, "imap_host");
    let mut user = setting(store, "imap_user");
    let mut port: u16 = setting(store, "imap_port").parse().unwrap_or(993);
    let mut secret_key = "imap_password".to_string();

    if host.trim().is_empty() || user.trim().is_empty() {
        // 旧版是「多账户」表：取第一个账户
        if let Some((id, legacy_host, legacy_port, legacy_user)) = first_legacy_account(store) {
            host = legacy_host;
            user = legacy_user;
            if legacy_port > 0 {
                port = legacy_port as u16;
            }
            secret_key = format!("imap_password_{id}");
        }
    }
    if host.trim().is_empty() || user.trim().is_empty() {
        return Err("还没配收件箱：设置 → 收件箱（IMAP）里填服务器和账号".to_string());
    }
    let encrypted = store
        .setting(&secret_key)
        .or_else(|| store.setting("imap_password"))
        .unwrap_or_default();
    let password = if encrypted.is_empty() {
        String::new()
    } else {
        crate::secrets::unprotect(&encrypted)?
    };
    if password.is_empty() {
        return Err("收件箱密码还没填（设置 → 收件箱 → 授权码）".to_string());
    }
    Ok(ImapConfig {
        host,
        port,
        user,
        password,
    })
}

/// 旧库 `imap_accounts` 里的第一个账户：(id, host, port, user)
fn first_legacy_account(store: &Store) -> Option<(i64, String, i64, String)> {
    let conn = store.connection();
    let mut stmt = conn
        .prepare("SELECT id, host, port, user FROM imap_accounts ORDER BY id LIMIT 1")
        .ok()?;
    let mut rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1).unwrap_or_default(),
                row.get::<_, i64>(2).unwrap_or(993),
                row.get::<_, String>(3).unwrap_or_default(),
            ))
        })
        .ok()?;
    rows.next()?.ok()
}

/// 旧库 `llm_profiles` 的第一套配置（AI 用）
pub fn first_llm_profile(store: &Store) -> Option<(i64, String, String)> {
    let conn = store.connection();
    let mut stmt = conn
        .prepare("SELECT id, base_url, model FROM llm_profiles ORDER BY id LIMIT 1")
        .ok()?;
    let mut rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1).unwrap_or_default(),
                row.get::<_, String>(2).unwrap_or_default(),
            ))
        })
        .ok()?;
    rows.next()?.ok()
}

pub struct ImapConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
}

fn connect(
    config: &ImapConfig,
) -> Result<imap::Session<native_tls::TlsStream<std::net::TcpStream>>, String> {
    let tls = TlsConnector::builder()
        .build()
        .map_err(|err| format!("TLS 初始化失败：{err}"))?;
    let client = imap::connect(
        (config.host.as_str(), config.port),
        config.host.as_str(),
        &tls,
    )
    .map_err(|err| format!("连不上 IMAP 服务器（{}:{}）：{err}", config.host, config.port))?;
    let session = client
        .login(&config.user, &config.password)
        .map_err(|(err, _)| format!("IMAP 登录失败：{err}"))?;
    Ok(session)
}

fn header_text<'a>(message: &'a mail_parser::Message<'_>, name: &str) -> String {
    message
        .header(name)
        .and_then(|header| header.as_text())
        .unwrap_or("")
        .to_string()
}

/// 主题：用解析好的字符串（RFC 2047 的 `=?utf-8?B?…?=` 会被解开）
fn subject_text(message: &mail_parser::Message<'_>) -> String {
    let raw = message.subject().unwrap_or("").trim().to_string();
    if raw.is_empty() {
        header_text(message, "Subject")
    } else {
        raw
    }
}

/// 发件人：同样用解析好的地址
fn from_text(message: &mail_parser::Message<'_>) -> String {
    if let Some(address) = message.from() {
        if let Some(first) = address.first() {
            let who = first.name().unwrap_or("").to_string();
            let mail = first.address().unwrap_or("").to_string();
            return if who.is_empty() {
                mail
            } else {
                format!("{who} <{mail}>")
            };
        }
    }
    address_text(message, "From")
}

fn address_text(message: &mail_parser::Message<'_>, name: &str) -> String {
    message
        .header(name)
        .and_then(|header| header.as_address())
        .and_then(|address| address.first())
        .map(|addr| {
            let who = addr.name().unwrap_or("").to_string();
            let mail = addr.address().unwrap_or("").to_string();
            if who.is_empty() {
                mail
            } else {
                format!("{who} <{mail}>")
            }
        })
        .unwrap_or_default()
}

/// 一封邮件的日期，**一定给得出**（本地时间 `YYYY-MM-DD HH:MM`）
///
/// 2026-11-14 用户报「收了新邮件但完全不显示」：原来直接取 `Date` 头的原文，
/// 有 8 封的 Date 头解析为空串 → 存进库就是空字符串 → `ORDER BY date DESC` 把它们排到最后，
/// 列表只取前 30 条，新邮件永远看不到。现在按「解析出来的 Date → Date 原文 → INTERNALDATE → 现在」
/// 的顺序兜底，确保每条都有可排序的日期。
fn summary_date(message: &mail_parser::Message<'_>, fetch: &imap::types::Fetch) -> String {
    let parsed = message.date().map(|date| date.to_timestamp());
    let raw = header_text(message, "Date");
    let internal = fetch.internal_date().map(|date| date.timestamp());
    pick_date_text(parsed, &raw, internal, chrono::Local::now().timestamp())
}

fn format_local(stamp: i64) -> String {
    chrono::DateTime::from_timestamp(stamp, 0)
        .map(|utc| {
            utc.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// 兜底顺序（纯函数，方便单测）：解析出来的 Date → Date 头原文 → IMAP 的 INTERNALDATE → 现在
///
/// **绝不允许返回空串**：空日期存进库后，`ORDER BY date DESC` 会把它排到最后，
/// 列表只取前 30 条 — 新邮件就永远看不见了（2026-11-14 的事故）。
fn pick_date_text(
    parsed: Option<i64>,
    raw: &str,
    internal: Option<i64>,
    now: i64,
) -> String {
    if let Some(stamp) = parsed {
        let text = format_local(stamp);
        if !text.is_empty() {
            return text;
        }
    }
    if let Ok(date) = chrono::DateTime::parse_from_rfc2822(raw.trim()) {
        let text = format_local(date.timestamp());
        if !text.is_empty() {
            return text;
        }
    }
    if let Some(stamp) = internal {
        let text = format_local(stamp);
        if !text.is_empty() {
            return text;
        }
    }
    format_local(now)
}

/// 拉最近 `limit` 封邮件的概要（不建缓存、不落盘）
pub fn list_inbox(store: &Store, limit: usize) -> Result<Vec<MailSummary>, String> {
    let config = config(store)?;
    let mut session = connect(&config)?;
    let mailbox = session
        .select("INBOX")
        .map_err(|err| format!("打开收件箱失败：{err}"))?;
    let total = mailbox.exists as usize;
    if total == 0 {
        let _ = session.logout();
        return Ok(Vec::new());
    }
    let start = total.saturating_sub(limit) + 1;
    let sequence = format!("{start}:{total}");
    let fetches = session
        .fetch(sequence, "(UID FLAGS BODY.PEEK[HEADER])")
        .map_err(|err| format!("取邮件头失败：{err}"))?;

    let mut rows: Vec<MailSummary> = Vec::new();
    for fetch in fetches.iter() {
        let Some(body) = fetch.header() else { continue };
        let message = mail_parser::MessageParser::default().parse(body);
        let Some(message) = message else { continue };
        let uid = fetch
            .uid
            .map(|value| value.to_string())
            .unwrap_or_default();
        let flags = format!("{:?}", fetch.flags());
        let subject = subject_text(&message);
        let from = from_text(&message);
        let date = summary_date(&message, &fetch);
        let content_type = header_text(&message, "Content-Type");
        rows.push(MailSummary {
            uid,
            subject: if subject.is_empty() {
                "（无主题）".to_string()
            } else {
                subject
            },
            from,
            date,
            unread: !flags.contains("Seen"),
            has_attachments: content_type.to_lowercase().contains("multipart"),
        });
    }
    rows.sort_by(|a, b| numeric(&b.uid).cmp(&numeric(&a.uid)));
    let _ = session.logout();
    Ok(rows)
}

fn numeric(value: &str) -> u64 {
    value.parse().unwrap_or(0)
}

/// 拉一封邮件的完整内容：正文 HTML / 纯文本 + 附件落盘
pub fn fetch_body(store: &Store, uid: &str) -> Result<MailBody, String> {
    let config = config(store)?;
    let mut session = connect(&config)?;
    session
        .select("INBOX")
        .map_err(|err| format!("打开收件箱失败：{err}"))?;
    let fetches = session
        .uid_fetch(uid, "(BODY.PEEK[])")
        .map_err(|err| format!("取邮件失败：{err}"))?;
    let fetch: Option<&Fetch> = fetches.iter().next();
    let raw = fetch
        .and_then(|item| item.body())
        .ok_or_else(|| "这封邮件取不到内容（可能已被删除）".to_string())?;
    let message = mail_parser::MessageParser::default()
        .parse(raw)
        .ok_or_else(|| "这封邮件的格式解析不了".to_string())?;

    let subject = {
        let value = subject_text(&message);
        if value.is_empty() {
            "（无主题）".to_string()
        } else {
            value
        }
    };
    let mut html = message
        .body_html(0)
        .map(|value| value.to_string())
        .unwrap_or_default();
    let text = message
        .body_text(0)
        .map(|value| value.to_string())
        .unwrap_or_default();

    // 附件落盘：data/mail_media/<uid>/，同时把图片附件的 data URL 收好当「替补图」
    let folder = media_dir(uid);
    let mut attachments = Vec::new();
    let mut spare: Vec<String> = Vec::new();
    if !html.is_empty() || message.attachment_count() > 0 {
        let _ = std::fs::create_dir_all(&folder);
    }
    for (index, part) in message.attachments().enumerate() {
        let raw_name = part
            .attachment_name()
            .map(|value| value.to_string())
            .unwrap_or_else(|| format!("附件{}", index + 1));
        let name = safe_name(&raw_name, index);
        let data = part.contents();
        let subtype = part
            .content_type()
            .and_then(|value| value.subtype())
            .map(|value| value.to_ascii_lowercase())
            .unwrap_or_default();
        let is_image = matches!(
            subtype.as_str(),
            "png" | "jpeg" | "jpg" | "gif" | "webp" | "bmp"
        ) || looks_like_image(&name, data);
        if is_image && !data.is_empty() && data.len() <= 4 * 1024 * 1024 {
            let mime = image_mime(&name, data);
            spare.push(format!(
                "data:image/{mime};base64,{}",
                crate::secrets::base64_encode(data)
            ));
        }
        let target: PathBuf = folder.join(&name);
        if std::fs::write(&target, data).is_ok() {
            attachments.push(Attachment {
                name: name.clone(),
                size: data.len() as u64,
                path: target.display().to_string(),
            });
        }
    }

    // 内联图片（cid:）换成 data URL：WebView 里不用任何自定义协议就能显示。
    // 注意：必须遍历**所有** part —— 内联图常常嵌在 multipart/related 里，
    // 只遍历 attachments() 会漏掉，于是正文里留一个破图框（用户报过）。
    if !html.is_empty() {
        for part in message.parts.iter() {
            let subtype = part
                .content_type()
                .and_then(|value| value.subtype())
                .map(|value| value.to_ascii_lowercase())
                .unwrap_or_default();
            let is_image = matches!(
                subtype.as_str(),
                "png" | "jpeg" | "jpg" | "gif" | "webp" | "bmp" | "svg+xml"
            );
            if !is_image {
                continue;
            }
            let data = part.contents();
            if data.is_empty() || data.len() > 4 * 1024 * 1024 {
                continue;
            }
            let mime = if subtype == "jpg" || subtype == "jpeg" {
                "jpeg"
            } else if subtype == "svg+xml" {
                "svg+xml"
            } else {
                subtype.as_str()
            };
            let url = format!("data:image/{mime};base64,{}", crate::secrets::base64_encode(data));

            // ① 按 Content-ID 替换（大小写、尖括号、空格编码都试一遍）
            if let Some(cid) = part.content_id() {
                let cid = cid.to_string();
                for needle in [
                    format!("cid:{cid}"),
                    format!("cid:{}", cid.replace(' ', "%20")),
                    format!("cid:<{cid}>"),
                    format!("CID:{cid}"),
                ] {
                    html = html.replace(&needle, &url);
                }
            }
            // ② 按附件名 / Content-Location 兜底（有些邮件拿文件名当 src）
            if let Some(name) = part.attachment_name() {
                let name = name.to_string();
                html = html.replace(&format!("cid:{name}"), &url);
                html = html.replace(&format!("src=\"{name}\""), &format!("src=\"{url}\""));
            }
            // ③ 复旦 webmail（coremail）把图片写成
            //    `https://mail.fudan.edu.cn/coremail/s/json?…&func=mbox:getComposeData&id=<Content-ID>`
            //    这种 URL 要 webmail 会话才取得回来，但图片就在附件里、名字带那段 ID。
            //    所以：拿 Content-ID / 附件名（去掉扩展名）当特征，把整个 src 换成 data URL。
            let mut tokens: Vec<String> = Vec::new();
            if let Some(cid) = part.content_id() {
                tokens.push(cid.to_string());
            }
            if let Some(name) = part.attachment_name() {
                let name = name.to_string();
                tokens.push(name.clone());
                if let Some(stem) = name.rsplit_once('.').map(|(left, _)| left.to_string()) {
                    tokens.push(stem);
                }
            }
            for token in tokens {
                // coremail 的地址里 @ 可能被编码成 %40，也可能只用了前半段 ID
                let mut candidates = vec![token.clone(), token.replace('@', "%40")];
                if let Some((head, _tail)) = token.split_once('@') {
                    candidates.push(head.to_string());
                }
                if let Some(stem) = token.rsplit_once('.').map(|(left, _)| left.to_string()) {
                    candidates.push(stem);
                }
                for candidate in candidates {
                    if candidate.len() >= 8 {
                        html = replace_img_src_by_token(&html, &candidate, &url);
                    }
                }
            }
        }

        // ④ 还有拿不到的图片（coremail 接口地址 / cid 没对上）：按**出现顺序**依次
        //    用图片附件的 data URL 顶上；替补用完了才删掉那个 <img>。
        //    注意替补必须来自 `attachments()`（`parts` 里拿不到这些字节，早先踩过）。
        html = fill_missing_images(&html, &spare);
    }

    let body = MailBody {
        uid: uid.to_string(),
        subject,
        from: from_text(&message),
        date: header_text(&message, "Date"),
        html,
        text,
        attachments,
    };
    let _ = session.logout();
    Ok(body)
}

fn media_dir(uid: &str) -> PathBuf {
    let safe = uid.chars().filter(|ch| ch.is_ascii_alphanumeric()).collect::<String>();
    crate::paths::data_dir().join("mail_media").join(safe)
}

/// 把「src 里带某个特征」的 `<img>` 的整个 src 换成给定 URL（用于 coremail 的接口式图片地址）
pub fn replace_img_src_by_token(html: &str, token: &str, url: &str) -> String {
    let lower = html.to_lowercase();
    let token_low = token.to_lowercase();
    let mut out = String::with_capacity(html.len());
    let mut index = 0usize;

    while let Some(start) = lower[index..].find("<img") {
        let at = index + start;
        out.push_str(&html[index..at]);
        let Some(close) = html[at..].find('>').map(|offset| at + offset) else {
            out.push_str(&html[at..]);
            return out;
        };
        let tag = &html[at..=close];
        let tag_low = tag.to_lowercase();
        if tag_low.contains(&token_low) && !tag_low.contains("src=\"data:") {
            // 找出 src="…" 并整段替换
            if let Some(src_at) = tag_low.find("src=") {
                let after = &tag[src_at + 4..];
                let quote = after.chars().next().unwrap_or('"');
                if quote == '"' || quote == '\'' {
                    if let Some(end) = after[1..].find(quote) {
                        let replaced = format!(
                            "{}{}{}{}{}",
                            &tag[..src_at],
                            "src=",
                            quote,
                            url,
                            &after[end + 1..]
                        );
                        out.push_str(&replaced);
                        index = close + 1;
                        continue;
                    }
                }
            }
        }
        out.push_str(tag);
        index = close + 1;
    }
    out.push_str(&html[index.min(html.len())..]);
    out
}

/// 图片识别：有些邮件把图片发成 `application/octet-stream`（旧版也踩过），
/// 所以除了 MIME 还要看扩展名和文件头。
pub fn looks_like_image(name: &str, data: &[u8]) -> bool {
    let lower = name.to_lowercase();
    let by_name = [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp"]
        .iter()
        .any(|suffix| lower.ends_with(suffix));
    let by_magic = data.starts_with(&[0x89, b'P', b'N', b'G'])
        || data.starts_with(&[0xFF, 0xD8, 0xFF])
        || data.starts_with(b"GIF8")
        || (data.len() > 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP")
        || data.starts_with(b"BM");
    by_name || by_magic
}

/// 图片的 MIME 推断（给 data URL 用）
fn image_mime(name: &str, data: &[u8]) -> &'static str {
    let lower = name.to_lowercase();
    if lower.ends_with(".png") || data.starts_with(&[0x89, b'P', b'N', b'G']) {
        return "png";
    }
    if lower.ends_with(".gif") || data.starts_with(b"GIF8") {
        return "gif";
    }
    if lower.ends_with(".webp") || (data.len() > 12 && data.starts_with(b"RIFF")) {
        return "webp";
    }
    if lower.ends_with(".bmp") || data.starts_with(b"BM") {
        return "bmp";
    }
    "jpeg"
}

/// 正文里「拿不到」的图片按顺序用替补图（图片附件）填上；替补用完了就删掉那个 `<img>`。
/// 复旦 webmail 的正文图片地址是 `…/coremail/s/json?…func=mbox:getComposeData`，
/// 要 webmail 会话才取得到；但图片字节就在附件里，所以按顺序顶上是可靠的做法。
pub fn fill_missing_images(html: &str, spare: &[String]) -> String {
    let lower = html.to_lowercase();
    let mut out = String::with_capacity(html.len());
    let mut index = 0usize;
    let mut used = 0usize;

    while let Some(start) = lower[index..].find("<img") {
        let at = index + start;
        out.push_str(&html[index..at]);
        let Some(close) = html[at..].find('>').map(|offset| at + offset) else {
            out.push_str(&html[at..]);
            return out;
        };
        let tag = &html[at..=close];
        let tag_low = tag.to_lowercase();
        let missing = tag_low.contains("src=\"cid:")
            || tag_low.contains("src='cid:")
            || tag_low.contains("src=cid:")
            || tag_low.contains("src=\"\"")
            || tag_low.contains("src=''")
            || tag_low.contains("func=mbox")
            || tag_low.contains("getcomposedata:");
        if !missing {
            out.push_str(tag);
        } else if used < spare.len() {
            if let Some(src_at) = tag_low.find("src=") {
                let after = &tag[src_at + 4..];
                let quote = after.chars().next().unwrap_or('"');
                if quote == '"' || quote == '\'' {
                    if let Some(end) = after[1..].find(quote) {
                        out.push_str(&format!(
                            "{}{}{}{}{}",
                            &tag[..src_at],
                            "src=",
                            quote,
                            spare[used],
                            &after[end + 1..]
                        ));
                        used += 1;
                        index = close + 1;
                        continue;
                    }
                }
            }
            used += 1;
        }
        index = close + 1;
    }
    out.push_str(&html[index.min(html.len())..]);
    out
}

/// 删掉正文里「拿不到」的图片：`cid:` 没替换成功的、空 src 的、coremail 接口地址的。
/// 远程 http(s) 图保留（WebView 会联网取），`data:` 图保留。
pub fn remove_broken_images(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let lower = html.to_lowercase();
    let mut index = 0usize;
    while let Some(start) = lower[index..].find("<img") {
        let at = index + start;
        out.push_str(&html[index..at]);
        let Some(close) = html[at..].find('>').map(|offset| at + offset) else {
            // 没有闭合：剩下的原样输出，避免吞内容
            out.push_str(&html[at..]);
            return out;
        };
        let tag = &html[at..=close];
        let tag_lower = tag.to_lowercase();
        let broken = tag_lower.contains("src=\"cid:")
            || tag_lower.contains("src='cid:")
            || tag_lower.contains("src=cid:")
            || tag_lower.contains("src=\"\"")
            || tag_lower.contains("src=''")
            // 复旦 webmail 的接口式图片地址：拿不到（要 webmail 会话）
            || tag_lower.contains("getcomposedata")
            || tag_lower.contains("func=mbox");
        if !broken {
            out.push_str(tag);
        }
        index = close + 1;
    }
    out.push_str(&html[index.min(html.len())..]);
    out
}

fn safe_name(name: &str, index: usize) -> String {
    let cleaned: String = name
        .chars()
        .map(|ch| if "\\/:*?\"<>|".contains(ch) { '_' } else { ch })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.');
    if trimmed.is_empty() {
        format!("附件{}", index + 1)
    } else {
        trimmed.to_string()
    }
}

fn file_url(path: &std::path::Path) -> String {
    path.display().to_string().replace('\\', "/")
}

/// 把本地文件转成 WebView 能显示的 URL
pub fn to_display_path(path: &str) -> String {
    path.replace('\\', "/")
}

// ------------------------------------------------------------------ 发信

pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub encryption: String,
    pub user: String,
    pub password: String,
    pub from: String,
}

pub fn smtp_config(store: &Store) -> Result<SmtpConfig, String> {
    let mut host = setting(store, "smtp_host");
    let mut user = setting(store, "smtp_user");
    let mut port: u16 = setting(store, "smtp_port").parse().unwrap_or(465);
    let mut encryption = setting(store, "smtp_enc");
    let mut secret_key = "smtp_password".to_string();

    if host.trim().is_empty() || user.trim().is_empty() {
        // 旧版：smtp_password::<id>，账户信息在 settings 里（smtp_host / smtp_user）
        let legacy_host = setting(store, "smtp_host");
        let legacy_user = setting(store, "smtp_user");
        if !legacy_host.trim().is_empty() {
            host = legacy_host;
            user = legacy_user;
        }
    }
    if host.trim().is_empty() || user.trim().is_empty() {
        return Err("还没配发件邮箱：设置 → 发件邮箱（SMTP）".to_string());
    }
    if let Some((id, _)) = first_smtp_account(store) {
        secret_key = format!("smtp_password::{id}");
    }
    let encrypted = store
        .setting(&secret_key)
        .or_else(|| store.setting("smtp_password"))
        .unwrap_or_default();
    let password = if encrypted.is_empty() {
        String::new()
    } else {
        crate::secrets::unprotect(&encrypted)?
    };
    if password.is_empty() {
        return Err("发件密码还没填（QQ / 163 要用授权码）".to_string());
    }
    if port == 0 {
        port = 465;
    }
    if encryption.trim().is_empty() {
        encryption = "SSL/TLS".to_string();
    }
    let from = {
        // 旧版 sender_email 优先（独立发件地址），否则 smtp_from / smtp_username
        let sender = setting(store, "sender_email");
        let value = if sender.trim().is_empty() {
            setting(store, "smtp_from")
        } else {
            sender
        };
        let address = if value.trim().is_empty() {
            let fallback = setting(store, "smtp_username");
            if fallback.trim().is_empty() {
                user.clone()
            } else {
                fallback
            }
        } else {
            value
        };
        // 显示名（旧版 sender_name）：`名字 <地址>`，地址里已经带 < 就不动
        let name = setting(store, "sender_name");
        if name.trim().is_empty() || address.contains('<') {
            address
        } else {
            format!("{} <{}>", name.trim(), address.trim())
        }
    };
    Ok(SmtpConfig {
        host,
        port,
        encryption,
        user,
        password,
        from,
    })
}

/// 旧版 SMTP 账户表（如果有）：(id, host)
fn first_smtp_account(store: &Store) -> Option<(i64, String)> {
    let conn = store.connection();
    let mut stmt = conn
        .prepare("SELECT id, host FROM smtp_accounts ORDER BY id LIMIT 1")
        .ok()?;
    let mut rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1).unwrap_or_default(),
            ))
        })
        .ok()?;
    rows.next()?.ok()
}

fn build_message(
    config: &SmtpConfig,
    to: &str,
    subject: &str,
    html: &str,
    attachments: &[String],
) -> Result<lettre::Message, String> {
    use lettre::message::header::{ContentType, HeaderValue};
    use lettre::message::{Attachment, MultiPart, SinglePart};
    use lettre::Message;

    let from_mailbox = config
        .from
        .parse::<lettre::message::Mailbox>()
        .map_err(|err| format!("发件地址不对：{err}"))?;
    let mut builder = Message::builder()
        .from(from_mailbox)
        .subject(subject)
        .header(ContentType::parse("text/html; charset=utf-8").unwrap());
    for address in to.split([';', ',']) {
        let trimmed = address.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mailbox = trimmed
            .parse::<lettre::message::Mailbox>()
            .map_err(|err| format!("收件人地址不对（{trimmed}）：{err}"))?;
        builder = builder.to(mailbox);
    }
    let body = SinglePart::builder()
        .header(ContentType::parse("text/html; charset=utf-8").unwrap())
        .body(html.to_string());
    if attachments.is_empty() {
        return builder
            .multipart(MultiPart::alternative().singlepart(body))
            .map_err(|err| format!("组邮件失败：{err}"));
    }
    let mut multipart = MultiPart::mixed().singlepart(body);
    for path in attachments {
        let data = std::fs::read(path).map_err(|err| format!("读附件失败（{path}）：{err}"))?;
        let name = std::path::Path::new(path)
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| "附件".to_string());
        let part = Attachment::new(name)
            .body(data, ContentType::parse("application/octet-stream").unwrap());
        multipart = multipart.singlepart(part);
    }
    let _ = &attachments;
    builder
        .multipart(multipart)
        .map_err(|err| format!("组邮件失败：{err}"))
}

pub fn send(
    store: &Store,
    to: &str,
    subject: &str,
    html: &str,
    attachments: &[String],
) -> Result<(), String> {
    use lettre::{SmtpTransport, Transport};

    let config = smtp_config(store)?;
    let message = build_message(&config, to, subject, html, attachments)?;
    let builder = match config.encryption.to_lowercase().as_str() {
        "starttls" => SmtpTransport::starttls_relay(&config.host)
            .map_err(|err| format!("连接失败：{err}"))?,
        "不加密" | "none" | "plain" => SmtpTransport::builder_dangerous(&config.host),
        _ => SmtpTransport::relay(&config.host).map_err(|err| format!("连接失败：{err}"))?,
    };
    let transport = builder
        .port(config.port)
        .credentials(lettre::transport::smtp::authentication::Credentials::new(
            config.user.clone(),
            config.password.clone(),
        ))
        .build();
    transport
        .send(&message)
        .map(|_response| ())
        .map_err(|err| explain(err.to_string()))
}

/// 把 SMTP 的英文报错翻成旧版那种人话
pub fn explain(raw: String) -> String {
    let lower = raw.to_lowercase();
    if lower.contains("535") || lower.contains("authentication") {
        return "登录被服务器拒绝（535）：账号或密码不对。QQ / 163 / 126 要用授权码，\
                Gmail 要用应用专用密码；到 设置 → 发件邮箱 里改一次密码。"
            .to_string();
    }
    if lower.contains("530") {
        return "服务器要求先认证（530）：这家服务器需要「账号 + 授权码」，\
                并且要先开启 SMTP 服务。"
            .to_string();
    }
    if lower.contains("timeout") || lower.contains("timed out") {
        return format!("连接超时：检查网络 / 端口（465 用 SSL，587 用 STARTTLS）。原始错误：{raw}");
    }
    if lower.contains("certificate") {
        return format!("证书校验失败：{raw}");
    }
    raw
}

#[allow(dead_code)]
pub fn write_debug(path: &str, data: &[u8]) {
    if let Ok(mut handle) = std::fs::File::create(path) {
        let _ = handle.write_all(data);
    }
}


#[cfg(test)]
mod tests {
    /// 日期兜底：任何一步都不许返回空串（空串会让新邮件在列表里排到最后 = 看不见）
    #[test]
    fn mail_date_falls_back_and_never_empty() {
        let stamp = 1_760_000_000_i64; // 2025-10-09 附近，随便一个确定的时间
        let text = format_local(stamp);
        assert!(text.len() >= 16 && text.contains(':'), "{text}");
        // ① 解析出来的 Date 优先
        assert_eq!(
            pick_date_text(Some(stamp), "Tue, 03 Jun 2025 09:15:00 +0000", Some(stamp + 60), stamp),
            text
        );
        // ② 没有解析结果时用 Date 原文（chrono 会校验星期，所以这里用真的星期二）
        let rfc = "Tue, 03 Jun 2025 09:15:00 +0000";
        let expect_raw = format_local(
            chrono::DateTime::parse_from_rfc2822(rfc).expect("样例日期要能解析").timestamp(),
        );
        assert_eq!(pick_date_text(None, rfc, None, stamp), expect_raw);
        assert!(!expect_raw.is_empty());
        // ③ Date 原文也没有 → INTERNALDATE
        assert_eq!(pick_date_text(None, "", Some(stamp), stamp), text);
        // ④ 全都没有 → 现在（也绝不能是空串）
        let now_text = pick_date_text(None, "", None, stamp);
        assert_eq!(now_text, text);
        assert!(!now_text.is_empty());
        // ⑤ 乱码 Date 也不至于空
        assert!(!pick_date_text(None, "not a date", None, stamp).is_empty());
    }

    use super::*;

    /// 真数据用例：数据来自临时副本（见 `crate::testdata`），不碰用户的真实库。
    fn real_store() -> Store {
        crate::testdata::real_store("mail")
    }

    #[test]
    fn broken_images_are_removed() {
        let html = r#"<p>a</p><img src="cid:abc@x"><img src="data:image/png;base64,AAA">
            <img src="https://e.com/a.png"><img src="">"#;
        let cleaned = remove_broken_images(html);
        assert!(!cleaned.contains("cid:abc@x"), "cid 图没删掉：{cleaned}");
        assert!(!cleaned.contains("src=\"\""), "空 src 没删掉：{cleaned}");
        assert!(cleaned.contains("data:image/png;base64,AAA"));
        assert!(cleaned.contains("https://e.com/a.png"));
        assert!(cleaned.contains("<p>a</p>"));
    }

    /// 真联网：拉一封带内联图的邮件，确认正文里没有残留 cid:
    #[test]
    #[ignore]
    fn inline_images_from_real_mail() {
        let store = real_store();
        let rows = list_inbox(&store, 30).expect("收不到邮件");
        let mut checked = 0;
        let mut total_images = 0;
        for row in rows.iter().take(10) {
            let Ok(body) = fetch_body(&store, &row.uid) else {
                continue;
            };
            let lower = body.html.to_lowercase();
            let inline = body.html.matches("data:image/").count();
            total_images += inline;
            checked += 1;
            println!(
                "- {}：正文 {} 字节 · data: 图 {} 张 · cid 残留 {} · 附件 {}",
                row.subject.chars().take(26).collect::<String>(),
                body.html.len(),
                inline,
                lower.matches("cid:").count(),
                body.attachments.len()
            );
            assert!(
                !lower.contains("cid:"),
                "《{}》正文里还有没替换掉的 cid:",
                row.subject
            );
        }
        println!("共检查 {checked} 封，内联图（data:）{total_images} 张");
        assert!(checked > 0);
    }

    /// 诊断：把某封邮件里所有 <img> 的 src 和所有附件的 cid/文件名打出来
    #[test]
    #[ignore]
    fn dump_image_refs() {
        let store = real_store();
        let rows = list_inbox(&store, 30).expect("收不到邮件");
        let Some(row) = rows.iter().find(|row| row.subject.contains("计算2班本周备忘")
            && !row.subject.contains("撤回")) else {
            println!("没找到目标邮件");
            return;
        };
        let body = fetch_body(&store, &row.uid).expect("取不到正文");
        let inline = body.html.matches("data:image/").count();
        println!("邮件：{}（uid={}）· 内联图 {} 张", row.subject, row.uid, inline);
        for part in body.html.split("<img").skip(1).take(3) {
            let snippet: String = part.chars().take(60).collect();
            println!("  <img{snippet}");
        }
        assert!(
            inline >= 3,
            "这封邮件的正文有 5 张内联图，应该都转成 data: 了，实际 {inline} 张"
        );
        for item in &body.attachments {
            println!("  附件：{}（{} 字节）", item.name, item.size);
        }
    }

    /// 临时目录 + 用完恢复 `LUMINA_DATA_DIR`（测试改进程级环境变量，必须还原）
    struct TempData {
        previous: Option<std::ffi::OsString>,
        dir: std::path::PathBuf,
    }

    impl TempData {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "danlume-mail-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let previous = std::env::var_os("LUMINA_DATA_DIR");
            std::env::set_var("LUMINA_DATA_DIR", &dir);
            Self { previous, dir }
        }
    }

    impl Drop for TempData {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var("LUMINA_DATA_DIR", value),
                None => std::env::remove_var("LUMINA_DATA_DIR"),
            }
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// AI 摘要那一列：老库自动补列，存进去能原样读回来，没写过的返回空串
    #[test]
    fn summary_column_migrates_and_round_trips() {
        let temp = TempData::new("sum");
        let store = crate::store::Store::open().expect("临时库");
        ensure_inbox_schema(&store);
        {
            let conn = store.connection();
            conn.execute(
                "INSERT INTO inbox_messages (account_id, uid, subject, fetched_at)
                 VALUES (0, 'u1', '选课通知', '2026-11-11 10:00')",
                [],
            )
            .expect("插一封邮件");
        }
        // 没生成过 = 空串（不是 None，前端好判断）
        assert_eq!(cached_summary(&store, "u1"), "");
        save_summary(&store, "u1", "· 教务处通知：周五前选课\n要做：周五 17:00 前提交");
        assert_eq!(
            cached_summary(&store, "u1"),
            "· 教务处通知：周五前选课\n要做：周五 17:00 前提交"
        );
        // 重新生成会盖掉旧的
        save_summary(&store, "u1", "· 换了一版");
        assert_eq!(cached_summary(&store, "u1"), "· 换了一版");
        // 库里没有的 uid 不 panic
        assert_eq!(cached_summary(&store, "不存在"), "");
        drop(store);
        drop(temp);
    }

    /// 真库副本：老库能补上 `summary_zh` 列，已有邮件一封不少，新列初始为空
    #[test]
    #[ignore]
    fn real_db_gains_summary_column() {
        /// 读表结构（照 `ensure_inbox_schema` 里的写法，避免借用活不过块尾）
        fn columns_of(store: &Store) -> Vec<String> {
            let conn = store.connection();
            let mut out: Vec<String> = Vec::new();
            if let Ok(mut stmt) = conn.prepare("PRAGMA table_info(inbox_messages)") {
                if let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(1)) {
                    out = rows.flatten().collect();
                }
            }
            out
        }

        let store = real_store();
        ensure_inbox_schema(&store);
        let columns = columns_of(&store);
        assert!(
            columns.iter().any(|one| one == "summary_zh"),
            "没补上摘要列：{columns:?}"
        );
        let rows = cached_inbox(&store, 5);
        println!("真库副本里读到 {} 封邮件", rows.len());
        assert!(!rows.is_empty(), "补列之后读不到邮件了");
        for row in &rows {
            assert_eq!(cached_summary(&store, &row.uid), "", "新列应当是空的");
        }
        let uid = rows[0].uid.clone();
        save_summary(&store, &uid, "· 测试摘要");
        assert_eq!(cached_summary(&store, &uid), "· 测试摘要");
    }
}

// ------------------------------------------------------------------ 收件箱本地缓存
//
// 对应旧版 `imap_inbox.py` 的 `save_messages` / `cached_messages`：收到的邮件头（正文按需）
// 存进 `inbox_messages`，之后进邮件页先读本地、只向上取「比本地最大 UID 更大」的增量。

#[derive(Debug, Clone, serde::Serialize)]
pub struct CachedMail {
    pub uid: String,
    pub subject: String,
    #[serde(rename = "from")]
    pub from_addr: String,
    pub date: String,
    pub unread: bool,
    pub has_attachments: bool,
    pub has_body: bool,
}

pub fn ensure_inbox_schema(store: &Store) {
    let conn = store.connection();
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS inbox_messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            account_id INTEGER NOT NULL DEFAULT 0,
            account_name TEXT NOT NULL DEFAULT '',
            uid TEXT NOT NULL,
            subject TEXT NOT NULL DEFAULT '',
            sender TEXT NOT NULL DEFAULT '',
            date TEXT NOT NULL DEFAULT '',
            snippet TEXT NOT NULL DEFAULT '',
            body TEXT NOT NULL DEFAULT '',
            body_text TEXT NOT NULL DEFAULT '',
            links TEXT NOT NULL DEFAULT '',
            unread INTEGER NOT NULL DEFAULT 0,
            has_attachments INTEGER NOT NULL DEFAULT 0,
            fetched_at TEXT NOT NULL,
            UNIQUE(account_id, uid)
        )",
        [],
    );
    // 老数据里 date 是空串的（Date 头解析不出来），拿 fetched_at 补一下：
    // 否则 `ORDER BY date DESC` 会把它们排到最后，列表前 30 条里永远看不到这些邮件
    //（2026-11-14 用户报「收了新邮件但完全不显示」就是这个）。
    let _ = conn.execute(
        "UPDATE inbox_messages
            SET date = substr(COALESCE(NULLIF(fetched_at, ''), datetime('now', 'localtime')), 1, 16)
          WHERE COALESCE(date, '') = ''",
        [],
    );

    // 老库（Python 建的）没有这两列，补上；补不了就算了，读的时候有默认值
    let columns: Vec<String> = {
        let mut out = Vec::new();
        if let Ok(mut stmt) = conn.prepare("PRAGMA table_info(inbox_messages)") {
            if let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(1)) {
                out = rows.flatten().collect();
            }
        }
        out
    };
    if !columns.iter().any(|one| one == "unread") {
        let _ = conn.execute(
            "ALTER TABLE inbox_messages ADD COLUMN unread INTEGER NOT NULL DEFAULT 0",
            [],
        );
    }
    if !columns.iter().any(|one| one == "has_attachments") {
        let _ = conn.execute(
            "ALTER TABLE inbox_messages ADD COLUMN has_attachments INTEGER NOT NULL DEFAULT 0",
            [],
        );
    }
    // 正文分 html / 纯文本两份存：以前合成一份，读回来只能猜，纯文本邮件就退化成 <pre> 等宽字体
    if !columns.iter().any(|one| one == "body_text") {
        let _ = conn.execute(
            "ALTER TABLE inbox_messages ADD COLUMN body_text TEXT NOT NULL DEFAULT ''",
            [],
        );
    }
    // AI 摘要（用户 2026-11-11：每封邮件配一个 AI 摘要，放在邮件上端）。
    // 存下来就不会每开一次邮件就调一次接口。
    if !columns.iter().any(|one| one == "summary_zh") {
        let _ = conn.execute(
            "ALTER TABLE inbox_messages ADD COLUMN summary_zh TEXT NOT NULL DEFAULT ''",
            [],
        );
    }
    // 附件清单（只有名字 / 大小，**不存内容**）：摘要里要写「带了什么附件」，
    // 用户明确说过「附件内容不用上传，只上传名称和格式」。
    if !columns.iter().any(|one| one == "attachments_json") {
        let _ = conn.execute(
            "ALTER TABLE inbox_messages ADD COLUMN attachments_json TEXT NOT NULL DEFAULT '[]'",
            [],
        );
    }
    // 摘要里「可能需要加进日程的事」：JSON 数组；`events_at` 非空表示已经抽过了
    // （抽过但确实是空的也要记下来，不然每次开这封邮件都要再问一次模型）
    if !columns.iter().any(|one| one == "events_json") {
        let _ = conn.execute(
            "ALTER TABLE inbox_messages ADD COLUMN events_json TEXT NOT NULL DEFAULT '[]'",
            [],
        );
    }
    if !columns.iter().any(|one| one == "events_at") {
        let _ = conn.execute(
            "ALTER TABLE inbox_messages ADD COLUMN events_at TEXT NOT NULL DEFAULT ''",
            [],
        );
    }
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_inbox_date ON inbox_messages(date)",
        [],
    );
}

/// 把某封邮件的 AI 摘要存进缓存
pub fn save_summary(store: &Store, uid: &str, text: &str) {
    ensure_inbox_schema(store);
    let conn = store.connection();
    let _ = conn.execute(
        "UPDATE inbox_messages SET summary_zh = ?1 WHERE uid = ?2",
        rusqlite::params![text, uid],
    );
}

/// 缓存里的 AI 摘要（没有就是空串）
pub fn cached_summary(store: &Store, uid: &str) -> String {
    ensure_inbox_schema(store);
    let conn = store.connection();
    conn.query_row(
        "SELECT summary_zh FROM inbox_messages WHERE uid = ?1 LIMIT 1",
        [uid],
        |row| row.get::<_, String>(0),
    )
    .unwrap_or_default()
}

fn account_tag(store: &Store) -> String {
    store.setting("imap_user").unwrap_or_default()
}

/// 把拉到的邮件头写进本地库（同 uid 覆盖；正文保留）
pub fn save_summaries(store: &Store, rows: &[MailSummary]) -> usize {
    ensure_inbox_schema(store);
    let account = account_tag(store);
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let conn = store.connection();
    let mut saved = 0usize;
    for row in rows {
        if row.uid.trim().is_empty() {
            continue;
        }
        let ok = conn.execute(
            "INSERT INTO inbox_messages(account_id, account_name, uid, subject, sender,
                date, fetched_at, unread, has_attachments)
             VALUES (0, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(account_id, uid) DO UPDATE SET
                subject = excluded.subject,
                sender = excluded.sender,
                date = excluded.date,
                fetched_at = excluded.fetched_at,
                unread = excluded.unread,
                has_attachments = excluded.has_attachments",
            rusqlite::params![
                account,
                row.uid,
                row.subject,
                row.from,
                row.date,
                stamp,
                row.unread as i64,
                row.has_attachments as i64,
            ],
        );
        if ok.is_ok() {
            saved += 1;
        }
    }
    saved
}

/// 本地缓存的邮件头（按日期倒序）
pub fn cached_inbox(store: &Store, limit: usize) -> Vec<CachedMail> {
    ensure_inbox_schema(store);
    let conn = store.connection();
    let mut out = Vec::new();
    let Ok(mut stmt) = conn.prepare(
        "SELECT uid, subject, sender, date, unread, has_attachments,
                length(COALESCE(body, '')) AS body_len
         FROM inbox_messages ORDER BY date DESC, id DESC LIMIT ?1",
    ) else {
        return out;
    };
    if let Ok(rows) = stmt.query_map([limit as i64], |row| {
        Ok(CachedMail {
            uid: row.get::<_, String>("uid").unwrap_or_default(),
            subject: row.get::<_, String>("subject").unwrap_or_default(),
            from_addr: row.get::<_, String>("sender").unwrap_or_default(),
            date: row.get::<_, String>("date").unwrap_or_default(),
            unread: row.get::<_, i64>("unread").unwrap_or(0) != 0,
            has_attachments: row.get::<_, i64>("has_attachments").unwrap_or(0) != 0,
            has_body: row.get::<_, i64>("body_len").unwrap_or(0) > 0,
        })
    }) {
        out = rows.flatten().collect();
    }
    out
}

/// 本地缓存了多少封
pub fn cached_count(store: &Store) -> usize {
    ensure_inbox_schema(store);
    let conn = store.connection();
    conn.query_row("SELECT COUNT(*) FROM inbox_messages", [], |row| {
        row.get::<_, i64>(0)
    })
    .unwrap_or(0) as usize
}

/// 缓存里最大的 UID（数字比较；没有缓存返回 0）
pub fn max_cached_uid(store: &Store) -> u64 {
    ensure_inbox_schema(store);
    let conn = store.connection();
    let mut best = 0u64;
    if let Ok(mut stmt) = conn.prepare("SELECT uid FROM inbox_messages") {
        if let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) {
            for text in rows.flatten() {
                best = best.max(numeric(&text));
            }
        }
    }
    best
}

/// 正文缓存：点开过的邮件不再联网
pub fn save_body(store: &Store, uid: &str, html: &str, text: &str) {
    ensure_inbox_schema(store);
    if html.trim().is_empty() && text.trim().is_empty() {
        return;
    }
    let conn = store.connection();
    let _ = conn.execute(
        "UPDATE inbox_messages SET body = ?1, body_text = ?2 WHERE uid = ?3",
        rusqlite::params![html, text, uid],
    );
}

/// 把附件清单（**只有名字和大小，不存内容**）记在邮件头上，摘要要用
pub fn save_attachments(store: &Store, uid: &str, attachments: &[serde_json::Value]) {
    ensure_inbox_schema(store);
    let text = serde_json::Value::Array(attachments.to_vec()).to_string();
    let conn = store.connection();
    let _ = conn.execute(
        "UPDATE inbox_messages SET attachments_json = ?1 WHERE uid = ?2",
        rusqlite::params![text, uid],
    );
}

/// 缓存里的附件清单：`[{name, size}]`；没存过就是空数组
pub fn cached_attachments(store: &Store, uid: &str) -> Vec<serde_json::Value> {
    ensure_inbox_schema(store);
    let conn = store.connection();
    let raw: String = conn
        .query_row(
            "SELECT attachments_json FROM inbox_messages WHERE uid = ?1 LIMIT 1",
            [uid],
            |row| row.get::<_, String>(0),
        )
        .unwrap_or_default();
    serde_json::from_str::<serde_json::Value>(&raw)
        .ok()
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
}

/// 把「可能要做的事」存进缓存（JSON 数组），同时打上「抽过了」的时间戳
pub fn save_events(store: &Store, uid: &str, items: &[serde_json::Value]) {
    ensure_inbox_schema(store);
    let text = serde_json::Value::Array(items.to_vec()).to_string();
    let conn = store.connection();
    let _ = conn.execute(
        "UPDATE inbox_messages SET events_json = ?1, events_at = ?2 WHERE uid = ?3",
        rusqlite::params![text, crate::now_str(), uid],
    );
}

/// 抽取逻辑的版本号：提示词 / 解析规则一改就要 +1
///
/// 为什么需要：抽出来的结果缓存在 `events_at` 里，改了规则也不会重抽，
/// 用户看到的一直是老结果（2026-11-11 用户两次报「怎么少一个」，就是老缓存没失效）。
/// 版本号对不上时把所有邮件的 `events_at` 清空，下次打开会按新规则重抽一次。
/// 3：日程改成「截止前 1 小时 → 截止」（用户 2026-11-14 的要求），老缓存要作废
pub const EVENTS_VERSION: u32 = 3;

/// 版本变了就清一遍「抽过了」标记（每个进程只做一次）
pub fn invalidate_old_event_cache(store: &Store) {
    ensure_inbox_schema(store);
    let current = store
        .setting("mail_events_version")
        .and_then(|text| text.trim().parse::<u32>().ok())
        .unwrap_or(0);
    if current == EVENTS_VERSION {
        return;
    }
    let conn = store.connection();
    let _ = conn.execute("UPDATE inbox_messages SET events_at = ''", []);
    drop(conn);
    let _ = store.set_setting("mail_events_version", &EVENTS_VERSION.to_string());
}

/// 缓存里的「可能要做的事」：`(items, 是否已经抽过)`
///
/// 第二个值很关键：抽过但结果为空也要记住，不然每次打开这封邮件都会再问一次模型。
pub fn cached_events(store: &Store, uid: &str) -> (Vec<serde_json::Value>, bool) {
    ensure_inbox_schema(store);
    let conn = store.connection();
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT events_json, events_at FROM inbox_messages WHERE uid = ?1 LIMIT 1",
            [uid],
            |row| {
                Ok((
                    row.get::<_, String>(0).unwrap_or_default(),
                    row.get::<_, String>(1).unwrap_or_default(),
                ))
            },
        )
        .ok();
    let (raw, at) = row.unwrap_or_default();
    let items = serde_json::from_str::<serde_json::Value>(&raw)
        .ok()
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    (items, !at.trim().is_empty())
}

/// 缓存里的正文 + 邮件头：`(html, text, subject, from, date)`
pub fn cached_body(store: &Store, uid: &str) -> Option<(String, String, String, String, String)> {
    ensure_inbox_schema(store);
    let conn = store.connection();
    conn.query_row(
        "SELECT body, body_text, subject, sender, date FROM inbox_messages WHERE uid = ?1 LIMIT 1",
        [uid],
        |row| {
            Ok((
                row.get::<_, String>(0).unwrap_or_default(),
                row.get::<_, String>(1).unwrap_or_default(),
                row.get::<_, String>(2).unwrap_or_default(),
                row.get::<_, String>(3).unwrap_or_default(),
                row.get::<_, String>(4).unwrap_or_default(),
            ))
        },
    )
    .ok()
    .filter(|(html, text, _, _, _)| !html.trim().is_empty() || !text.trim().is_empty())
}

/// 增量同步：只取服务器上「UID 比本地最大 UID 更大」的邮件；本地空缓存时取最近 `limit` 封。
pub fn sync_inbox(store: &Store, limit: usize) -> Result<serde_json::Value, String> {
    let config = config(store)?;
    let before = cached_count(store);
    let max_uid = max_cached_uid(store);
    let mut session = connect(&config)?;
    session
        .select("INBOX")
        .map_err(|err| format!("打开收件箱失败：{err}"))?;
    // 增量：`UID (max+1):*`；服务器不支持该写法时会返回全部，我们再用 UID 过滤兜底
    let query = format!("UID {}:*", max_uid + 1);
    let uids = session
        .uid_search(&query)
        .map_err(|err| format!("查询新邮件失败：{err}"))?;
    let mut wanted: Vec<u32> = uids
        .into_iter()
        .filter(|uid| (*uid as u64) > max_uid)
        .collect();
    wanted.sort_unstable();
    if wanted.len() > limit {
        wanted = wanted[wanted.len() - limit..].to_vec();
    }
    if wanted.is_empty() {
        let _ = session.logout();
        return Ok(json!({ "ok": true, "added": 0, "cached": before, "fetched": 0 }));
    }
    let set = wanted
        .iter()
        .map(|uid| uid.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let fetches = session
        .uid_fetch(&set, "(UID FLAGS BODY.PEEK[HEADER])")
        .map_err(|err| format!("取邮件头失败：{err}"))?;
    let mut rows: Vec<MailSummary> = Vec::new();
    for fetch in fetches.iter() {
        let Some(body) = fetch.header() else { continue };
        let Some(message) = mail_parser::MessageParser::default().parse(body) else {
            continue;
        };
        let uid = fetch
            .uid
            .map(|value| value.to_string())
            .unwrap_or_default();
        let flags = format!("{:?}", fetch.flags());
        let subject = subject_text(&message);
        let content_type = header_text(&message, "Content-Type");
        rows.push(MailSummary {
            uid,
            subject: if subject.is_empty() {
                "（无主题）".to_string()
            } else {
                subject
            },
            from: from_text(&message),
            date: header_text(&message, "Date"),
            unread: !flags.contains("Seen"),
            has_attachments: content_type.to_lowercase().contains("multipart"),
        });
    }
    let _ = session.logout();
    let fetched = rows.len();
    let added = save_summaries(store, &rows);
    Ok(json!({
        "ok": true,
        "added": added,
        "fetched": fetched,
        "cached": cached_count(store),
    }))
}
