//! 发件身份 / 签名 / SMTP 认证方式（对应旧版 `accounts.py` + `mailer.py` 里那几项设置）。
//!
//! * `sender_name`：发件显示名（旧版用 `发件人名字 <地址>` 的格式）；
//! * `sender_email`：发件地址（不填就用 SMTP 用户名）；
//! * `mail_signature`：每封信末尾自动追加的签名，多行；
//! * `smtp_security`：`starttls` / `ssl` / `plain`（不加密），会映射到 v2 的 `smtp_enc`；
//! * `smtp_oauth`：设置了就明确拒绝——v2 还没做 OAuth2 发信，不能假装能发。
use crate::store::Store;

fn setting(store: &Store, key: &str) -> String {
    store.setting(key).unwrap_or_default().trim().to_string()
}

/// 发件身份：(显示名, 地址)；地址优先 `sender_email`，否则空（沿用 SMTP 用户名）
pub fn identity(store: &Store) -> (String, String) {
    (setting(store, "sender_name"), setting(store, "sender_email"))
}

/// 把签名追加到 HTML 正文末尾（纯文本行转 <br>）
pub fn with_signature(store: &Store, html: &str) -> String {
    let signature = store.setting("mail_signature").unwrap_or_default();
    let signature = signature.trim();
    if signature.is_empty() {
        return html.to_string();
    }
    let escaped = signature
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace("\r\n", "\n")
        .replace('\n', "<br>");
    format!(
        "{html}<hr style=\"border:0;border-top:1px solid #ddd;margin:18px 0 10px\">\
         <div style=\"font-size:13px;color:#666\">{escaped}</div>"
    )
}

/// 发信前的检查：OAuth 未支持要明确拒绝；`smtp_security` 映射成 `smtp_enc`
pub fn prepare(store: &Store) -> Result<(), String> {
    if setting(store, "smtp_oauth") != "" && setting(store, "smtp_oauth") != "0" {
        return Err(
            "这台机器配的是 OAuth2 授权（smtp_oauth），v2 还没做 OAuth 发信；\
             请到设置 → 发件邮箱里改用「授权码 / 密码」方式"
                .to_string(),
        );
    }
    let security = setting(store, "smtp_security").to_lowercase();
    if security.is_empty() {
        return Ok(());
    }
    let mapped = match security.as_str() {
        "ssl" | "smtps" | "ssl/tls" => "ssl",
        "starttls" | "tls" => "starttls",
        "plain" | "none" | "不加密" => "不加密",
        _ => return Ok(()),
    };
    // 只在用户没显式填 smtp_enc 时才映射，不覆盖手填的值
    if setting(store, "smtp_enc").is_empty() {
        store
            .set_setting("smtp_enc", mapped)
            .map_err(|exc| format!("写 smtp_enc 失败：{exc}"))?;
    }
    Ok(())
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

    #[test]
    fn signature_is_appended_and_escaped() {
        let store = temp_store("danlume-mailident-test");
        assert_eq!(with_signature(&store, "<p>hi</p>"), "<p>hi</p>");
        let _ = store.set_setting("mail_signature", "李同学\n2026 级 <b>");
        let html = with_signature(&store, "<p>hi</p>");
        println!("带签名：{html}");
        assert!(html.contains("李同学"));
        assert!(html.contains("2026 级 &lt;b&gt;"));
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-mailident-test"));
    }

    #[test]
    fn security_maps_and_oauth_refuses() {
        let store = temp_store("danlume-mailsec-test");
        let _ = store.set_setting("smtp_enc", "");
        let _ = store.set_setting("smtp_security", "starttls");
        prepare(&store).unwrap();
        assert_eq!(store.setting("smtp_enc").unwrap_or_default(), "starttls");
        let _ = store.set_setting("smtp_enc", "");
        let _ = store.set_setting("smtp_security", "plain");
        prepare(&store).unwrap();
        assert_eq!(store.setting("smtp_enc").unwrap_or_default(), "不加密");

        let _ = store.set_setting("smtp_oauth", "1");
        let err = prepare(&store).expect_err("OAuth 应该被拒绝");
        println!("拒绝信息：{err}");
        assert!(err.contains("OAuth"));
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-mailsec-test"));
    }
}
