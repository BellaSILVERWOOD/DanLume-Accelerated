//! AI 记忆库（对应旧版 `lumina/memory.py`）。
//!
//! 旧版是 `<data>/memory.md` 一份纯文本，`- 内容（2026-01-01 10:00）` 一行一条，
//! 每次对话把整个文件（截断到 4000 字）拼进系统提示词；模型想记东西时在回答最后
//! 输出一行 `[[记住]] 内容`，程序把它摘出来写进文件、并从回答里删掉。
//!
//! 这一版语义一样，只是数据放进 `lumina.db` 的 `memories` 表（字段比旧版多：
//! 来源 / 标签 / 命中次数 / 最近使用时间），注入位置、格式、4000 字上限都照抄。
//!
//! 对外：
//!     add / list / search / update / delete / clear     增删改查
//!     context_block        -> (拼好的系统提示词片段, 用了几条)，顺手记命中
//!     build_memory_block   -> 纯函数：只要条目，产出那段文字（单测用）
//!     extract / absorb     -> `[[记住]]` 的摘取与写库
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

use crate::store::Store;

/// 带进系统提示词的上限（旧版 `memory.MAX_CHARS = 4000`）
pub const MAX_CHARS: usize = 4000;
/// 一次最多带多少条（旧版是全带，这里加个保险，免得库太大把提示词撑爆）
pub const MAX_ITEMS: usize = 50;
/// 模型输出这个标记表示「这句话要记住」
pub const MARK: &str = "[[记住]]";
/// 自动记录的开关（旧版没有开关，永远开；这一版留个键，默认开）
pub const AUTO_REMEMBER_KEY: &str = "memory_auto_remember";
/// 单条记忆的长度上限
pub const MAX_ENTRY_CHARS: usize = 2000;
/// 记忆库条数上限（超过就淘汰；可以在设置里用 `memory_max_entries` 覆盖）
pub const DEFAULT_MAX_ENTRIES: i64 = 200;
/// 条数上限的设置键
pub const MAX_ENTRIES_KEY: &str = "memory_max_entries";

/// 一条记忆（字段名给前端直接用）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRow {
    pub id: i64,
    pub content: String,
    /// 从哪儿来的：`手动` / `AI 自动写入` / 外部导入
    pub source: String,
    /// 逗号分隔的标签
    pub tags: String,
    pub created_at: String,
    pub updated_at: String,
    /// 被带进对话几次
    pub hits: i64,
    /// 最近一次被带进对话的时间
    pub last_used_at: Option<String>,
}

impl MemoryRow {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            content: row.get::<_, String>(1).unwrap_or_default(),
            source: row.get::<_, String>(2).unwrap_or_default(),
            tags: row.get::<_, String>(3).unwrap_or_default(),
            created_at: row.get::<_, String>(4).unwrap_or_default(),
            updated_at: row.get::<_, String>(5).unwrap_or_default(),
            hits: row.get::<_, i64>(6).unwrap_or(0),
            last_used_at: row.get::<_, Option<String>>(7).unwrap_or_default(),
        })
    }
}

const SELECT_COLS: &str =
    "SELECT id, content, source, tags, created_at, updated_at, hits, last_used_at FROM memories";

fn query(conn: &Connection, sql: &str, args: impl rusqlite::Params) -> Vec<MemoryRow> {
    let Ok(mut stmt) = conn.prepare(sql) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map(args, MemoryRow::from_row) else {
        return Vec::new();
    };
    rows.flatten().collect()
}

/// 列表：最近改过的排前面
pub fn list(store: &Store, limit: usize) -> Vec<MemoryRow> {
    let conn = store.connection();
    let sql = format!("{SELECT_COLS} ORDER BY updated_at DESC, id DESC LIMIT ?1");
    query(&conn, &sql, [limit.clamp(1, 500) as i64])
}

/// 搜内容 / 标签 / 来源（`%关键词%`，大小写按 SQLite 的 LIKE 来）
pub fn search(store: &Store, keyword: &str) -> Vec<MemoryRow> {
    let text = keyword.trim();
    if text.is_empty() {
        return list(store, 200);
    }
    let pattern = format!("%{}%", escape_like(text));
    let conn = store.connection();
    let sql = format!(
        "{SELECT_COLS} WHERE content LIKE ?1 ESCAPE '\\'
            OR tags LIKE ?1 ESCAPE '\\' OR source LIKE ?1 ESCAPE '\\'
         ORDER BY updated_at DESC, id DESC LIMIT 200"
    );
    query(&conn, &sql, params![pattern])
}

fn escape_like(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 4);
    for ch in text.chars() {
        if ch == '%' || ch == '_' || ch == '\\' {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn find_by_content(conn: &Connection, content: &str) -> Option<i64> {
    let mut stmt = conn
        .prepare("SELECT id FROM memories WHERE content = ?1 ORDER BY id LIMIT 1")
        .ok()?;
    let mut rows = stmt.query(params![content]).ok()?;
    let row = rows.next().ok()??;
    row.get::<_, i64>(0).ok()
}

/// 写一条记忆。
///
/// **与旧版有意不同**：旧版 `memory.append()` 是无脑追加，同一句话记两次就有两行；
/// 这一版按内容去重（`content` 完全相同就当成同一条，刷新时间 / 补上来源与标签），
/// 返回 `(id, 是不是命中了已有条目)`。
pub fn add(store: &Store, content: &str, source: &str, tags: &str) -> Result<(i64, bool), String> {
    let text = content.trim();
    if text.is_empty() {
        return Err("记忆内容不能为空".to_string());
    }
    if text.chars().count() > MAX_ENTRY_CHARS {
        return Err(format!("一条记忆最多 {MAX_ENTRY_CHARS} 字，这条太长了"));
    }
    let now = crate::now_str();
    let source = source.trim();
    let tags = tags.trim();
    // 先读上限：`max_entries` 要拿连接锁，必须在下面 `store.connection()` 之前算好，
    // 否则同一个线程重复加锁会把测试卡死。
    let limit = max_entries(store);
    let conn = store.connection();
    if let Some(id) = find_by_content(&conn, text) {
        conn.execute(
            "UPDATE memories SET
               source = CASE WHEN ?2 <> '' THEN ?2 ELSE source END,
               tags = CASE WHEN ?3 <> '' THEN ?3 ELSE tags END,
               updated_at = ?4
             WHERE id = ?1",
            params![id, source, tags, now],
        )
        .map_err(|err| err.to_string())?;
        return Ok((id, true));
    }
    conn.execute(
        "INSERT INTO memories(content, source, tags, created_at, updated_at, hits)
         VALUES (?1, ?2, ?3, ?4, ?4, 0)",
        params![text, source, tags, now],
    )
    .map_err(|err| err.to_string())?;
    let id = conn.last_insert_rowid();
    // 记住之后再按上限淘汰一批：记忆只增不减，久了会把提示词吃满
    let _ = prune_with(&conn, limit);
    Ok((id, false))
}

/// 当前允许保留多少条（设置里没写 / 写得不对就用默认值）
pub fn max_entries(store: &Store) -> i64 {
    let raw = store.setting(MAX_ENTRIES_KEY).unwrap_or_default();
    match raw.trim().parse::<i64>() {
        Ok(value) if value >= 1 => value,
        _ => DEFAULT_MAX_ENTRIES,
    }
}

/// 按「命中次数 → 最近使用时间 → 更新时间」淘汰，留下最常用的那些。
///
/// 淘汰顺序（先被删的在前）：
/// 1. 命中次数少的；
/// 2. 从没被用过的优先；
/// 3. 同分时越老的越先删。
pub fn prune_with(conn: &Connection, limit: i64) -> rusqlite::Result<usize> {
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))?;
    let keep = limit.max(1);
    if total <= keep {
        return Ok(0);
    }
    conn.execute(
        "DELETE FROM memories WHERE id IN (
            SELECT id FROM memories
            ORDER BY hits ASC,
                     (last_used_at IS NULL) DESC,
                     COALESCE(last_used_at, updated_at) ASC,
                     id ASC
            LIMIT ?1
        )",
        params![total - keep],
    )
}

/// 对外：按设置里的上限淘汰一次，返回删掉的条数。
pub fn prune(store: &Store) -> usize {
    let limit = max_entries(store);   // 先读设置（要锁），再拿连接
    let conn = store.connection();
    prune_with(&conn, limit).unwrap_or(0)
}

/// 改内容 / 来源 / 标签（只改传了的；内容不能改成空）
pub fn update(
    store: &Store,
    id: i64,
    content: Option<&str>,
    source: Option<&str>,
    tags: Option<&str>,
) -> Result<bool, String> {
    let text = match content {
        Some(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Err("记忆内容不能改成空的".to_string());
            }
            if trimmed.chars().count() > MAX_ENTRY_CHARS {
                return Err(format!("一条记忆最多 {MAX_ENTRY_CHARS} 字，这条太长了"));
            }
            Some(trimmed.to_string())
        }
        None => None,
    };
    let now = crate::now_str();
    let conn = store.connection();
    let changed = conn
        .execute(
            "UPDATE memories SET
               content = COALESCE(?2, content),
               source = COALESCE(?3, source),
               tags = COALESCE(?4, tags),
               updated_at = ?5
             WHERE id = ?1",
            params![
                id,
                text,
                source.map(|one| one.trim().to_string()),
                tags.map(|one| one.trim().to_string()),
                now
            ],
        )
        .map_err(|err| err.to_string())?;
    Ok(changed > 0)
}

pub fn delete(store: &Store, id: i64) -> Result<bool, String> {
    let conn = store.connection();
    let changed = conn
        .execute("DELETE FROM memories WHERE id = ?1", params![id])
        .map_err(|err| err.to_string())?;
    Ok(changed > 0)
}

/// 清空，返回删掉几条
pub fn clear(store: &Store) -> Result<i64, String> {
    let conn = store.connection();
    let changed = conn
        .execute("DELETE FROM memories", [])
        .map_err(|err| err.to_string())?;
    Ok(changed as i64)
}

/// 直接数一下有多少条（给状态用）
pub fn count(store: &Store) -> i64 {
    let conn = store.connection();
    conn.query_row("SELECT COUNT(*) FROM memories", [], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
}

/// 纯函数：把条目拼成系统提示词里的那一段。
///
/// 格式与旧版 `memory.context_block()` 逐字一致（中文字符串照抄），
/// 超长时按字符截断并加一句「…（记忆太长已截断）」。
pub fn build_memory_block(items: &[String], limit_chars: usize) -> String {
    let entries: Vec<&str> = items
        .iter()
        .map(|one| one.trim())
        .filter(|one| !one.is_empty())
        .collect();
    if entries.is_empty() {
        return String::new();
    }
    let mut body = entries
        .iter()
        .map(|one| format!("- {one}"))
        .collect::<Vec<_>>()
        .join("\n");
    if body.chars().count() > limit_chars {
        body = body.chars().take(limit_chars).collect::<String>();
        body.push_str("\n…（记忆太长已截断）");
    }
    format!(
        "下面是我让你记住的长期记忆，回答时请参考：\n{body}\n\n\
         如果我这次说「记住……」，请在回答最后单独一行输出：\n\
         {MARK} 要记住的内容\n（这一行只会被程序收进记忆，不会显示给我看。）"
    )
}

/// 拼好这次对话要带上的记忆（顺手把「命中次数 / 最近使用时间」记上）
///
/// 返回 `(那段文字, 用了几条)`；一条记忆都没有就返回空串。
pub fn context_block(store: &Store, limit_chars: usize) -> (String, usize) {
    let rows: Vec<(i64, String)> = {
        let conn = store.connection();
        let mut out = Vec::new();
        let Ok(mut stmt) = conn.prepare(
            "SELECT id, content FROM memories ORDER BY updated_at DESC, id DESC LIMIT ?1",
        ) else {
            return (String::new(), 0);
        };
        match stmt.query_map([MAX_ITEMS as i64], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1).unwrap_or_default(),
            ))
        }) {
            Ok(rows) => {
                for row in rows.flatten() {
                    out.push(row);
                }
            }
            Err(_) => return (String::new(), 0),
        }
        out
    };
    if rows.is_empty() {
        return (String::new(), 0);
    }
    let contents: Vec<String> = rows.iter().map(|(_, text)| text.clone()).collect();
    let block = build_memory_block(&contents, limit_chars);
    if block.is_empty() {
        return (String::new(), 0);
    }
    let ids: Vec<i64> = rows.iter().map(|(id, _)| *id).collect();
    mark_used(store, &ids);
    (block, contents.len())
}

/// 给 `ai.rs` 用：`Some((提示词片段, 条数))` 或者 `None`（没记忆）
pub fn injected(store: &Store) -> Option<(String, usize)> {
    let (block, used) = context_block(store, MAX_CHARS);
    if block.is_empty() {
        None
    } else {
        Some((block, used))
    }
}

fn mark_used(store: &Store, ids: &[i64]) {
    if ids.is_empty() {
        return;
    }
    let now = crate::now_str();
    let conn = store.connection();
    for id in ids {
        let _ = conn.execute(
            "UPDATE memories SET hits = hits + 1, last_used_at = ?2 WHERE id = ?1",
            params![id, now],
        );
    }
}

/// 把回答里 `[[记住]] …` 的行摘出来（旧版 `memory.extract()` 的正则逐行版，
/// 允许行首有 `-` / `*` 列表符号）。
pub fn extract(answer: &str) -> (String, Vec<String>) {
    let mut remembered: Vec<String> = Vec::new();
    let mut kept: Vec<String> = Vec::new();
    for line in answer.lines() {
        let mut rest = line.trim_start();
        if let Some(tail) = rest.strip_prefix('-').or_else(|| rest.strip_prefix('*')) {
            rest = tail.trim_start();
        }
        if let Some(tail) = rest.strip_prefix(MARK) {
            let item = tail.trim();
            if !item.is_empty() {
                remembered.push(item.to_string());
                continue;
            }
        }
        kept.push(line.to_string());
    }
    (kept.join("\n").trim().to_string(), remembered)
}

/// 模型回答收尾：摘掉 `[[记住]]` 行、把内容写进记忆库，返回（干净的回答, 写了几条）
///
/// `memory_auto_remember=0` 时仍然会把标记行从回答里删掉（不显示给用户），只是不写库。
pub fn absorb(store: &Store, answer: &str) -> (String, usize) {
    let (clean, items) = extract(answer);
    if items.is_empty() {
        return (clean, 0);
    }
    let enabled = store
        .setting(AUTO_REMEMBER_KEY)
        .map(|value| value.trim() != "0")
        .unwrap_or(true);
    if !enabled {
        return (clean, 0);
    }
    let mut saved = 0usize;
    for item in &items {
        if add(store, item, "AI 自动写入", "").is_ok() {
            saved += 1;
        }
    }
    (clean, saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 记得在测试结束时把 `LUMINA_DATA_DIR` 恢复回去：这个环境变量是进程级的，
    /// 不恢复会影响同一进程里其它模块的测试（之前就踩过：其它用例去已删除的临时目录建库）。
    struct EnvGuard(Option<std::ffi::OsString>);

    impl EnvGuard {
        fn new() -> Self {
            EnvGuard(std::env::var_os("LUMINA_DATA_DIR"))
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.0 {
                Some(value) => std::env::set_var("LUMINA_DATA_DIR", value),
                None => std::env::remove_var("LUMINA_DATA_DIR"),
            }
        }
    }

    /// 临时目录名带上进程 id：万一同时有另一个 `cargo test` 在跑同一个 crate，
    /// 两边不会用同一个临时库（同名目录互删会把对方的行读进来）。
    fn test_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("{name}-{}", std::process::id()))
    }

    fn temp_store(name: &str) -> (Store, EnvGuard) {
        let guard = EnvGuard::new();
        let dir = test_dir(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("LUMINA_DATA_DIR", &dir);
        let store = Store::open().expect("建临时库失败");
        (store, guard)
    }

    fn cleanup(name: &str) {
        let _ = std::fs::remove_dir_all(test_dir(name));
    }

    /// E4：超过上限时按「命中次数少 / 更久没用」先淘汰，常用的留在库里。
    #[test]
    fn prunes_by_hits_then_age() {
        let (store, _guard) = temp_store("danlume-memory-prune");
        store.set_setting(MAX_ENTRIES_KEY, "3").expect("写不了设置");
        for i in 0..5 {
            add(&store, &format!("记忆 {i}"), "test", "").expect("写记忆失败");
        }
        assert_eq!(count(&store), 3, "超过上限应当淘汰到 3 条");
        let texts: Vec<String> = list(&store, 10).iter().map(|row| row.content.clone()).collect();
        assert!(texts.iter().any(|text| text == "记忆 4"), "最新的一条必须在：{texts:?}");
        assert!(!texts.iter().any(|text| text == "记忆 0"), "最老的应该被淘汰：{texts:?}");

        // 常用的那条即使更老也不能被淘汰
        let (hot_id, _) = add(&store, "常用记忆", "test", "").expect("写记忆失败");
        let conn = store.connection();
        conn.execute("UPDATE memories SET hits = 99 WHERE id = ?1", params![hot_id])
            .expect("改命中次数失败");
        drop(conn);
        for i in 0..5 {
            add(&store, &format!("新记忆 {i}"), "test", "").expect("写记忆失败");
        }
        let texts: Vec<String> = list(&store, 20).iter().map(|row| row.content.clone()).collect();
        assert!(texts.iter().any(|text| text == "常用记忆"), "命中次数多的不该被淘汰：{texts:?}");
        assert_eq!(texts.len(), 3, "上限仍然是 3 条：{texts:?}");
        cleanup("danlume-memory-prune");
    }

    /// 新增 → 列表 → 搜索 → 更新 → 删除 → 清空（一条链走完）
    #[test]
    fn crud_flow() {        let (store, _guard) = temp_store("danlume-memory-test");
        assert!(list(&store, 100).is_empty(), "新库应该是空的");

        let (id, dup) = add(&store, "  我周三下午有课  ", "手动", "课表").unwrap();
        assert!(!dup, "第一次写入不算重复");
        let rows = list(&store, 100);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].content, "我周三下午有课", "两头空格要被裁掉");
        assert_eq!(rows[0].tags, "课表");
        assert_eq!(rows[0].hits, 0);
        assert!(!rows[0].created_at.is_empty());

        // 同一段内容再写一次：去重（命中原条目），不新增行
        let (again, duplicated) = add(&store, "我周三下午有课", "AI 自动写入", "").unwrap();
        assert_eq!(again, id, "命中的是同一条");
        assert!(duplicated);
        assert_eq!(list(&store, 100).len(), 1, "去重后还是 1 条");
        let row = list(&store, 100).remove(0);
        assert_eq!(row.source, "AI 自动写入", "来源被补上");
        assert_eq!(row.tags, "课表", "标签没传就保留原来的");

        add(&store, "我不吃香菜", "手动", "饮食").unwrap();
        add(&store, "每周五交周报", "手动", "工作,周报").unwrap();
        assert_eq!(list(&store, 100).len(), 3);

        // 搜索：内容命中
        let hit = search(&store, "香菜");
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].content, "我不吃香菜");
        // 搜索：标签命中
        assert_eq!(search(&store, "周报").len(), 1);
        // 搜索：搜不到就是空
        assert!(search(&store, "不存在的东西").is_empty());
        // 搜索里的 % / _ 当普通字符，而不是通配符
        assert!(search(&store, "%").is_empty());
        // 空关键词 = 全list
        assert_eq!(search(&store, "   ").len(), 3);

        // 更新
        assert!(update(&store, id, Some("我周三下午三点有课"), None, Some("课表,重要")).unwrap());
        let row = list(&store, 100).into_iter().find(|one| one.id == id).unwrap();
        assert_eq!(row.content, "我周三下午三点有课");
        assert_eq!(row.tags, "课表,重要");
        assert_eq!(row.source, "AI 自动写入", "没动来源");
        // 改成空的要被拒
        assert!(update(&store, id, Some("   "), None, None).is_err());
        // 更新不存在的 id 返回 false
        assert!(!update(&store, 99999, Some("随便"), None, None).unwrap());

        // 空内容不许写
        assert!(add(&store, "   ", "", "").is_err());

        // 删除
        assert!(delete(&store, id).unwrap());
        assert!(!delete(&store, id).unwrap(), "删过的再删就是 false");
        assert_eq!(list(&store, 100).len(), 2);

        // 清空
        assert_eq!(clear(&store).unwrap(), 2);
        assert!(list(&store, 100).is_empty());
        // 清空之后还能继续用
        assert_eq!(clear(&store).unwrap(), 0);
        add(&store, "清空后的第一条", "手动", "").unwrap();
        assert_eq!(count(&store), 1);
        cleanup("danlume-memory-test");
    }

    /// 空库上 list / search / context_block 都不能炸
    #[test]
    fn empty_library_is_safe() {
        let (store, _guard) = temp_store("danlume-memory-empty-test");
        assert!(list(&store, 100).is_empty());
        assert!(search(&store, "任何词").is_empty());
        let (block, used) = context_block(&store, MAX_CHARS);
        assert!(block.is_empty(), "没记忆就别往提示词里塞东西");
        assert_eq!(used, 0);
        assert!(injected(&store).is_none());
        assert_eq!(count(&store), 0);
        assert!(delete(&store, 1).unwrap() == false);
        cleanup("danlume-memory-empty-test");
    }

    /// 注入：格式与旧版一致，超长按字符截断，命中次数会涨
    #[test]
    fn injects_in_order_and_counts_hits() {
        let (store, _guard) = temp_store("danlume-memory-inject-test");
        add(&store, "暗号是芝麻开门", "手动", "").unwrap();
        add(&store, "周五晚上要交周报", "手动", "").unwrap();
        let (block, used) = context_block(&store, MAX_CHARS);
        assert_eq!(used, 2);
        assert!(block.starts_with("下面是我让你记住的长期记忆，回答时请参考：\n- "));
        assert!(block.contains("- 暗号是芝麻开门"));
        assert!(block.contains("\n如果我这次说「记住……」，请在回答最后单独一行输出：\n"));
        assert!(block.contains("[[记住]] 要记住的内容"));
        let rows = list(&store, 10);
        assert!(rows.iter().all(|one| one.hits == 1), "带进过一次对话");
        assert!(rows.iter().all(|one| one.last_used_at.is_some()));
        // 再注入一次，命中次数继续涨
        let _ = context_block(&store, MAX_CHARS);
        let rows = list(&store, 10);
        assert!(rows.iter().all(|one| one.hits == 2));
        cleanup("danlume-memory-inject-test");
    }

    /// 纯函数：拼装格式与截断（对应旧版 context_block 的 4000 字上限）
    #[test]
    fn build_block_formats_and_truncates() {
        assert_eq!(build_memory_block(&[], MAX_CHARS), "");
        // 全是空白也算空
        assert_eq!(build_memory_block(&["  ".to_string()], MAX_CHARS), "");

        let block = build_memory_block(
            &["第一件事".to_string(), "第二件事".to_string()],
            MAX_CHARS,
        );
        let expected = "下面是我让你记住的长期记忆，回答时请参考：\n\
                        - 第一件事\n- 第二件事\n\n\
                        如果我这次说「记住……」，请在回答最后单独一行输出：\n\
                        [[记住]] 要记住的内容\n（这一行只会被程序收进记忆，不会显示给我看。）";
        assert_eq!(block, expected);

        // 截断：body 只保留 limit 个字符，后面补一句提示，尾巴那几句指令还在
        let long = "很长的记忆".repeat(40); // 200 字
        let cut = build_memory_block(&[long], 20);
        assert!(cut.contains("…（记忆太长已截断）"));
        assert!(cut.contains("[[记住]] 要记住的内容"), "尾部指令不能被截掉");
        let body = cut
            .split('\n')
            .nth(1)
            .expect("第二行是记忆正文")
            .to_string();
        assert_eq!(body.chars().count(), 20, "截断按字符算，正好 limit 个");
        // 刚好到上限不截断
        let exact = build_memory_block(&["a".repeat(MAX_CHARS - 2)], MAX_CHARS);
        assert!(!exact.contains("记忆太长已截断"));
    }

    /// `[[记住]]` 的摘取与自动写库
    #[test]
    fn extracts_and_absorbs_remember_lines() {
        let (clean, items) = extract("好的，记下了。\n[[记住]] 我不吃香菜\n下次注意。\n");
        assert_eq!(items, vec!["我不吃香菜".to_string()]);
        assert_eq!(clean, "好的，记下了。\n下次注意。");
        // 列表符号、前导空格、空标记
        let (_, items) = extract("  - [[记住]]  周三下午三点有课  \n* [[记住]] 周五交周报\n[[记住]]   \n");
        assert_eq!(
            items,
            vec!["周三下午三点有课".to_string(), "周五交周报".to_string()]
        );
        // 普通行原样保留
        let (clean, items) = extract("没有标记的回答。");
        assert!(items.is_empty());
        assert_eq!(clean, "没有标记的回答。");

        let (store, _guard) = temp_store("danlume-memory-absorb-test");
        let (clean, saved) = absorb(&store, "知道了。\n[[记住]] 我周三下午有课\n");
        assert_eq!(saved, 1);
        assert_eq!(clean, "知道了。");
        let rows = list(&store, 10);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].content, "我周三下午有课");
        assert_eq!(rows[0].source, "AI 自动写入");
        // 再记一次同样的内容：去重，不新增
        let (_, saved) = absorb(&store, "[[记住]] 我周三下午有课");
        assert_eq!(saved, 1, "写库成功（命中去重）");
        assert_eq!(list(&store, 10).len(), 1);
        // 关掉自动记录后：标记行照样从回答里删掉，但不写库
        store.set_setting(AUTO_REMEMBER_KEY, "0").unwrap();
        let (clean, saved) = absorb(&store, "好\n[[记住]] 新的内容\n");
        assert_eq!(saved, 0);
        assert_eq!(clean, "好");
        assert_eq!(list(&store, 10).len(), 1);
        cleanup("danlume-memory-absorb-test");
    }
}
