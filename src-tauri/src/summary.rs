//! 文献自动概述（对应旧版 `lit_auto_summary` 那条链路）。
//!
//! 语义：开关 `lit_auto_summary=1` 时，把「还没有中文概述」的文献逐篇丢给当前 AI 配置，
//! 生成 80–120 字的中文概述写回 `papers.summary_zh`。失败不写库、只计入 failed，
//! 下次再来还能重试（幂等，不会覆盖已有概述）。
use serde_json::{json, Value};

use crate::store::Store;

/// 批量生成概述；`limit` 是本次最多处理几篇（默认 5，避免一次打爆配额）
pub fn auto_summarize(store: &Store, limit: usize) -> Value {
    let enabled = store
        .setting("lit_auto_summary")
        .map(|text| text.trim() == "1")
        .unwrap_or(false);
    if !enabled {
        return json!({
            "ok": false,
            "error": "「文献自动概述」没打开：设置 → 文献库 → 自动概述 打勾（lit_auto_summary=1）",
        });
    }
    // 取还没有概述的文献
    let todo: Vec<(i64, String, String)> = {
        let conn = store.connection();
        let mut out = Vec::new();
        if let Ok(mut stmt) = conn.prepare(
            "SELECT id, title, abstract FROM papers
             WHERE (summary_zh IS NULL OR TRIM(summary_zh) = '')
               AND TRIM(title) <> ''
             ORDER BY id LIMIT ?1",
        ) {
            if let Ok(mapped) = stmt.query_map([limit.clamp(1, 20) as i64], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1).unwrap_or_default(),
                    row.get::<_, String>(2).unwrap_or_default(),
                ))
            }) {
                for one in mapped.flatten() {
                    out.push(one);
                }
            }
        }
        out
    };
    if todo.is_empty() {
        return json!({ "ok": true, "done": 0, "failed": 0, "message": "没有需要概述的文献" });
    }
    let mut done = 0usize;
    let mut failed: Vec<String> = Vec::new();
    for (id, title, abstract_text) in &todo {
        let prompt = format!(
            "请用中文写一段 80–120 字的概述，讲清这篇论文做了什么、用了什么方法、主要结论。\
             只输出概述正文，不要标题、不要项目符号。\n\n标题：{title}\n摘要：{}",
            abstract_text.chars().take(1200).collect::<String>()
        );
        let messages = vec![json!({ "role": "user", "content": prompt })];
        match crate::ai::chat(store, &messages, Some(0.3)) {
            Ok(text) if !text.trim().is_empty() => {
                let summary: String = text.trim().chars().take(400).collect();
                let conn = store.connection();
                let changed = conn
                    .execute(
                        "UPDATE papers SET summary_zh = ?1 WHERE id = ?2",
                        rusqlite::params![summary, id],
                    )
                    .unwrap_or(0);
                if changed > 0 {
                    done += 1;
                } else {
                    failed.push(format!("{title}（写库失败）"));
                }
            }
            Ok(_) => failed.push(format!("{title}（模型没返回内容）")),
            Err(exc) => failed.push(format!("{title}（{exc}）")),
        }
    }
    json!({
        "ok": true,
        "done": done,
        "failed": failed.len(),
        "errors": failed,
        "total_candidates": todo.len(),
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
    fn refuses_when_switch_off() {
        let store = temp_store("danlume-litsum-test");
        let result = auto_summarize(&store, 3);
        println!("关闭时：{result}");
        assert_eq!(result["ok"], false);
        assert!(result["error"].as_str().unwrap_or("").contains("lit_auto_summary"));
        let _ = store.set_setting("lit_auto_summary", "1");
        let result = auto_summarize(&store, 3);
        println!("打开但没有文献：{result}");
        assert_eq!(result["ok"], true);
        assert_eq!(result["done"], 0);
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("danlume-litsum-test"));
    }
}
