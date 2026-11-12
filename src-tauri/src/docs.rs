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

/// 从 OOXML 里抠纯文本（DOCX / PPTX / XLSX）。
///
/// 对应旧版 `docs_reader.py` 的那几个解析器：旧版是用 zipfile + 正则直接读 OOXML，
/// 这里同样不引 XML 库——这些标签都是规整的，扫一遍取文本就够看。
fn office_text(path: &Path) -> Result<String, String> {
    let ext = path
        .extension()
        .map(|one| one.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let file = std::fs::File::open(path).map_err(|err| format!("打不开文件：{err}"))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|err| format!("不是合法的 docx/pptx/xlsx：{err}"))?;

    // 取一个条目当字符串
    fn read_entry(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<String> {
        let mut entry = zip.by_name(name).ok()?;
        let mut text = String::new();
        std::io::Read::read_to_string(&mut entry, &mut text).ok()?;
        Some(text)
    }

    /// 取 `<tag ...>文字</tag>` 里的文字，`</p>` 当换行
    fn texts_of(xml: &str, tag: &str) -> String {
        let mut out = String::new();
        let open = format!("<{tag}");
        let close = format!("</{tag}>");
        let mut rest = xml;
        while let Some(start) = rest.find(&open) {
            let Some(head_end) = rest[start..].find('>') else { break };
            let body_start = start + head_end + 1;
            let after = &rest[body_start..];
            let end = after.find(&close)
                .or_else(|| after.find('<'))
                .unwrap_or(after.len());
            let chunk = &after[..end];
            out.push_str(&unescape_xml(chunk));
            rest = &after[end..];
            // 段落结束就换行
            if rest.starts_with("</p>") || rest.starts_with("</a:p>") {
                out.push('\n');
            }
        }
        out
    }

    /// 简单的 XML 反转义（只处理常见五种 + 换行）
    fn unescape_xml(text: &str) -> String {
        text.replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&#10;", "\n")
    }

    let mut out = String::new();
    match ext.as_str() {
        "docx" => {
            let xml = read_entry(&mut zip, "word/document.xml")
                .ok_or_else(|| "docx 里没找到 word/document.xml".to_string())?;
            // 先按段落切，再取每段里的 <w:t>
            for paragraph in xml.split("</w:p>") {
                let line = texts_of(paragraph, "w:t");
                let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
                if !line.trim().is_empty() {
                    out.push_str(&line);
                    out.push('\n');
                }
                let _ = &xml; // 段落里已经拿到文本
            }
        }
        "pptx" => {
            // 幻灯片按序号排，每页之间空一行
            let mut slides: Vec<(u32, String)> = Vec::new();
            for index in 0..zip.len() {
                let name = {
                    let Ok(entry) = zip.by_index(index) else { continue };
                    entry.name().to_string()
                };
                if !name.starts_with("ppt/slides/slide") || !name.ends_with(".xml") {
                    continue;
                }
                let order: u32 = name
                    .trim_start_matches("ppt/slides/slide")
                    .trim_end_matches(".xml")
                    .parse()
                    .unwrap_or(0);
                if let Some(xml) = read_entry(&mut zip, &name) {
                    let text = texts_of(&xml, "a:t");
                    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                    if !text.trim().is_empty() {
                        slides.push((order, text));
                    }
                }
            }
            slides.sort_by_key(|(order, _)| *order);
            for (order, text) in slides {
                out.push_str(&format!("—— 第 {order} 页 ——\n{text}\n\n"));
            }
        }
        "xlsx" | "xlsm" => {
            // 共享字符串 + 每个工作表里的单元格（数值直接给，字符串去查共享表）
            let shared: Vec<String> = read_entry(&mut zip, "xl/sharedStrings.xml")
                .map(|xml| {
                    xml.split("</si>")
                        .map(|one| texts_of(one, "t"))
                        .map(|one| one.split_whitespace().collect::<Vec<_>>().join(" "))
                        .collect()
                })
                .unwrap_or_default();
            let mut sheets: Vec<String> = Vec::new();
            for index in 0..zip.len() {
                let name = {
                    let Ok(entry) = zip.by_index(index) else { continue };
                    entry.name().to_string()
                };
                if name.starts_with("xl/worksheets/sheet") && name.ends_with(".xml") {
                    sheets.push(name);
                }
            }
            sheets.sort();
            for name in sheets {
                let Some(xml) = read_entry(&mut zip, &name) else { continue };
                out.push_str(&format!("—— {} ——\n", name.trim_start_matches("xl/worksheets/")));
                for row in xml.split("</row>") {
                    let mut cells: Vec<String> = Vec::new();
                    let mut rest = row;
                    while let Some(pos) = rest.find("<c ") {
                        let Some(head_end) = rest[pos..].find('>') else { break };
                        let head = &rest[pos..pos + head_end];
                        let after = &rest[pos + head_end + 1..];
                        let body_end = after.find("</c>").unwrap_or(0);
                        let body = &after[..body_end];
                        let value = texts_of(body, "v");
                        let value = value.trim().to_string();
                        let is_shared = head.contains("t=\"s\"");
                        if is_shared {
                            if let Ok(at) = value.parse::<usize>() {
                                if let Some(text) = shared.get(at) {
                                    cells.push(text.clone());
                                }
                            }
                        } else if !value.is_empty() {
                            cells.push(value);
                        }
                        rest = if body_end > 0 { &after[body_end..] } else { "" };
                        if rest.is_empty() {
                            break;
                        }
                    }
                    let line = cells.join("\t");
                    if !line.trim().is_empty() {
                        out.push_str(&line);
                        out.push('\n');
                    }
                }
                out.push('\n');
            }
        }
        _ => return Err("不是 OOXML 文档".to_string()),
    }
    if out.trim().is_empty() {
        return Err("这个文档里没抽到文字（可能是扫描件或图片版）".to_string());
    }
    Ok(out)
}

/// 文档预览：文本直接读，DOCX / PPTX / XLSX 抽正文，其它让用户用系统程序打开
pub fn preview_text(path: &str, limit: usize) -> Result<String, String> {
    let target = PathBuf::from(path);
    let ext = target
        .extension()
        .map(|one| one.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let text = match ext.as_str() {
        "docx" | "pptx" | "xlsx" | "xlsm" => office_text(&target)?,
        _ => return read_text(path, limit),
    };
    if text.chars().count() > limit {
        let head: String = text.chars().take(limit).collect();
        Ok(format!("{head}\n\n（只显示前 {limit} 字，完整内容请用系统程序打开）"))
    } else {
        Ok(text)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn write_zip(path: &Path, entries: &[(&str, &str)]) {
        use std::io::Write;
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default();
        for (name, body) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn extracts_docx_text() {
        let dir = std::env::temp_dir().join(format!("danlume-docx-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("sample.docx");
        write_zip(
            &path,
            &[(
                "word/document.xml",
                "<?xml version=\"1.0\"?><w:document><w:body>\
                 <w:p><w:r><w:t>第一段</w:t></w:r></w:p>\
                 <w:p><w:r><w:t>第二段 &amp; 转义</w:t></w:r></w:p>\
                 </w:body></w:document>",
            )],
        );
        let text = preview_text(&path.display().to_string(), 10_000).unwrap();
        assert!(text.contains("第一段"), "{text}");
        assert!(text.contains("第二段 & 转义"), "{text}");
    }

    #[test]
    fn extracts_pptx_text_by_slide() {
        let dir = std::env::temp_dir().join(format!("danlume-pptx-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("sample.pptx");
        write_zip(
            &path,
            &[
                ("ppt/slides/slide1.xml", "<p:sld><a:p><a:r><a:t>第一页</a:t></a:r></a:p></p:sld>"),
                ("ppt/slides/slide2.xml", "<p:sld><a:p><a:r><a:t>第二页</a:t></a:r></a:p></p:sld>"),
            ],
        );
        let text = preview_text(&path.display().to_string(), 10_000).unwrap();
        assert!(text.contains("第 1 页"), "{text}");
        assert!(text.contains("第一页"), "{text}");
        assert!(text.contains("第二页"), "{text}");
    }

    #[test]
    fn reads_plain_text_files() {
        let dir = std::env::temp_dir().join(format!("danlume-txt-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("a.txt");
        std::fs::write(&path, "hello 世界").unwrap();
        let text = preview_text(&path.display().to_string(), 1000).unwrap();
        assert_eq!(text, "hello 世界");
    }
}
