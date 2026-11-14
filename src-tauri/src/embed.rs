//! 内嵌网页（对应旧版 `web_panel.EmbeddedWebPanel` / `page_ehall.py` / `page_homework.py`）。
//!
//! 旧版用 QtWebEngine 把 eHall / eLearning **嵌在页面里**；Rust 之前只会弹独立窗口。
//! 这里用 Tauri 的「子 Webview」（`Window::add_child`，需要 `unstable` 特性）把网页贴到
//! 主窗口的指定位置，前端给一个占位 div 的坐标即可——效果和 Python 版一致：不弹窗。
//!
//! 注意：`add_child` 内部要把活派发到主线程再等结果，所以**必须在工作线程里调**
//! （命令都用 async + `spawn_blocking`），否则会自锁，表现像「点了没反应」。
//!
//! 多标签页（2026-11-14，用户：「所有内置浏览器窗口都应该能够多标签页」）：
//! 一个 kind 现在是一组标签页（旧版 `EmbeddedWebPanel._views` + `QTabBar`），
//! 只有当前那个是 show 的，其余 `hide()` 掉；`eval` / `navigate` / `reload` 都作用在
//! 当前标签页上，抓表格的隐藏页面（`classroom`）照旧只有一个标签。
//! 网页自己 `window.open` 的时候不开新窗口，只把地址记进 `pending`，
//! 前端轮询看到就调 `new_tab`——和旧版 `newWindowRequested → new_tab(url)` 一致。
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

use serde_json::{json, Value};
use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewBuilder, WebviewUrl};

/// 一个标签页
struct Tab {
    label: String,
    webview: tauri::Webview,
    /// 页面自己报的标题（`document.title`），没加载出来时用地址兜着
    title: String,
    url: String,
}

/// 一个「内嵌网页面板」：同一个 kind 的所有标签页
struct Panel {
    tabs: Vec<Tab>,
    active: usize,
    /// 占位 div 的位置（逻辑像素）；新标签页「接着这个位置开」
    rect: (f64, f64, f64, f64),
    /// 网页通过 `window.open` 想开的新地址，等前端来取
    pending: Option<String>,
    /// 下一个标签页编号（`embed-ehall-2`、`-3`…）
    next_id: usize,
}

fn panels() -> &'static Mutex<HashMap<String, Panel>> {
    static PANELS: OnceLock<Mutex<HashMap<String, Panel>>> = OnceLock::new();
    PANELS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_panels() -> Result<MutexGuard<'static, HashMap<String, Panel>>, String> {
    panels().lock().map_err(|_| "内嵌网页状态被占用".to_string())
}

/// 多标签页开不开（设置里的 `browser_tabs`；旧版默认关，这里默认开——用户明确要的）
pub fn tabs_enabled() -> bool {
    crate::get_store()
        .setting("browser_tabs")
        .map(|text| text.trim() != "0")
        .unwrap_or(true)
}

fn apply_rect(tab: &Tab, rect: (f64, f64, f64, f64)) {
    let (x, y, width, height) = rect;
    let _ = tab.webview.set_position(LogicalPosition::new(x, y));
    let _ = tab.webview.set_size(LogicalSize::new(width, height));
}

/// 只让第 `active` 个标签露出来（其余藏掉；不关，切回去还是原样）
fn show_only(tabs: &[Tab], active: usize) {
    for (index, tab) in tabs.iter().enumerate() {
        if index == active {
            let _ = tab.webview.show();
        } else {
            let _ = tab.webview.hide();
        }
    }
}

/// 地址怎么显示成标题：`https://elearning.fudan.edu.cn/courses/12` → `elearning.fudan.edu.cn/courses/12`
fn short_url(url: &str) -> String {
    let text = url.trim();
    let text = text
        .strip_prefix("https://")
        .or_else(|| text.strip_prefix("http://"))
        .unwrap_or(text);
    if text.chars().count() > 46 {
        format!("{}…", text.chars().take(46).collect::<String>())
    } else {
        text.to_string()
    }
}

fn tab_summary(panel: &Panel) -> Value {
    json!({
        "ok": true,
        "enabled": true,
        "active": panel.active,
        "pending": panel.pending.clone().unwrap_or_default(),
        "tabs": panel
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let title = tab.title.trim();
                json!({
                    "index": index,
                    "title": if title.is_empty() { short_url(&tab.url) } else { title.to_string() },
                    "url": tab.url.clone(),
                })
            })
            .collect::<Vec<Value>>(),
    })
}

/// 标题回调（在网页线程里跑）：只改状态，并且**不能**排队等锁，拿不到就算了
fn record_title(label: &str, title: &str) {
    let Ok(mut guard) = panels().try_lock() else {
        return;
    };
    for panel in guard.values_mut() {
        if let Some(tab) = panel.tabs.iter_mut().find(|one| one.label == label) {
            tab.title = title.trim().chars().take(48).collect();
            return;
        }
    }
}

/// 地址回调：页面跳到哪里就记哪里（前端标签页上的地址跟着变）
fn record_url(label: &str, url: &str) {
    let Ok(mut guard) = panels().try_lock() else {
        return;
    };
    for panel in guard.values_mut() {
        if let Some(tab) = panel.tabs.iter_mut().find(|one| one.label == label) {
            tab.url = url.to_string();
            return;
        }
    }
}

/// 网页想开新窗口：记下地址，让前端开一个新标签页
fn record_pending(kind: &str, url: &str) {
    let Ok(mut guard) = panels().try_lock() else {
        return;
    };
    if let Some(panel) = guard.get_mut(kind) {
        panel.pending = Some(url.to_string());
    }
}

/// 建一个子 Webview（还没插进面板里）
///
/// 位置和大小在 `add_child` 时一次给对：先建到窗口外面再 `set_position` 挪回来，
/// 中间那一瞬会在界面正中闪一下，而且挪完要等下一次重绘才刷新。
/// （2026-11-14 在文献库弹窗里看到「一片黑」，其实是 arxiv 连不上、Chromium 还没画出来；
/// 等超时后错误页自己就出来了，不是重绘问题——别被它误导。）
fn create_webview(
    app: &AppHandle,
    kind: &str,
    label: &str,
    url: &str,
    rect: (f64, f64, f64, f64),
) -> Result<tauri::Webview, String> {
    let window = app
        .get_window("main")
        .ok_or_else(|| "找不到主窗口".to_string())?;
    let parsed = url
        .parse::<tauri::Url>()
        .map_err(|exc| format!("地址不对：{exc}"))?;
    // 内嵌网页也要「自动 UIS 登录」：和登录窗口注入的是同一段脚本，
    // 页面一旦跳到统一身份认证就把学号密码填好并点登录。
    let mut builder = WebviewBuilder::new(label.to_string(), WebviewUrl::External(parsed));
    let autofill = crate::uis_autofill::script(crate::get_store());
    if !autofill.is_empty() {
        builder = builder.initialization_script(autofill);
    }
    // 标签页标题跟着页面标题走（旧版 `_sync_tab_title` 也是这条线）
    let title_label = label.to_string();
    builder = builder.on_document_title_changed(move |_webview, title| {
        record_title(&title_label, &title);
    });
    // 页面地址 + 「开出来了」事件（前端靠 embed:loaded 撤掉「8 秒还没出来」的提示）
    let load_label = label.to_string();
    let load_kind = kind.to_string();
    builder = builder.on_page_load(move |webview, payload| {
        record_url(&load_label, payload.url().as_str());
        let event = if payload.event() == PageLoadEvent::Finished {
            "finished"
        } else {
            "started"
        };
        let _ = webview.app_handle().emit(
            "embed:loaded",
            json!({ "kind": load_kind, "event": event }),
        );
    });
    // 网页自己 window.open 的：不开新窗口，记下来让前端开新标签页
    let new_kind = kind.to_string();
    builder = builder.on_new_window(move |url, _features| {
        record_pending(&new_kind, url.as_str());
        NewWindowResponse::Deny
    });
    window
        .add_child(
            builder,
            LogicalPosition::new(rect.0, rect.1),
            LogicalSize::new(rect.2, rect.3),
        )
        .map_err(|exc| format!("内嵌网页打不开：{exc}"))
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
    url.parse::<tauri::Url>()
        .map_err(|exc| format!("地址不对：{exc}"))?;
    // 已经有这个内嵌网页：挪位置、必要时换地址，再显示出来
    {
        let mut guard = lock_panels()?;
        let empty = guard
            .get(kind)
            .map(|panel| panel.tabs.is_empty())
            .unwrap_or(false);
        if empty {
            guard.remove(kind);
        }
        if let Some(panel) = guard.get_mut(kind) {
            panel.rect = (x, y, width, height);
            if panel.active >= panel.tabs.len() {
                panel.active = panel.tabs.len() - 1;
            }
            let active = panel.active;
            let rect = panel.rect;
            apply_rect(&panel.tabs[active], rect);
            show_only(&panel.tabs, active);
            if panel.tabs[active].url != url {
                if let Ok(parsed) = url.parse::<tauri::Url>() {
                    let _ = panel.tabs[active].webview.navigate(parsed);
                    panel.tabs[active].url = url.to_string();
                    panel.tabs[active].title.clear();
                }
            }
            return Ok(panel.tabs[active].label.clone());
        }
    }
    let label = format!("embed-{kind}");
    let webview = create_webview(app, kind, &label, url, (x, y, width, height))?;
    let tab = Tab {
        label: label.clone(),
        webview,
        title: String::new(),
        url: url.to_string(),
    };
    // 建完不用再 set_position / show 一次：`add_child` 已经按占位框摆好了，
    // 再动一下只会多一次重排（`open_hidden` 那种放到窗口外的也是同一个入口）。
    let mut guard = lock_panels()?;
    guard.insert(
        kind.to_string(),
        Panel {
            tabs: vec![tab],
            active: 0,
            rect: (x, y, width, height),
            pending: None,
            next_id: 2,
        },
    );
    Ok(label)
}

/// 跟随页面布局挪位置 / 改大小（滚动、切页、窗口缩放都要调）
pub fn bounds(kind: &str, x: f64, y: f64, width: f64, height: f64) -> Result<(), String> {
    let mut guard = lock_panels()?;
    let Some(panel) = guard.get_mut(kind) else {
        return Ok(()); // 还没建出来，下次 open 时自然带上位置
    };
    panel.rect = (x, y, width, height);
    let rect = panel.rect;
    if let Some(tab) = panel.tabs.get(panel.active) {
        apply_rect(tab, rect);
    }
    Ok(())
}

/// 关掉（切到别的页面时调用，否则它一直浮在最上面）
pub fn close(kind: &str) -> Result<(), String> {
    let taken = {
        let mut guard = lock_panels()?;
        guard.remove(kind)
    };
    if let Some(panel) = taken {
        for tab in panel.tabs {
            let _ = tab.webview.close();
        }
    }
    Ok(())
}

pub fn reload(kind: &str) -> Result<(), String> {
    let guard = lock_panels()?;
    if let Some(panel) = guard.get(kind) {
        if let Some(tab) = panel.tabs.get(panel.active) {
            let _ = tab.webview.reload();
        }
    }
    Ok(())
}

pub fn navigate(kind: &str, url: &str) -> Result<(), String> {
    let parsed = url
        .parse::<tauri::Url>()
        .map_err(|exc| format!("地址不对：{exc}"))?;
    let mut guard = lock_panels()?;
    if let Some(panel) = guard.get_mut(kind) {
        let active = panel.active;
        if let Some(tab) = panel.tabs.get_mut(active) {
            tab.webview
                .navigate(parsed)
                .map_err(|exc| format!("跳转失败：{exc}"))?;
            tab.url = url.to_string();
            tab.title.clear();
        }
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
/// 多标签页之后，作用在**当前**这个标签页上。
pub fn eval_callback(
    kind: &str,
    js: &str,
    callback: impl Fn(String) + Send + 'static,
) -> Result<(), String> {
    let guard = lock_panels()?;
    let Some(panel) = guard.get(kind) else {
        return Err("内嵌网页还没打开（先点「显示网页」登录一次）".to_string());
    };
    let Some(tab) = panel.tabs.get(panel.active) else {
        return Err("内嵌网页还没打开（先点「显示网页」登录一次）".to_string());
    };
    tab.webview
        .eval_with_callback(js.to_string(), callback)
        .map_err(|exc| format!("页面脚本执行失败：{exc}"))
}

/// 现在真正打开的地址（前端显示用；没开就返回空串）
pub fn current_url(kind: &str) -> String {
    let Ok(guard) = panels().lock() else {
        return String::new();
    };
    guard
        .get(kind)
        .and_then(|panel| panel.tabs.get(panel.active))
        .map(|tab| tab.url.clone())
        .unwrap_or_default()
}

/// 是否有这个内嵌网页
pub fn exists(kind: &str) -> bool {
    registry_has(kind)
}

fn registry_has(kind: &str) -> bool {
    panels()
        .lock()
        .map(|guard| guard.contains_key(kind))
        .unwrap_or(false)
}

pub fn status(kind: &str) -> Value {
    let tabs = tabs_of(kind);
    json!({
        "open": exists(kind),
        "url": current_url(kind),
        "enabled": tabs["enabled"].clone(),
        "active": tabs["active"].clone(),
        "tabs": tabs["tabs"].clone(),
    })
}

/// 标签页列表（前端每 1.5 秒问一次：标题、当前是第几个、有没有 pending 的新窗口）
pub fn tabs_of(kind: &str) -> Value {
    let enabled = tabs_enabled();
    let Ok(guard) = panels().lock() else {
        return json!({ "ok": false, "enabled": enabled, "active": 0, "pending": "", "tabs": [] });
    };
    match guard.get(kind) {
        Some(panel) => {
            let mut value = tab_summary(panel);
            value["enabled"] = json!(enabled);
            value
        }
        None => json!({ "ok": false, "enabled": enabled, "active": 0, "pending": "", "tabs": [] }),
    }
}

/// 开一个新标签页（旧版 `EmbeddedWebPanel.new_tab`）
pub fn new_tab(
    app: &AppHandle,
    kind: &str,
    url: &str,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<Value, String> {
    let target = {
        let text = url.trim().to_string();
        if !text.is_empty() {
            text
        } else {
            // 没给地址就开这个面板第一个标签的地址（旧版用面板首页）
            let guard = lock_panels()?;
            let known = guard
                .get(kind)
                .and_then(|panel| panel.tabs.first())
                .map(|tab| tab.url.clone())
                .unwrap_or_default();
            known
        }
    };
    if target.is_empty() {
        return Err("没有可用的地址".to_string());
    }
    let (label, rect) = {
        let mut guard = lock_panels()?;
        let Some(panel) = guard.get_mut(kind) else {
            return Err("内嵌网页还没打开".to_string());
        };
        if panel.tabs.len() >= 12 {
            return Err("标签页最多 12 个，先关掉几个".to_string());
        }
        // 新标签「接着当前占位框的位置开」，前端给的坐标更准
        if width > 40.0 && height > 40.0 {
            panel.rect = (x, y, width, height);
        }
        let rect = panel.rect;
        let id = panel.next_id;
        panel.next_id += 1;
        (format!("embed-{kind}-{id}"), rect)
    };
    // 建的时候位置就要给对（建完再挪，WebView2 会留一块黑）
    let webview = create_webview(app, kind, &label, &target, rect)?;
    let tab = Tab {
        label,
        webview,
        title: String::new(),
        url: target,
    };
    let mut guard = lock_panels()?;
    let Some(panel) = guard.get_mut(kind) else {
        return Err("内嵌网页已经关掉了".to_string());
    };
    let index = panel.tabs.len();
    panel.tabs.push(tab);
    panel.active = index;
    panel.pending = None;
    // 新标签已经在正确的坐标上建好了：只把**别的**藏掉，别动它自己（见上面的注释）
    for (position, one) in panel.tabs.iter().enumerate() {
        if position != index {
            let _ = one.webview.hide();
        }
    }
    Ok(tab_summary(panel))
}

/// 切到第 `index` 个标签页
pub fn select_tab(kind: &str, index: usize) -> Result<Value, String> {
    let mut guard = lock_panels()?;
    let Some(panel) = guard.get_mut(kind) else {
        return Err("内嵌网页还没打开".to_string());
    };
    if index >= panel.tabs.len() {
        return Err("没有这个标签页".to_string());
    }
    panel.active = index;
    let rect = panel.rect;
    apply_rect(&panel.tabs[index], rect);
    show_only(&panel.tabs, index);
    let _ = panel.tabs[index].webview.set_focus();
    Ok(tab_summary(panel))
}

/// 关掉第 `index` 个标签页；全关完了整个面板就没了（前端会重新开一个首页）
pub fn close_tab(kind: &str, index: usize) -> Result<Value, String> {
    let mut guard = lock_panels()?;
    let now_empty;
    {
        let Some(panel) = guard.get_mut(kind) else {
            return Err("内嵌网页还没打开".to_string());
        };
        if index >= panel.tabs.len() {
            return Err("没有这个标签页".to_string());
        }
        let tab = panel.tabs.remove(index);
        let _ = tab.webview.close();
        now_empty = panel.tabs.is_empty();
        if !now_empty {
            if panel.active > index {
                panel.active -= 1;
            }
            if panel.active >= panel.tabs.len() {
                panel.active = panel.tabs.len() - 1;
            }
            let active = panel.active;
            let rect = panel.rect;
            apply_rect(&panel.tabs[active], rect);
            show_only(&panel.tabs, active);
        }
    }
    if now_empty {
        guard.remove(kind);
        return Ok(json!({
            "ok": true, "enabled": true, "active": 0, "pending": "",
            "tabs": Vec::<Value>::new(), "closed": true,
        }));
    }
    match guard.get(kind) {
        Some(panel) => Ok(tab_summary(panel)),
        None => Ok(json!({ "ok": true, "enabled": true, "active": 0, "pending": "", "tabs": [] })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_url_strips_scheme_and_trims() {
        assert_eq!(
            short_url("https://elearning.fudan.edu.cn/course/12"),
            "elearning.fudan.edu.cn/course/12"
        );
        assert_eq!(short_url("  http://a.b/c  "), "a.b/c");
        let long = format!("https://example.com/{}", "x".repeat(80));
        let text = short_url(&long);
        assert!(text.chars().count() <= 47, "{text}");
        assert!(text.ends_with('…'), "{text}");
    }
}
