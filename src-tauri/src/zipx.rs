//! 解压工具（AI 的 `zip_extract` 用）。防路径穿越：所有条目都必须落在目标目录里。
use std::path::{Component, Path, PathBuf};

pub fn extract(archive: &Path, dest: &Path) -> Result<usize, String> {
    if !archive.exists() {
        return Err(format!("压缩包不存在：{}", archive.display()));
    }
    let file = std::fs::File::open(archive).map_err(|err| format!("打不开压缩包：{err}"))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|err| format!("不是合法的 zip：{err}"))?;
    std::fs::create_dir_all(dest).map_err(|err| format!("建目录失败：{err}"))?;
    let base = dest
        .canonicalize()
        .unwrap_or_else(|_| dest.to_path_buf());
    let mut count = 0usize;
    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|err| format!("读第 {index} 个条目失败：{err}"))?;
        let raw = entry.name().to_string();
        let clean = sanitize(&raw);
        let Some(clean) = clean else {
            continue;
        };
        let target = base.join(&clean);
        if !target.starts_with(&base) {
            return Err(format!("压缩包里有越界路径，已拒绝：{raw}"));
        }
        if raw.ends_with('/') {
            let _ = std::fs::create_dir_all(&target);
            continue;
        }
        if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut out =
            std::fs::File::create(&target).map_err(|err| format!("写文件失败：{err}"))?;
        std::io::copy(&mut entry, &mut out).map_err(|err| format!("解压失败：{err}"))?;
        count += 1;
    }
    Ok(count)
}

/// 把 `../` 这类相对跳转全部消掉；返回 None 表示这个条目应当跳过
fn sanitize(name: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in Path::new(name).components() {
        match part {
            Component::Normal(one) => out.push(one),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::RootDir | Component::Prefix(_) => {}
        }
    }
    if out.as_os_str().is_empty() {
        None
    } else {
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_blocks_traversal() {
        assert_eq!(sanitize("../../evil.txt"), Some(PathBuf::from("evil.txt")));
        assert_eq!(sanitize("a/b/c.txt"), Some(PathBuf::from("a/b/c.txt")));
        assert_eq!(sanitize("../"), None);
    }
}
