//! 数据目录解析：完全沿用旧版 `lumina/config.py` 的规则。
//!
//! 顺序：
//! 1. 环境变量 `LUMINA_DATA_DIR`（测试 / 调试用）
//! 2. `%LOCALAPPDATA%\DanLume\data_dir.txt` 里写的路径（打包版的指针文件）
//! 3. exe（或当前目录）同级的 `data\`
use std::path::{Path, PathBuf};

/// 指针文件名（旧版打包版就用它指定真实数据目录）
const POINTER_DIR: &str = "DanLume";
const POINTER_FILE: &str = "data_dir.txt";

fn local_app_data() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        // macOS 的配置指针与数据库保存在用户 Application Support 目录。
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("Library").join("Application Support"));
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    }
}

/// 读指针文件里的路径（不存在 / 读不出来就返回 None）
fn pointer_target() -> Option<PathBuf> {
    let base = local_app_data()?;
    let path = base.join(POINTER_DIR).join(POINTER_FILE);
    let text = std::fs::read_to_string(path).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(PathBuf::from(trimmed))
}

/// 当前应该用的数据目录
pub fn data_dir() -> PathBuf {
    if let Some(value) = std::env::var_os("LUMINA_DATA_DIR") {
        let text = PathBuf::from(value);
        if !text.as_os_str().is_empty() {
            return text;
        }
    }
    if let Some(target) = pointer_target() {
        return target;
    }
    #[cfg(target_os = "macos")]
    {
        // 避免把数据库写进签名的 .app 目录，升级应用时数据也能保留。
        if let Some(base) = local_app_data() {
            return base.join(POINTER_DIR).join("data");
        }
    }
    // exe 同级 data/（开发时是 target/debug/…，所以退到当前工作目录）
    let base = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));
    let portable = base.join("data");
    if portable.exists() {
        return portable;
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let here = cwd.join("data");
    if here.exists() {
        here
    } else {
        portable
    }
}

/// 数据库文件
pub fn db_path() -> PathBuf {
    data_dir().join("lumina.db")
}

/// 确保数据目录存在
pub fn ensure_data_dir() -> PathBuf {
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    dir
}
