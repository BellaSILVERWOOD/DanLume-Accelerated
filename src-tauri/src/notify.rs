//! 服务器公告的「实时」接收。
//!
//! ## 公告通道：5 秒轮询（曾尝试 WebSocket，因依赖问题回退）
//!
//! 服务端契约里有 `WS /api/v1/stream?token=...`，但客户端这边满足不了：
//!
//! 1. `src-tauri/Cargo.toml`（以及 `Cargo.lock`）里**没有任何 WebSocket 客户端**：
//!    `tokio-tungstenite` / `tungstenite` / `tauri-plugin-websocket` 都没有；`tokio` 只是
//!    tauri 的间接依赖，不提供 WS 能力。约束写明「不要新增 crate」，所以不能建 WS 连接。
//! 2. 现有 HTTP 客户端 `ureq` 不把底层 TCP/TLS 流交出来，手搓 WebSocket 握手拿不到通道。
//! 3. 服务器上 `/api/v1/stream` 确实在（普通 GET 回 404，带 `Upgrade: websocket` 头会回 403，
//!    因为它只接 WS 帧），所以「没得连」不是原因——原因就是客户端没有可用的 WS 实现。
//!
//! 因此这里退化成 **5 秒一次的长轮询** `GET /api/v1/announcements?since=<last_id>&limit=200`：
//! 公告是低频事件，「几十秒内到达」足够；登录成功、改服务器地址、点「立刻检查」时还会
//! `kick()` 马上拉一次，第一次基本是秒级到。`tauri.conf.json` 不需要任何改动。
//!
//! ## 行为
//!
//! * 只有「配了服务器 + 已登录」才轮询（没登录就每 1 秒看一眼设置，几乎不耗资源）；
//! * 新公告：写 SQLite（`store.rs` 的 `announcements` 表）→ 更新 `last_announcement_id`
//!   → 通过 Tauri 事件 `danlume://announcement` 逐条推给前端；
//! * 断线重连：指数退避 1s → 2s → 4s …… 60s 上限，成功一次就复位；
//! * 服务器地址 / token 变了（`kick()` 或下一轮读到设置变化）：清退避、立刻重建；
//! * 服务器没有公告接口（老版本回 404）时标记 `unsupported`，改成 300 秒探一次，
//!   不刷屏也不放弃——服务器升级后会自动恢复。
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::Emitter;

use crate::account;
use crate::store::Store;

/// 推给前端的事件名（Tauri 只允许字母数字和 `- / : _`，`://` 是合法的）
pub const ANNOUNCEMENT_EVENT: &str = "danlume://announcement";
/// 正常情况下每 5 秒拉一次
pub const POLL_SECS: u64 = 5;
/// 退避上限
pub const MAX_BACKOFF_SECS: u64 = 60;
/// 服务器没有公告接口时，多久再探一次
pub const UNSUPPORTED_RETRY_SECS: u64 = 300;
/// 循环心跳：500ms 看一次有没有活要干（这样 kick() 能立刻生效，也不费 CPU）
const TICK: Duration = Duration::from_millis(500);

static STARTED: AtomicBool = AtomicBool::new(false);
static KICK: AtomicBool = AtomicBool::new(false);

/// 轮询线程的运行状态（设置页 / 报告里能看到到底有没有在工作）
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

/// 后台线程只起一次；重复调用是空操作
pub fn spawn(app: tauri::AppHandle) {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::Builder::new()
        .name("danlume-announce".to_string())
        .spawn(move || run(app))
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
                    due = Instant::now() + Duration::from_secs(POLL_SECS);
                    update_state(|state| {
                        state.last_ok_at = stamp();
                        state.last_error.clear();
                        state.last_error_kind.clear();
                        state.unsupported = false;
                        state.backoff_secs = 0;
                        state.next_delay_secs = POLL_SECS;
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
    json!({
        "mode": "polling",
        "reason": "WS 依赖在当前环境加载失败，改用 5 秒长轮询（近实时）",
        "interval_secs": POLL_SECS,
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
}

