//! 宏包勾选 + 导言区生成（旧版 `latex_builder.PACKAGES` / `latex_packages.py` 的 Rust 版）。
//!
//! 用户 2026-11-14 的待办 J2-11：公式页要能**勾选宏包**（自动写进导言区，hyperref 放最后）。
//! 旧版把 19 个宏包做成一个勾选弹窗，选中项存进设置（`latex_packages`），导出时由
//! `preamble()` 拼成 `\usepackage…`；这里逐条照搬，连 `geometry` 的选项和 hyperref 的位置都一样。
//!
//! 兼容旧值：设置里以前存的是「逗号/空格分隔的宏包名」，而宏包名和这里的 id 是同一套
//! （amsmath / geometry / hyperref …），所以老值照样认；认不出的名字原样写 `\usepackage{名字}`。

use serde_json::{json, Value};

use crate::store::Store;

pub struct Package {
    pub id: &'static str,
    pub name: &'static str,
    pub line: &'static str,
    pub default: bool,
}

/// 与旧版 `latex_builder.PACKAGES` 一一对应（顺序也一样）
pub const PACKAGES: &[Package] = &[
    Package {
        id: "amsmath",
        name: "amsmath · 数学公式排版",
        line: r"\usepackage{amsmath}",
        default: true,
    },
    Package {
        id: "amssymb",
        name: "amssymb · 数学符号（ℝ ℕ ⇒ 等）",
        line: r"\usepackage{amssymb}",
        default: true,
    },
    Package {
        id: "mathtools",
        name: "mathtools · amsmath 增强",
        line: r"\usepackage{mathtools}",
        default: false,
    },
    Package {
        id: "bm",
        name: r"bm · 数学粗体 \bm{}",
        line: r"\usepackage{bm}",
        default: true,
    },
    Package {
        id: "graphicx",
        name: "graphicx · 插图",
        line: r"\usepackage{graphicx}",
        default: true,
    },
    Package {
        id: "booktabs",
        name: "booktabs · 三线表",
        line: r"\usepackage{booktabs}",
        default: true,
    },
    Package {
        id: "array",
        name: "array · 表格列格式",
        line: r"\usepackage{array}",
        default: false,
    },
    Package {
        id: "enumitem",
        name: "enumitem · 列表间距",
        line: r"\usepackage{enumitem}",
        default: true,
    },
    Package {
        id: "geometry",
        name: "geometry · 页面尺寸",
        line: r"\usepackage[a4paper,margin=2.4cm]{geometry}",
        default: true,
    },
    Package {
        id: "xcolor",
        name: "xcolor · 颜色",
        line: r"\usepackage{xcolor}",
        default: true,
    },
    Package {
        id: "hyperref",
        name: "hyperref · 超链接（最后加载）",
        line: r"\usepackage{hyperref}",
        default: false,
    },
    Package {
        id: "cancel",
        name: r"cancel · 划线约分 \cancel{}",
        line: r"\usepackage{cancel}",
        default: false,
    },
    Package {
        id: "siunitx",
        name: "siunitx · 单位与数值",
        line: r"\usepackage{siunitx}",
        default: false,
    },
    Package {
        id: "tcolorbox",
        name: "tcolorbox · 彩色框",
        line: r"\usepackage{tcolorbox}",
        default: false,
    },
    Package {
        id: "multicol",
        name: "multicol · 分栏",
        line: r"\usepackage{multicol}",
        default: false,
    },
    Package {
        id: "longtable",
        name: "longtable · 跨页表格",
        line: r"\usepackage{longtable}",
        default: false,
    },
];

pub fn default_ids() -> Vec<String> {
    PACKAGES
        .iter()
        .filter(|one| one.default)
        .map(|one| one.id.to_string())
        .collect()
}

/// 设置里那一串 → id 列表（逗号 / 中文逗号 / 分号 / 空格 / 换行都算分隔符，去重保序）
pub fn parse_ids(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for one in text.split([',', '，', ';', '；', ' ', '\t', '\n', '\r']) {
        let name = one.trim();
        if name.is_empty() {
            continue;
        }
        if out.iter().any(|seen| seen == name) {
            continue;
        }
        out.push(name.to_string());
    }
    out
}

/// 一个 id（或旧值里的裸宏包名）→ 导言区那一行
fn line_of(name: &str) -> String {
    match PACKAGES.iter().find(|one| one.id == name) {
        Some(item) => item.line.to_string(),
        None => format!(r"\usepackage{{{name}}}"),
    }
}

/// 生成导言区（旧版 `latex_builder.preamble`）：hyperref 永远最后，自定义导言区接在后面
pub fn preamble_text(ids: &[String], extra: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut late: Vec<String> = Vec::new();
    for name in ids {
        let line = line_of(name);
        if name == "hyperref" {
            late.push(line);
        } else {
            lines.push(line);
        }
    }
    lines.extend(late);
    let extra = extra.trim();
    if !extra.is_empty() {
        lines.push(extra.trim_end().to_string());
    }
    lines.join("\n")
}

/// 整份 `.tex`（旧版 `latex_builder.build_document`）
pub fn build_document(body: &str, ids: &[String], extra: &str, class: &str, options: &str) -> String {
    let preamble = preamble_text(ids, extra);
    let class = if class.trim().is_empty() { "ctexart" } else { class.trim() };
    let options = if options.trim().is_empty() { "a4paper,12pt" } else { options.trim() };
    format!(
        "% 由 DanLume 的公式编辑器生成\n\\documentclass[{options}]{{{class}}}\n{preamble}\n\
         \\setlength{{\\parindent}}{{0pt}}\n\\setlength{{\\parskip}}{{0.62em}}\n\
         \n\\begin{{document}}\n\n{body}\n\n\\end{{document}}\n"
    )
}

pub const KEY: &str = "latex_packages";

/// 当前选择：没存过（或存了空串）就用默认那 8 个
pub fn selected_ids(store: &Store) -> Vec<String> {
    let text = store.setting(KEY).unwrap_or_default();
    let ids = parse_ids(&text);
    if ids.is_empty() {
        default_ids()
    } else {
        ids
    }
}

/// 给前端：宏包清单 + 当前勾选 + 现在的导言区长什么样
pub fn state(store: &Store) -> Value {
    let selected = selected_ids(store);
    let items: Vec<Value> = PACKAGES
        .iter()
        .map(|one| {
            json!({
                "id": one.id,
                "name": one.name,
                "line": one.line,
                "default": one.default,
                "checked": selected.iter().any(|name| name == one.id),
            })
        })
        .collect();
    json!({
        "ok": true,
        "items": items,
        "selected": selected,
        "defaults": default_ids(),
        "preamble": preamble_text(&selected, &store.setting("latex_preamble").unwrap_or_default()),
    })
}

pub fn set_selected(store: &Store, ids: &[String]) -> Value {
    let text = ids.join(",");
    let _ = store.set_setting(KEY, &text);
    state(store)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_python() {
        let ids = default_ids();
        assert_eq!(
            ids,
            vec![
                "amsmath", "amssymb", "bm", "graphicx", "booktabs", "enumitem", "geometry", "xcolor",
            ]
        );
    }

    /// geometry 要带选项、hyperref 永远排最后（旧版 `preamble` 就是这么排的）
    #[test]
    fn preamble_puts_hyperref_last_and_keeps_geometry_options() {
        let ids = vec!["hyperref".to_string(), "geometry".to_string(), "amsmath".to_string()];
        let text = preamble_text(&ids, "");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], r"\usepackage[a4paper,margin=2.4cm]{geometry}");
        assert_eq!(lines[1], r"\usepackage{amsmath}");
        assert_eq!(lines[2], r"\usepackage{hyperref}");
    }

    /// 自定义导言区接在后面；认不出的名字原样写 \usepackage{}
    #[test]
    fn unknown_names_and_extra_preamble() {
        let ids = vec!["amsmath".to_string(), "mystyle".to_string()];
        let text = preamble_text(&ids, "\\newcommand{\\R}{\\mathbb{R}}\n");
        assert!(text.contains(r"\usepackage{mystyle}"), "{text}");
        assert!(text.ends_with(r"\newcommand{\R}{\mathbb{R}}"), "{text}");
    }

    #[test]
    fn parse_ids_dedups_and_splits() {
        assert_eq!(
            parse_ids("amsmath, amssymb，geometry;;amsmath"),
            vec!["amsmath", "amssymb", "geometry"]
        );
        assert!(parse_ids("   ").is_empty());
    }

    #[test]
    fn document_has_class_and_spacing() {
        let ids = default_ids();
        let text = build_document("正文", &ids, "", "ctexart", "a4paper,12pt");
        assert!(text.starts_with("% 由 DanLume"), "{text}");
        assert!(text.contains(r"\documentclass[a4paper,12pt]{ctexart}"), "{text}");
        assert!(text.contains(r"\setlength{\parindent}{0pt}"), "{text}");
        assert!(text.contains("\\begin{document}\n\n正文"), "{text}");
        assert!(text.trim_end().ends_with(r"\end{document}"), "{text}");
    }
}
