//! EndNote 检测（对应旧版 `lumina/endnote.py` 的 `detect()`）。
//!
//! 用户要的是：导出 `.enw` / `.ris` 之后，**装了 EndNote 就直接拿它打开**；
//! 没装就告诉他去 eHall 软件中心装，而不是让他双击之后一脸问号。
//!
//! 只看本机注册表和几个常见安装目录，不联网。
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const EXE_NAMES: [&str; 4] = ["EndNote.exe", "EndNote20.exe", "EndNote21.exe", "EndNote X9.exe"];

/// 注册表里可能写着安装位置的地方（与旧版 `_registry_path()` 一致）
const REG_KEYS: [&str; 5] = [
    r"HKLM\SOFTWARE\Clarivate\EndNote",
    r"HKLM\SOFTWARE\WOW6432Node\Clarivate\EndNote",
    r"HKCU\SOFTWARE\Clarivate\EndNote",
    r"HKLM\SOFTWARE\ISI ResearchSoft\EndNote",
    r"HKLM\SOFTWARE\WOW6432Node\ISI ResearchSoft\EndNote",
];

/// 常见安装目录（多盘符都看，用户可能装在 D:）
fn candidate_dirs() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for name in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
        if let Ok(value) = std::env::var(name) {
            if !value.trim().is_empty() {
                roots.push(PathBuf::from(value));
            }
        }
    }
    for drive in ["C:", "D:", "E:"] {
        roots.push(PathBuf::from(format!(r"{drive}\Program Files")));
        roots.push(PathBuf::from(format!(r"{drive}\Program Files (x86)")));
    }
    let mut dirs = Vec::new();
    for root in roots {
        for name in [
            "EndNote",
            "EndNote 20",
            "EndNote 21",
            "EndNote X9",
            r"Clarivate\EndNote",
            r"Clarivate\EndNote 21",
            r"ResearchSoft\EndNote",
        ] {
            let path = root.join(name);
            if path.is_dir() {
                dirs.push(path);
            }
        }
    }
    dirs
}

/// 在注册表里翻一遍：值里既有 .exe 又有 endnote 的，或者 InstallDir 这种目录值
#[cfg(windows)]
fn registry_path() -> Option<String> {
    #[cfg(windows)]
    use std::os::windows::process::CommandExt;
    for key in REG_KEYS {
        let output = {
            let mut command = std::process::Command::new("reg");
            command.args(["query", key, "/s"]);
            // 别在用户面前闪一个黑框
            #[cfg(windows)]
            command.creation_flags(0x0800_0000);
            command.output()
        };
        let Ok(output) = output else { continue };
        if !output.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&output.stdout).to_string();
        for line in text.lines() {
            let Some((name, rest)) = line.split_once("REG_SZ") else {
                continue;
            };
            let value = rest.trim().trim_matches('"');
            if value.is_empty() {
                continue;
            }
            let lower = value.to_lowercase();
            if lower.ends_with(".exe") && lower.contains("endnote") && Path::new(value).is_file() {
                return Some(value.to_string());
            }
            let name = name.trim().to_lowercase();
            if ["installdir", "installdirectory", "install_path", "path"].contains(&name.as_str()) {
                for exe in EXE_NAMES {
                    let candidate = Path::new(value).join(exe);
                    if candidate.is_file() {
                        return Some(candidate.display().to_string());
                    }
                }
            }
        }
    }
    None
}

#[cfg(not(windows))]
fn registry_path() -> Option<String> {
    None
}

/// 找 EndNote 主程序：注册表 → 常见目录（目录里再翻一层 exe）
pub fn find() -> Option<String> {
    if let Some(found) = registry_path() {
        return Some(found);
    }
    for dir in candidate_dirs() {
        for exe in EXE_NAMES {
            let candidate = dir.join(exe);
            if candidate.is_file() {
                return Some(candidate.display().to_string());
            }
        }
        // 有些版本装在 `EndNote\Program\` 这类子目录里，往下翻一层
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    for exe in EXE_NAMES {
                        let candidate = path.join(exe);
                        if candidate.is_file() {
                            return Some(candidate.display().to_string());
                        }
                    }
                }
                let name = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
                if name.ends_with(".exe") && name.contains("endnote") {
                    return Some(path.display().to_string());
                }
            }
        }
    }
    None
}

/// 给前端用：`{found, path, summary}`
pub fn detect() -> Value {
    match find() {
        Some(path) => json!({
            "found": true,
            "path": path,
            "summary": format!("这台机器装了 EndNote：{path}"),
        }),
        None => json!({
            "found": false,
            "path": "",
            "summary": "这台机器没找到 EndNote。导出的 .enw / .ris 可以双击交给 Zotero / Mendeley 打开，\
                        也可以到 eHall 的软件正版化里装 EndNote。",
        }),
    }
}

/// 用 EndNote 打开刚导出的文献文件；没装就返回 false（前端好给提示）
pub fn open_with(path: &str) -> Result<bool, String> {
    let Some(exe) = find() else {
        return Ok(false);
    };
    if !Path::new(path).is_file() {
        return Err("这个文件不在了".to_string());
    }
    std::process::Command::new(exe)
        .arg(path)
        .spawn()
        .map_err(|err| format!("调不起 EndNote：{err}"))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_always_returns_a_summary() {
        let value = detect();
        assert!(value.get("found").is_some(), "必须带 found 字段");
        let summary = value.get("summary").and_then(|one| one.as_str()).unwrap_or("");
        assert!(!summary.trim().is_empty(), "必须有给人看的一句话");
        // 没装时 path 是空串，前端就不用判断 null
        if value["found"] == json!(false) {
            assert_eq!(value["path"], json!(""));
        }
    }

    #[test]
    fn open_without_endnote_is_not_an_error() {
        // 传一个不存在的路径：没装 EndNote 时应当返回 Ok(false)，而不是炸掉
        let result = open_with("C:\\这个文件不存在.enw");
        match result {
            Ok(false) => {}
            Ok(true) => println!("这台机器装了 EndNote，跳过了"),
            Err(err) => panic!("不该报错：{err}"),
        }
    }
}
