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
        let date = header_text(&message, "Date");
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
    use super::*;

    fn real_store() -> Store {
        std::env::set_var(
            "LUMINA_DATA_DIR",
            r"E:\FIles\Lena‘s Files\Work\znw\Agent\data",
        );
        Store::open().expect("打不开数据库")
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
}
