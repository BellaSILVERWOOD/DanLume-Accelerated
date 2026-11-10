//! 文献库：直接读旧库的 `papers` 表（字段与旧版一致），支持星级 / 项目筛选。
use serde_json::{json, Value};

use crate::store::Store;

fn row_to_json(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(json!({
        "id": row.get::<_, i64>("id")?,
        "title": row.get::<_, String>("title").unwrap_or_default(),
        "authors": row.get::<_, String>("authors").unwrap_or_default(),
        "year": row.get::<_, i64>("year").unwrap_or(0),
        "venue": row.get::<_, String>("venue").unwrap_or_default(),
        "doi": row.get::<_, String>("doi").unwrap_or_default(),
        "abstract": row.get::<_, String>("abstract").unwrap_or_default(),
        "url": row.get::<_, String>("url").unwrap_or_default(),
        "pdf_url": row.get::<_, String>("pdf_url").unwrap_or_default(),
        "file_path": row.get::<_, String>("file_path").unwrap_or_default(),
        "project": row.get::<_, String>("project").unwrap_or_default(),
        "note": row.get::<_, String>("note").unwrap_or_default(),
        "summary_zh": row.get::<_, String>("summary_zh").unwrap_or_default(),
        "stars": row.get::<_, i64>("stars").unwrap_or(0),
        "source": row.get::<_, String>("source").unwrap_or_default(),
        "added_at": row.get::<_, String>("added_at").unwrap_or_default(),
    }))
}

pub fn papers(store: &Store, project: &str, stars_min: i64, keyword: &str)
    -> rusqlite::Result<Vec<Value>> {
    let conn = store.connection();
    let mut sql = String::from("SELECT * FROM papers WHERE stars >= ?1");
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(stars_min)];
    if !project.trim().is_empty() {
        sql.push_str(" AND project = ?2");
        args.push(Box::new(project.trim().to_string()));
    }
    sql.push_str(" ORDER BY stars DESC, id DESC");
    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|item| item.as_ref()).collect();
    let rows = stmt.query_map(refs.as_slice(), row_to_json)?;
    let mut out: Vec<Value> = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    let text = keyword.trim().to_lowercase();
    if !text.is_empty() {
        out.retain(|item| {
            let haystack = format!(
                "{} {} {} {}",
                item["title"].as_str().unwrap_or(""),
                item["authors"].as_str().unwrap_or(""),
                item["venue"].as_str().unwrap_or(""),
                item["summary_zh"].as_str().unwrap_or("")
            )
            .to_lowercase();
            haystack.contains(&text)
        });
    }
    Ok(out)
    }

pub fn projects(store: &Store) -> rusqlite::Result<Vec<(String, i64)>> {
    let conn = store.connection();
    let mut stmt = conn.prepare(
        "SELECT COALESCE(project, '') AS name, COUNT(*) AS total FROM papers
         GROUP BY name ORDER BY total DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    rows.collect()
}

pub fn set_meta(store: &Store, id: i64, stars: Option<i64>, note: Option<&str>,
                summary: Option<&str>) -> rusqlite::Result<bool> {
    let conn = store.connection();
    if let Some(value) = stars {
        conn.execute("UPDATE papers SET stars = ?1 WHERE id = ?2", rusqlite::params![value, id])?;
    }
    if let Some(value) = note {
        conn.execute("UPDATE papers SET note = ?1 WHERE id = ?2", rusqlite::params![value, id])?;
    }
    if let Some(value) = summary {
        conn.execute("UPDATE papers SET summary_zh = ?1 WHERE id = ?2", rusqlite::params![value, id])?;
    }
    Ok(true)
}

/// GB/T 7714 风格的引用（旧版也是这个格式）
pub fn citation(paper: &Value) -> String {
    let authors = paper["authors"].as_str().unwrap_or("");
    let title = paper["title"].as_str().unwrap_or("");
    let venue = paper["venue"].as_str().unwrap_or("");
    let year = paper["year"].as_i64().unwrap_or(0);
    let first = authors.split([';', ',']).next().unwrap_or("").trim();
    if first.is_empty() {
        format!("{title}[J]. {venue}, {year}.")
    } else {
        format!("{first}, 等. {title}[J]. {venue}, {year}.")
    }
}
