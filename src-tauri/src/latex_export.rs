//! 公式页：导出 PDF（调本机 XeLaTeX，与旧版 `latex_compile.py` 同一条路）。
use std::path::PathBuf;
use std::process::Command;

/// 生成完整的 .tex（导言区与旧版一致：ctexart + amsmath + 无段首缩进）
pub fn wrap_document(body: &str) -> String {
    wrap_document_with("", "", body)
}

/// 带自定义导言区：`latex_packages`（逗号/空格分隔的宏包）+ `latex_preamble`（原样插入）
pub fn wrap_document_with(packages: &str, preamble: &str, body: &str) -> String {
    let extra: String = packages
        .split([',', '，', ';', ' ', '\n'])
        .map(|one| one.trim())
        .filter(|one| !one.is_empty())
        .map(|one| format!("\\usepackage{{{one}}}\n"))
        .collect();
    let preamble = preamble.trim();
    let preamble_block = if preamble.is_empty() {
        String::new()
    } else {
        format!("{preamble}\n")
    };
    format!(
        r#"\documentclass[11pt,a4paper]{{ctexart}}
\usepackage{{amsmath,amssymb,bm}}
\usepackage{{xcolor}}
\usepackage[margin=2.4cm]{{geometry}}
\usepackage{{enumitem}}
{extra}{preamble_block}\setlength{{\parindent}}{{0pt}}
\setlength{{\parskip}}{{0.62em}}
\begin{{document}}
{body}
\end{{document}}
"#
    )
}

fn build_dir() -> PathBuf {
    let dir = crate::paths::data_dir().join("latex_build");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn xelatex_path() -> String {
    // 允许用设置里的自定义路径
    "xelatex".to_string()
}

/// 编译一份 LaTeX 成 PDF（读设置：`latex_packages` / `latex_preamble` / `latex_dir`）
pub fn export_pdf(store: &crate::store::Store, body: &str, stem: &str) -> Result<String, String> {
    if body.trim().is_empty() {
        return Err("还没有内容可导出".to_string());
    }
    let packages = store.setting("latex_packages").unwrap_or_default();
    let preamble = store.setting("latex_preamble").unwrap_or_default();
    // latex_dir 优先，其次旧版的 dir_latex_build，最后才是 data/latex_build
    let wanted = store
        .setting("latex_dir")
        .filter(|text| !text.trim().is_empty())
        .or_else(|| store.setting("dir_latex_build").filter(|text| !text.trim().is_empty()));
    let dir = match wanted {
        Some(text) => {
            let path = PathBuf::from(text.trim());
            let _ = std::fs::create_dir_all(&path);
            if path.is_dir() {
                path
            } else {
                build_dir()
            }
        }
        _ => build_dir(),
    };
    let safe: String = stem
        .chars()
        .map(|ch| if ch.is_alphanumeric() || ch == '-' || ch == '_' { ch } else { '-' })
        .collect();
    let name = if safe.trim().is_empty() {
        "danlume".to_string()
    } else {
        safe
    };
    let tex_path = dir.join(format!("{name}.tex"));
    std::fs::write(&tex_path, wrap_document_with(&packages, &preamble, body))
        .map_err(|err| format!("写 .tex 失败：{err}"))?;
    // 记下这次生成的 .tex，设置里的 `latex_generated` 就是旧版这个用途
    let _ = store.set_setting("latex_generated", &tex_path.display().to_string());

    let mut last_output = String::new();
    for round in 1..=2 {
        let output = Command::new(xelatex_path())
            .current_dir(&dir)
            .args([
                "-interaction=nonstopmode",
                "-halt-on-error",
                "-file-line-error",
                &tex_path.file_name().unwrap_or_default().to_string_lossy(),
            ])
            .output();
        match output {
            Ok(result) => {
                let text = String::from_utf8_lossy(&result.stdout).to_string();
                last_output = text.clone();
                if !result.status.success() {
                    return Err(format!(
                        "XeLaTeX 编译失败（第 {round} 遍）：{}",
                        tail(&text)
                    ));
                }
            }
            Err(err) => {
                return Err(format!(
                    "找不到 xelatex（{err}）：公式导出需要本机装 MiKTeX 或 TeX Live"
                ))
            }
        }
    }
    let pdf = dir.join(format!("{name}.pdf"));
    if pdf.is_file() {
        let _ = last_output;
        Ok(pdf.display().to_string())
    } else {
        Err(format!("编译完成但没有生成 PDF：{}", tail(&last_output)))
    }
}

/// 取日志最后几行有用的信息
fn tail(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let start = lines.len().saturating_sub(4);
    lines[start..].join(" / ").chars().take(400).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_has_preamble() {
        let text = wrap_document("$x^2$");
        assert!(text.contains("\\documentclass"));
        assert!(text.contains("\\begin{document}"));
        assert!(text.contains("\\parindent}{0pt"));
    }

    #[test]
    fn real_xelatex_compile() {
        std::env::set_var(
            "LUMINA_DATA_DIR",
            r"E:\FIles\Lena‘s Files\Work\znw\Agent\data",
        );
        let body = "\\section*{编译测试}\n行内 $a^2+b^2=c^2$，行间：\n$$\\int_0^1 x^2\\,dx=\\frac{1}{3}$$";
        let store = crate::store::Store::open().expect("打不开数据库");
        match export_pdf(&store, body, "danlume-test") {
            Ok(path) => println!("生成 PDF：{}", path),
            Err(err) => panic!("编译失败：{}", err),
        }
    }
}
