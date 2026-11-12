//! 内嵌网页版面板的后端支撑（对齐旧版 `web_panel.py`）。
//!
//! 旧版用 QtWebEngine 内嵌网页，Rust 版直接交给前端的 `<iframe>`——iframe 是普通 HTML 元素，
//! 不需要任何 Tauri 权限。这里只剩一件事后端才做得动：
//! 「被站点禁止嵌入（X-Frame-Options / CSP）时，用系统默认浏览器打开」。

/// 用系统默认浏览器打开一个 http(s) 地址。
///
/// 只收 http / https：别的协议（file:、javascript:…）一律拒绝，
/// 免得设置里写错一行就变成「随便执行点什么」。
///
/// Windows 上不再走 `cmd /C start "" "<url>"`：`std::process::Command` 会自己给参数加引号，
/// 于是那对人工加的引号变成了字面上的 `\"`，用户看到的是
/// 「Windows 找不到文件 '\https://…\ '」（2026-11-12 用户实测截图）。
/// 改成直接调 ShellExecuteW：参数原样交给系统，地址里的 `&` 也不会被当成命令分隔符。
pub fn open_url(url: &str) -> Result<(), String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("地址是空的：先在 设置 → AI → 网页版面板 里填一个 http(s) 地址".to_string());
    }
    let lower = trimmed.to_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err("只支持 http / https 地址".to_string());
    }
    if trimmed.contains('\n') || trimmed.contains('\r') {
        return Err("地址里有换行，不打开".to_string());
    }
    #[cfg(windows)]
    {
        use windows::core::HSTRING;
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let verb = HSTRING::from("open");
        let target = HSTRING::from(trimmed);
        // ShellExecuteW 的返回值 > 32 才算成功
        let code = unsafe { ShellExecuteW(None, &verb, &target, None, None, SW_SHOWNORMAL) };
        let raw = code.0 as isize;
        if raw <= 32 {
            return Err(format!("打不开系统浏览器（ShellExecute 返回 {raw}）"));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open")
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
        // 换行要挡掉
        assert!(open_url("https://a.com/\nx").is_err());
        // 带引号和 & 的地址不该被我们自己拦掉（现在没走 cmd，不需要人工加引号）
        assert!(open_url("https://a.com/\"x").is_ok());
        assert!(open_url("https://a.com/?a=1&b=2").is_ok());
    }
}
