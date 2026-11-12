//! 数据迁移：把整个 `lumina.db` 打成一个 `.dlpk` 数据包（其实是 zip），
//! 以及从数据包导入（换电脑 / 重装系统 / Python 版与 Rust 版之间搬家）。
//!
//! 包格式与 Python 版**完全一致**（两边同时实现，别单方面改）：
//!
//! ```text
//! manifest.json   包信息（format / format_version / schema_version / tables …）
//! lumina.db       用 `VACUUM INTO` 打出来的一致性快照（不是直接复制正在写的库）
//! README.txt      给人看的一页说明
//! ```
//!
//! 密钥剥离：`settings` 里值以 `dpapi:` 开头的行（DPAPI 密文换机器解不开）导出时整行删掉，
//! 被删的 key 记进 manifest 的 `stripped_keys`，导入后由用户重新填。
//!
//! 导入**不覆盖正在使用的库**：先自动备份当前库到 `data/backups/pre-import-时间.db`，
//! 再把包里的库解成 `data/lumina.db.imported` 并写下 `pending_import` 标记；
//! 真正换库发生在下一次启动，`run()` 里任何 sqlite 连接建立之前（见 [`apply_pending`]）。

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::{json, Value};

/// manifest 里的固定字段（两版必须一致）
pub const FORMAT: &str = "danlume-backup";
pub const FORMAT_VERSION: i64 = 1;
/// 两版共用同一套表，暂时固定为 1；导入时只接受这个值
pub const SCHEMA_VERSION: i64 = 1;
/// 本实现是哪一版（Python 版写 `"python"`）
pub const APP: &str = "rust";

const MANIFEST_ENTRY: &str = "manifest.json";
const DB_ENTRY: &str = "lumina.db";
const README_ENTRY: &str = "README.txt";
/// 旧版（Python）把 AI 记忆存在数据目录的纯文本里，新版存在 memories 表。
/// 包里带上这一份，两边就都能看到对方的记忆。
const MEMORY_ENTRY: &str = "memory.md";

/// 生成的 memory.md 头部（与 Python 版 `lumina/memory.py` 的 `_HEADER` 一致）
const MEMORY_MD_HEADER: &str = "# DanLume 记忆\n\n\
> 这里的内容会随每次 AI 对话带给模型（有长度上限）。\n\
> 想让 AI 做什么/记住什么，直接跟它说「记住：……」即可；也可以手动编辑这个文件。\n";

/// 包里没有 memories 表时，按这一份建（与 `store.rs` 的 SCHEMA 一致）
const MEMORY_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS memories (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    content TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT '',
    tags TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    hits INTEGER NOT NULL DEFAULT 0,
    last_used_at TEXT
);";

/// DPAPI 密文前缀（与 `secrets.rs` 一致）
pub const SECRET_PREFIX: &str = "dpapi:";
/// 导入后等待换库的临时库名
const IMPORTED_SUFFIX: &str = "lumina.db.imported";
/// 导入标记（写在库里，重启换库后清掉）
pub const PENDING_KEY: &str = "pending_import";
pub const PENDING_FROM_KEY: &str = "pending_import_from";

// ------------------------------------------------------------------ 工具

fn stamp_for_file() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

/// 临时快照路径（放在数据包输出目录之外，避免污染用户选的目录）
fn temp_snapshot(tag: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "danlume-{tag}-{}-{unique}.db",
        std::process::id()
    ))
}

/// 每张表多少行（导入时给用户看「包里有什么」用的展示信息）
fn table_counts(conn: &Connection) -> BTreeMap<String, i64> {
    let mut out = BTreeMap::new();
    let names: Vec<String> = {
        let Ok(mut stmt) = conn.prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        ) else {
            return out;
        };
        stmt.query_map([], |row| row.get::<_, String>(0))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    };
    for name in names {
        // 表名来自 sqlite_master，双引号包一层就够（顺手把内层引号转义掉）
        let sql = format!("SELECT COUNT(*) FROM \"{}\"", name.replace('"', "\"\""));
        let count: i64 = conn.query_row(&sql, [], |row| row.get(0)).unwrap_or(0);
        out.insert(name, count);
    }
    out
}

/// 打开包里的某个条目（不存在就报中文错误）
fn read_entry(archive: &mut zip::ZipArchive<File>, name: &str) -> Result<Vec<u8>, String> {
    let mut entry = archive
        .by_name(name)
        .map_err(|err| format!("数据包里读不到 {name}：{err}"))?;
    let mut out = Vec::new();
    entry
        .read_to_end(&mut out)
        .map_err(|err| format!("读 {name} 失败：{err}"))?;
    Ok(out)
}

/// 写 `settings` 的键值（两版同一个表结构，INSERT … ON CONFLICT 比 REPLACE 稳）
fn put_setting(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )?;
    Ok(())
}

/// 密钥剥离用的 LIKE 模式（`dpapi:%`，与 Python 版同一个写法）
fn secret_pattern() -> String {
    format!("{SECRET_PREFIX}%")
}

// ------------------------------------------------------------------ 导出

/// 导出数据包。
///
/// * `db_path` —— 正在用的 `data/lumina.db`
/// * `out`    —— 用户选的 `.dlpk` 路径
///
/// 返回 `{ok, path, bytes, manifest, tables, stripped_keys, error}`。
pub fn export(db_path: &Path, out: &Path) -> Result<Value, String> {
    if !db_path.is_file() {
        return Err(format!("数据库不存在：{}", db_path.display()));
    }
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|err| format!("建不了输出目录：{err}"))?;
        }
    }

    let snapshot = temp_snapshot("export");
    let _ = std::fs::remove_file(&snapshot);

    // ① 一致性快照：VACUUM INTO 由 SQLite 自己保证（直接 fs::copy 正在写的库会拿到半个事务）
    {
        let conn = Connection::open(db_path).map_err(|err| format!("打不开数据库：{err}"))?;
        let _ = conn.busy_timeout(std::time::Duration::from_secs(10));
        let target = snapshot.display().to_string().replace('\'', "''");
        conn.execute_batch(&format!("VACUUM INTO '{target}'"))
            .map_err(|err| format!("生成一致性快照失败：{err}"))?;
    }

    let outcome = (|| -> Result<Value, String> {
        // ② 打开快照，剥掉 DPAPI 密文
        let conn =
            Connection::open(&snapshot).map_err(|err| format!("快照打不开：{err}"))?;
        let stripped_keys: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT key FROM settings WHERE value LIKE ?1 ORDER BY key")
                .map_err(|err| format!("读设置失败：{err}"))?;
            let rows = stmt
                .query_map(rusqlite::params![secret_pattern()], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(|err| format!("读设置失败：{err}"))?;
            let keys: Vec<String> = rows.flatten().collect();
            keys
        };
        let stripped_rows = conn
            .execute(
                "DELETE FROM settings WHERE value LIKE ?1",
                rusqlite::params![secret_pattern()],
            )
            .map_err(|err| format!("剥离密钥失败：{err}"))?;
        let tables = table_counts(&conn);
        let (memory_md, memory_count) = memory_file_from(&conn);
        let integrity: String = conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .map_err(|err| format!("快照自检失败：{err}"))?;
        drop(conn);
        if !integrity.eq_ignore_ascii_case("ok") {
            return Err(format!("快照自检没通过：{integrity}"));
        }

        let manifest = json!({
            "format": FORMAT,
            "format_version": FORMAT_VERSION,
            "app": APP,
            "app_version": env!("CARGO_PKG_VERSION"),
            "exported_at": chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            "schema_version": SCHEMA_VERSION,
            "tables": tables.clone(),
            "secrets_stripped": stripped_rows > 0,
            "stripped_keys": stripped_keys.clone(),
            "memory_included": memory_md.is_some(),
            "memory_entries": memory_count,
        });

        // ③ 快照 + manifest + README（+ memory.md）打进 zip
        write_package(out, &manifest, &snapshot, memory_md.as_deref())?;
        let bytes = std::fs::metadata(out).map(|meta| meta.len()).unwrap_or(0);
        Ok(json!({
            "ok": true,
            "path": out.display().to_string(),
            "bytes": bytes,
            "manifest": manifest,
            "tables": tables,
            "stripped_keys": stripped_keys,
            "memory_entries": memory_count,
            "error": "",
        }))
    })();

    let _ = std::fs::remove_file(&snapshot);
    outcome
}

fn write_package(
    out: &Path,
    manifest: &Value,
    db_file: &Path,
    memory_md: Option<&str>,
) -> Result<(), String> {
    let file = File::create(out).map_err(|err| format!("建不了数据包：{err}"))?;
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let text = serde_json::to_string_pretty(manifest)
        .map_err(|err| format!("manifest 序列化失败：{err}"))?;

    writer
        .start_file(MANIFEST_ENTRY, options)
        .map_err(|err| format!("写 manifest 失败：{err}"))?;
    writer
        .write_all(text.as_bytes())
        .map_err(|err| format!("写 manifest 失败：{err}"))?;

    writer
        .start_file(README_ENTRY, options)
        .map_err(|err| format!("写 README 失败：{err}"))?;
    writer
        .write_all(readme(manifest).as_bytes())
        .map_err(|err| format!("写 README 失败：{err}"))?;

    writer
        .start_file(DB_ENTRY, options)
        .map_err(|err| format!("写数据库失败：{err}"))?;
    let mut source = File::open(db_file).map_err(|err| format!("读快照失败：{err}"))?;
    std::io::copy(&mut source, &mut writer).map_err(|err| format!("写数据库失败：{err}"))?;

    // 记忆单独再存一份纯文本：Python 版读的是 data/memory.md，不看数据库
    if let Some(text) = memory_md {
        writer
            .start_file(MEMORY_ENTRY, options)
            .map_err(|err| format!("写 memory.md 失败：{err}"))?;
        writer
            .write_all(text.as_bytes())
            .map_err(|err| format!("写 memory.md 失败：{err}"))?;
    }

    let file = writer
        .finish()
        .map_err(|err| format!("收尾数据包失败：{err}"))?;
    file.sync_all().map_err(|err| format!("数据包落盘失败：{err}"))?;
    Ok(())
}

/// 把 memories 表导出成 Python 版能读的 memory.md（没有记忆时返回 None）。
fn memory_file_from(conn: &Connection) -> (Option<String>, i64) {
    let mut items: Vec<String> = Vec::new();
    if let Ok(mut stmt) = conn.prepare("SELECT content FROM memories ORDER BY created_at ASC, id ASC")
    {
        if let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) {
            for text in rows.flatten() {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    items.push(trimmed.to_string());
                }
            }
        }
    }
    if items.is_empty() {
        return (None, 0);
    }
    let count = items.len() as i64;
    let mut text = String::from(MEMORY_MD_HEADER);
    for item in items {
        text.push_str("- ");
        text.push_str(&item);
        text.push('\n');
    }
    (Some(text), count)
}

/// 去掉 Python 版追加在条目末尾的「（2026-11-10 13:00）」时间戳。
fn strip_memory_timestamp(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(pos) = trimmed.rfind('（') {
        let tail = &trimmed[pos..];
        if tail.ends_with('）') {
            let inner = &tail['（'.len_utf8()..tail.len() - '）'.len_utf8()];
            let looks_like_time = inner.chars().count() >= 10
                && inner
                    .chars()
                    .all(|c| c.is_ascii_digit() || c == '-' || c == ':' || c == ' ');
            if looks_like_time {
                return trimmed[..pos].trim().to_string();
            }
        }
    }
    trimmed.to_string()
}

/// 把 memory.md 里的「- 条目」灌进 memories 表（按内容去重），返回新增条数。
fn ingest_memory_md(conn: &Connection, text: &str) -> rusqlite::Result<usize> {
    conn.execute_batch(MEMORY_SCHEMA)?;
    let now = crate::now_str();
    let mut added = 0usize;
    for line in text.lines() {
        let trimmed = line.trim_start();
        let body = if let Some(rest) = trimmed.strip_prefix("- ") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("* ") {
            rest
        } else {
            continue;
        };
        let content = strip_memory_timestamp(body);
        if content.is_empty() {
            continue;
        }
        let existing: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memories WHERE content = ?1",
            rusqlite::params![content],
            |row| row.get(0),
        )?;
        if existing > 0 {
            continue;
        }
        conn.execute(
            "INSERT INTO memories(content, source, tags, created_at, updated_at, hits)
             VALUES (?1, 'import', '', ?2, ?2, 0)",
            rusqlite::params![content, now],
        )?;
        added += 1;
    }
    Ok(added)
}

/// 包里那张给人看的一页说明（两版内容可以不同，都是人读的）
fn readme(manifest: &Value) -> String {
    let app_version = manifest["app_version"].as_str().unwrap_or("?");
    let exported_at = manifest["exported_at"].as_str().unwrap_or("?");
    let tables = manifest["tables"]
        .as_object()
        .map(|map| {
            map.iter()
                .map(|(name, count)| format!("  {name}：{} 行", count.as_i64().unwrap_or(0)))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    let stripped: Vec<String> = manifest["stripped_keys"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|one| one.as_str().map(|text| text.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let stripped_text = if stripped.is_empty() {
        "  这次没有需要剥离的密钥。".to_string()
    } else {
        let mut text = String::from("  这次被去掉的设置键（导入后需要重新填）：\n");
        for key in &stripped {
            text.push_str(&format!("    - {key}\n"));
        }
        text
    };

    format!(
        "DanLume 数据包\n\
         ============\n\n\
         导出时间：{exported_at}\n\
         导出程序：DanLume（{app} 版）{app_version}\n\
         包格式：danlume-backup v{format_version}（库结构 schema v{schema_version}）\n\n\
         怎么用\n\
         ------\n\
         1. 打开 DanLume → 设置 → 数据迁移 →「从数据包导入…」，选中这个 .dlpk 文件。\n\
         2. 导入会先把你现在的数据库备份到 data/backups/pre-import-时间.db，\n\
            然后要求重新打开应用，下次启动才真正换库。\n\
         3. 换电脑 / 重装系统：先在新机器上装好 DanLume 并启动一次，再导入这个包；\n\
            已有的数据同样会被自动备份，不会直接丢。\n\n\
         包里有什么\n\
         ----------\n\
         manifest.json  这个包的信息（版本号、每张表的行数）\n\
         lumina.db      数据库（日程、备忘、通知、文献、邮件记录、设置……）\n\
         README.txt     本说明\n\n\
         各表行数：\n\
         {tables}\n\n\
         密钥\n\
         ----\n\
         密码类设置用 Windows DPAPI 加密，换机器解不开，所以没有导出：\n\
         {stripped_text}\n\
         导入后需要重新填一次：SMTP 授权码、IMAP 授权码、AI Key、UIS 口令、代理密码、\n\
         以及账号服务器登录（重新登录即可）。\n\n\
         注意\n\
         ----\n\
         * 包里只有数据库。作业下载的文件、PDF、LaTeX 编译产物等在数据目录里，\n\
           需要的话请自己把它们一起拷过去。\n\
         * 导入只替换 lumina.db，数据目录里的其它文件不动。\n",
        exported_at = exported_at,
        app = manifest["app"].as_str().unwrap_or("?"),
        app_version = app_version,
        format_version = manifest["format_version"].as_i64().unwrap_or(0),
        schema_version = manifest["schema_version"].as_i64().unwrap_or(0),
        tables = if tables.is_empty() { "  （没有表）".to_string() } else { tables },
        stripped_text = stripped_text,
    )
}

// ------------------------------------------------------------------ 导入

/// 从数据包导入（不覆盖正在用的库）。
///
/// 顺序很重要：**所有校验都在动用户数据之前做完** —— 包坏了、缺文件、库坏了都直接报错，
/// 现有数据一个字节都不改。
pub fn import(db_path: &Path, pkg: &Path, data_dir: &Path) -> Result<Value, String> {
    if !pkg.is_file() {
        return Err(format!("数据包不存在：{}", pkg.display()));
    }
    let file = File::open(pkg).map_err(|err| format!("打不开数据包：{err}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|err| format!("不是合法的 zip 数据包：{err}"))?;

    // ① manifest.json
    if archive.file_names().all(|name| name != MANIFEST_ENTRY) {
        return Err("数据包里缺少 manifest.json（不是 DanLume 数据包？）".to_string());
    }
    let raw = read_entry(&mut archive, MANIFEST_ENTRY)?;
    let manifest: Value =
        serde_json::from_slice(&raw).map_err(|err| format!("manifest.json 不是合法 JSON：{err}"))?;

    let format = manifest["format"].as_str().unwrap_or("");
    if format != FORMAT {
        return Err(format!("不是 DanLume 数据包（format = \"{format}\"）"));
    }
    let format_version = manifest["format_version"].as_i64().unwrap_or(-1);
    if format_version != FORMAT_VERSION {
        return Err(format!(
            "数据包的 format_version 是 {format_version}，当前版本只认 {FORMAT_VERSION}，没法导入"
        ));
    }
    let schema_version = manifest["schema_version"].as_i64().unwrap_or(-1);
    if schema_version != SCHEMA_VERSION {
        return Err(format!(
            "数据包的 schema_version 是 {schema_version}，与当前版本（{SCHEMA_VERSION}）不一致，为免弄坏数据已拒绝导入"
        ));
    }

    // ② memory.md（可选条目）：Python 版把记忆存成纯文本，包里带了就灌进 memories 表
    let memory_text: Option<String> = if archive.file_names().any(|name| name == MEMORY_ENTRY) {
        read_entry(&mut archive, MEMORY_ENTRY)
            .ok()
            .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
    } else {
        None
    };

    // ③ lumina.db 必须在，先解到数据目录里的临时文件
    if archive.file_names().all(|name| name != DB_ENTRY) {
        return Err("数据包里缺少 lumina.db（包不完整，可能拷贝时中断了）".to_string());
    }
    let staged = data_dir.join(format!(".import-{}.db", stamp_for_file()));
    let staging = (|| -> Result<(BTreeMap<String, i64>, String), String> {
        std::fs::create_dir_all(data_dir).map_err(|err| format!("建不了数据目录：{err}"))?;
        {
            let mut entry = archive
                .by_name(DB_ENTRY)
                .map_err(|err| format!("包里读不到 lumina.db：{err}"))?;
            let mut out = File::create(&staged).map_err(|err| format!("解包失败：{err}"))?;
            std::io::copy(&mut entry, &mut out).map_err(|err| format!("解包失败：{err}"))?;
            out.sync_all().map_err(|err| format!("写临时库失败：{err}"))?;
        }
        let conn = Connection::open(&staged).map_err(|err| format!("包里的库打不开：{err}"))?;
        // ③ 完整性自检：不是 ok 就一个字节都不改
        let integrity: String = conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .map_err(|err| format!("包里库自检失败：{err}"))?;
        if !integrity.eq_ignore_ascii_case("ok") {
            return Err(format!("包里的库自检没通过（{integrity}），已拒绝导入"));
        }
        let tables = table_counts(&conn);
        let has_settings: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='settings'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        drop(conn);
        if has_settings == 0 {
            return Err("包里的库没有 settings 表，不像是 DanLume 的库，已拒绝导入".to_string());
        }
        Ok((tables, integrity))
    })();
    let (tables, _) = match staging {
        Ok(value) => value,
        Err(err) => {
            let _ = std::fs::remove_file(&staged);
            return Err(err);
        }
    };

    // ④ 到这一步才动用户数据：先自动备份当前库
    let folder = data_dir.join("backups");
    std::fs::create_dir_all(&folder).map_err(|err| format!("建不了备份目录：{err}"))?;
    let backup = folder.join(format!("pre-import-{}.db", stamp_for_file()));
    let backup_done = if db_path.is_file() {
        // 同样用 VACUUM INTO 取一致性快照（WAL 里已提交的内容也带上）
        let snapshot = (|| -> Result<(), String> {
            let conn = Connection::open(db_path).map_err(|err| err.to_string())?;
            let _ = conn.busy_timeout(std::time::Duration::from_secs(10));
            let target = backup.display().to_string().replace('\'', "''");
            conn.execute_batch(&format!("VACUUM INTO '{target}'"))
                .map_err(|err| err.to_string())
        })();
        match snapshot {
            Ok(()) => true,
            // 备用方案：至少留一份字节拷贝（总比没有强）
            Err(_) => std::fs::copy(db_path, &backup).is_ok(),
        }
    } else {
        // 本来就没有旧库：留一个空文件，表示「没有数据被覆盖」
        File::create(&backup).is_ok()
    };
    if !backup_done {
        let _ = std::fs::remove_file(&staged);
        return Err(format!(
            "备份当前数据库失败，为免丢数据已取消导入（目标：{}）",
            backup.display()
        ));
    }

    // ⑤ 展开成 data/lumina.db.imported（下次启动才用它换库）
    let imported = data_dir.join(IMPORTED_SUFFIX);
    if imported.exists() {
        let _ = std::fs::remove_file(&imported);
    }
    if let Err(err) = std::fs::rename(&staged, &imported) {
        if std::fs::copy(&staged, &imported).is_err() {
            let _ = std::fs::remove_file(&staged);
            return Err(format!("把库放成 {} 失败：{err}", imported.display()));
        }
        let _ = std::fs::remove_file(&staged);
    }

    // ⑥ 写标记：导入后的库（下次启动就是它）和当前库各写一份，
    //    当前库那份重启换库后就随旧库一起没了；导入库那份换库后会被 apply_pending 清掉。
    //    顺手把包里的 memory.md 灌进导入库的 memories 表（Python 版旧记忆就是这么带过来的）。
    let pkg_text = pkg.display().to_string();
    let mut memory_added = 0usize;
    let marker = (|| -> rusqlite::Result<()> {
        let conn = Connection::open(&imported)?;
        put_setting(&conn, PENDING_KEY, "1")?;
        put_setting(&conn, PENDING_FROM_KEY, &pkg_text)?;
        if let Some(text) = memory_text.as_deref() {
            memory_added = ingest_memory_md(&conn, text)?;
        }
        Ok(())
    })();
    if let Err(err) = marker {
        let _ = std::fs::remove_file(&imported);
        return Err(format!("写导入标记失败：{err}"));
    }
    if db_path.is_file() {
        if let Ok(conn) = Connection::open(db_path) {
            let _ = conn.busy_timeout(std::time::Duration::from_millis(500));
            let _ = put_setting(&conn, PENDING_KEY, "1");
            let _ = put_setting(&conn, PENDING_FROM_KEY, &pkg_text);
        }
    }

    let stripped_keys: Vec<String> = manifest["stripped_keys"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|one| one.as_str().map(|text| text.to_string()))
                .collect()
        })
        .unwrap_or_default();

    Ok(json!({
        "ok": true,
        "needs_restart": true,
        "backup": backup.display().to_string(),
        "imported": imported.display().to_string(),
        "tables": tables,
        "stripped_keys": stripped_keys,
        "app": manifest["app"].clone(),
        "app_version": manifest["app_version"].clone(),
        "exported_at": manifest["exported_at"].clone(),
        "memory_imported": memory_added,
        "error": "",
    }))
}

// ------------------------------------------------------------------ 启动换库

/// 启动时（**任何 sqlite 连接建立之前**）把 `data/lumina.db.imported` 换成正式库。
///
/// 返回 `Ok(Some(库路径))` 表示这次换了库，`Ok(None)` 表示没有待导入的包。
/// 换库失败时保留 `lumina.db.imported`，下次启动还能再试。
pub fn apply_pending(data_dir: &Path) -> Result<Option<String>, String> {
    let imported = data_dir.join(IMPORTED_SUFFIX);
    if !imported.is_file() {
        return Ok(None);
    }
    let db = data_dir.join("lumina.db");
    let sidecars = [
        data_dir.join("lumina.db-wal"),
        data_dir.join("lumina.db-shm"),
        data_dir.join("lumina.db-journal"),
    ];

    // 换之前先验一遍：坏文件绝不能把用户原来的库换掉（宁可留着下次再试）
    match integrity_ok(&imported) {
        Ok(true) => {}
        Ok(false) => {
            return Err(format!(
                "{} 自检没通过，旧库没动；请重新导入一次数据包",
                imported.display()
            ))
        }
        Err(err) => {
            return Err(format!(
                "{} 打不开（{err}），旧库没动；请重新导入一次数据包",
                imported.display()
            ))
        }
    }

    // 旧库的 -wal / -shm / -journal 必须先删掉，否则会被当成新库的日志
    for path in &sidecars {
        let _ = std::fs::remove_file(path);
    }
    remove_with_retry(&db)?;
    // 删旧库成功后（说明老进程已经放手）再清一次，避免它退出前又写了一份
    for path in &sidecars {
        let _ = std::fs::remove_file(path);
    }

    std::fs::copy(&imported, &db).map_err(|err| {
        format!(
            "换库失败：复制 {} → {} 出错（{err}），旧库已备份，下次启动会再试",
            imported.display(),
            db.display()
        )
    })?;
    if std::fs::metadata(&db).map(|meta| meta.len()).unwrap_or(0) == 0 {
        return Err("换库失败：新库是空文件，已保留 lumina.db.imported 等下次启动再试".to_string());
    }
    let _ = std::fs::remove_file(&imported);

    // 标记用完就删（导入库可能是从另一台机器导出的，也可能带着上一次的标记）
    if let Ok(conn) = Connection::open(&db) {
        let _ = conn.execute(
            "DELETE FROM settings WHERE key IN (?1, ?2)",
            rusqlite::params![PENDING_KEY, PENDING_FROM_KEY],
        );
        let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    }
    Ok(Some(db.display().to_string()))
}

/// 这个库文件能不能打开、`PRAGMA integrity_check` 过不过
fn integrity_ok(path: &Path) -> Result<bool, String> {
    let conn = Connection::open(path).map_err(|err| err.to_string())?;
    let result: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|err| err.to_string())?;
    Ok(result.eq_ignore_ascii_case("ok"))
}

/// 删旧库；Windows 上「刚重启」时老进程可能还开着它，等一会儿重试
fn remove_with_retry(path: &Path) -> Result<(), String> {    if !path.exists() {
        return Ok(());
    }
    let mut last = String::new();
    for _ in 0..24 {
        match std::fs::remove_file(path) {
            Ok(()) => return Ok(()),
            Err(err) => {
                last = err.to_string();
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        }
    }
    Err(format!(
        "换库失败：删不掉旧库 {}（{last}）。旧库没动，数据包已展开，下次启动会再试",
        path.display()
    ))
}

// ------------------------------------------------------------------ 测试

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    /// 造一个临时数据目录 + 一个「像在用」的库（建表 + 插几条 events / settings）
    fn seed(dir: &Path) -> PathBuf {
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir).unwrap();
        let db = dir.join("lumina.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(crate::store::SCHEMA).unwrap();
        let now = "2026-11-09 10:00:00";
        for (title, date) in [
            ("组会", "2026-11-10"),
            ("交作业", "2026-11-11"),
            ("体检", "2026-11-12"),
        ] {
            conn.execute(
                "INSERT INTO events(title, description, date, start_time, end_time,
                  notified, done, color, repeat, created_at, updated_at)
                 VALUES (?1, '', ?2, '09:00', '10:00', 0, 0, 'violet', '', ?3, ?3)",
                params![title, date, now],
            )
            .unwrap();
        }
        for (key, value) in [
            ("user_name", "Helena"),
            ("theme_mode", "dark"),
            // 这两条是 DPAPI 密文，导出时应当被剥掉
            ("smtp_password", "dpapi:QUJDRA=="),
            ("llm_api_key", "dpapi:RUZHSA=="),
            ("notice_interval", "60"),
        ] {
            conn.execute(
                "INSERT INTO settings(key, value) VALUES (?1, ?2)",
                params![key, value],
            )
            .unwrap();
        }
        drop(conn);
        db
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("danlume-backup-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn export_writes_manifest_and_strips_dpapi_rows() {
        let dir = temp_dir("export");
        let db = seed(&dir.join("data"));
        let pkg = dir.join("out.dlpk");

        let result = export(&db, &pkg).expect("导出应当成功");
        assert_eq!(result["ok"], true, "{result}");
        assert!(pkg.is_file(), "没有生成数据包");
        assert_eq!(result["stripped_keys"], json!(["llm_api_key", "smtp_password"]));

        // 用 zip 打开，检查三个条目都在
        let file = File::open(&pkg).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let names: Vec<String> = archive.file_names().map(|name| name.to_string()).collect();
        assert!(names.iter().any(|name| name == MANIFEST_ENTRY), "{names:?}");
        assert!(names.iter().any(|name| name == DB_ENTRY), "{names:?}");
        assert!(names.iter().any(|name| name == README_ENTRY), "{names:?}");

        // manifest 字段一个都不能少
        let manifest: Value =
            serde_json::from_slice(&read_entry(&mut archive, MANIFEST_ENTRY).unwrap()).unwrap();
        assert_eq!(manifest["format"], "danlume-backup");
        assert_eq!(manifest["format_version"], 1);
        assert_eq!(manifest["schema_version"], 1);
        assert_eq!(manifest["app"], "rust");
        assert_eq!(manifest["app_version"], env!("CARGO_PKG_VERSION"));
        assert!(manifest["exported_at"].as_str().unwrap().ends_with('Z'));
        assert_eq!(manifest["secrets_stripped"], true);
        assert_eq!(manifest["stripped_keys"], json!(["llm_api_key", "smtp_password"]));
        assert_eq!(manifest["tables"]["events"], 3);
        assert_eq!(manifest["tables"]["settings"], 3, "剥掉两条后只剩三条普通设置");

        // 包里的库里 dpapi: 行已经没了，events 条数一致
        let unpacked = dir.join("unpacked.db");
        std::fs::write(&unpacked, read_entry(&mut archive, DB_ENTRY).unwrap()).unwrap();
        let conn = Connection::open(&unpacked).unwrap();
        let secrets: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM settings WHERE value LIKE 'dpapi:%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(secrets, 0, "DPAPI 密文没剥干净");
        let events: i64 = conn
            .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(events, 3, "日程条数对不上");
        let theme = conn
            .query_row("SELECT value FROM settings WHERE key = 'theme_mode'", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap();
        assert_eq!(theme, "dark", "普通设置不该被动");
        let integrity: String = conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .unwrap();
        assert_eq!(integrity, "ok");
        drop(conn);

        // README 是给人看的：得提到重新填密钥
        let readme = String::from_utf8(read_entry(&mut archive, README_ENTRY).unwrap()).unwrap();
        assert!(readme.contains("SMTP"), "{readme}");
        assert!(readme.contains("smtp_password"), "{readme}");
        drop(archive);

        // 库里还有别的连接在使用（WAL 里也有刚提交的行）时照样能导出：
        // VACUUM INTO 拿的是一致性快照，不会把半个事务拷进去
        {
            let live = Connection::open(&db).unwrap();
            live.execute(
                "INSERT INTO events(title, description, date, start_time, end_time,
                  notified, done, color, repeat, created_at, updated_at)
                 VALUES ('临时加的', '', '2026-11-13', '09:00', '10:00', 0, 0, 'violet', '', ?1, ?1)",
                params!["2026-11-09 10:00:00"],
            )
            .unwrap();
            let pkg2 = dir.join("out-live.dlpk");
            export(&db, &pkg2).expect("库正被别的连接打开时也应当能导出");
            let mut archive2 = zip::ZipArchive::new(File::open(&pkg2).unwrap()).unwrap();
            let manifest2: Value =
                serde_json::from_slice(&read_entry(&mut archive2, MANIFEST_ENTRY).unwrap()).unwrap();
            assert_eq!(manifest2["tables"]["events"], 4, "快照里应当带上刚提交的那条");
            drop(archive2);
            // 导出不改原库
            let count: i64 = live
                .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
                .unwrap();
            assert_eq!(count, 4);
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_stages_db_backs_up_and_marks_pending() {
        let dir = temp_dir("import");
        let source = dir.join("source");
        let db = seed(&source);
        let pkg = dir.join("out.dlpk");
        export(&db, &pkg).expect("导出应当成功");

        // 换到另一台机器：新的（空）数据目录里已经有一个自己的库（用户先启动过一次）
        let target = dir.join("target");
        let target_db = seed(&target);
        let _ = std::fs::remove_dir_all(target.join("backups"));

        let result = import(&target_db, &pkg, &target).expect("导入应当成功");
        assert_eq!(result["ok"], true, "{result}");
        assert_eq!(result["needs_restart"], true);
        assert_eq!(result["stripped_keys"], json!(["llm_api_key", "smtp_password"]));
        assert_eq!(result["tables"]["events"], 3);

        // lumina.db.imported 生成了
        let imported = target.join(IMPORTED_SUFFIX);
        assert!(imported.is_file(), "没有生成 lumina.db.imported");
        // 包里的库带着 pending_import 标记
        let conn = Connection::open(&imported).unwrap();
        let pending: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![PENDING_KEY],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pending, "1");
        let from: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![PENDING_FROM_KEY],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(from, pkg.display().to_string());
        drop(conn);

        // 当前库也写上了标记（界面上能显示「等重启」）
        let conn = Connection::open(&target_db).unwrap();
        let pending: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![PENDING_KEY],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pending, "1");
        drop(conn);

        // 导入前自动备份了当前库
        let backups: Vec<PathBuf> = std::fs::read_dir(target.join("backups"))
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .collect();
        assert_eq!(backups.len(), 1, "{backups:?}");
        let name = backups[0].file_name().unwrap().to_string_lossy().to_string();
        assert!(
            name.starts_with("pre-import-") && name.ends_with(".db"),
            "{name}"
        );
        // 备份里是「导入前」的数据：有自己的库（events 3 条 + 自己的设置）
        let conn = Connection::open(&backups[0]).unwrap();
        let events: i64 = conn
            .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(events, 3);
        let integrity: String = conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .unwrap();
        assert_eq!(integrity, "ok", "备份得是个能打开的库");
        drop(conn);

        // 真换库：apply_pending 把 imported 换成正式库，并清掉标记
        let swapped = apply_pending(&target).expect("换库应当成功");
        assert!(swapped.is_some());
        assert!(!imported.exists(), "换完库要删掉 .imported");
        let conn = Connection::open(&target_db).unwrap();
        let pending: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM settings WHERE key = ?1",
                params![PENDING_KEY],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pending, 0, "换库后要清掉 pending_import");
        let secrets: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM settings WHERE value LIKE 'dpapi:%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(secrets, 0);
        drop(conn);

        // 没有待导入的包时啥也不干
        assert_eq!(apply_pending(&target).unwrap(), None);

        // 待换库文件是坏的：绝不换掉现有库
        let good = std::fs::read(&target_db).unwrap();
        std::fs::write(&imported, b"not a sqlite database at all").unwrap();
        let err = apply_pending(&target).unwrap_err();
        assert!(err.contains("旧库没动"), "{err}");
        assert_eq!(std::fs::read(&target_db).unwrap(), good, "坏文件不能动现有库");
        assert!(imported.is_file(), "坏文件留着，等用户重新导入");
        let _ = std::fs::remove_file(&imported);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_rejects_broken_packages_without_touching_data() {
        let dir = temp_dir("broken");
        let target = dir.join("target");
        let target_db = seed(&target);
        let before = std::fs::read(&target_db).unwrap();
        let settings_before = {
            let conn = Connection::open(&target_db).unwrap();
            let count: i64 = conn
                .query_row("SELECT COUNT(*) FROM settings", [], |row| row.get(0))
                .unwrap();
            count
        };

        let assert_untouched = |err: String, case: &str| {
            assert!(!err.is_empty(), "{case}：应当有中文报错");
            assert!(
                err.chars().any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch)),
                "{case}：报错应当是中文：{err}"
            );
            // 现有数据一个字节都不能动
            assert_eq!(std::fs::read(&target_db).unwrap(), before, "{case}：库里内容被改了");
            assert!(!target.join(IMPORTED_SUFFIX).exists(), "{case}：不该生成 .imported");
            assert!(
                !target.join("backups").exists(),
                "{case}：校验没通过就不该建备份目录"
            );
        };

        // ① 坏 zip：随便写点字节
        let bad_zip = dir.join("bad.dlpk");
        std::fs::write(&bad_zip, b"this is not a zip file at all").unwrap();
        let err = import(&target_db, &bad_zip, &target).unwrap_err();
        assert_untouched(err, "坏 zip");

        // ② 缺 manifest.json（只有 lumina.db）
        let no_manifest = dir.join("no-manifest.dlpk");
        write_zip_for_test(&no_manifest, &[(DB_ENTRY, b"whatever".as_slice())]);
        let err = import(&target_db, &no_manifest, &target).unwrap_err();
        assert!(err.contains("manifest.json"), "{err}");
        assert_untouched(err, "缺 manifest");

        // ③ 缺 lumina.db（只有 manifest.json）
        let no_db = dir.join("no-db.dlpk");
        let manifest = json!({
            "format": "danlume-backup",
            "format_version": 1,
            "app": "python",
            "app_version": "2.0.0",
            "exported_at": "2026-11-09T13:40:00Z",
            "schema_version": 1,
            "tables": {},
            "secrets_stripped": false,
            "stripped_keys": [],
        });
        write_zip_for_test(
            &no_db,
            &[(MANIFEST_ENTRY, serde_json::to_vec(&manifest).unwrap().as_slice())],
        );
        let err = import(&target_db, &no_db, &target).unwrap_err();
        assert!(err.contains(DB_ENTRY), "{err}");
        assert_untouched(err, "缺 lumina.db");

        // ④ schema_version 不对
        let bad_schema = dir.join("bad-schema.dlpk");
        let mut wrong = manifest.clone();
        wrong["schema_version"] = json!(2);
        write_zip_for_test(
            &bad_schema,
            &[(
                MANIFEST_ENTRY,
                serde_json::to_vec(&wrong).unwrap().as_slice(),
            )],
        );
        let err = import(&target_db, &bad_schema, &target).unwrap_err();
        assert!(err.contains("schema"), "{err}");
        assert_untouched(err, "schema 不对");

        // ⑤ 包里的 lumina.db 是个垃圾文件（integrity_check 通不过）
        let corrupt = dir.join("corrupt.dlpk");
        write_zip_for_test(
            &corrupt,
            &[
                (MANIFEST_ENTRY, serde_json::to_vec(&manifest).unwrap().as_slice()),
                (DB_ENTRY, b"not a sqlite database".as_slice()),
            ],
        );
        let err = import(&target_db, &corrupt, &target).unwrap_err();
        assert!(!err.is_empty(), "{err}");
        assert_untouched(err, "库是坏的");

        // 校验失败后设置条数也没变
        let conn = Connection::open(&target_db).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM settings", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, settings_before);
        drop(conn);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 手工验收用：留一个真实的数据包在临时目录里，方便 `unzip -l` 看一眼结构。
    /// 不参与常规测试：
    /// `cargo test backup::tests::demo_package_export -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn demo_package_export() {
        let dir = std::env::temp_dir().join("danlume-demo-export");
        let db = seed(&dir.join("data"));
        let pkg = dir.join("DanLume-demo.dlpk");
        let result = export(&db, &pkg).expect("导出应当成功");
        println!("数据包：{}", result["path"]);
        println!("大小：{} 字节", result["bytes"]);
        println!("manifest：{}", result["manifest"]);
    }

    /// 测试用：按「条目名 → 内容」写一个最小 zip
    fn write_zip_for_test(path: &Path, entries: &[(&str, &[u8])]) {
        let file = File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, body) in entries {
            writer.start_file(*name, options).unwrap();
            writer.write_all(body).unwrap();
        }
        writer.finish().unwrap();
    }

    /// Python 版会往记忆条目末尾追加「（2026-11-10 13:00）」，导入时要去掉。
    #[test]
    fn memory_md_timestamp_is_stripped() {
        assert_eq!(
            strip_memory_timestamp("我不喜欢用分号（2026-11-10 13:00）"),
            "我不喜欢用分号"
        );
        assert_eq!(
            strip_memory_timestamp("原文里有（括号）但不像时间"),
            "原文里有（括号）但不像时间"
        );
        assert_eq!(strip_memory_timestamp("  前后空格  "), "前后空格");
    }

    /// A5：导出包里带 memory.md（Python 版读的是纯文本），导入时再灌回 memories 表。
    #[test]
    fn export_carries_memory_md_and_import_reads_it_back() {
        let dir = temp_dir("memory-md");
        let data = dir.join("data");
        let db = seed(&data);
        {
            let conn = Connection::open(&db).unwrap();
            for (text, at) in [
                ("我不喜欢用分号", "2026-11-01 09:00:00"),
                ("专业课是高等代数", "2026-11-02 09:00:00"),
            ] {
                conn.execute(
                    "INSERT INTO memories(content, source, tags, created_at, updated_at, hits)
                     VALUES (?1, 'manual', '', ?2, ?2, 0)",
                    params![text, at],
                )
                .unwrap();
            }
        }

        let pkg = dir.join("with-memory.dlpk");
        let result = export(&db, &pkg).expect("导出应当成功");
        assert_eq!(result["memory_entries"], 2, "{result}");

        // 包里确实有 memory.md，且是 Python 版认识的那种「- 」条目
        let mut archive = zip::ZipArchive::new(File::open(&pkg).unwrap()).unwrap();
        let names: Vec<String> = archive.file_names().map(|name| name.to_string()).collect();
        assert!(names.iter().any(|name| name == MEMORY_ENTRY), "{names:?}");
        let text = String::from_utf8(read_entry(&mut archive, MEMORY_ENTRY).unwrap()).unwrap();
        assert!(text.starts_with("# DanLume 记忆"), "{text}");
        assert!(text.contains("- 我不喜欢用分号"), "{text}");
        drop(archive);

        // 导入到另一个库：包里的记忆以 source='import' 进了 memories 表
        let target_dir = temp_dir("memory-md-target");
        let target_data = target_dir.join("data");
        let target_db = seed(&target_data);
        let outcome = import(&target_db, &pkg, &target_data).expect("导入应当成功");
        assert_eq!(outcome["memory_imported"], 0, "库里本来就有的内容不该重复计入：{outcome}");
        let imported = target_data.join(IMPORTED_SUFFIX);
        assert!(imported.is_file(), "没有生成 lumina.db.imported");
        let conn = Connection::open(&imported).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2, "memory.md 的条目没有进 memories 表");
        drop(conn);

        // 单独再造一个「只有 manifest + lumina.db + memory.md」的包，验证纯文本来源也能灌进去
        let plain = dir.join("plain.dlpk");
        let manifest: Value = serde_json::from_slice(
            &read_entry(
                &mut zip::ZipArchive::new(File::open(&pkg).unwrap()).unwrap(),
                MANIFEST_ENTRY,
            )
            .unwrap(),
        )
        .unwrap();
        let db_bytes = {
            let mut archive = zip::ZipArchive::new(File::open(&pkg).unwrap()).unwrap();
            read_entry(&mut archive, DB_ENTRY).unwrap()
        };
        let memory_md = "# DanLume 记忆\n\n\
- 我不喜欢用分号（2026-11-10 13:00）\n\
- 周末练琴\n\
- 常驻图书馆（2026-11-10 13:00）\n"
            .to_string();
        write_zip_for_test(
            &plain,
            &[
                (MANIFEST_ENTRY, serde_json::to_vec(&manifest).unwrap().as_slice()),
                (DB_ENTRY, db_bytes.as_slice()),
                (MEMORY_ENTRY, memory_md.as_bytes()),
            ],
        );
        let fresh_dir = temp_dir("memory-md-plain");
        let fresh_data = fresh_dir.join("data");
        let fresh_db = seed(&fresh_data);
        // 先把库里已有的记忆清掉，确保统计的是 memory.md 灌进来的
        {
            let conn = Connection::open(&fresh_db).unwrap();
            conn.execute("DELETE FROM memories", []).unwrap();
        }
        let outcome = import(&fresh_db, &plain, &fresh_data).expect("导入应当成功");
        // 包里库里已经有「我不喜欢用分号」，所以只该新增「周末练琴」「常驻图书馆」两条
        assert_eq!(outcome["memory_imported"], 2, "{outcome}");
        let conn = Connection::open(fresh_data.join(IMPORTED_SUFFIX)).unwrap();
        let contents: Vec<String> = {
            let mut stmt = conn.prepare("SELECT content FROM memories ORDER BY id").unwrap();
            let rows = stmt.query_map([], |row| row.get::<_, String>(0)).unwrap();
            rows.flatten().collect()
        };
        assert!(contents.iter().any(|one| one == "我不喜欢用分号"), "{contents:?}");
        assert!(contents.iter().any(|one| one == "常驻图书馆"), "{contents:?}");
        assert!(!contents.iter().any(|one| one.contains("2026-11-10")), "时间戳没去掉：{contents:?}");
    }
}
