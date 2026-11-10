//! 文档页：目录浏览、文本预览、用系统默认程序打开（旧版 page_docs.py 的核心）
use std::path::{Path, PathBuf};

use serde::Serialize;

/// 一个文件/文件夹条目
#[derive(Debug, Clone, Serialize)]
pub struct DocEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: String,
    pub kind: String,
}

fn kind_of(path: &Path) -> String {
    let ext = path
        .extension()
        .map(|value| value.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "pdf" => "PDF",
        "doc" | "docx" => "Word",
        "ppt" | "pptx" => "PPT",
        "xls" | "xlsx" | "csv" => "Excel",
        "txt" | "md" | "log" | "json" => "文本",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" => "图片",
        "" => "文件",
        other => {
            return other.to_uppercase();
        }
    }
    .to_string()
}

fn modified_text(time: std::io::Result<std::time::SystemTime>) -> String {
    use chrono::{DateTime, Local};

    match time {
        Ok(value) => {
            let datetime: DateTime<Local> = value.into();
            datetime.format("%Y-%m-%d %H:%M").to_string()
        }
        Err(_) => String::new(),
    }
}

/// 列一个目录（文件夹在前、文件在后，各自按名字排）
pub fn list_dir(dir: &str) -> Result<Vec<DocEntry>, String> {
    let path = PathBuf::from(if dir.trim().is_empty() { "." } else { dir });
    let reader = std::fs::read_dir(&path).map_err(|err| format!("读不了这个目录：{err}"))?;
    let mut dirs: Vec<DocEntry> = Vec::new();
    let mut files: Vec<DocEntry> = Vec::new();
    for entry in reader.flatten() {
        let file_path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let meta = entry.metadata().ok();
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let item = DocEntry {
            name,
            path: file_path.display().to_string(),
            is_dir,
            size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
            modified: modified_text(
                meta.as_ref()
                    .and_then(|m| m.modified().ok())
                    .ok_or(std::io::Error::other("no mtime")),
            ),
            kind: if is_dir {
                "文件夹".to_string()
            } else {
                kind_of(&file_path)
            },
        };
        if is_dir {
            dirs.push(item);
        } else {
            files.push(item);
        }
    }
    dirs.sort_by(|a, b| a.name.cmp(&b.name));
    files.sort_by(|a, b| a.name.cmp(&b.name));
    dirs.extend(files);
    Ok(dirs)
}

/// 读文本文件的前 `limit` 字节（预览用）
pub fn read_text(path: &str, limit: usize) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|err| format!("读不了这个文件：{err}"))?;
    let slice = if data.len() > limit {
        &data[..limit]
    } else {
        &data[..]
    };
    let text = String::from_utf8_lossy(slice).to_string();
    if data.len() > limit {
        Ok(format!("{text}\n\n（只显示前 {limit} 字节，完整内容请用系统程序打开）"))
    } else {
        Ok(text)
    }
}

/// 用系统默认程序打开（等同于双击）
pub fn open_with_system(path: &str) -> Result<(), String> {
    if !Path::new(path).exists() {
        return Err(format!("这个文件不在了：{path}"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        // 0x08000000 = CREATE_NO_WINDOW
        std::process::Command::new("cmd")
            .args(["/C", "start", "", path])
            .creation_flags(0x0800_0000)
            .spawn()
            .map_err(|err| format!("打不开：{err}"))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map_err(|err| format!("打不开：{err}"))?;
        Ok(())
    }
}

/// 在资源管理器里显示（选中这个文件）
pub fn reveal_in_explorer(path: &str) -> Result<(), String> {
    if !Path::new(path).exists() {
        return Err(format!("这个文件不在了：{path}"));
    }
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(format!("/select,{path}"))
            .spawn()
            .map_err(|err| format!("打不开资源管理器：{err}"))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        open_with_system(path)
    }
}
