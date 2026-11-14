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

// ------------------------------------------------------------------ 在线检索 / 入库
//
// 对应旧版 `lit.py` 的 `search_all()` / `add_paper()`：Rust 这边以前只读本地 `papers` 表，
// **没有检索、也没法把检索结果存进来**（J1-2）。这里先补 arXiv + Crossref 两路（都不用 Key）。

/// 从 Atom XML 里抠 `<entry>` 的字段（arXiv 返回的格式很规整，不引 XML 库）
fn parse_arxiv(xml: &str, limit: usize) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for chunk in xml.split("<entry>").skip(1) {
        if out.len() >= limit {
            break;
        }
        let take = |tag: &str| -> String {
            let open = format!("<{tag}");
            let close = format!("</{tag}>");
            let Some(start) = chunk.find(&open) else {
                return String::new();
            };
            let Some(head_end) = chunk[start..].find('>') else {
                return String::new();
            };
            let body = &chunk[start + head_end + 1..];
            let end = body.find(&close).unwrap_or(body.len());
            body[..end]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        };
        let title = take("title");
        if title.is_empty() {
            continue;
        }
        let id = take("id");
        // 作者：每个 <author><name>…</name></author>
        let mut authors: Vec<String> = Vec::new();
        for author in chunk.split("<author>").skip(1) {
            let name = {
                let Some(start) = author.find("<name>") else { break };
                let rest = &author[start + 6..];
                let end = rest.find("</name>").unwrap_or(rest.len());
                rest[..end].trim().to_string()
            };
            if !name.is_empty() {
                authors.push(name);
            }
            if authors.len() >= 20 {
                break;
            }
        }
        let published = take("published");
        let year: String = published.chars().take(4).collect();
        let link = {
            let mut found = String::new();
            for piece in chunk.split("<link").skip(1) {
                if let Some(href_at) = piece.find("href=\"") {
                    let after = &piece[href_at + 6..];
                    let end = after.find('"').unwrap_or(after.len());
                    let href = &after[..end];
                    if href.contains("/abs/") || found.is_empty() {
                        found = href.to_string();
                    }
                }
            }
            found
        };
        let doi = take("arxiv:doi");
        out.push(json!({
            "source": "arxiv",
            "source_id": if id.is_empty() { title.clone() } else { id },
            "title": title,
            "authors": authors.join(", "),
            "year": year,
            "venue": "arXiv",
            "abstract": take("summary"),
            "url": link,
            "pdf_url": link.replace("/abs/", "/pdf/"),
            "doi": doi,
        }));
    }
    out
}

/// Crossref 按关键词检索（JSON，不用 Key）。返回 (条目, Crossref 报的总数)
fn search_crossref(query: &str, limit: usize, offset: usize) -> Result<(Vec<Value>, usize), String> {
    let url = format!(
        "https://api.crossref.org/works?rows={limit}&offset={offset}&query={}",
        urlencode(query)
    );
    let response = crate::net::with_proxy(
     ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(15)),
     crate::get_store(),
     &url,
 )
 .build()
        .get(&url)
        .set("User-Agent", "DanLume/2.0 (mailto:noreply@example.com)")
        .call()
        .map_err(|exc| format!("Crossref 请求失败：{exc}"))?;
    let text = response
        .into_string()
        .map_err(|exc| format!("Crossref 内容读不出来：{exc}"))?;
    let value: Value =
        serde_json::from_str(&text).map_err(|_| "Crossref 返回的不是 JSON".to_string())?;
    let mut out: Vec<Value> = Vec::new();
    if let Some(items) = value
        .get("message")
        .and_then(|one| one.get("items"))
        .and_then(|one| one.as_array())
    {
        for item in items.iter().take(limit) {
            let title = item
                .get("title")
                .and_then(|one| one.as_array())
                .map(|rows| rows.iter().filter_map(|one| one.as_str()).collect::<Vec<_>>().join(" "))
                .unwrap_or_default();
            if title.trim().is_empty() {
                continue;
            }
            let authors = item
                .get("author")
                .and_then(|one| one.as_array())
                .map(|rows| {
                    rows.iter()
                        .map(|person| {
                            let family = person.get("family").and_then(|v| v.as_str()).unwrap_or("");
                            let given = person.get("given").and_then(|v| v.as_str()).unwrap_or("");
                            format!("{family}, {given}").trim_end_matches(", ").to_string()
                        })
                        .filter(|one| !one.is_empty())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let year = item
                .get("issued")
                .and_then(|one| one.get("date-parts"))
                .and_then(|one| one.as_array())
                .and_then(|one| one.first())
                .and_then(|one| one.as_array())
                .and_then(|one| one.first())
                .map(|one| one.to_string().trim_matches('"').to_string())
                .unwrap_or_default();
            let doi = item.get("DOI").and_then(|one| one.as_str()).unwrap_or("");
            out.push(json!({
                "source": "crossref",
                "source_id": if doi.is_empty() { title.clone() } else { doi.to_string() },
                "title": title,
                "authors": authors,
                "year": year,
                "venue": item.get("container-title")
                    .and_then(|one| one.as_array())
                    .map(|rows| rows.iter().filter_map(|one| one.as_str()).collect::<Vec<_>>().join(" "))
                    .unwrap_or_default(),
                "abstract": "",
                "url": item.get("URL").and_then(|one| one.as_str()).unwrap_or(""),
                "pdf_url": "",
                "doi": doi,
            }));
        }
    }
    let total = value
        .get("message")
        .and_then(|one| one.get("total-results"))
        .and_then(|one| one.as_u64())
        .unwrap_or(0) as usize;
    Ok((out, total))
}

/// Semantic Scholar 的返回（`data: [...]`）→ 统一的条目结构
/// （照旧版 `lit.parse_semantic`，`openAccessPdf.url` 就是能直接下的 PDF）
fn parse_semantic(payload: &Value, limit: usize) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let Some(rows) = payload.get("data").and_then(|one| one.as_array()) else {
        return out;
    };
    for entry in rows.iter().take(limit) {
        let title = entry
            .get("title")
            .and_then(|one| one.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if title.is_empty() {
            continue;
        }
        let external = entry.get("externalIds");
        let doi = external
            .and_then(|one| one.get("DOI"))
            .and_then(|one| one.as_str())
            .unwrap_or("")
            .to_string();
        let paper_id = entry
            .get("paperId")
            .and_then(|one| one.as_str())
            .unwrap_or("")
            .to_string();
        let pdf = entry
            .get("openAccessPdf")
            .and_then(|one| one.get("url"))
            .and_then(|one| one.as_str())
            .unwrap_or("")
            .to_string();
        let authors = entry
            .get("authors")
            .and_then(|one| one.as_array())
            .map(|rows| {
                rows.iter()
                    .take(8)
                    .filter_map(|person| person.get("name").and_then(|v| v.as_str()))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let url = if !doi.is_empty() {
            format!("https://doi.org/{doi}")
        } else {
            format!("https://www.semanticscholar.org/paper/{paper_id}")
        };
        let abstract_text = entry
            .get("abstract")
            .and_then(|one| one.as_str())
            .unwrap_or("")
            .chars()
            .take(3000)
            .collect::<String>();
        out.push(json!({
            "source": "semantic",
            "source_id": if paper_id.is_empty() {
                if doi.is_empty() { title.clone() } else { doi.clone() }
            } else {
                paper_id.clone()
            },
            "title": title,
            "authors": authors,
            "year": entry.get("year").map(|one| one.to_string().trim_matches('"').to_string()).unwrap_or_default(),
            "venue": entry.get("venue").and_then(|one| one.as_str()).unwrap_or(""),
            "abstract": abstract_text,
            "url": url,
            "pdf_url": pdf,
            "doi": doi,
        }));
    }
    out
}

/// Semantic Scholar Graph API（不用 Key，但**没 Key 时限速很严**：实测同一台机器
/// 连着请求基本必回 429）。所以：最多试三次、逐次拉长等待，还不行就把话说清楚，
/// 让 arXiv / Crossref 的结果照常显示（旧版 `lit.search_all` 的 `with_semantic` 也是「失败只报错不拦」）。
fn search_semantic(query: &str, limit: usize, offset: usize) -> Result<(Vec<Value>, usize), String> {
    let url = format!(
        "https://api.semanticscholar.org/graph/v1/paper/search?query={}&limit={limit}&offset={offset}&fields=title,abstract,year,venue,authors,externalIds,openAccessPdf,paperId",
        urlencode(query)
    );
    let agent = crate::net::with_proxy(
     ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(20)),
     crate::get_store(),
     &url,
 )
 .build();
    let waits = [1_200_u64, 2_500_u64];
    let mut round = 0usize;
    loop {
        let response = agent.get(&url).set("User-Agent", "DanLume/2.0").call();
        match response {
            Ok(value) => {
                let text = value
                    .into_string()
                    .map_err(|exc| format!("Semantic Scholar 内容读不出来：{exc}"))?;
                let payload: Value = serde_json::from_str(&text)
                    .map_err(|_| "Semantic Scholar 返回的不是 JSON".to_string())?;
                let rows = parse_semantic(&payload, limit);
                let total = payload
                    .get("total")
                    .and_then(|one| one.as_u64())
                    .unwrap_or(0) as usize;
                return Ok((rows, total));
            }
            Err(ureq::Error::Status(429, _)) if round < waits.len() => {
                // 限速了：歇一下再来（总共最多等 3.7 秒，不让检索卡太久）
                std::thread::sleep(std::time::Duration::from_millis(waits[round]));
                round += 1;
                continue;
            }
            Err(ureq::Error::Status(429, _)) => {
                return Err(
                    "Semantic Scholar 正在限速（免费接口没有 Key 时很常见），过一会儿再搜".to_string(),
                );
            }
            Err(exc) => return Err(format!("Semantic Scholar 请求失败：{exc}")),
        }
    }
}


/// arXiv 报的总条数（`<opensearch:totalResults>`），拿不到就是 0
fn arxiv_total(xml: &str) -> usize {
    for (open, close) in [
        ("<opensearch:totalResults", "</opensearch:totalResults>"),
        ("<totalResults", "</totalResults>"),
    ] {
        if let Some(start) = xml.find(open) {
            let after = &xml[start..];
            if let Some(head_end) = after.find('>') {
                let body = &after[head_end + 1..];
                let end = body.find(close).unwrap_or(body.len());
                let digits: String = body[..end]
                    .chars()
                    .take_while(|ch| ch.is_ascii_digit())
                    .collect();
                if let Ok(value) = digits.parse::<usize>() {
                    return value;
                }
            }
        }
    }
    0
}

fn urlencode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// 把一句关键词切成词（去掉首尾标点），最多取 5 个
fn query_terms(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(|word| {
            word.trim_matches(|ch: char| {
                ch.is_ascii_punctuation() || ch == '，' || ch == '。' || ch == '、' || ch == '：'
            })
            .to_string()
        })
        .filter(|word| !word.is_empty())
        .take(5)
        .collect()
}

/// arXiv 的检索式：多个词用 `AND` 连起来，每个词都加引号（避免被当成前缀/短语展开）
///
/// 以前是 `all:{整句}`，`+` 让 arXiv 只按第一个词匹配——用户搜「MRI crossfield」
/// 拿到一堆只有 MRI 的结果，于是质问「检索结果和 crossfield 疑似没有任何关系」。
/// 常见学术复合词前缀：`crossfield` ↔ `cross-field` 这类写法要都能搜到
const COMPOUND_PREFIXES: [&str; 18] = [
    "cross", "multi", "self", "semi", "non", "pre", "post", "sub", "super", "inter", "intra",
    "auto", "ultra", "hyper", "meta", "micro", "macro", "neuro",
];

/// 给一个「连写」的复合词生成带连字符的写法（`crossfield` → `cross-field`）
///
/// 论文里几乎不写 `crossfield`，而是 `cross-field` / `cross field`；
/// arXiv 的分词器把它拆成两个词，所以直接搜 `all:"crossfield"` 一条都搜不到
/// （用户就是这么撞上「没有 mri 跨场强翻译的论文」的）。
fn hyphen_variants(term: &str) -> Vec<String> {
    if !term.is_ascii() || term.len() < 8 {
        return Vec::new();
    }
    let lower = term.to_ascii_lowercase();
    let mut out: Vec<String> = Vec::new();
    for prefix in COMPOUND_PREFIXES {
        if lower.starts_with(prefix) && lower.len() > prefix.len() + 3 {
            let variant = format!("{}-{}", &term[..prefix.len()], &term[prefix.len()..]);
            if !out.contains(&variant) {
                out.push(variant);
            }
        }
    }
    out
}

/// arXiv 的检索式：`join` 决定多个词之间用 `AND` 还是 `OR`
///
/// 单个词如果像复合词（`crossfield`），就写成 `(all:"crossfield" OR all:"cross-field")`，
/// 两种写法都能命中。
fn arxiv_search_query_with(terms: &[String], join: &str) -> String {
    let parts: Vec<String> = terms
        .iter()
        .map(|term| {
            let variants = hyphen_variants(term);
            if variants.is_empty() {
                format!("all:\"{term}\"")
            } else {
                let mut pieces = vec![format!("all:\"{term}\"")];
                for variant in variants {
                    pieces.push(format!("all:\"{variant}\""));
                }
                format!("({})", pieces.join(" OR "))
            }
        })
        .collect();
    urlencode(&parts.join(&format!(" {join} ")))
}

fn arxiv_search_query(terms: &[String]) -> String {
    arxiv_search_query_with(terms, "AND")
}

/// 比对用的归一化：小写，并且去掉连字符 / 下划线 / 空格
///
/// 学术词里 `cross-field`、`cross field`、`crossfield` 是同一个东西，
/// `self-attention`/`open_set` 也一样；统一去掉分隔符再比，三种写法就都能对上。
fn normalize_for_match(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|ch| !matches!(ch, '-' | '‐' | '–' | '—' | '_' | ' ' | '\u{3000}'))
        .collect()
}

/// 每个关键词命中了几个（标题 / 摘要 / 作者 / 期刊里出现过就算）
fn term_hits(item: &Value, terms: &[String]) -> usize {
    let field = |key: &str| item.get(key).and_then(|one| one.as_str()).unwrap_or("");
    let blob = normalize_for_match(&format!(
        "{} {} {} {}",
        field("title"),
        field("abstract"),
        field("authors"),
        field("venue")
    ));
    terms
        .iter()
        .filter(|term| blob.contains(&normalize_for_match(term)))
        .count()
}

/// 标题 / 摘要 / 作者 / 期刊里是否**每个关键词都出现**（大小写不敏感）
///
/// Crossref 的 `query` 是「任意词命中」的模糊检索，只搜 `MRI crossfield` 时
/// `crossfield` 完全不出现也照样返回一堆 MRI 论文；这一层过滤把「只沾上一个词」的清掉。
fn keep_relevant(item: &Value, terms: &[String]) -> bool {
    if terms.len() <= 1 {
        return true;
    }
    term_hits(item, terms) == terms.len()
}

/// 一轮检索：按 `join`（AND/OR）问两个来源，返回（条目, 每源条数, 每源总量, 每源取回量, 错误）
struct Pass {
    rows: Vec<Value>,
    counts: Value,
    totals: Value,
    raw: Value,
    errors: Vec<String>,
}

fn run_pass(text: &str, terms: &[String], cap: usize, pool: usize, start: usize, join: &str) -> Pass {
    let mut rows: Vec<Value> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut counts = json!({ "arxiv": 0, "crossref": 0, "semantic": 0 });
    let mut totals = json!({ "arxiv": 0, "crossref": 0, "semantic": 0 });
    let mut raw = json!({ "arxiv": 0, "crossref": 0, "semantic": 0 });

    let arxiv_url = format!(
        "https://export.arxiv.org/api/query?search_query={}&start={start}&max_results={pool}&sortBy=relevance",
        arxiv_search_query_with(terms, join)
    );
    match crate::net::with_proxy(
     ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(20)),
     crate::get_store(),
     &arxiv_url,
 )
 .build()
        .get(&arxiv_url)
        .set("User-Agent", "DanLume/2.0")
        .call()
    {
        Ok(response) => match response.into_string() {
            Ok(xml) => {
                let mut got = parse_arxiv(&xml, pool);
                raw["arxiv"] = json!(got.len());
                if join == "AND" {
                    got.retain(|one| keep_relevant(one, terms));
                }
                got.truncate(cap);
                counts["arxiv"] = json!(got.len());
                totals["arxiv"] = json!(arxiv_total(&xml));
                rows.extend(got);
            }
            Err(exc) => errors.push(format!("arXiv 读取失败：{exc}")),
        },
        Err(exc) => errors.push(format!("arXiv 请求失败：{exc}")),
    }
    match search_crossref(text, pool, start) {
        Ok((got, total)) => {
            raw["crossref"] = json!(got.len());
            let mut got: Vec<Value> = if join == "AND" {
                got.into_iter().filter(|one| keep_relevant(one, terms)).collect()
            } else {
                got
            };
            got.truncate(cap);
            counts["crossref"] = json!(got.len());
            totals["crossref"] = json!(total);
            rows.extend(got);
        }
        Err(exc) => errors.push(exc),
    }
    // 第三路：Semantic Scholar（旧版有，Rust 之前漏了）。它最容易 429，
    // 所以放在最后，失败了只记一条错误，前两路的结果照常显示。
    match search_semantic(text, pool, start) {
        Ok((got, total)) => {
            raw["semantic"] = json!(got.len());
            let mut got: Vec<Value> = if join == "AND" {
                got.into_iter().filter(|one| keep_relevant(one, terms)).collect()
            } else {
                got
            };
            got.truncate(cap);
            counts["semantic"] = json!(got.len());
            totals["semantic"] = json!(total);
            rows.extend(got);
        }
        Err(exc) => errors.push(exc),
    }
    Pass { rows, counts, totals, raw, errors }
}

/// 按 DOI / 标题去重（保持顺序）
fn dedupe(items: Vec<Value>) -> Vec<Value> {
    let mut seen: Vec<String> = Vec::new();
    let mut out: Vec<Value> = Vec::new();
    for item in items {
        let key = {
            let doi = item.get("doi").and_then(|one| one.as_str()).unwrap_or("").to_lowercase();
            if doi.is_empty() {
                item.get("title")
                    .and_then(|one| one.as_str())
                    .unwrap_or("")
                    .to_lowercase()
            } else {
                doi
            }
        };
        if key.is_empty() || seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(item);
    }
    out
}

/// arXiv + Crossref 一起搜，按 DOI / 标题去重（旧版 `search_all` 的两路版本）
///
/// 三个用户反馈都写在这里，免得以后改回去：
/// * 「怎么可能只有 20 条啊」→ 以前固定 10 条/来源且没有翻页，现在给总量 + 翻页；
/// * 「检索结果和 crossfield 疑似没有任何关系」→ 多关键词按 **AND** 搜，
///   并且结果必须**每个词都出现**（arXiv 检索式 + Crossref 结果的二次过滤）；
/// * 「怎么可能没有 mri 跨场强翻译的论文」→ 严格匹配一条都没有时**自动放宽一次**
///   （任一词命中），按命中词数排序，并在每条上标出「命中 2/3 个词」。
///   英语论文里「跨场强」通常写成 `across field strengths` / `field-strength`，
///   硬要求 `crossfield` 这个词出现会把相关论文全筛掉。
pub fn search_online(query: &str, limit: usize, offset: usize) -> Value {
    let text = query.trim();
    if text.is_empty() {
        return json!({ "ok": false, "results": [], "errors": ["关键词是空的"] });
    }
    let terms = query_terms(text);
    if terms.is_empty() {
        return json!({ "ok": false, "results": [], "errors": ["关键词里没有可用的词"] });
    }
    let cap = limit.clamp(1, 50);
    let start = offset;
    // 多关键词要过滤，得多取一些才能凑够一页
    let pool = if terms.len() > 1 { (cap * 4).clamp(cap, 100) } else { cap };

    let mut pass = run_pass(text, &terms, cap, pool, start, "AND");
    let mut relaxed = false;
    if pass.rows.is_empty() && terms.len() > 1 {
        // 严格匹配没结果：放宽成 OR，按命中词数排序（相关的排前面）
        let fallback = run_pass(text, &terms, cap * 2, pool, start, "OR");
        if !fallback.rows.is_empty() {
            pass = fallback;
            relaxed = true;
        }
    }

    // 每条标出「命中了几个词」，界面上显示成 2/3，方便判断相关性
    let total_terms = terms.len();
    let mut rows: Vec<Value> = pass
        .rows
        .into_iter()
        .map(|mut item| {
            let hits = term_hits(&item, &terms);
            if let Some(map) = item.as_object_mut() {
                map.insert("matched_terms".to_string(), json!(hits));
                map.insert("total_terms".to_string(), json!(total_terms));
            }
            item
        })
        .collect();
    if relaxed {
        rows.sort_by_key(|item| {
            std::cmp::Reverse(item.get("matched_terms").and_then(|one| one.as_u64()).unwrap_or(0))
        });
    }
    let found = dedupe(rows);
    let more = pass.raw["arxiv"].as_u64().unwrap_or(0) >= pool as u64
        || pass.raw["crossref"].as_u64().unwrap_or(0) >= pool as u64
        || pass.raw["semantic"].as_u64().unwrap_or(0) >= pool as u64;
    json!({
        "ok": true,
        "results": found,
        "errors": pass.errors,
        "counts": pass.counts,
        "totals": pass.totals,
        "raw": pass.raw,
        "terms": terms,
        "relaxed": relaxed,
        "offset": start,
        "limit": cap,
        "more": more,
    })
}

/// 写一条文献（同来源同 id 就更新），对应旧版 `lit.add_paper`
pub fn add_paper(store: &Store, item: &Value, project: &str) -> Option<i64> {
    let text = |key: &str| -> String {
        item.get(key)
            .and_then(|one| one.as_str())
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let source = if text("source").is_empty() { "manual".to_string() } else { text("source") };
    let source_id = {
        let raw = if !text("source_id").is_empty() {
            text("source_id")
        } else if !text("doi").is_empty() {
            text("doi")
        } else if !text("title").is_empty() {
            text("title")
        } else {
            text("url")
        };
        raw
    };
    if source_id.is_empty() {
        return None;
    }
    let conn = store.connection();
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let _ = conn.execute(
        "INSERT INTO papers(source, source_id, doi, title, authors, year, venue,
            abstract, url, pdf_url, project, note, added_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,'',?12)
         ON CONFLICT(source, source_id) DO UPDATE SET
            doi=excluded.doi, title=excluded.title, authors=excluded.authors,
            year=excluded.year, venue=excluded.venue, abstract=excluded.abstract,
            url=excluded.url, pdf_url=excluded.pdf_url",
        rusqlite::params![
            source,
            source_id.chars().take(300).collect::<String>(),
            text("doi").chars().take(200).collect::<String>(),
            text("title").chars().take(500).collect::<String>(),
            text("authors").chars().take(500).collect::<String>(),
            text("year").chars().take(16).collect::<String>(),
            text("venue").chars().take(300).collect::<String>(),
            text("abstract").chars().take(4000).collect::<String>(),
            text("url").chars().take(600).collect::<String>(),
            text("pdf_url").chars().take(600).collect::<String>(),
            if project.is_empty() { text("project") } else { project.to_string() },
            stamp,
        ],
    );
    let id: Option<i64> = conn
        .query_row(
            "SELECT id FROM papers WHERE source = ?1 AND source_id = ?2",
            rusqlite::params![source, source_id],
            |row| row.get(0),
        )
        .ok();
    id
}

// ---------------------------------------------------------------- 下载 PDF（J1-2）

/// 把标题 / 项目名变成安全的文件夹名（照旧版 `lit.sanitize`）
fn sanitize_name(name: &str, limit: usize) -> String {
    let cleaned: String = name
        .chars()
        .map(|ch| match ch {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\r' | '\n' | '\t' => ' ',
            other => other,
        })
        .collect();
    let squeezed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = squeezed.chars().take(limit).collect();
    let text = cut.trim().trim_matches('.').to_string();
    if text.is_empty() {
        "未命名".to_string()
    } else {
        text
    }
}

/// 文献库目录：`library/<项目或未分类>/<年份或未知年份>`（旧版 `lit.folder_for`）
pub fn library_folder(project: &str, year: &str) -> std::path::PathBuf {
    let base = crate::paths::data_dir().join("library");
    let project_name = if project.trim().is_empty() { "未分类" } else { project.trim() };
    let year_name = if year.trim().is_empty() { "未知年份" } else { year.trim() };
    let folder = base
        .join(sanitize_name(project_name, 60))
        .join(sanitize_name(year_name, 12));
    let _ = std::fs::create_dir_all(&folder);
    folder
}

/// 下载这篇文献的 PDF 到文献库（对应旧版 `lit.download_pdf`）
///
/// 规则都一样：已经有文件就直接说「下载过了」；没 `pdf_url` 就退回 `https://doi.org/<doi>`；
/// 返回内容不是 `%PDF` 开头就判定「要登录 / 付费」，不落盘。
pub fn download_pdf(store: &Store, id: i64) -> Value {
    let row = {
        let conn = store.connection();
        conn.query_row(
            "SELECT COALESCE(title,''), COALESCE(project,''), COALESCE(year,''),
                    COALESCE(pdf_url,''), COALESCE(doi,''), COALESCE(file_path,'')
             FROM papers WHERE id = ?1",
            rusqlite::params![id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .ok()
    };
    let Some((title, project, year, pdf_url, doi, file_path)) = row else {
        return json!({ "ok": false, "summary": "没有这篇文献" });
    };
    if !file_path.trim().is_empty() && std::path::Path::new(&file_path).exists() {
        return json!({
            "ok": true,
            "path": file_path,
            "summary": format!("已经下载过了：{file_path}"),
        });
    }
    let url = if !pdf_url.trim().is_empty() {
        pdf_url.trim().to_string()
    } else if !doi.trim().is_empty() {
        format!("https://doi.org/{}", doi.trim())
    } else {
        String::new()
    };
    if url.is_empty() {
        return json!({ "ok": false, "summary": "这篇没有可下载的 PDF 地址（可能需要机构登录）" });
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return json!({ "ok": false, "summary": "PDF 地址不是 http(s)，不下载" });
    }
    let agent = crate::net::with_proxy(
     ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(40)),
     crate::get_store(),
     &url,
 )
 .build();
    let response = match agent
        .get(&url)
        .set("User-Agent", "DanLume/2.0 (mailto:noreply@example.com)")
        .call()
    {
        Ok(value) => value,
        Err(ureq::Error::Status(code, _)) => {
            return json!({
                "ok": false,
                "summary": format!("下载失败：HTTP {code}（可能要机构登录）"),
            })
        }
        Err(exc) => return json!({ "ok": false, "summary": format!("下载失败：{exc}") }),
    };
    let mut bytes: Vec<u8> = Vec::new();
    {
        use std::io::Read;
        if response.into_reader().read_to_end(&mut bytes).is_err() {
            return json!({ "ok": false, "summary": "下载中断" });
        }
    }
    if !bytes.starts_with(b"%PDF") {
        return json!({
            "ok": false,
            "summary": "这个地址返回的不是 PDF（多半是需要登录 / 验证的页面）",
        });
    }
    let folder = library_folder(&project, &year);
    let stem = sanitize_name(if title.trim().is_empty() { "paper" } else { &title }, 70);
    let mut target = folder.join(format!("{stem}.pdf"));
    let mut index = 2;
    while target.exists() {
        let short = sanitize_name(if title.trim().is_empty() { "paper" } else { &title }, 60);
        target = folder.join(format!("{short} ({index}).pdf"));
        index += 1;
        if index > 200 {
            break;
        }
    }
    if let Err(exc) = std::fs::write(&target, &bytes) {
        return json!({ "ok": false, "summary": format!("存不下来：{exc}") });
    }
    let path_text = target.display().to_string();
    let folder_text = folder.display().to_string();
    {
        let conn = store.connection();
        // 老库没有 folder 列时退回只写 file_path（写不进去也无所谓，文件已经落盘了）
        let done = conn.execute(
            "UPDATE papers SET file_path = ?1, folder = ?2 WHERE id = ?3",
            rusqlite::params![path_text, folder_text, id],
        );
        if done.is_err() {
            let _ = conn.execute(
                "UPDATE papers SET file_path = ?1 WHERE id = ?2",
                rusqlite::params![path_text, id],
            );
        }
    }
    json!({
        "ok": true,
        "path": path_text,
        "folder": folder_text,
        "bytes": bytes.len(),
        "summary": format!("已下载到 {path_text}（{} KB）", bytes.len() / 1024),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 多关键词：arXiv 上 `all:MRI crossfield` 会退化成「只要有一个词」，
    /// 搜出来一堆只有 MRI 的结果（用户就是这么发现的）。这里锁住 AND 写法。
    #[test]
    fn multiword_query_is_anded() {
        let terms = query_terms("MRI crossfield");
        assert_eq!(terms, vec!["MRI".to_string(), "crossfield".to_string()]);
        let encoded = arxiv_search_query(&terms);
        assert!(encoded.contains("AND"), "多个词必须用 AND 连接：{encoded}");
        assert!(encoded.contains("%22MRI%22"), "每个词要加引号：{encoded}");
        // 单个词也走同一套：`all:"词"`（arXiv 对单个词的引号无影响，省得两条路径不一样）
        let single = query_terms("segmentation");
        assert_eq!(arxiv_search_query(&single), "all%3A%22segmentation%22");
    }

    /// 结果里每个关键词都必须出现（Crossref 是模糊匹配，不筛就会出现「跟第二个词无关」的条目）
    #[test]
    fn relevance_requires_every_term() {
        let terms = query_terms("MRI crossfield");
        let unrelated = json!({
            "title": "Pediatric Pancreas Segmentation from MRI Scans with Deep Learning",
            "authors": "Eli F. Keles",
            "venue": "arXiv",
            "abstract": "",
        });
        assert!(!keep_relevant(&unrelated, &terms), "只含 MRI 的不该留下");
        let related = json!({
            "title": "Quantitative MRI of the prostate",
            "authors": "J. R. Crossfield",
            "venue": "Medical Image Analysis",
            "abstract": "",
        });
        assert!(keep_relevant(&related, &terms), "作者/标题里出现两个词就该留下");
        // 单个词不做过滤
        let single = query_terms("MRI");
        assert!(keep_relevant(&unrelated, &single));
    }

    #[test]
    fn query_terms_strip_punctuation() {
        assert_eq!(query_terms("  MRI,  crossfield.  "), vec!["MRI", "crossfield"]);
        assert!(query_terms("   ").is_empty());
    }

    /// 命中词数：放宽那次检索要靠它排序（3 个词命中 2 个的排前面）
    #[test]
    fn term_hits_counts_each_word() {
        let terms = query_terms("MRI crossfield translation");
        let partial = json!({
            "title": "MRI-to-CT translation with contrastive learning",
            "authors": "",
            "venue": "",
            "abstract": "",
        });
        assert_eq!(term_hits(&partial, &terms), 2);
        assert!(!keep_relevant(&partial, &terms), "不是 3/3 就不算严格命中");
        let full = json!({
            "title": "Cross-field MRI translation across field strengths",
            "authors": "",
            "venue": "",
            "abstract": "",
        });
        assert_eq!(term_hits(&full, &terms), 3);
        assert!(keep_relevant(&full, &terms));
    }

    /// 放宽那次用 OR 拼（arXiv 的 `AND` 会一条都搜不到时，才走这一步）
    #[test]
    fn relaxed_query_uses_or() {
        let terms = query_terms("MRI crossfield translation");
        assert!(arxiv_search_query_with(&terms, "OR").contains("OR"));
        assert!(arxiv_search_query(&terms).contains("AND"));
    }

    /// 连写的复合词要自动补一个带连字符的写法（`crossfield` → `cross-field`）
    #[test]
    fn compound_words_get_hyphen_variant() {
        assert_eq!(hyphen_variants("crossfield"), vec!["cross-field".to_string()]);
        assert!(hyphen_variants("multiomics").contains(&"multi-omics".to_string()));
        assert!(hyphen_variants("MRI").is_empty());
        assert!(hyphen_variants("attention").is_empty());
        let terms = query_terms("MRI crossfield translation");
        let q = arxiv_search_query(&terms);
        assert!(q.contains("cross-field"), "检索式里要带连字符写法：{q}");
        assert!(q.contains("AND"));
    }

    /// 空格也算同一种写法：`cross field` / `cross-field` / `crossfield` 都算命中
    #[test]
    fn spaces_and_hyphens_are_equivalent() {
        let terms = query_terms("MRI crossfield translation");
        for title in [
            "Task-Adaptive 3D Cross-Field MRI Translation",
            "Cross field MRI translation with transformers",
            "Crossfield MRI translation revisited",
        ] {
            let item = json!({ "title": title, "authors": "", "venue": "", "abstract": "" });
            assert_eq!(term_hits(&item, &terms), 3, "{title} 应该三个词全中");
        }
    }

    /// 连字符写法要能互相对上：`cross-field` ↔ `crossfield`
    #[test]
    fn hyphens_do_not_block_matching() {        let terms = query_terms("MRI crossfield");
        let item = json!({
            "title": "Cross-field encoding in MRI",
            "authors": "",
            "venue": "",
            "abstract": "",
        });
        assert!(keep_relevant(&item, &terms));
        let reverse = query_terms("MRI cross-field");
        let plain = json!({ "title": "MRI crossfield artifacts", "authors": "", "venue": "", "abstract": "" });
        assert!(keep_relevant(&plain, &reverse));
    }

    #[test]
    fn parses_arxiv_atom_entries() {
        let xml = r#"<?xml version="1.0"?><feed>
        <entry>
          <id>http://arxiv.org/abs/2401.00001v1</id>
          <title>A  Test
            Paper</title>
          <summary>  Abstract here. </summary>
          <published>2024-01-02T00:00:00Z</published>
          <author><name>Doe, John</name></author>
          <author><name>Roe, Jane</name></author>
          <link href="http://arxiv.org/abs/2401.00001v1" rel="alternate"/>
        </entry></feed>"#;
        let rows = parse_arxiv(xml, 10);
        assert_eq!(rows.len(), 1, "{rows:?}");
        let one = &rows[0];
        assert_eq!(one.get("title").unwrap().as_str().unwrap(), "A Test Paper");
        assert_eq!(one.get("authors").unwrap().as_str().unwrap(), "Doe, John, Roe, Jane");
        assert_eq!(one.get("year").unwrap().as_str().unwrap(), "2024");
        assert!(one.get("pdf_url").unwrap().as_str().unwrap().contains("/pdf/"));
    }

    #[test]
    fn urlencodes_queries() {
        assert_eq!(urlencode("a b&c"), "a+b%26c");
    }

    /// J1-2 第三路：Semantic Scholar 的 JSON → 统一条目（含 openAccessPdf）
    #[test]
    fn parses_semantic_scholar_payload() {
        let payload = json!({
            "total": 2,
            "data": [
                {
                    "paperId": "abc123",
                    "title": " Cross-field MRI translation ",
                    "abstract": "we study ...",
                    "year": 2024,
                    "venue": "MICCAI",
                    "authors": [{"name": "Li Mengqing"}, {"name": "Doe John"}],
                    "externalIds": {"DOI": "10.1000/xyz"},
                    "openAccessPdf": {"url": "https://example.org/paper.pdf"}
                },
                {
                    "paperId": "no-title",
                    "title": "   "
                }
            ]
        });
        let rows = parse_semantic(&payload, 10);
        assert_eq!(rows.len(), 1, "没有标题的条目要丢掉：{rows:?}");
        let one = &rows[0];
        assert_eq!(one["source"], "semantic");
        assert_eq!(one["source_id"], "abc123");
        assert_eq!(one["title"], "Cross-field MRI translation");
        assert_eq!(one["authors"], "Li Mengqing, Doe John");
        assert_eq!(one["year"], "2024");
        assert_eq!(one["venue"], "MICCAI");
        assert_eq!(one["doi"], "10.1000/xyz");
        assert_eq!(one["url"], "https://doi.org/10.1000/xyz");
        assert_eq!(one["pdf_url"], "https://example.org/paper.pdf");
    }

    /// 文献库目录 + 文件名清洗（照旧版 `lit.sanitize` / `folder_for`）
    #[test]
    fn sanitizes_names_for_library_folders() {
        assert_eq!(sanitize_name("a/b:c*?", 60), "a b c");
        assert_eq!(sanitize_name("   多   空格   ", 60), "多 空格");
        assert_eq!(sanitize_name("", 60), "未命名");
        assert_eq!(sanitize_name("...", 60), "未命名");
        assert_eq!(sanitize_name("一二三四五", 3), "一二三");
    }

    /// 临时数据目录（`LUMINA_DATA_DIR` 是进程级的，用完必须还原）
    struct TempData {
        previous: Option<std::ffi::OsString>,
        dir: std::path::PathBuf,
    }

    impl TempData {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "danlume-papers-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let previous = std::env::var_os("LUMINA_DATA_DIR");
            std::env::set_var("LUMINA_DATA_DIR", &dir);
            Self { previous, dir }
        }
    }

    impl Drop for TempData {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var("LUMINA_DATA_DIR", value),
                None => std::env::remove_var("LUMINA_DATA_DIR"),
            }
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// `papers` 表是旧版建的，Rust 的 SCHEMA 里没有——临时库里要自己补一份
    fn ensure_papers_table(store: &Store) {
        let conn = store.connection();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS papers (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source TEXT NOT NULL DEFAULT '',
                source_id TEXT NOT NULL DEFAULT '',
                doi TEXT NOT NULL DEFAULT '',
                title TEXT NOT NULL DEFAULT '',
                authors TEXT NOT NULL DEFAULT '',
                year TEXT NOT NULL DEFAULT '',
                venue TEXT NOT NULL DEFAULT '',
                abstract TEXT NOT NULL DEFAULT '',
                url TEXT NOT NULL DEFAULT '',
                pdf_url TEXT NOT NULL DEFAULT '',
                project TEXT NOT NULL DEFAULT '',
                folder TEXT NOT NULL DEFAULT '',
                file_path TEXT NOT NULL DEFAULT '',
                added_at TEXT NOT NULL DEFAULT '',
                note TEXT NOT NULL DEFAULT '',
                summary_zh TEXT NOT NULL DEFAULT '',
                stars INTEGER NOT NULL DEFAULT 0,
                UNIQUE(source, source_id)
            );",
        )
        .expect("建 papers 表");
    }

    /// 没有 PDF 地址的文献：给一句人话，不去联网
    #[test]
    fn download_without_url_reports_reason() {
        let _temp = TempData::new("nourl");
        let store = Store::open().expect("临时库");
        ensure_papers_table(&store);
        let id: i64 = {
            let conn = store.connection();
            conn.execute(
                "INSERT INTO papers(source, source_id, title, project, year, pdf_url, doi, file_path)
                 VALUES ('manual','no-pdf','No PDF paper','','2024','','','')",
                [],
            )
            .expect("写一条文献");
            conn.last_insert_rowid()
        };
        let result = download_pdf(&store, id);
        assert_eq!(result["ok"], json!(false), "{result}");
        assert!(
            result["summary"].as_str().unwrap().contains("没有可下载的 PDF"),
            "{result}"
        );
        // 不存在的 id 也要有话说
        let missing = download_pdf(&store, 99999);
        assert_eq!(missing["ok"], json!(false));
        assert!(missing["summary"].as_str().unwrap().contains("没有这篇文献"));
    }

    /// 真下一次 arXiv 的 PDF（默认跳过：要联网）。手动跑：
    /// `cargo test papers::tests::download_real_arxiv_pdf -- --ignored --test-threads=1`
    #[test]
    #[ignore = "需要联网：真的从 arXiv 下一个 PDF"]
    fn download_real_arxiv_pdf() {
        let temp = TempData::new("realpdf");
        let store = Store::open().expect("临时库");
        ensure_papers_table(&store);
        let id: i64 = {
            let conn = store.connection();
            conn.execute(
                "INSERT INTO papers(source, source_id, title, project, year, pdf_url, doi, file_path)
                 VALUES ('arxiv','1706.03762','Attention Is All You Need','测试项目','2017',
                         'https://arxiv.org/pdf/1706.03762','','')",
                [],
            )
            .expect("写一条文献");
            conn.last_insert_rowid()
        };
        let result = download_pdf(&store, id);
        assert_eq!(result["ok"], json!(true), "下载失败：{result}");
        let path = result["path"].as_str().unwrap().to_string();
        println!("saved to {path}");
        assert!(path.contains("library"), "要落进文献库目录：{path}");
        assert!(path.contains("测试项目"), "要按项目分目录：{path}");
        assert!(path.ends_with(".pdf"), "{path}");
        let bytes = std::fs::read(&path).expect("读回下载的 PDF");
        assert!(bytes.starts_with(b"%PDF"), "下回来的不是 PDF");
        assert!(bytes.len() > 50_000, "PDF 太小了：{} 字节", bytes.len());
        // 第二次：直接说「已经下载过了」，不再联网
        let again = download_pdf(&store, id);
        assert_eq!(again["ok"], json!(true), "{again}");
        assert!(again["summary"].as_str().unwrap().contains("已经下载过"));
        // 记住路径了（文献页的「打开 PDF」靠它）
        let listed = papers(&store, "", 0, "").unwrap();
        assert_eq!(listed[0]["file_path"], json!(path));
        drop(temp);
    }

    /// 文献库目录结构：library/<项目>/<年份>
    #[test]
    fn library_folder_follows_project_and_year() {
        let temp = TempData::new("folder");
        let folder = library_folder("MRI 跨场强", "2024");
        let text = folder.to_string_lossy().to_string();
        assert!(text.ends_with("library\\MRI 跨场强\\2024") || text.ends_with("library/MRI 跨场强/2024"), "{text}");
        assert!(folder.exists(), "目录要被建出来：{text}");
        // 空项目 / 空年份给默认名
        let fallback = library_folder("", "");
        let tail = fallback.to_string_lossy().to_string();
        assert!(tail.contains("未分类") && tail.contains("未知年份"), "{tail}");
        drop(temp);
    }
}
