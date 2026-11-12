//! 文献库「按意思搜」（B 方案：模型跑在服务器本机，不花 API 钱）。
//!
//! 分工：
//! * **服务器**只负责「文字 → 512 维向量」，走 `/api/v1/embed`（账号令牌鉴权、按账号限流）；
//! * **客户端**把向量存在本地 `paper_vectors` 表，检索时在本地算余弦相似度 ——
//!   所以每次搜索只有「问句那一句」走一次网络（约 0.1 秒），不是每次都把文献发上去。
//!
//! 为什么不把模型装到客户端：要带上 onnxruntime + 90 MB 模型，安装包会大一圈、
//! 还得处理三平台的库文件；服务器上跑一份更省事（那台机内存够，见 `_工作日志.md`）。
//!
//! 数据边界：只发「标题 + 作者 + 摘要」，不发备注/项目名/路径；服务器不落库。
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::store::Store;

/// 一次发给服务器多少篇（服务器上限 64 条；32 条约 0.2 秒）
const BATCH: usize = 32;
/// 每篇参与向量化的文字上限（再长对短文本模型没意义，只占带宽）
const TEXT_CHARS: usize = 1200;
/// 请求超时（冷启动那一下会慢一点）
const TIMEOUT_SECS: u64 = 45;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS paper_vectors (
    paper_id   INTEGER PRIMARY KEY,
    model      TEXT NOT NULL DEFAULT '',
    dim        INTEGER NOT NULL DEFAULT 0,
    vec        BLOB NOT NULL,
    text_hash  TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL DEFAULT ''
);";

/// 建表（Rust 以前只读旧表，全新数据目录里没有这张表）
pub fn ensure_schema(store: &Store) {
    let conn = store.connection();
    let _ = conn.execute_batch(SCHEMA);
}

// ---------------------------------------------------------------- 小工具

/// 参与向量化的文字：标题 + 作者 + 摘要（摘要截断）
fn paper_text(title: &str, authors: &str, abstract_text: &str) -> String {
    let mut text = String::new();
    let title = title.trim();
    if !title.is_empty() {
        text.push_str(title);
        text.push_str("。");
    }
    let authors = authors.trim();
    if !authors.is_empty() {
        text.push_str(authors);
        text.push_str("。");
    }
    let summary = abstract_text.trim();
    if !summary.is_empty() {
        text.push_str(summary);
    }
    text.chars().take(TEXT_CHARS).collect()
}

fn digest(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let bytes = hasher.finalize();
    bytes.iter().take(8).map(|one| format!("{one:02x}")).collect()
}

fn to_blob(vector: &[f64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(vector.len() * 4);
    for value in vector {
        out.extend_from_slice(&(*value as f32).to_le_bytes());
    }
    out
}

fn from_blob(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

/// 余弦相似度：服务器回来的向量已经 L2 归一化，所以直接点积就行
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let limit = a.len().min(b.len());
    let mut total = 0.0_f32;
    for index in 0..limit {
        total += a[index] * b[index];
    }
    total
}

// ---------------------------------------------------------------- 与服务器通信

/// 调 `/api/v1/embed`，把一批文本换成向量；返回（模型名, 向量）。
///
/// 模型名要带回来存进 `paper_vectors.model`：以后服务器换模型时，
/// 客户端能看出「这些向量是旧模型建的」。
fn embed_remote(store: &Store, texts: &[String]) -> Result<(String, Vec<Vec<f64>>), String> {
    let base = crate::account::server_url(store);
    if base.is_empty() {
        return Err("没配账号服务器（设置 → 账号），语义检索需要它出向量".to_string());
    }
    let token = crate::account::token(store);
    if token.is_empty() {
        return Err("先登录 DanLume 账号，语义检索由服务器算向量".to_string());
    }
    let url = format!("{base}/api/v1/embed");
    let body = json!({ "texts": texts });
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .build();
    let response = match agent
        .post(&url)
        .set("Content-Type", "application/json")
        .set("Authorization", &format!("Bearer {token}"))
        .send_json(body)
    {
        Ok(value) => value,
        Err(ureq::Error::Status(401, _)) => {
            return Err("服务器不认这个账号令牌（在设置 → 账号里重新登录一次）".to_string())
        }
        Err(ureq::Error::Status(503, _)) => {
            return Err("服务器上的 embedding 服务没在跑，语义检索暂时不可用".to_string())
        }
        Err(ureq::Error::Status(code, _)) => {
            return Err(format!("服务器拒绝了这次请求：HTTP {code}"))
        }
        Err(exc) => return Err(format!("连不上账号服务器：{exc}")),
    };
    let payload: Value = response
        .into_json()
        .map_err(|exc| format!("服务器返回的不是 JSON：{exc}"))?;
    let model = payload
        .get("model")
        .and_then(|one| one.as_str())
        .unwrap_or("")
        .to_string();
    let vectors = payload
        .get("vectors")
        .and_then(|one| one.as_array())
        .ok_or_else(|| "服务器没返回 vectors".to_string())?;
    if vectors.len() != texts.len() {
        return Err(format!(
            "服务器返回的向量条数不对：要 {} 条，回了 {} 条",
            texts.len(),
            vectors.len()
        ));
    }
    let mut out: Vec<Vec<f64>> = Vec::with_capacity(vectors.len());
    for row in vectors {
        let values = row
            .as_array()
            .ok_or_else(|| "向量不是数组".to_string())?
            .iter()
            .map(|one| one.as_f64().unwrap_or(0.0))
            .collect::<Vec<f64>>();
        out.push(values);
    }
    Ok((model, out))
}

// ---------------------------------------------------------------- 建索引

/// 需要建/重建向量的文献（`force` = 全部重来）
fn pending_rows(store: &Store, force: bool) -> Vec<(i64, String, String)> {
    let conn = store.connection();
    let mut out: Vec<(i64, String, String)> = Vec::new();
    let Ok(mut statement) = conn.prepare(
        "SELECT p.id, COALESCE(p.title,''), COALESCE(p.authors,''), COALESCE(p.abstract,''),
                COALESCE(v.text_hash,'') AS old_hash
         FROM papers p LEFT JOIN paper_vectors v ON v.paper_id = p.id
         ORDER BY p.id",
    ) else {
        return out;
    };
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    });
    let Ok(rows) = rows else {
        return out;
    };
    for row in rows.flatten() {
        let (id, title, authors, abstract_text, old_hash) = row;
        let text = paper_text(&title, &authors, &abstract_text);
        if text.trim().is_empty() {
            continue;
        }
        if !force && old_hash == digest(&text) {
            continue;
        }
        out.push((id, text, String::new()));
    }
    out
}

/// 给还没建向量的文献算向量（已是最新的自动跳过）
pub fn index(store: &Store, force: bool) -> Value {
    ensure_schema(store);
    let pending = pending_rows(store, force);
    if pending.is_empty() {
        let total = count(store);
        return json!({
            "ok": true,
            "indexed": 0,
            "total": total,
            "message": format!("{total} 篇文献的索引都是最新的"),
        });
    }
    let mut done = 0usize;
    let mut model = String::new();
    let mut dim = 0usize;
    for chunk in pending.chunks(BATCH) {
        let texts: Vec<String> = chunk.iter().map(|(_, text, _)| text.clone()).collect();
        let (model_name, vectors) = match embed_remote(store, &texts) {
            Ok(value) => value,
            Err(exc) => {
                return json!({
                    "ok": false,
                    "indexed": done,
                    "total": count(store),
                    "error": exc,
                })
            }
        };
        {
            let conn = store.connection();
            let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
            for ((paper_id, text, _), vector) in chunk.iter().zip(vectors.iter()) {
                dim = vector.len();
                model = model_name.clone();
                let _ = conn.execute(
                    "INSERT INTO paper_vectors(paper_id, model, dim, vec, text_hash, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(paper_id) DO UPDATE SET
                        model = excluded.model, dim = excluded.dim, vec = excluded.vec,
                        text_hash = excluded.text_hash, updated_at = excluded.updated_at",
                    rusqlite::params![
                        paper_id,
                        model,
                        dim as i64,
                        to_blob(vector),
                        digest(text),
                        stamp
                    ],
                );
                done += 1;
            }
        }
    }
    json!({
        "ok": true,
        "indexed": done,
        "total": count(store),
        "dim": dim,
        "model": model,
        "message": format!("给 {done} 篇文献建好了向量（共 {} 篇）", count(store)),
    })
}

/// 本地已经有多少篇建了向量
pub fn count(store: &Store) -> usize {
    ensure_schema(store);
    let conn = store.connection();
    conn.query_row("SELECT COUNT(*) FROM paper_vectors", [], |row| {
        row.get::<_, i64>(0)
    })
    .unwrap_or(0) as usize
}

// ---------------------------------------------------------------- 检索

/// 按意思搜：把问句换成向量（这一步要联网），再和本地所有文献向量算余弦
pub fn search(store: &Store, query: &str, limit: usize) -> Value {
    ensure_schema(store);
    let text = query.trim();
    if text.is_empty() {
        return json!({ "ok": false, "error": "先写个问句/关键词" });
    }
    let vectors = match embed_remote(store, &[text.to_string()]) {
        Ok((_model, value)) => value,
        Err(exc) => return json!({ "ok": false, "error": exc }),
    };
    let Some(query_vector) = vectors.first() else {
        return json!({ "ok": false, "error": "服务器没给出向量" });
    };
    let query_blob = to_blob(query_vector);
    let query_f32 = from_blob(&query_blob);
    let mut result = rank(store, &query_f32, limit);
    if let Some(map) = result.as_object_mut() {
        map.insert("query".to_string(), json!(text));
    }
    result
}

/// 纯本地的那一半：拿一个问句向量和本地所有文献向量比（不发网络请求，便于单测）
pub fn rank(store: &Store, query_f32: &[f32], limit: usize) -> Value {
    ensure_schema(store);

    // 注意：下面这段里**不能**再顺手调 `count(store)`——`store.connection()` 是一把
    // 不可重入的锁，手里拿着连接再去要一次会自己把自己锁死（2026-11-12 踩过，
    // 表现是「按意思搜」一直转圈，测试进程直接挂住）。
    let indexed_before = count(store);
    let conn = store.connection();
    let mut scored: Vec<(f32, i64)> = Vec::new();
    let mut stale = 0usize;
    if let Ok(mut statement) = conn.prepare(
        "SELECT v.paper_id, v.vec FROM paper_vectors v
         JOIN papers p ON p.id = v.paper_id",
    ) {
        if let Ok(rows) = statement.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
        }) {
            for row in rows.flatten() {
                let (paper_id, blob) = row;
                let stored = from_blob(&blob);
                // 服务器换了模型时，旧向量的维度会不一样 —— 直接跳过并报数量，
                // 别把两种维度的向量混着算（那样分数全是噪声）
                if stored.len() != query_f32.len() {
                    stale += 1;
                    continue;
                }
                scored.push((cosine(&query_f32, &stored), paper_id));
            }
        }
    }
    if scored.is_empty() {
        return json!({
            "ok": false,
            "error": if stale > 0 {
                format!("有 {stale} 篇的向量是旧模型建的，点「重建索引」重来一次")
            } else {
                "本地还没有文献向量（先点「建索引」）".to_string()
            },
            "indexed": 0,
            "stale": stale,
        });
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let keep = limit.clamp(1, 50);
    let mut hits: Vec<Value> = Vec::new();
    for (score, paper_id) in scored.into_iter().take(keep) {
        if let Ok(item) = conn.query_row(
            "SELECT id, COALESCE(title,''), COALESCE(authors,''), COALESCE(year,''),
                    COALESCE(venue,''), COALESCE(file_path,'')
             FROM papers WHERE id = ?1",
            rusqlite::params![paper_id],
            |row| {
                Ok(json!({
                    "id": row.get::<_, i64>(0)?,
                    "title": row.get::<_, String>(1)?,
                    "authors": row.get::<_, String>(2)?,
                    "year": row.get::<_, String>(3)?,
                    "venue": row.get::<_, String>(4)?,
                    "file_path": row.get::<_, String>(5)?,
                    "score": (score * 1000.0).round() / 1000.0,
                }))
            },
        ) {
            hits.push(item);
        }
    }
    json!({
        "ok": true,
        "hits": hits,
        "indexed": indexed_before,
        "stale": stale,
        "note": "相似度 0–1，越大越接近；这不是关键词匹配，是按意思排的",
    })
}

/// 语义检索能不能用（本地有没有索引 + 服务器通不通）
pub fn status(store: &Store) -> Value {
    ensure_schema(store);
    let indexed = count(store);
    let conn = store.connection();
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM papers", [], |row| row.get(0))
        .unwrap_or(0);
    json!({
        "ok": true,
        "indexed": indexed,
        "papers": total,
        "need_index": (indexed as i64) < total,
        "logged_in": crate::account::logged_in(store),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_text_is_title_authors_abstract_and_capped() {
        let text = paper_text("Attention Is All You Need", "Vaswani", "We propose ...");
        assert_eq!(text, "Attention Is All You Need。Vaswani。We propose ...");
        assert_eq!(paper_text("", "", ""), "");
        let long = paper_text("t", "", &"字".repeat(5000));
        assert_eq!(long.chars().count(), TEXT_CHARS);
    }

    #[test]
    fn digest_changes_with_text() {
        assert_eq!(digest("abc"), digest("abc"));
        assert_ne!(digest("abc"), digest("abd"));
        assert_eq!(digest("abc").len(), 16);
    }

    #[test]
    fn blobs_roundtrip_and_cosine_is_dot_product() {
        let vector = vec![0.6_f64, 0.8_f64];
        let blob = to_blob(&vector);
        let back = from_blob(&blob);
        assert_eq!(back, vec![0.6_f32, 0.8_f32]);
        // 归一化向量的余弦 = 点积
        assert!((cosine(&back, &back) - 1.0).abs() < 1e-5);
        let other = from_blob(&to_blob(&vec![0.8_f64, 0.6_f64]));
        assert!((cosine(&back, &other) - 0.96).abs() < 1e-5);
    }

    /// 拿真实文献库跑一遍「建索引 + 按意思搜」（默认跳过）。
    ///
    /// 为什么要有这个：界面验收要抢焦点（锁屏时就做不了），而这个测试直接调
    /// 同一套 Rust 函数，不碰窗口，跑法：
    ///   $env:DANLUME_TEST_TOKEN="<账号 access token>"
    ///   cargo test semantic::tests::real_library_roundtrip -- --ignored --nocapture --test-threads=1
    ///
    /// 数据库取的是**本机已装客户端的数据目录**（`%LOCALAPPDATA%\DanLume\data_dir.txt` 指的那份）的副本，
    /// 不会动真库。
    #[test]
    #[ignore = "需要联网 + 一个账号令牌（DANLUME_TEST_TOKEN）"]
    fn real_library_roundtrip() {
        let Some(token) = std::env::var("DANLUME_TEST_TOKEN").ok().filter(|one| !one.trim().is_empty())
        else {
            println!("没有 DANLUME_TEST_TOKEN，跳过");
            return;
        };
        let Some(source_db) = real_client_db() else {
            println!("找不到本机客户端数据库（data_dir.txt 没指对），跳过");
            return;
        };
        let dir = std::env::temp_dir().join(format!("danlume-semantic-real-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::copy(&source_db, dir.join("lumina.db")).expect("复制数据库");
        for suffix in ["-wal", "-shm"] {
            let from = source_db.with_extension(format!("db{suffix}"));
            if from.exists() {
                let _ = std::fs::copy(&from, dir.join(format!("lumina.db{suffix}")));
            }
        }
        let previous = std::env::var_os("LUMINA_DATA_DIR");
        std::env::set_var("LUMINA_DATA_DIR", &dir);
        let store = crate::store::Store::open().expect("打开数据库副本");
        store.set_setting("server_token", token.trim()).expect("写令牌");
        store
            .set_setting("server_url", crate::account::DEFAULT_SERVER)
            .expect("写服务器地址");

        let result = index(&store, true);
        println!("建索引：{}", result);
        assert_eq!(result["ok"], json!(true), "建索引失败：{result}");
        assert!(result["indexed"].as_u64().unwrap_or(0) > 0);

        for query in ["跨场强 MRI 图像翻译", "注意力机制", "翻译模型"] {
            let found = search(&store, query, 5);
            println!("\n问：{query}");
            for hit in found["hits"].as_array().cloned().unwrap_or_default() {
                println!(
                    "   {:.3}  #{}  {}",
                    hit["score"].as_f64().unwrap_or(0.0),
                    hit["id"],
                    hit["title"].as_str().unwrap_or("")
                );
            }
            assert_eq!(found["ok"], json!(true), "检索失败：{found}");
        }

        match previous {
            Some(value) => std::env::set_var("LUMINA_DATA_DIR", value),
            None => std::env::remove_var("LUMINA_DATA_DIR"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 本机客户端真实数据库（打包版用 `%LOCALAPPDATA%\DanLume\data_dir.txt` 指过去）
    fn real_client_db() -> Option<std::path::PathBuf> {
        let base = std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?);
        let pointer = base.join("DanLume").join("data_dir.txt");
        let text = std::fs::read_to_string(pointer).ok()?;
        let dir = std::path::PathBuf::from(text.trim());
        let db = dir.join("lumina.db");
        if db.is_file() {
            Some(db)
        } else {
            None
        }
    }

    /// 本地排序那半段：不联网也要能跑，而且要能**跑完**（以前这里自己把自己锁死了）
    #[test]
    fn rank_orders_by_cosine_and_does_not_deadlock() {
        let dir = std::env::temp_dir().join(format!("danlume-rank-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let previous = std::env::var_os("LUMINA_DATA_DIR");
        std::env::set_var("LUMINA_DATA_DIR", &dir);
        let store = crate::store::Store::open().expect("临时库");
        ensure_schema(&store);
        {
            let conn = store.connection();
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS papers (
                    id INTEGER PRIMARY KEY AUTOINCREMENT, source TEXT DEFAULT '',
                    source_id TEXT DEFAULT '', doi TEXT DEFAULT '', title TEXT DEFAULT '',
                    authors TEXT DEFAULT '', year TEXT DEFAULT '', venue TEXT DEFAULT '',
                    abstract TEXT DEFAULT '', url TEXT DEFAULT '', pdf_url TEXT DEFAULT '',
                    project TEXT DEFAULT '', folder TEXT DEFAULT '', file_path TEXT DEFAULT '',
                    added_at TEXT DEFAULT '', note TEXT DEFAULT '', summary_zh TEXT DEFAULT '',
                    stars INTEGER DEFAULT 0);",
            )
            .expect("建 papers 表");
            for (id, title) in [
                (1, "Cross-Field MRI Translation"),
                (2, "Attention Is All You Need"),
                (3, "Music Transformer"),
            ] {
                conn.execute(
                    "INSERT INTO papers(id, title) VALUES (?1, ?2)",
                    rusqlite::params![id, title],
                )
                .expect("写文献");
            }
            // 三个二维向量：问句 [1,0] 跟第一个完全同向、第二个成 45°、第三个垂直
            for (id, vector) in [
                (1, vec![1.0_f64, 0.0]),
                (2, vec![0.7071, 0.7071]),
                (3, vec![0.0, 1.0]),
            ] {
                conn.execute(
                    "INSERT INTO paper_vectors(paper_id, model, dim, vec, text_hash, updated_at)
                     VALUES (?1, 'm', 2, ?2, 'h', 'now')",
                    rusqlite::params![id, to_blob(&vector)],
                )
                .expect("写向量");
            }
        }
        let result = rank(&store, &[1.0_f32, 0.0], 3);
        assert_eq!(result["ok"], json!(true), "{result}");
        let hits = result["hits"].as_array().unwrap();
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0]["id"], json!(1), "同向的排第一");
        assert_eq!(hits[1]["id"], json!(2), "45 度的排第二");
        assert_eq!(hits[2]["id"], json!(3), "垂直的排最后");
        assert!((hits[0]["score"].as_f64().unwrap() - 1.0).abs() < 0.01);
        assert!((hits[1]["score"].as_f64().unwrap() - 0.707).abs() < 0.01);
        assert!(hits[2]["score"].as_f64().unwrap().abs() < 0.01);

        // 维度对不上（服务器换了模型）的向量要跳过并报数量，不能混着算
        {
            let conn = store.connection();
            conn.execute(
                "UPDATE paper_vectors SET dim = 3, vec = X'0000803F0000004000000000' WHERE paper_id = 3",
                [],
            )
            .expect("换成三维向量");
        }
        let again = rank(&store, &[1.0_f32, 0.0], 3);
        assert_eq!(again["stale"], json!(1), "{again}");

        match previous {
            Some(value) => std::env::set_var("LUMINA_DATA_DIR", value),
            None => std::env::remove_var("LUMINA_DATA_DIR"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn schema_creates_table_in_fresh_db() {
        let dir = std::env::temp_dir().join(format!("danlume-semantic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let previous = std::env::var_os("LUMINA_DATA_DIR");
        std::env::set_var("LUMINA_DATA_DIR", &dir);
        let store = crate::store::Store::open().expect("临时库");
        ensure_schema(&store);
        assert_eq!(count(&store), 0);
        let conn = store.connection();
        conn.execute(
            "INSERT INTO paper_vectors(paper_id, model, dim, vec, text_hash, updated_at)
             VALUES (1, 'm', 2, X'0000803F00000040', 'h', 'now')",
            [],
        )
        .expect("写一条向量");
        drop(conn);
        assert_eq!(count(&store), 1);
        match previous {
            Some(value) => std::env::set_var("LUMINA_DATA_DIR", value),
            None => std::env::remove_var("LUMINA_DATA_DIR"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
