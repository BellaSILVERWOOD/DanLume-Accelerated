//! 内嵌网页版面板的后端支撑（对齐旧版 `web_panel.py`）。
//!
//! 旧版用 QtWebEngine 内嵌网页，Rust 版直接交给前端的 `<iframe>`——iframe 是普通 HTML 元素，
//! 不需要任何 Tauri 权限。这里只剩一件事后端才做得动：
//! 「被站点禁止嵌入（X-Frame-Options / CSP）时，用系统默认浏览器打开」。
use std::process::Command;

/// 用系统默认浏览器打开一个 http(s) 地址。
///
/// 只收 http / https：别的协议（file:、javascript:…）一律拒绝，
/// 免得设置里写错一行就变成「随便执行点什么」。
pub fn open_url(url: &str) -> Result<(), String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("地址是空的：先在 设置 → AI → 网页版面板 里填一个 http(s) 地址".to_string());
    }
    let lower = trimmed.to_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err("只支持 http / https 地址".to_string());
    }
    if trimmed.contains('"') || trimmed.contains('\n') || trimmed.contains('\r') {
        return Err("地址里有非法字符（引号 / 换行）".to_string());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        // `start` 的第一个参数是窗口标题占位；地址加引号，地址里的 & 才不会被 cmd 拆开。
        // 0x08000000 = CREATE_NO_WINDOW
        Command::new("cmd")
            .args(["/C", "start", "", &format!("\"{trimmed}\"")])
            .creation_flags(0x0800_0000)
            .spawn()
            .map_err(|err| format!("打不开系统浏览器：{err}"))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Command::new("xdg-open")
            .arg(trimmed)
            .spawn()
            .map_err(|err| format!("打不开系统浏览器：{err}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_http_urls() {
        for bad in ["", "   ", "file:///C:/Windows/System32", "javascript:alert(1)", "chat.deepseek.com"] {
            let result = open_url(bad);
            println!("{bad:?} → {result:?}");
            assert!(result.is_err(), "{bad:?} 应该被拒绝");
        }
        // 引号 / 换行也要挡掉（否则 cmd 的参数会被拆坏）
        assert!(open_url("https://a.com/\"x").is_err());
        assert!(open_url("https://a.com/\nx").is_err());
    }
}
