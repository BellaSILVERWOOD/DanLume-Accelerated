//! 内嵌网页（对应旧版 `web_panel.EmbeddedWebPanel` / `page_ehall.py` / `page_homework.py`）。
//!
//! 旧版用 QtWebEngine 把 eHall / eLearning **嵌在页面里**；Rust 之前只会弹独立窗口。
//! 这里用 Tauri 的「子 Webview」（`Window::add_child`，需要 `unstable` 特性）把网页贴到
//! 主窗口的指定位置，前端给一个占位 div 的坐标即可——效果和 Python 版一致：不弹窗。
//!
//! 注意：`add_child` 内部要把活派发到主线程再等结果，所以**必须在工作线程里调**
//! （命令都用 async + `spawn_blocking`），否则会自锁，表现像「点了没反应」。
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde_json::{json, Value};
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, WebviewBuilder, WebviewUrl};

fn registry() -> &'static Mutex<HashMap<String, tauri::Webview>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, tauri::Webview>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn label_of(kind: &str) -> String {
    format!("embed-{kind}")
}

/// 打开（或复用）一个内嵌网页，位置/大小用逻辑像素（前端直接给 CSS 像素）
pub fn open(
    app: &AppHandle,
    kind: &str,
    url: &str,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<String, String> {
    if url.trim().is_empty() {
        return Err("地址是空的".to_string());
    }
    let label = label_of(kind);
    // 已经有这个内嵌网页：挪位置、必要时换地址，再显示出来
    {
        let mut guard = registry().lock().map_err(|_| "内嵌网页状态被占用".to_string())?;
        if let Some(webview) = guard.get(&label) {
            let _ = webview.set_position(LogicalPosition::new(x, y));
            let _ = webview.set_size(LogicalSize::new(width, height));
            let current = webview.url().map(|one| one.to_string()).unwrap_or_default();
            if current != url {
                let _ = webview.navigate(
                    url.parse::<tauri::Url>()
                        .map_err(|exc| format!("地址不对：{exc}"))?,
                );
            }
            let _ = webview.show();
            return Ok(label);
        }
    }
    let window = app
        .get_window("main")
        .ok_or_else(|| "找不到主窗口".to_string())?;
    let parsed = url
        .parse::<tauri::Url>()
        .map_err(|exc| format!("地址不对：{exc}"))?;
    // 内嵌网页也要「自动 UIS 登录」：和登录窗口注入的是同一段脚本，
    // 页面一旦跳到统一身份认证就把学号密码填好并点登录。
    let mut builder = WebviewBuilder::new(label.clone(), WebviewUrl::External(parsed));
    let autofill = crate::uis_autofill::script(crate::get_store());
    if !autofill.is_empty() {
        builder = builder.initialization_script(autofill);
    }
    let webview = window
        .add_child(
            builder,
            LogicalPosition::new(x, y),
            LogicalSize::new(width, height),
        )
        .map_err(|exc| format!("内嵌网页打不开：{exc}"))?;
    let mut guard = registry().lock().map_err(|_| "内嵌网页状态被占用".to_string())?;
    guard.insert(label.clone(), webview);
    Ok(label)
}

/// 跟随页面布局挪位置 / 改大小（滚动、切页、窗口缩放都要调）
pub fn bounds(kind: &str, x: f64, y: f64, width: f64, height: f64) -> Result<(), String> {
    let guard = registry().lock().map_err(|_| "内嵌网页状态被占用".to_string())?;
    let Some(webview) = guard.get(&label_of(kind)) else {
        return Ok(()); // 还没建出来，下次 open 时自然带上位置
    };
    let _ = webview.set_position(LogicalPosition::new(x, y));
    let _ = webview.set_size(LogicalSize::new(width, height));
    Ok(())
}

/// 关掉（切到别的页面时调用，否则它一直浮在最上面）
pub fn close(kind: &str) -> Result<(), String> {
    let label = label_of(kind);
    let taken = {
        let mut guard = registry().lock().map_err(|_| "内嵌网页状态被占用".to_string())?;
        guard.remove(&label)
    };
    if let Some(webview) = taken {
        let _ = webview.close();
    }
    Ok(())
}

pub fn reload(kind: &str) -> Result<(), String> {
    let guard = registry().lock().map_err(|_| "内嵌网页状态被占用".to_string())?;
    if let Some(webview) = guard.get(&label_of(kind)) {
        let _ = webview.reload();
    }
    Ok(())
}

pub fn navigate(kind: &str, url: &str) -> Result<(), String> {
    let parsed = url
        .parse::<tauri::Url>()
        .map_err(|exc| format!("地址不对：{exc}"))?;
    let guard = registry().lock().map_err(|_| "内嵌网页状态被占用".to_string())?;
    if let Some(webview) = guard.get(&label_of(kind)) {
        webview
            .navigate(parsed)
            .map_err(|exc| format!("跳转失败：{exc}"))?;
    }
    Ok(())
}

/// 在**看不见的地方**打开一个内嵌网页（空教室取数用）。
///
/// 位置放到窗口右侧之外：子 WebView 会被窗口客户区裁掉，用户看不到，
/// 但它照样按 1200×900 正常渲染，JS 抓表格不受影响。
/// 旧版对应「网页收起来，抓取照样在后台跑」（`page_classroom._set_web_visible(False)`）。
pub fn open_hidden(app: &AppHandle, kind: &str, url: &str) -> Result<(), String> {
    let width = 1200.0;
    open(app, kind, url, 20000.0, 0.0, width, 900.0)?;
    Ok(())
}

/// 跑一段 JS 并等它把结果交回来（读表格 / 选下拉 / 点查询都用这个）
pub fn eval_callback_collect(
    kind: &str,
    js: &str,
    timeout: std::time::Duration,
) -> Result<String, String> {
    let (sender, receiver) = std::sync::mpsc::channel::<String>();
    eval_callback(kind, js, move |raw| {
        let _ = sender.send(raw);
    })?;
    receiver
        .recv_timeout(timeout)
        .map_err(|_| "页面没有回数据（可能还没加载完）：等一会儿再试一次".to_string())
}

/// 在内嵌网页里跑一段 JS，把结果（JSON 字符串）交回给调用方。
///
/// 用途：空教室页的「抓取当前页面」——旧版是 `page.runJavaScript(CLASSROOM_JS, cb)`，
/// 这里换成 `eval_with_callback`，页面里的表格就能直接回给 Rust 解析。
pub fn eval_callback(
    kind: &str,
    js: &str,
    callback: impl Fn(String) + Send + 'static,
) -> Result<(), String> {
    let guard = registry()
        .lock()
        .map_err(|_| "内嵌网页状态被占用".to_string())?;
    let Some(webview) = guard.get(&label_of(kind)) else {
        return Err("内嵌网页还没打开（先点「显示网页」登录一次）".to_string());
    };
    webview
        .eval_with_callback(js.to_string(), callback)
        .map_err(|exc| format!("页面脚本执行失败：{exc}"))
}

/// 现在真正打开的地址（前端显示用；没开就返回空串）
pub fn current_url(kind: &str) -> String {
    let Ok(guard) = registry().lock() else {
        return String::new();
    };
    guard
        .get(&label_of(kind))
        .and_then(|webview| webview.url().ok())
        .map(|url| url.to_string())
        .unwrap_or_default()
}

/// 是否有这个内嵌网页
pub fn exists(kind: &str) -> bool {
    registry()
        .lock()
        .map(|guard| guard.contains_key(&label_of(kind)))
        .unwrap_or(false)
}

pub fn status(kind: &str) -> Value {
    json!({ "open": exists(kind), "url": current_url(kind) })
}
