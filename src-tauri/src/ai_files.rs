//! AI 附件：把「拖进来的路径」展开成可当附件的文件（对应旧版 `ai_files.expand_paths`）。
//!
//! 单独成模块是为了能写单测：文件夹递归、按后缀过滤、去重、隐藏目录跳过、
//! 以及「不支持就明说」这几条都在这里锁住。
use serde_json::{json, Value};

/// 拖一个文件夹进来时最多扫这么多文件（旧版 `MAX_FOLDER_FILES`）
pub const MAX_FOLDER_FILES: usize = 200;

const TEXT: &[&str] = &[
    "txt", "md", "markdown", "rst", "tex", "bib", "csv", "tsv", "log", "json", "jsonl", "yaml",
    "yml", "toml", "ini", "cfg", "conf", "py", "js", "ts", "jsx", "tsx", "java", "c", "h", "cpp",
    "hpp", "cs", "go", "rs", "rb", "php", "sh", "ps1", "bat", "sql", "html", "htm", "xml", "css",
];
const OFFICE: &[&str] = &["docx", "pptx", "xlsx"];
const PDF: &[&str] = &["pdf"];
const IMAGE: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp"];

/// 这个后缀能当什么用（空串 = 不支持）
fn classify(path: &std::path::Path) -> &'static str {
    let ext = path
        .extension()
        .map(|one| one.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if IMAGE.contains(&ext.as_str()) {
        "image"
    } else if PDF.contains(&ext.as_str()) {
        "pdf"
    } else if OFFICE.contains(&ext.as_str()) {
        "office"
    } else if TEXT.contains(&ext.as_str()) {
        "text"
    } else {
        ""
    }
}

/// 展开路径：返回 (文件列表, 提示信息)
pub fn expand(paths: &[String]) -> (Vec<Value>, Vec<String>) {
    let mut files: Vec<Value> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for raw in paths {
        let path = std::path::PathBuf::from(raw);
        if !path.exists() {
            notes.push(format!("{raw}：找不到这个文件"));
            continue;
        }
        let mut batch: Vec<std::path::PathBuf> = Vec::new();
        if path.is_dir() {
            let mut stack = vec![path.clone()];
            while let Some(dir) = stack.pop() {
                let Ok(entries) = std::fs::read_dir(&dir) else {
                    continue;
                };
                let mut rows: Vec<std::path::PathBuf> =
                    entries.flatten().map(|entry| entry.path()).collect();
                rows.sort();
                for child in rows {
                    if batch.len() >= MAX_FOLDER_FILES {
                        break;
                    }
                    if child.is_dir() {
                        // 跳过隐藏目录（.git 之类）
                        let hidden = child
                            .file_name()
                            .map(|name| name.to_string_lossy().starts_with('.'))
                            .unwrap_or(false);
                        if !hidden {
                            stack.push(child);
                        }
                        continue;
                    }
                    if child.is_file() && !classify(&child).is_empty() {
                        batch.push(child);
                    }
                }
            }
            if batch.is_empty() {
                notes.push(format!("{}：里面没有可读的文件", path.display()));
            }
        } else {
            batch.push(path.clone());
        }
        for child in batch {
            let kind = classify(&child);
            if kind.is_empty() {
                notes.push(format!(
                    "{}：暂不支持这种格式（PDF / Word / PPT / Excel / 文本 / 图片才认）",
                    child.display()
                ));
                continue;
            }
            let key = child.to_string_lossy().to_lowercase();
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            let size = child.metadata().map(|one| one.len()).unwrap_or(0);
            files.push(json!({
                "path": child.display().to_string(),
                "name": child
                    .file_name()
                    .map(|one| one.to_string_lossy().to_string())
                    .unwrap_or_default(),
                "size": size,
                "kind": kind,
            }));
        }
    }
    (files, notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|one| one.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "danlume-aifiles-{tag}-{}-{nanos}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }

    /// 单文件：认得出类型，大小也带上
    #[test]
    fn single_files_are_classified() {
        let dir = temp_dir("single");
        let txt = dir.join("笔记.txt");
        let pdf = dir.join("论文.pdf");
        let png = dir.join("截图.png");
        std::fs::write(&txt, b"hello").unwrap();
        std::fs::write(&pdf, b"%PDF").unwrap();
        std::fs::write(&png, b"\x89PNG").unwrap();
        let (files, notes) = expand(&[
            txt.display().to_string(),
            pdf.display().to_string(),
            png.display().to_string(),
        ]);
        assert!(notes.is_empty(), "不该有提示：{notes:?}");
        assert_eq!(files.len(), 3);
        assert_eq!(files[0]["kind"], json!("text"));
        assert_eq!(files[1]["kind"], json!("pdf"));
        assert_eq!(files[2]["kind"], json!("image"));
        assert_eq!(files[0]["size"], json!(5));
    }

    /// 文件夹递归 + 只收认识的后缀 + 跳过隐藏目录 + 去重
    #[test]
    fn folders_are_expanded_and_filtered() {
        let dir = temp_dir("folder");
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join("a.md"), b"a").unwrap();
        std::fs::write(dir.join("b.bin"), b"b").unwrap();
        std::fs::write(sub.join("c.docx"), b"c").unwrap();
        std::fs::write(dir.join(".git").join("config"), b"x").unwrap();

        let (files, _notes) = expand(&[dir.display().to_string(), dir.display().to_string()]);
        let names: Vec<String> = files
            .iter()
            .map(|one| one["name"].as_str().unwrap_or("").to_string())
            .collect();
        assert!(names.contains(&"a.md".to_string()), "{names:?}");
        assert!(names.contains(&"c.docx".to_string()), "{names:?}");
        assert!(!names.contains(&"b.bin".to_string()), "不认识的格式不该进来");
        assert!(!names.contains(&"config".to_string()), "隐藏目录要跳过");
        // 同一个文件夹给两次也只算一份
        assert_eq!(names.len(), 2, "{names:?}");
    }

    /// 不支持的格式要明说，不静默丢掉
    #[test]
    fn unsupported_extension_is_reported() {
        let dir = temp_dir("unsupported");
        let path = dir.join("老文件.doc");
        std::fs::write(&path, b"x").unwrap();
        let (files, notes) = expand(&[path.display().to_string()]);
        assert!(files.is_empty());
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("暂不支持"), "{:?}", notes);
    }

    /// 路径不存在时给一句人话
    #[test]
    fn missing_path_is_reported() {
        let (files, notes) = expand(&["E:\\definitely-not-here\\x.pdf".to_string()]);
        assert!(files.is_empty());
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("找不到"), "{:?}", notes);
    }
}
