//! 服务器公告的「实时」接收。
//!
//! ## 公告通道：真 WebSocket 优先，5 秒轮询兜底（2026-11-23）
//!
//! 服务端契约：`WS /api/v1/stream?token=...`，连上先发 `{"type":"hello",…}`，
//! 之后有新公告推 `{"type":"announcement","data":{…}}`，每 30 秒发一次 `ping`。
//!
//! 客户端两条腿同时跑：
//!
//! * `danlume-announce-ws`：阻塞式 `tungstenite` 连接（专用线程，rustls + webpki roots）。
//!   收到 announcement 帧 → 写 SQLite（`store.add_announcement`，按 id 去重）→
//!   更新 `last_announcement_id` → `emit("danlume://announcement")`。读超时 90 秒
//!   （服务器每 30 秒 ping 一次，两轮没帧就当连接死了），断了按 1→2→4…→60 秒退避重连；
//!   服务器地址 / token 变了立刻断开重连（登录、退出、换服务器都会触发）。
//! * `danlume-announce`：原来那条轮询线程**保留当兜底**（服务器没开 WS、代理挡了
//!   Upgrade、或 WS 断了）。WS 连着时它放慢到 60 秒一次（补漏网公告 / 探活），
//!   WS 断了就恢复 5 秒一次——所以最坏情况仍然是「几十秒内到达」。
//!
//! 历史坑：当年加 `tungstenite` **默认特性**会让测试二进制报
//! `STATUS_ENTRYPOINT_NOT_FOUND`（它又拉了一份原生 TLS）。现在一律
//! `default-features = false` + `rustls-tls-webpki-roots`，`cargo tree -i openssl-sys` 为空，
//! `notify::tests::real_stream_connects`（`#[ignore]`）真连过线上服务器。
//!
//! ## 行为
//!
//! * 只有「配了服务器 + 已登录」才连 / 才轮询（没登录就每 1 秒看一眼设置，几乎不耗资源）；
//! * 新公告：写 SQLite（`store.rs` 的 `announcements` 表）→ 更新 `last_announcement_id`
//!   → 通过 Tauri 事件 `danlume://announcement` 逐条推给前端；
//! * 断线重连：指数退避 1s → 2s → 4s …… 60s 上限，成功一次就复位；
//! * 服务器没有公告接口（老版本回 404）时标记 `unsupported`，改成 300 秒探一次；
//! * 状态里的 `mode` 会写 `websocket` / `polling`，设置页据此显示通道。
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::Emitter;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use crate::account;
use crate::store::Store;

/// 推给前端的事件名（Tauri 只允许字母数字和 `- / : _`，`://` 是合法的）
pub const ANNOUNCEMENT_EVENT: &str = "danlume://announcement";
/// 正常情况下每 5 秒拉一次
pub const POLL_SECS: u64 = 5;
/// WS 连着时轮询放慢到这个间隔（只做补漏 / 探活）
pub const WS_POLL_SECS: u64 = 60;
/// 退避上限
pub const MAX_BACKOFF_SECS: u64 = 60;
/// 服务器没有公告接口时，多久再探一次
pub const UNSUPPORTED_RETRY_SECS: u64 = 300;
/// WS 读超时：服务器每 30 秒 ping 一次，90 秒没任何帧就当连接死了
const WS_READ_TIMEOUT: Duration = Duration::from_secs(90);
/// 循环心跳：500ms 看一次有没有活要干（这样 kick() 能立刻生效，也不费 CPU）
const TICK: Duration = Duration::from_millis(500);

static STARTED: AtomicBool = AtomicBool::new(false);
static KICK: AtomicBool = AtomicBool::new(false);
/// WS 当前是否连着（轮询线程据此放慢；status() 据此显示通道）
static WS_CONNECTED: AtomicBool = AtomicBool::new(false);

/// 轮询 / WS 线程的运行状态（设置页 / 报告里能看到到底有没有在工作）
#[derive(Debug, Clone, Default)]
struct RuntimeState {
    polls: u64,
    delivered: u64,
    last_poll_at: String,
    last_ok_at: String,
    last_error: String,
    last_error_kind: String,
    unsupported: bool,
    backoff_secs: u64,
    next_delay_secs: u64,
    /// WS：成功连上过几次 / 最后一条错误 / 最后一帧到达时间
    ws_connects: u64,
    ws_last_error: String,
    ws_last_frame_at: String,
}

fn state_cell() -> &'static Mutex<RuntimeState> {
    static CELL: OnceLock<Mutex<RuntimeState>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(RuntimeState::default()))
}

fn update_state(patch: impl FnOnce(&mut RuntimeState)) {
    if let Ok(mut guard) = state_cell().lock() {
        patch(&mut guard);
    }
}

fn stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// 让轮询线程下一拍立刻干活（登录成功、退出、改服务器地址、点「立刻检查」时用）
pub fn kick() {
    KICK.store(true, Ordering::SeqCst);
}

/// 后台线程只起一次；重复调用是空操作（轮询 + WS 各一条）
pub fn spawn(app: tauri::AppHandle) {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let poll_app = app.clone();
    std::thread::Builder::new()
        .name("danlume-announce".to_string())
        .spawn(move || run(poll_app))
        .ok();
    std::thread::Builder::new()
        .name("danlume-announce-ws".to_string())
        .spawn(move || run_ws(app))
        .ok();
}

/// 拉一次公告：返回 (新的 last_announcement_id, 新公告列表)
///
/// `app` 为 None 时不发事件（测试 / 手动检查时用）。
fn poll_once(store: &Store, since: i64, app: Option<&tauri::AppHandle>) -> Result<(i64, Vec<Value>), account::ApiError> {
    let value = account::announcements(store, since)?;
    let items: Vec<Value> = value
        .get("items")
        .and_then(|one| one.as_array())
        .cloned()
        .unwrap_or_default();
    let mut max_id = since;
    let mut fresh: Vec<Value> = Vec::new();
    for item in items {
        let id = item.get("id").and_then(|one| one.as_i64()).unwrap_or(0);
        if id <= 0 {
            continue;
        }
        if id > max_id {
            max_id = id;
        }
        // 已经存过的（id 相同）不会再插，也不会重复推送
        if store.add_announcement(&item).unwrap_or(false) {
            fresh.push(item);
        }
    }
    if max_id > since {
        let _ = store.set_setting("last_announcement_id", &max_id.to_string());
    }
    if let Some(handle) = app {
        for item in &fresh {
            // 前端没在听也无所谓，事件丢了还有「通知」页和标题栏徽标兜底
            let _ = handle.emit(ANNOUNCEMENT_EVENT, item.clone());
        }
    }
    Ok((max_id, fresh))
}

// ---------------------------------------------------------------- WebSocket 通道

/// 把设置里的 HTTP(S) 地址换成 WS(S) 地址。
///
/// `https://43.108.99.38.sslip.io` → `wss://43.108.99.38.sslip.io/api/v1/stream?token=…`。
/// 设置里存的一律是 http(s)，直接拿去 `tungstenite::connect` 会报 URL scheme not supported。
fn ws_url(base: &str, token: &str) -> String {
    let base = base.trim_end_matches('/');
    let ws = if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        base.to_string()
    };
    format!("{ws}/api/v1/stream?token={token}")
}

/// 给底层 TCP 设读超时（TLS 的话要摸到里面的 `TcpStream`）。
///
/// rustls 的 `StreamOwned` 把套接字放在公开字段 `sock` 里；拿不到就静默跳过
/// （没有读超时也还能靠「服务器每 30 秒 ping」和轮询兜底）。
fn set_read_timeout(socket: &WebSocket<MaybeTlsStream<TcpStream>>, timeout: Duration) {
    match socket.get_ref() {
        MaybeTlsStream::Plain(stream) => {
            let _ = stream.set_read_timeout(Some(timeout));
        }
        MaybeTlsStream::Rustls(tls) => {
            let _ = tls.sock.set_read_timeout(Some(timeout));
        }
        // 其它 TLS 后端（目前没启用的 native-tls）：拿不到裸 TcpStream，跳过超时
        _ => {}
    }
}

/// 处理一帧文本：是「新公告」就写库（按 id 去重）并返回它，其它帧返回 None。
///
/// 写库 / 更新 `last_announcement_id` 与轮询走同一条路（`store.add_announcement` 幂等），
/// 所以 WS 与轮询同时收到同一条也不会重复推送。
fn apply_frame(store: &Store, text: &str) -> Option<Value> {
    let value: Value = serde_json::from_str(text).ok()?;
    match value.get("type").and_then(|one| one.as_str()).unwrap_or("") {
        "announcement" => {
            let item = value.get("data")?.clone();
            let id = item.get("id").and_then(|one| one.as_i64()).unwrap_or(0);
            if id <= 0 {
                return None;
            }
            if !store.add_announcement(&item).unwrap_or(false) {
                return None; // 已经有了（轮询先拿到 / 重复帧）
            }
            let current = store
                .setting("last_announcement_id")
                .and_then(|text| text.trim().parse::<i64>().ok())
                .unwrap_or(0);
            let _ = store.set_setting("last_announcement_id", &current.max(id).to_string());
            Some(item)
        }
        _ => None,
    }
}

/// 跑一次 WS 会话（阻塞，直到断开 / 读超时 / 配置变了才返回）。
///
/// 返回 `Ok(())` = 正常断开（服务器关连接、或服务器地址 / token 变了）；
/// `Err` = 握手/读帧失败，交给外层按退避重连。
fn ws_session(
    store: &Store,
    base: &str,
    token: &str,
    app: &tauri::AppHandle,
) -> Result<(), String> {
    // rustls 0.23 要求进程级默认 CryptoProvider；不装的话 wss 握手会 panic
    let _ = rustls::crypto::ring::default_provider().install_default();
    let url = ws_url(base, token);
    let (mut socket, response) =
        tungstenite::connect(url.as_str()).map_err(|exc| format!("握手失败：{exc}"))?;
    set_read_timeout(&socket, WS_READ_TIMEOUT);
    println!("[公告] WebSocket 已连接（HTTP {}）", response.status());
    WS_CONNECTED.store(true, Ordering::SeqCst);
    update_state(|state| {
        state.ws_connects += 1;
        state.ws_last_error.clear();
        state.last_error.clear();
        state.last_error_kind.clear();
        state.unsupported = false;
        state.backoff_secs = 0;
        state.ws_last_frame_at = stamp();
    });

    // 连上先补一次增量：断线期间可能漏了公告（顺带把 token 的 401 续期也走一遍）
    match poll_once(store, current_since(store), Some(app)) {
        Ok((_, fresh)) => update_state(|state| {
            state.last_ok_at = stamp();
            state.delivered += fresh.len() as u64;
        }),
        Err(err) => update_state(|state| {
            state.last_error = err.error.clone();
            state.last_error_kind = err.kind.clone();
        }),
    }

    let active_key = format!("{base}\u{1}{token}");
    loop {
        match socket.read() {
            Ok(Message::Text(text)) => {
                update_state(|state| state.ws_last_frame_at = stamp());
                if let Some(item) = apply_frame(store, text.as_str()) {
                    let title = item
                        .get("title")
                        .and_then(|one| one.as_str())
                        .unwrap_or("")
                        .to_string();
                    println!("[公告] WebSocket 收到新公告：{title}");
                    update_state(|state| {
                        state.delivered += 1;
                        state.last_ok_at = stamp();
                    });
                    // 前端没在听也无所谓，事件丢了还有「通知」页和标题栏徽标兜底
                    let _ = app.emit(ANNOUNCEMENT_EVENT, item);
                }
            }
            Ok(Message::Ping(payload)) => {
                let _ = socket.send(Message::Pong(payload));
                update_state(|state| state.ws_last_frame_at = stamp());
            }
            Ok(Message::Close(_)) => {
                WS_CONNECTED.store(false, Ordering::SeqCst);
                return Ok(());
            }
            Ok(_) => {
                // Pong / Binary 之类：记一下「还活着」就行
                update_state(|state| state.ws_last_frame_at = stamp());
            }
            Err(tungstenite::Error::Io(exc))
                if exc.kind() == std::io::ErrorKind::WouldBlock
                    || exc.kind() == std::io::ErrorKind::TimedOut =>
            {
                WS_CONNECTED.store(false, Ordering::SeqCst);
                return Err(format!(
                    "读超时（{} 秒没收到任何帧，当成连接已死）",
                    WS_READ_TIMEOUT.as_secs()
                ));
            }
            Err(exc) => {
                WS_CONNECTED.store(false, Ordering::SeqCst);
                return Err(format!("读帧失败：{exc}"));
            }
        }
        // 服务器地址 / token 变了（登录、退出、换服务器）：主动断开，外层用新配置重连
        let current = {
            let fresh = crate::get_store();
            format!(
                "{}\u{1}{}",
                account::server_url(fresh),
                account::token(fresh)
            )
        };
        if current != active_key {
            println!("[公告] 配置变了，断开 WebSocket 重新连接");
            WS_CONNECTED.store(false, Ordering::SeqCst);
            return Ok(());
        }
    }
}

/// WS 监督线程：登录了就尽量维持一条长连接，断了按指数退避重连。
fn run_ws(app: tauri::AppHandle) {
    // 启动时先让别人（课表、通知抓取、上报）跑，别一起抢网络
    std::thread::sleep(Duration::from_secs(8));
    let mut backoff = 1u64;
    let mut active_key = String::new();
    loop {
        let store = crate::get_store();
        let base = account::server_url(store);
        let token = account::token(store);
        let key = format!("{base}\u{1}{token}");
        if key != active_key {
            active_key = key;
            backoff = 1;
            update_state(|state| {
                state.ws_last_error.clear();
                state.backoff_secs = 0;
            });
        }
        if base.is_empty() || token.is_empty() {
            WS_CONNECTED.store(false, Ordering::SeqCst);
            std::thread::sleep(Duration::from_secs(1));
            continue;
        }

        let outcome = ws_session(store, &base, &token, &app);
        WS_CONNECTED.store(false, Ordering::SeqCst);
        match outcome {
            Ok(()) => {
                backoff = 1;
                // 正常断开：多半是配置变了，下一圈立刻重连
                continue;
            }
            Err(exc) => {
                println!("[公告] WebSocket 断开：{exc}");
                update_state(|state| state.ws_last_error = exc);
            }
        }

        // 退避等待；期间每 500ms 看一眼配置有没有变（登录成功要尽快重连）
        let wait = backoff.min(MAX_BACKOFF_SECS);
        let deadline = Instant::now() + Duration::from_secs(wait);
        while Instant::now() < deadline {
            std::thread::sleep(TICK);
            let fresh = crate::get_store();
            let current = format!(
                "{}\u{1}{}",
                account::server_url(fresh),
                account::token(fresh)
            );
            if current != active_key {
                break;
            }
        }
        update_state(|state| {
            state.backoff_secs = wait;
            state.next_delay_secs = wait;
        });
        backoff = (backoff * 2).min(MAX_BACKOFF_SECS);
    }
}

/// 手动检查一次（设置页 / 通知页的「立刻检查」按钮），顺便推事件
pub fn check_now(app: &tauri::AppHandle) -> Value {
    let store = crate::get_store();
    let since = current_since(store);
    update_state(|state| {
        state.polls += 1;
        state.last_poll_at = stamp();
    });
    match poll_once(store, since, Some(app)) {
        Ok((max_id, fresh)) => {
            update_state(|state| {
                state.last_ok_at = stamp();
                state.last_error.clear();
                state.last_error_kind.clear();
                state.unsupported = false;
                state.backoff_secs = 0;
                state.delivered += fresh.len() as u64;
            });
            json!({
                "ok": true, "added": fresh.len(), "since": since, "last_announcement_id": max_id,
                "items": fresh, "unread": store.announcement_unread_count(),
            })
        }
        Err(err) => {
            if err.kind == "unsupported" {
                update_state(|state| state.unsupported = true);
            }
            update_state(|state| {
                state.last_error = err.error.clone();
                state.last_error_kind = err.kind.clone();
            });
            json!({
                "ok": false, "added": 0, "since": since,
                "kind": err.kind, "offline": err.offline, "error": err.error,
                "http_status": err.http_status,
            })
        }
    }
}

/// 增量拉取的起点：设置里的 last_announcement_id 与库里已有的最大 id 取大
fn current_since(store: &Store) -> i64 {
    let from_setting = store
        .setting("last_announcement_id")
        .and_then(|text| text.trim().parse::<i64>().ok())
        .unwrap_or(0);
    from_setting.max(store.announcement_max_id()).max(0)
}

/// 后台循环
fn run(app: tauri::AppHandle) {
    // 启动时先让别人（课表、通知抓取、上报）跑，别一起抢网络
    std::thread::sleep(Duration::from_secs(8));
    let mut backoff = 1u64;
    let mut due = Instant::now();
    let mut active_key = String::new();
    loop {
        {
            let store = crate::get_store();
            let base = account::server_url(store);
            let token = account::token(store);
            let key = format!("{base}\u{1}{token}");
            if key != active_key {
                // 换了服务器 / 登录状态变了 → 清退避、立刻重建
                active_key = key;
                backoff = 1;
                due = Instant::now();
                update_state(|state| {
                    state.unsupported = false;
                    state.backoff_secs = 0;
                    state.last_error.clear();
                    state.last_error_kind.clear();
                });
            }
            // 没配服务器（空 = 不用服务器）或没登录：什么都不拉
            if base.is_empty() || token.is_empty() {
                if KICK.swap(false, Ordering::SeqCst) {
                    due = Instant::now();
                }
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
            if KICK.swap(false, Ordering::SeqCst) {
                due = Instant::now();
                backoff = 1;
            }
            if Instant::now() < due {
                std::thread::sleep(TICK);
                continue;
            }

            let since = current_since(store);
            update_state(|state| {
                state.polls += 1;
                state.last_poll_at = stamp();
            });
            match poll_once(store, since, Some(&app)) {
                Ok((_, fresh)) => {
                    backoff = 1;
                    // WS 连着时放慢到 60 秒（只补漏 / 探活），断了就回到 5 秒
                    let interval = if WS_CONNECTED.load(Ordering::SeqCst) {
                        WS_POLL_SECS
                    } else {
                        POLL_SECS
                    };
                    due = Instant::now() + Duration::from_secs(interval);
                    update_state(|state| {
                        state.last_ok_at = stamp();
                        state.last_error.clear();
                        state.last_error_kind.clear();
                        state.unsupported = false;
                        state.backoff_secs = 0;
                        state.next_delay_secs = interval;
                        state.delivered += fresh.len() as u64;
                    });
                    if !fresh.is_empty() {
                        println!("[公告] 收到 {} 条新公告", fresh.len());
                    }
                }
                Err(err) if err.kind == "unsupported" => {
                    // 服务器还没有公告接口（老版本）：慢慢探，别刷屏
                    due = Instant::now() + Duration::from_secs(UNSUPPORTED_RETRY_SECS);
                    update_state(|state| {
                        state.unsupported = true;
                        state.last_error = err.error.clone();
                        state.last_error_kind = err.kind.clone();
                        state.next_delay_secs = UNSUPPORTED_RETRY_SECS;
                    });
                }
                Err(err) => {
                    let wait = if err.is_offline() || err.kind == "server" {
                        backoff
                    } else {
                        // 401 / 配置问题：别高频重试
                        MAX_BACKOFF_SECS
                    };
                    due = Instant::now() + Duration::from_secs(wait);
                    update_state(|state| {
                        state.last_error = err.error.clone();
                        state.last_error_kind = err.kind.clone();
                        state.backoff_secs = wait;
                        state.next_delay_secs = wait;
                    });
                    backoff = (backoff * 2).min(MAX_BACKOFF_SECS);
                }
            }
        }
    }
}

/// 状态（设置页展示「公告通道」用）
pub fn status() -> Value {
    let state = state_cell()
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default();
    let ws = WS_CONNECTED.load(Ordering::SeqCst);
    json!({
        "mode": if ws { "websocket" } else { "polling" },
        "reason": if ws {
            "WebSocket 实时推送已连接（轮询放慢到 60 秒做兜底）"
        } else {
            "轮询模式（每 5 秒；WebSocket 连不上时自动兜底）"
        },
        "interval_secs": if ws { WS_POLL_SECS } else { POLL_SECS },
        "max_backoff_secs": MAX_BACKOFF_SECS,
        "started": STARTED.load(Ordering::SeqCst),
        "polls": state.polls,
        "delivered": state.delivered,
        "last_poll_at": state.last_poll_at,
        "last_ok_at": state.last_ok_at,
        "last_error": state.last_error,
        "last_error_kind": state.last_error_kind,
        "unsupported": state.unsupported,
        "backoff_secs": state.backoff_secs,
        "next_delay_secs": state.next_delay_secs,
        "ws_connected": ws,
        "ws_connects": state.ws_connects,
        "ws_last_error": state.ws_last_error,
        "ws_last_frame_at": state.ws_last_frame_at,
        "event": ANNOUNCEMENT_EVENT,
    })
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
    fn since_uses_max_of_setting_and_db() {
        let store = temp_store("danlume-notify-since-test");
        assert_eq!(current_since(&store), 0);
        store.set_setting("last_announcement_id", "12").unwrap();
        assert_eq!(current_since(&store), 12);
        // 库里已经有更新的公告（比如设置被清过），要以库里为准，否则会重复拉
        store
            .add_announcement(&json!({ "id": 31, "title": "t" }))
            .unwrap();
        assert_eq!(current_since(&store), 31);
        // 脏设置不 panic
        store.set_setting("last_announcement_id", "不是数字").unwrap();
        assert_eq!(current_since(&store), 31);
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-notify-since-test"));
    }

    #[test]
    fn state_starts_idle_and_reports_polling_mode() {
        let status = status();
        assert_eq!(status["mode"], "polling");
        assert_eq!(status["interval_secs"], POLL_SECS);
        assert_eq!(status["ws_connected"], false, "测试里没连 WS");
        assert_eq!(status["event"], ANNOUNCEMENT_EVENT);
        assert_eq!(status["started"], false, "测试里没起线程");
        println!("状态：{status}");
    }

    /// 真连服务器跑一轮轮询（`cargo test live_ -- --ignored --nocapture --test-threads=1`）
    ///
    /// `/announcements` 是公开接口，所以不登录也能验证「拉取 → 去重 → 更新
    /// last_announcement_id」这条链路。服务器当前没发公告时返回 0 条，同样算通过。
    #[test]
    #[ignore]
    fn live_poll_against_real_server() {
        let store = temp_store("danlume-notify-live-test");
        let _ = store.set_setting("server_url", account::DEFAULT_SERVER);
        println!("== 轮询起点 last_announcement_id = {}", current_since(&store));
        match poll_once(&store, 0, None) {
            Ok((max_id, fresh)) => {
                println!(
                    "== 轮询成功：新的 last_announcement_id={max_id}，新公告 {} 条",
                    fresh.len()
                );
                assert_eq!(
                    store.setting("last_announcement_id").unwrap_or_default(),
                    if max_id > 0 { max_id.to_string() } else { String::new() }
                );
            }
            Err(err) => panic!("轮询失败：kind={} {}", err.kind, err.error),
        }
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-notify-live-test"));
    }

    /// 真连一次公告 WebSocket（默认跳过）。
    ///
    /// 这条用例的价值不在「服务器能不能推」，而在 **这个 crate 跟主程序一起链接后，
    /// 测试二进制照样能加载、能跑 WS**。当年 `tungstenite` 默认特性拉了一份原生 TLS，
    /// 让测试二进制报 `STATUS_ENTRYPOINT_NOT_FOUND`，公告通道因此退回 5 秒轮询。
    /// 现在的依赖是 `tokio-tungstenite` + `default-features = false` + rustls，
    /// 跑通这条就说明可以往下写「WS 优先、轮询兜底」。
    ///
    /// `cargo test notify::tests::real_stream_connects -- --ignored --nocapture --test-threads=1`
    #[test]
    #[ignore]
    fn real_stream_connects() {
        use futures_util::StreamExt;

        let store = crate::testdata::real_store("notify-ws");
        let base = account::server_url(&store);
        // access token 只有 30 分钟；先续一次再连（写的是真实库的**临时副本**，不碰用户数据）。
        // 不续的话经常拿到 403 —— uvicorn 会把 WS 的 4401 关连接表现成握手 403。
        if !account::refresh_token(&store).is_empty() {
            match account::refresh(&store) {
                Ok(_) => println!("== 已用 refresh_token 续期"),
                Err(err) => println!("== 续期失败（继续用现有 token 试）：{}", err.error),
            }
        }
        let token = account::token(&store);
        assert!(!base.is_empty(), "没配服务器地址（server_url 为空）");
        assert!(
            !token.is_empty(),
            "本机没登录（server_token 为空）：先在客户端登录 DanLume 账号"
        );
        // 设置里存的是 HTTP(S) 地址，WS 要换成 ws(s):// 才能握手
        let ws_base = if let Some(rest) = base.strip_prefix("https://") {
            format!("wss://{rest}")
        } else if let Some(rest) = base.strip_prefix("http://") {
            format!("ws://{rest}")
        } else {
            base.clone()
        };
        let url = format!(
            "{}/api/v1/stream?token={}",
            ws_base.trim_end_matches('/'),
            token
        );
        println!("== 准备连接 {}/api/v1/stream", ws_base);

        // rustls 0.23 要求进程级默认 CryptoProvider；不装的话 connect_async 会 panic
        let _ = rustls::crypto::ring::default_provider().install_default();

        // 外面套一层 watchdog：连接或读帧卡住时最多等 25 秒，别让测试一直挂着
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let out = tauri::async_runtime::block_on(async move {
                let (mut socket, response) = tokio_tungstenite::connect_async(&url)
                    .await
                    .map_err(|exc| format!("握手失败：{exc}"))?;
                println!("== 握手成功：HTTP {}", response.status());
                let first = socket
                    .next()
                    .await
                    .ok_or_else(|| "连接在收到第一帧之前就关了".to_string())?
                    .map_err(|exc| format!("读帧失败：{exc}"))?;
                Ok::<String, String>(match first {
                    tokio_tungstenite::tungstenite::Message::Text(text) => text.to_string(),
                    other => format!("(非文本帧：{other:?})"),
                })
            });
            let _ = tx.send(out);
        });
        let frame = rx
            .recv_timeout(std::time::Duration::from_secs(25))
            .expect("25 秒内没拿到结果：连接或读帧卡住了")
            .expect("WS 连接失败");
        println!("== 第一帧：{frame}");
        assert!(
            frame.contains("\"hello\""),
            "第一帧不是 hello（服务器契约变了？）：{frame}"
        );
    }

    // ------------------------------------------------------------ WS 帧处理（离线单测）

    /// 真连一次（**阻塞 API**，与 `ws_session` 完全同一条路径），读第一帧。
    ///
    /// `cargo test notify::tests::real_ws_blocking_handshake -- --ignored --nocapture --test-threads=1`
    #[test]
    #[ignore]
    fn real_ws_blocking_handshake() {
        let store = crate::testdata::real_store("notify-ws-blocking");
        if !account::refresh_token(&store).is_empty() {
            let _ = account::refresh(&store);
        }
        let base = account::server_url(&store);
        let token = account::token(&store);
        assert!(!base.is_empty(), "没配服务器地址");
        assert!(!token.is_empty(), "本机没登录");
        let url = ws_url(&base, &token);
        println!("== 连接 {}/api/v1/stream（token 不打印）", base);
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (mut socket, response) = tungstenite::connect(url.as_str()).expect("握手失败");
        println!("== 握手 HTTP {}", response.status());
        set_read_timeout(&socket, Duration::from_secs(15));
        match socket.read().expect("读第一帧失败") {
            Message::Text(text) => {
                println!("== 第一帧：{}", text.as_str());
                assert!(text.as_str().contains("\"hello\""));
            }
            other => panic!("第一帧不是文本：{other:?}"),
        }
    }

    #[test]
    fn ws_url_swaps_scheme() {
        assert_eq!(
            ws_url("https://43.108.99.38.sslip.io/", "tok"),
            "wss://43.108.99.38.sslip.io/api/v1/stream?token=tok"
        );
        assert_eq!(
            ws_url("http://127.0.0.1:8451", "tok"),
            "ws://127.0.0.1:8451/api/v1/stream?token=tok"
        );
        // 已经是 ws/wss 的原样用
        assert_eq!(
            ws_url("wss://example.com", "t"),
            "wss://example.com/api/v1/stream?token=t"
        );
    }

    #[test]
    fn apply_frame_stores_announcement_once() {
        let store = temp_store("danlume-notify-ws-frame");
        let text = r#"{"type":"announcement","data":{"id":7,"title":"停机维护","body":"今晚 23:00","level":"info","pinned":false,"created_at":"2026-11-23T00:00:00Z","expires_at":""}}"#;
        let first = apply_frame(&store, text).expect("第一条应当入库");
        assert_eq!(first["title"], json!("停机维护"));
        assert_eq!(
            store.setting("last_announcement_id").unwrap_or_default(),
            "7"
        );
        assert_eq!(store.announcement_unread_count(), 1);

        // 重复帧（或轮询先拿到）不再返回、也不再插
        assert!(apply_frame(&store, text).is_none());
        assert_eq!(store.announcement_unread_count(), 1);

        // 迟到的旧公告照常入库，但不会把 last_announcement_id 拉回去
        let older = r#"{"type":"announcement","data":{"id":3,"title":"旧公告","body":"","level":"info","created_at":"2026-11-01T00:00:00Z"}}"#;
        assert!(apply_frame(&store, older).is_some());
        assert_eq!(
            store.setting("last_announcement_id").unwrap_or_default(),
            "7"
        );
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-notify-ws-frame"));
    }

    #[test]
    fn apply_frame_ignores_non_announcement_frames() {
        let store = temp_store("danlume-notify-ws-ignore");
        assert!(apply_frame(
            &store,
            r#"{"type":"hello","server_time":"2026-11-23T00:00:00Z","unread_count":0}"#
        )
        .is_none());
        assert!(apply_frame(&store, r#"{"type":"ping","ts":1}"#).is_none());
        assert!(apply_frame(&store, "not json").is_none());
        assert!(apply_frame(&store, r#"{"type":"announcement"}"#).is_none());
        assert_eq!(
            store.setting("last_announcement_id").unwrap_or_default(),
            ""
        );
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-notify-ws-ignore"));
    }
}

