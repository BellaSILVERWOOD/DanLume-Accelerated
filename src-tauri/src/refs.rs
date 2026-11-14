//! 参考文献：存条目、从 BibTeX / RIS / DOI 导入、导出成 EndNote 能吃的格式。
//! 对应旧版 `lumina/refs.py`（表结构、去重规则、导出格式都照抄）。
use serde_json::{json, Value};

use crate::store::Store;

fn ensure_schema(store: &Store) {
    let conn = store.connection();
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS refs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL DEFAULT 'article',
            title TEXT NOT NULL DEFAULT '',
            authors TEXT NOT NULL DEFAULT '',
            year TEXT NOT NULL DEFAULT '',
            venue TEXT NOT NULL DEFAULT '',
            doi TEXT NOT NULL DEFAULT '',
            url TEXT NOT NULL DEFAULT '',
            note TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL
        )",
        [],
    );
}

/// 一条文献（字段与旧版 `refs` 表一致）
#[derive(Debug, Clone, Default)]
pub struct Entry {
    pub kind: String,
    pub title: String,
    pub authors: String,
    pub year: String,
    pub venue: String,
    pub doi: String,
    pub url: String,
    pub note: String,
}

impl Entry {
    fn to_json(&self) -> Value {
        json!({
            "kind": self.kind,
            "title": self.title,
            "authors": self.authors,
            "year": self.year,
            "venue": self.venue,
            "doi": self.doi,
            "url": self.url,
            "note": self.note,
        })
    }
}

/// 解析 BibTeX（旧版 `parse_bibtex`：只认 `@type{key, ... }` 的常见写法）
pub fn parse_bibtex(text: &str) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        // 找 @type{
        if chars[i] != '@' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let mut kind = String::new();
        while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
            kind.push(chars[j]);
            j += 1;
        }
        while j < chars.len() && chars[j].is_whitespace() {
            j += 1;
        }
        if kind.is_empty() || j >= chars.len() || chars[j] != '{' {
            i += 1;
            continue;
        }
        // 找配对的右括号
        let mut depth = 0i32;
        let mut end = None;
        let mut k = j;
        while k < chars.len() {
            match chars[k] {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(k);
                        break;
                    }
                }
                _ => {}
            }
            k += 1;
        }
        let Some(end) = end else {
            break;
        };
        let body: String = chars[j + 1..end].iter().collect();
        i = end + 1;
        // 去掉第一段「引用键,」
        let body = match body.find(',') {
            Some(at) => body[at + 1..].to_string(),
            None => body,
        };
        let fields = bib_fields(&body);
        if fields.is_empty() {
            continue;
        }
        let get = |names: &[&str]| -> String {
            for name in names {
                if let Some((_, value)) = fields
                    .iter()
                    .find(|(key, _)| key.as_str() == *name)
                {
                    return value.clone();
                }
            }
            String::new()
        };
        let title = get(&["title"]);
        if title.is_empty() {
            continue;
        }
        let authors = get(&["author"]).replace(" and ", ", ");
        out.push(Entry {
            kind: "article".to_string(),
            title,
            authors,
            year: get(&["year"]),
            venue: {
                let journal = get(&["journal"]);
                if journal.is_empty() {
                    get(&["booktitle"])
                } else {
                    journal
                }
            },
            doi: get(&["doi"]),
            url: get(&["url"]),
            note: get(&["note"]),
        });
        let _ = kind;
    }
    out
}

/// `title = {xxx}, author = {yyy}` → [(title, xxx), (author, yyy)]
fn bib_fields(body: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let chars: Vec<char> = body.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        // 先跳过空白和逗号，再读字段名
        while i < chars.len() && (chars[i].is_whitespace() || chars[i] == ',') {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let mut name = String::new();
        while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '-') {
            name.push(chars[i]);
            i += 1;
        }
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() || chars[i] != '=' {
            // 不是字段名：跳到下一个逗号继续
            while i < chars.len() && chars[i] != ',' {
                i += 1;
            }
            continue;
        }
        i += 1;
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let mut value = String::new();
        let open = chars[i];
        if open == '{' || open == '"' {
            let close = if open == '{' { '}' } else { '"' };
            i += 1;
            let mut depth = 1;
            while i < chars.len() {
                let ch = chars[i];
                if ch == open && open == '{' {
                    depth += 1;
                } else if ch == close {
                    depth -= 1;
                    if depth == 0 {
                        i += 1;
                        break;
                    }
                }
                value.push(ch);
                i += 1;
            }
        } else {
            while i < chars.len() && chars[i] != ',' {
                value.push(chars[i]);
                i += 1;
            }
        }
        let name = name.to_lowercase();
        if !name.is_empty() {
            let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
            out.push((name, value.trim().to_string()));
        }
        // 跳到下一个逗号之后
        while i < chars.len() && chars[i] != ',' {
            i += 1;
        }
        i += 1;
    }
    out
}

/// 解析 RIS（旧版 `parse_ris`：`TY  - JOUR` … `ER  - ` 为一条）
pub fn parse_ris(text: &str) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    let mut tags: Vec<(String, String)> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim_end();
        if line.trim().is_empty() {
            continue;
        }
        // `XX  - value`（标签两个字符，后跟 1~2 个空格、短横线、空格）
        let bytes: Vec<char> = line.chars().collect();
        if bytes.len() < 4 {
            continue;
        }
        let tag: String = bytes.iter().take(2).collect();
        if !tag.chars().next().map(|ch| ch.is_ascii_uppercase()).unwrap_or(false) {
            continue;
        }
        if !tag.chars().nth(1).map(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit()).unwrap_or(false) {
            continue;
        }
        let rest: String = bytes[2..].iter().collect();
        let rest = rest.trim_start();
        let Some(value) = rest.strip_prefix('-') else {
            continue;
        };
        let value = value.trim().to_string();
        tags.push((tag, value));
        if tags.last().map(|(key, _)| key == "ER").unwrap_or(false) {
            let get = |names: &[&str]| -> Vec<String> {
                let mut found = Vec::new();
                for (key, item) in &tags {
                    if names.contains(&key.as_str()) && !item.is_empty() {
                        found.push(item.clone());
                    }
                }
                found
            };
            let title = get(&["TI", "T1"]).join(" ");
            if !title.trim().is_empty() {
                let year = get(&["PY", "Y1"])
                    .first()
                    .cloned()
                    .unwrap_or_default();
                let year: String = year.chars().take(4).collect();
                out.push(Entry {
                    kind: "article".to_string(),
                    title,
                    authors: get(&["AU", "A1"]).join(", "),
                    year,
                    venue: {
                        let venue = get(&["JO", "JF", "T2"]);
                        if venue.is_empty() {
                            get(&["JA"]).join(" ")
                        } else {
                            venue.join(" ")
                        }
                    },
                    doi: get(&["DO"]).join(" "),
                    url: get(&["UR"]).join(" "),
                    note: get(&["N1"]).join(" "),
                });
            }
            tags.clear();
        }
    }
    out
}

/// 用 Crossref 把 DOI 变成一条文献（旧版 `fetch_doi`）
pub fn fetch_doi(doi: &str) -> Result<Entry, String> {
    let raw = doi.trim();
    let cleaned = raw
        .trim_start_matches("https://doi.org/")
        .trim_start_matches("http://doi.org/")
        .trim_start_matches("https://dx.doi.org/")
        .trim_start_matches("http://dx.doi.org/")
        .to_string();
    if cleaned.is_empty() {
        return Err("DOI 是空的".to_string());
    }
    let url = format!("https://api.crossref.org/works/{cleaned}");
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
    let value: Value = serde_json::from_str(&text)
        .map_err(|_| "Crossref 返回的不是 JSON".to_string())?;
    let message = value.get("message").cloned().unwrap_or_else(|| json!({}));
    let title = message
        .get("title")
        .and_then(|one| one.as_array())
        .map(|rows| rows.iter().filter_map(|one| one.as_str()).collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    if title.trim().is_empty() {
        return Err("Crossref 没给出标题（DOI 可能不对）".to_string());
    }
    let authors = message
        .get("author")
        .and_then(|one| one.as_array())
        .map(|rows| {
            rows.iter()
                .map(|person| {
                    let family = person.get("family").and_then(|v| v.as_str()).unwrap_or("");
                    let given = person.get("given").and_then(|v| v.as_str()).unwrap_or("");
                    let name = person.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    if !family.is_empty() {
                        format!("{family}, {given}").trim_end_matches(", ").to_string()
                    } else {
                        name.to_string()
                    }
                })
                .filter(|one| !one.is_empty())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let year = message
        .get("issued")
        .and_then(|one| one.get("date-parts"))
        .and_then(|one| one.as_array())
        .and_then(|one| one.first())
        .and_then(|one| one.as_array())
        .and_then(|one| one.first())
        .map(|one| one.to_string().trim_matches('"').to_string())
        .unwrap_or_default();
    let venue = message
        .get("container-title")
        .and_then(|one| one.as_array())
        .map(|rows| rows.iter().filter_map(|one| one.as_str()).collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    Ok(Entry {
        kind: "article".to_string(),
        title: title.trim().to_string(),
        authors,
        year,
        venue,
        doi: cleaned.clone(),
        url: message
            .get("URL")
            .and_then(|one| one.as_str())
            .map(|one| one.to_string())
            .unwrap_or_else(|| format!("https://doi.org/{cleaned}")),
        note: String::new(),
    })
}

/// 写入（按「标题 / DOI」去重，旧版 `add_many`）
pub fn add_many(store: &Store, entries: &[Entry]) -> usize {
    ensure_schema(store);
    let conn = store.connection();
    let mut added = 0usize;
    for entry in entries {
        let title = entry.title.trim();
        if title.is_empty() && entry.doi.trim().is_empty() {
            continue;
        }
        let existing: Option<i64> = conn
            .query_row(
                "SELECT id FROM refs WHERE (title <> '' AND title = ?1)
                   OR (doi <> '' AND lower(doi) = ?2) LIMIT 1",
                rusqlite::params![title, entry.doi.trim().to_lowercase()],
                |row| row.get(0),
            )
            .ok();
        if existing.is_some() {
            continue;
        }
        let stamp = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
        let ok = conn.execute(
            "INSERT INTO refs(kind, title, authors, year, venue, doi, url, note, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            rusqlite::params![
                if entry.kind.trim().is_empty() { "article" } else { entry.kind.trim() },
                title,
                entry.authors.trim(),
                entry.year.trim(),
                entry.venue.trim(),
                entry.doi.trim(),
                entry.url.trim(),
                entry.note.trim(),
                stamp,
            ],
        );
        if ok.is_ok() {
            added += 1;
        }
    }
    added
}

/// 列出参考文献（旧版 `list_refs`：关键词在标题 / 作者 / 期刊 / DOI 里找）
pub fn list_refs(store: &Store, keyword: &str, limit: usize) -> Vec<Value> {
    ensure_schema(store);
    let conn = store.connection();
    let mut out: Vec<Value> = Vec::new();
    let Ok(mut stmt) =
        conn.prepare("SELECT * FROM refs ORDER BY id DESC LIMIT ?1")
    else {
        return out;
    };
    let rows = stmt.query_map([limit as i64], |row| {
        Ok(json!({
            "id": row.get::<_, i64>("id").unwrap_or(0),
            "kind": row.get::<_, String>("kind").unwrap_or_default(),
            "title": row.get::<_, String>("title").unwrap_or_default(),
            "authors": row.get::<_, String>("authors").unwrap_or_default(),
            "year": row.get::<_, String>("year").unwrap_or_default(),
            "venue": row.get::<_, String>("venue").unwrap_or_default(),
            "doi": row.get::<_, String>("doi").unwrap_or_default(),
            "url": row.get::<_, String>("url").unwrap_or_default(),
            "note": row.get::<_, String>("note").unwrap_or_default(),
            "created_at": row.get::<_, String>("created_at").unwrap_or_default(),
        }))
    });
    if let Ok(mapped) = rows {
        out = mapped.flatten().collect();
    }
    let text = keyword.trim().to_lowercase();
    if !text.is_empty() {
        out.retain(|item| {
            ["title", "authors", "venue", "doi"]
                .iter()
                .any(|key| {
                    item.get(*key)
                        .and_then(|one| one.as_str())
                        .map(|one| one.to_lowercase().contains(&text))
                        .unwrap_or(false)
                })
        });
    }
    out
}

pub fn delete_ref(store: &Store, id: i64) {
    ensure_schema(store);
    let conn = store.connection();
    let _ = conn.execute("DELETE FROM refs WHERE id = ?1", [id]);
}

/// 导出成 RIS（EndNote / Zotero 都吃）
pub fn to_ris(entries: &[Value]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for item in entries {
        let text = |key: &str| {
            item.get(key)
                .and_then(|one| one.as_str())
                .unwrap_or("")
                .trim()
                .to_string()
        };
        lines.push("TY  - JOUR".to_string());
        if !text("title").is_empty() {
            lines.push(format!("TI  - {}", text("title")));
        }
        for author in text("authors").split(',') {
            let name = author.trim();
            if !name.is_empty() {
                lines.push(format!("AU  - {name}"));
            }
        }
        for (key, tag) in [("year", "PY"), ("venue", "JO"), ("doi", "DO"), ("url", "UR"), ("note", "N1")] {
            let value = text(key);
            if !value.is_empty() {
                lines.push(format!("{tag}  - {value}"));
            }
        }
        lines.push("ER  - ".to_string());
        lines.push(String::new());
    }
    lines.join("\n")
}

/// 导出成 BibTeX
pub fn to_bibtex(entries: &[Value]) -> String {
    let mut chunks: Vec<String> = Vec::new();
    for (index, item) in entries.iter().enumerate() {
        let text = |key: &str| {
            item.get(key)
                .and_then(|one| one.as_str())
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let authors = text("authors");
        let first = authors.split(',').next().unwrap_or("").trim().to_string();
        let last_word = first
            .split_whitespace()
            .last()
            .unwrap_or("ref")
            .to_string();
        let key: String = last_word.chars().filter(|ch| ch.is_ascii_alphanumeric()).collect();
        let year = text("year");
        let label = format!(
            "{}{}",
            if key.is_empty() { "ref".to_string() } else { key },
            if year.is_empty() {
                (index + 1).to_string()
            } else {
                year.clone()
            }
        );
        let mut fields: Vec<String> = Vec::new();
        if !text("title").is_empty() {
            fields.push(format!("  title = {{{}}}", text("title")));
        }
        if !authors.is_empty() {
            fields.push(format!("  author = {{{}}}", authors.replace(", ", " and ")));
        }
        if !text("venue").is_empty() {
            fields.push(format!("  journal = {{{}}}", text("venue")));
        }
        if !year.is_empty() {
            fields.push(format!("  year = {{{year}}}"));
        }
        if !text("doi").is_empty() {
            fields.push(format!("  doi = {{{}}}", text("doi")));
        }
        if !text("url").is_empty() {
            fields.push(format!("  url = {{{}}}", text("url")));
        }
        chunks.push(format!("@article{{{label},\n{}\n}}", fields.join(",\n")));
    }
    chunks.join("\n\n")
}

/// 导出到文件：`format` = "ris" / "bibtex"，返回写出的路径
pub fn export(store: &Store, path: &str, format: &str) -> Result<String, String> {
    let entries = list_refs(store, "", 2000);
    if entries.is_empty() {
        return Err("参考文献是空的，没什么可导出".to_string());
    }
    let content = if format.eq_ignore_ascii_case("bibtex") || format.eq_ignore_ascii_case("bib") {
        to_bibtex(&entries)
    } else if format.eq_ignore_ascii_case("enw") || format.eq_ignore_ascii_case("endnote") {
        to_enw(&entries)
    } else {
        to_ris(&entries)
    };
    let target = std::path::PathBuf::from(path);
    if let Some(parent) = target.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&target, content).map_err(|exc| format!("写文件失败：{exc}"))?;
    Ok(target.display().to_string())
}

/// 把 `Entry` 转成 JSON（命令层返回用）
pub fn entry_json(entry: &Entry) -> Value {
    entry.to_json()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bibtex_entries() {
        let text = r#"
@article{zhang2026,
  title = {A Study of {LLM} Agents},
  author = {Zhang, San and Li, Si},
  journal = {Journal of Tests},
  year = {2026},
  doi = {10.1234/xyz}
}
@inproceedings{wang2025,
  title = {Another Paper},
  author = {Wang, Wu},
  booktitle = {Proc. of Something},
  year = {2025}
}
"#;
        let entries = parse_bibtex(text);
        assert_eq!(entries.len(), 2, "{entries:?}");
        assert_eq!(entries[0].title, "A Study of {LLM} Agents");
        assert_eq!(entries[0].authors, "Zhang, San, Li, Si");
        assert_eq!(entries[0].year, "2026");
        assert_eq!(entries[0].doi, "10.1234/xyz");
        assert_eq!(entries[1].venue, "Proc. of Something");
    }

    #[test]
    fn parses_ris_entries() {
        let text = "TY  - JOUR\nTI  - Hello World\nAU  - Doe, John\nAU  - Roe, Jane\nPY  - 2024\nJO  - Some Journal\nDO  - 10.9999/abc\nER  - \n";
        let entries = parse_ris(text);
        assert_eq!(entries.len(), 1, "{entries:?}");
        assert_eq!(entries[0].title, "Hello World");
        assert_eq!(entries[0].authors, "Doe, John, Roe, Jane");
        assert_eq!(entries[0].year, "2024");
        assert_eq!(entries[0].doi, "10.9999/abc");
    }

    #[test]
    fn round_trips_ris_and_bibtex() {
        let entries = vec![serde_json::json!({
            "title": "Round Trip",
            "authors": "Doe, John",
            "year": "2026",
            "venue": "J. Test",
            "doi": "10.1/x",
            "url": "https://example.com",
            "note": ""
        })];
        let ris = to_ris(&entries);
        assert!(ris.contains("TY  - JOUR") && ris.contains("ER  - "));
        let back = parse_ris(&ris);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].title, "Round Trip");

        let bib = to_bibtex(&entries);
        assert!(bib.starts_with("@article{"));
        let back = parse_bibtex(&bib);
        assert_eq!(back.len(), 1, "{bib}\n{back:?}");
        assert_eq!(back[0].title, "Round Trip");
        assert_eq!(back[0].authors, "Doe, John");
    }
}

/// 导出成 EndNote 能直接导入的 `.enw` 文本（旧版 `lit.py` 的 enw 格式）
pub fn to_enw(entries: &[Value]) -> String {
    let mut chunks: Vec<String> = Vec::new();
    for item in entries {
        let text = |key: &str| {
            item.get(key)
                .and_then(|one| one.as_str())
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let mut lines: Vec<String> = Vec::new();
        lines.push("%0 Journal Article".to_string());
        for author in text("authors").split(',') {
            let name = author.trim();
            if !name.is_empty() {
                lines.push(format!("%A {name}"));
            }
        }
        if !text("title").is_empty() {
            lines.push(format!("%T {}", text("title")));
        }
        if !text("venue").is_empty() {
            lines.push(format!("%J {}", text("venue")));
        }
        if !text("year").is_empty() {
            lines.push(format!("%D {}", text("year")));
        }
        if !text("doi").is_empty() {
            lines.push(format!("%R {}", text("doi")));
        }
        if !text("url").is_empty() {
            lines.push(format!("%U {}", text("url")));
        }
        if !text("note").is_empty() {
            lines.push(format!("%Z {}", text("note")));
        }
        lines.push(String::new());
        chunks.push(lines.join("\n"));
    }
    chunks.join("\n")
}
