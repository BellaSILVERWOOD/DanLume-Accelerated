//! Agent 沙箱：给 AI 一块**只能在自己的目录里动手**的地方。
//!
//! 为什么单独一块：`fs_write` 这类工具能碰用户整个磁盘，做「Agent」时太宽；
//! 这里给一个固定根目录（`data/agent_sandbox/`），
//! * 读写都限制在根目录内（拒绝 `..`、绝对路径、盘符）；
//! * 跑命令只在这个目录里、输出截断；
//! * **跑命令默认是关的**（`agent_sandbox_run`），要用户自己打开才允许，
//!   而且每次动手前还有一道工具审核（`ai:confirm`）。
//!
//! 时长：**默认不限制**（用户 2026-11-11 明确要求「没必要限制这个时长」）。
//! 真需要上限就设 `agent_sandbox_timeout_secs`（秒，0 或不填 = 不限）。
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

pub const RUN_KEY: &str = "agent_sandbox_run";
/// 时长上限的设置键：`0` / 空 = 不限
pub const TIMEOUT_KEY: &str = "agent_sandbox_timeout_secs";
/// 输出最多留这么多字
const MAX_OUTPUT: usize = 8000;

/// 单次命令的时长上限（秒）；`0` = 不限
///
/// 以前写死 30 秒，用户报「不该限制时长」——现在默认不限，需要的话设置里给个数。
pub fn timeout_secs(store: &crate::store::Store) -> u64 {
    store
        .setting(TIMEOUT_KEY)
        .and_then(|text| text.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

/// 沙箱根目录（不存在就建）
pub fn root() -> PathBuf {
    let dir = crate::paths::data_dir().join("agent_sandbox");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// 相对路径 → 沙箱内的绝对路径；越界一律拒绝
pub fn safe_join(rel: &str) -> Result<PathBuf, String> {
    let text = rel.trim().trim_matches('"');
    let base = root();
    if text.is_empty() {
        return Ok(base);
    }
    let candidate = Path::new(text);
    if candidate.is_absolute() {
        return Err("只能写沙箱里的相对路径（不要给绝对路径）".to_string());
    }
    // 逐段检查，拒绝任何 `..`
    for part in candidate.components() {
        match part {
            std::path::Component::ParentDir => {
                return Err("路径里不能有 `..`（只能在沙箱里活动）".to_string())
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                return Err("只能写沙箱里的相对路径".to_string())
            }
            _ => {}
        }
    }
    Ok(base.join(candidate))
}

/// 跑命令允许吗（默认关）
pub fn run_enabled(store: &crate::store::Store) -> bool {
    store
        .setting(RUN_KEY)
        .map(|text| text.trim() == "1")
        .unwrap_or(false)
}

pub fn set_run_enabled(store: &crate::store::Store, on: bool) -> Result<Value, String> {
    store
        .set_setting(RUN_KEY, if on { "1" } else { "0" })
        .map_err(|err| err.to_string())?;
    Ok(json!({ "ok": true, "run": on }))
}

pub fn list(rel: &str) -> Result<Value, String> {
    let dir = safe_join(rel)?;
    if !dir.exists() {
        return Ok(json!({ "ok": true, "path": rel, "entries": [] }));
    }
    let mut entries: Vec<Value> = Vec::new();
    let rows = std::fs::read_dir(&dir).map_err(|err| format!("读不了目录：{err}"))?;
    for entry in rows.flatten() {
        let path = entry.path();
        let is_dir = path.is_dir();
        let size = path.metadata().map(|one| one.len()).unwrap_or(0);
        let name = entry.file_name().to_string_lossy().to_string();
        let inside = path
            .strip_prefix(root())
            .map(|one| one.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| name.clone());
        entries.push(json!({ "name": name, "path": inside, "dir": is_dir, "size": size }));
    }
    entries.sort_by(|left, right| {
        let l = left["dir"].as_bool().unwrap_or(false);
        let r = right["dir"].as_bool().unwrap_or(false);
        r.cmp(&l).then_with(|| {
            left["name"]
                .as_str()
                .unwrap_or("")
                .cmp(right["name"].as_str().unwrap_or(""))
        })
    });
    Ok(json!({ "ok": true, "path": rel, "entries": entries, "root": root().display().to_string() }))
}

pub fn read(rel: &str) -> Result<Value, String> {
    let path = safe_join(rel)?;
    let text = std::fs::read_to_string(&path).map_err(|err| format!("读不了文件：{err}"))?;
    let clipped: String = text.chars().take(MAX_OUTPUT).collect();
    Ok(json!({
        "ok": true,
        "path": rel,
        "size": text.chars().count(),
        "text": clipped,
        "truncated": text.chars().count() > MAX_OUTPUT,
    }))
}

pub fn write(rel: &str, content: &str) -> Result<Value, String> {
    if rel.trim().is_empty() {
        return Err("要给一个文件名（相对沙箱目录）".to_string());
    }
    let path = safe_join(rel)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| format!("建不了目录：{err}"))?;
    }
    std::fs::write(&path, content).map_err(|err| format!("写不了文件：{err}"))?;
    Ok(json!({
        "ok": true,
        "path": rel,
        "chars": content.chars().count(),
        "where": path.display().to_string(),
    }))
}

/// 在沙箱里跑一条命令（Windows 走 `cmd /C`）。**只有用户打开开关才允许。**
pub fn run(store: &crate::store::Store, command: &str) -> Result<Value, String> {
    let cmd = command.trim();
    if cmd.is_empty() {
        return Err("命令是空的".to_string());
    }
    if !run_enabled(store) {
        return Ok(json!({
            "ok": false,
            "error": "沙箱里跑命令是关着的：让用户在 AI 面板里打开「允许跑命令」后再试",
        }));
    }
    let dir = root();
    #[cfg(windows)]
    let mut child = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // 同样不要弹黑窗口
        Command::new("cmd")
            .arg("/C")
            .arg(cmd)
            .current_dir(&dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|err| format!("起不了命令：{err}"))?
    };
    #[cfg(not(windows))]
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(&dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| format!("起不了命令：{err}"))?;

    // 默认不限时长；只有用户自己设了 `agent_sandbox_timeout_secs` 才会掐
    let limit = timeout_secs(store);
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if limit > 0 && started.elapsed() >= std::time::Duration::from_secs(limit) {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(std::time::Duration::from_millis(120));
            }
            Err(err) => return Err(format!("等待命令结束失败：{err}")),
        }
    };
    let mut text = String::new();
    if let Some(out) = child.stdout.take() {
        use std::io::Read;
        let mut buf = Vec::new();
        let _ = out.take(MAX_OUTPUT as u64).read_to_end(&mut buf);
        text.push_str(&String::from_utf8_lossy(&buf));
    }
    if let Some(err_out) = child.stderr.take() {
        use std::io::Read;
        let mut buf = Vec::new();
        let _ = err_out.take(MAX_OUTPUT as u64).read_to_end(&mut buf);
        let extra = String::from_utf8_lossy(&buf);
        if !extra.trim().is_empty() {
            text.push_str("\n[stderr]\n");
            text.push_str(&extra);
        }
    }
    let clipped: String = text.chars().take(MAX_OUTPUT).collect();
    Ok(json!({
        "ok": status.map(|one| one.success()).unwrap_or(false),
        "timed_out": status.is_none(),
        "timeout_secs": limit,
        "code": status.and_then(|one| one.code()).unwrap_or(-1),
        "output": clipped,
        "where": dir.display().to_string(),
    }))
}

/// 给界面用的一份概览：根目录 + 开关 + 前若干项
pub fn overview(store: &crate::store::Store) -> Value {
    let listing = list("").unwrap_or_else(|_| json!({ "entries": [] }));
    json!({
        "ok": true,
        "root": root().display().to_string(),
        "run": run_enabled(store),
        "entries": listing.get("entries").cloned().unwrap_or(json!([])),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_escaping_paths() {
        assert!(safe_join("notes/a.txt").is_ok());
        assert!(safe_join("../outside.txt").is_err());
        assert!(safe_join("a/../../b.txt").is_err());
        assert!(safe_join("C:\\Windows\\x.txt").is_err());
        assert!(safe_join("/etc/passwd").is_err());
    }

    #[test]
    fn write_read_roundtrip_stays_inside() {
        let name = format!("test-{}.txt", std::process::id());
        let written = write(&name, "沙箱测试").expect("写入");
        assert_eq!(written["chars"], json!(4));
        let read_back = read(&name).expect("读回");
        assert_eq!(read_back["text"], json!("沙箱测试"));
        // 真的落在 agent_sandbox 目录里
        let path = safe_join(&name).unwrap();
        assert!(path.starts_with(root()));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn run_is_off_by_default() {
        let dir = std::env::temp_dir().join(format!("danlume-sandbox-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("LUMINA_DATA_DIR", &dir);
        let store = crate::store::Store::open().expect("临时库");
        assert!(!run_enabled(&store), "默认必须是关的");
        let blocked = run(&store, "echo hi").expect("调用本身不该失败");
        assert_eq!(blocked["ok"], json!(false));
        assert!(blocked["error"]
            .as_str()
            .unwrap_or("")
            .contains("关着"));
        set_run_enabled(&store, true).unwrap();
        assert!(run_enabled(&store));
        // 收尾：删掉临时数据目录，并把环境变量指回原来的（-1 表示没有）
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 用户 2026-11-11：「没必要限制这个时长」——默认不限，用户自己设了才掐
    #[test]
    fn timeout_is_unlimited_by_default() {
        let dir = std::env::temp_dir().join(format!("danlume-sandbox-to-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("LUMINA_DATA_DIR", &dir);
        let store = crate::store::Store::open().expect("临时库");
        assert_eq!(timeout_secs(&store), 0, "默认不该限制时长");
        let _ = store.set_setting(TIMEOUT_KEY, "5");
        assert_eq!(timeout_secs(&store), 5, "设了就用设的值");
        let _ = store.set_setting(TIMEOUT_KEY, "");
        assert_eq!(timeout_secs(&store), 0, "清空 = 不限");
        let _ = store.set_setting(TIMEOUT_KEY, "乱写的");
        assert_eq!(timeout_secs(&store), 0, "写错也当不限，不要因此跑不了命令");
        // 打开开关后真跑一条：不限时长时命令能正常结束
        set_run_enabled(&store, true).unwrap();
        let done = run(&store, "echo sandbox-ok").expect("命令该跑起来");
        assert_eq!(done["ok"], json!(true), "{done}");
        assert_eq!(done["timed_out"], json!(false));
        assert!(
            done["output"].as_str().unwrap_or("").contains("sandbox-ok"),
            "输出不见了：{done}"
        );
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
