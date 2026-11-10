//! 联网类与文献类 AI 工具（对应旧版 `ai_tools.py` 里的 web / literature / 邮件 / 压缩包那批）。
use serde_json::{json, Value};

use crate::store::Store;

const TIMEOUT: u64 = 25;

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(TIMEOUT))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) DanLume/2.0")
        .build()
}

fn get_text(url: &str) -> Result<String, String> {
    let response = agent()
        .get(url)
        .call()
        .map_err(|err| format!("请求失败：{err}"))?;
    response
        .into_string()
        .map_err(|err| format!("读响应失败：{err}"))
}

fn get_json(url: &str) -> Result<Value, String> {
    let text = get_text(url)?;
    serde_json::from_str(&text).map_err(|err| format!("返回的不是 JSON：{err}"))
}

fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    let decoded = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn ok(summary: impl Into<String>) -> Value {
    json!({ "ok": true, "summary": summary.into() })
}

fn with_items(summary: impl Into<String>, items: Value) -> Value {
    json!({ "ok": true, "summary": summary.into(), "items": items })
}

fn fail(message: impl Into<String>) -> Value {
    json!({ "ok": false, "summary": message.into() })
}

fn arg_str(args: &Value, key: &str) -> String {
    args.get(key)
        .and_then(|one| one.as_str())
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn arg_i64(args: &Value, key: &str, fallback: i64) -> i64 {
    args.get(key).and_then(|one| one.as_i64()).unwrap_or(fallback)
}

fn arg_ids(args: &Value) -> Vec<i64> {
    args.get("ids")
        .and_then(|one| one.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|one| {
                    one.as_i64()
                        .or_else(|| one.as_str().and_then(|text| text.parse().ok()))
                })
                .collect()
        })
        .unwrap_or_default()
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

// ------------------------------------------------------------------ 网页
pub fn web_search(args: &Value) -> Value {
    let query = arg_str(args, "query");
    if query.is_empty() {
        return fail("要传 query");
    }
    let limit = arg_i64(args, "limit", 5).clamp(1, 10) as usize;
    let url = format!("https://html.duckduckgo.com/html/?q={}", urlencode(&query));
    let html = match get_text(&url) {
        Ok(text) => text,
        Err(err) => return fail(format!("搜索失败：{err}")),
    };
    let mut items = Vec::new();
    let mut rest = html.as_str();
    while let Some(pos) = rest.find("result__a") {
        rest = &rest[pos..];
        let Some(href_at) = rest.find("href=\"") else { break };
        let after = &rest[href_at + 6..];
        let Some(end) = after.find('"') else { break };
        let href = after[..end].to_string();
        let tail = &after[end..];
        let title = {
            let Some(gt) = tail.find('>') else { break };
            let Some(lt) = tail[gt..].find("</a>") else { break };
            strip_tags(&tail[gt + 1..gt + lt])
        };
        let link = if let Some(stripped) = href.split("uddg=").nth(1) {
            let raw = stripped.split('&').next().unwrap_or_default();
            percent_decode(raw)
        } else {
            href
        };
        if !link.is_empty() && !title.is_empty() {
            items.push(json!({ "title": title, "url": link }));
        }
        rest = &tail[3..];
        if items.len() >= limit {
            break;
        }
    }
    if items.is_empty() {
        return with_items(format!("「{query}」没搜到结果（可能被限流）"), json!([]));
    }
    with_items(format!("「{query}」搜到 {} 条", items.len()), json!(items))
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(value) = u8::from_str_radix(
                std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or(""),
                16,
            ) {
                out.push(value);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

pub fn web_fetch(args: &Value) -> Value {
    let url = arg_str(args, "url");
    if url.is_empty() {
        return fail("要传 url");
    }
    let limit = arg_i64(args, "limit", 4000).clamp(200, 20000) as usize;
    let html = match get_text(&url) {
        Ok(text) => text,
        Err(err) => return fail(format!("抓取失败：{err}")),
    };
    let text: String = strip_tags(&html).chars().take(limit).collect();
    with_items(
        format!("抓到 {} 个字符", text.chars().count()),
        json!({ "url": url, "text": text }),
    )
}

pub fn video_search(args: &Value) -> Value {
    let query = arg_str(args, "query");
    if query.is_empty() {
        return fail("要传 query");
    }
    let mut wrapped = args.clone();
    wrapped["query"] = json!(format!("{query} B站 课程 视频"));
    let result = web_search(&wrapped);
    if result["ok"] == true {
        json!({
            "ok": true,
            "summary": format!("「{query}」的视频检索结果（B 站 / MOOC）"),
            "items": result["items"],
        })
    } else {
        result
    }
}

pub fn course_resource_search(store: &Store, args: &Value) -> Value {
    let keyword = arg_str(args, "keyword");
    if keyword.is_empty() {
        return fail("要传 keyword");
    }
    // 先翻本地课程文件
    let mut local = Vec::new();
    let dir = crate::paths::data_dir().join("elearning");
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.contains(&keyword) {
                local.push(json!({ "name": name, "path": entry.path().display().to_string() }));
            }
        }
    }
    let mut wrapped = args.clone();
    wrapped["query"] = json!(format!("{keyword} 课件 讲义 习题 pdf"));
    let web = web_search(&wrapped);
    with_items(
        format!(
            "本地课程文件 {} 个，网上结果 {} 条",
            local.len(),
            web["items"].as_array().map(|one| one.len()).unwrap_or(0)
        ),
        json!({ "local": local, "web": web["items"] }),
    )
}

pub fn resource_push(store: &Store, args: &Value) -> Value {
    let items = args
        .get("items")
        .and_then(|one| one.as_array())
        .cloned()
        .unwrap_or_default();
    if items.is_empty() {
        return fail("要传 items（[{title, url}]）");
    }
    let mut html = String::new();
    for item in &items {
        html.push_str(&format!(
            "<p><b>{}</b><br>{}</p>",
            item.get("title").and_then(|one| one.as_str()).unwrap_or(""),
            item.get("url").and_then(|one| one.as_str()).unwrap_or(""),
        ));
    }
    let title = format!(
        "学习资源（{} 条）{}",
        items.len(),
        if arg_str(args, "kind").is_empty() {
            String::new()
        } else {
            format!(" · {}", arg_str(args, "kind"))
        }
    );
    match store.add_memo(&title, &html) {
        Ok(id) => ok(format!("已把 {} 条资源存进备忘 #{id}", items.len())),
        Err(err) => fail(format!("存资源失败：{err}")),
    }
}

// ------------------------------------------------------------------ 文献
fn parse_arxiv(xml: &str, limit: usize) -> Vec<Value> {
    let mut out = Vec::new();
    for entry in xml.split("<entry>").skip(1) {
        let title = between(entry, "<title>", "</title>");
        let id = between(entry, "<id>", "</id>");
        let summary = between(entry, "<summary>", "</summary>");
        let published = between(entry, "<published>", "</published>");
        if title.is_empty() {
            continue;
        }
        out.push(json!({
            "title": title.split_whitespace().collect::<Vec<_>>().join(" "),
            "url": id,
            "abstract": summary.split_whitespace().collect::<Vec<_>>().join(" "),
            "year": published.chars().take(4).collect::<String>(),
            "source": "arXiv",
        }));
        if out.len() >= limit {
            break;
        }
    }
    out
}

fn between(text: &str, start: &str, end: &str) -> String {
    let Some(from) = text.find(start) else {
        return String::new();
    };
    let rest = &text[from + start.len()..];
    match rest.find(end) {
        Some(to) => rest[..to].trim().to_string(),
        None => String::new(),
    }
}

pub fn arxiv_search(args: &Value) -> Value {
    let query = arg_str(args, "query");
    if query.is_empty() {
        return fail("要传 query");
    }
    let limit = arg_i64(args, "limit", 5).clamp(1, 20) as usize;
    let url = format!(
        "http://export.arxiv.org/api/query?search_query=all:{}&start=0&max_results={limit}",
        urlencode(&query)
    );
    match get_text(&url) {
        Ok(xml) => {
            let items = parse_arxiv(&xml, limit);
            with_items(format!("arXiv 搜到 {} 篇", items.len()), json!(items))
        }
        Err(err) => fail(format!("arXiv 检索失败：{err}")),
    }
}

pub fn crossref_search(args: &Value) -> Value {
    let query = arg_str(args, "query");
    if query.is_empty() {
        return fail("要传 query");
    }
    let limit = arg_i64(args, "limit", 5).clamp(1, 20);
    let url = format!(
        "https://api.crossref.org/works?rows={limit}&select=title,DOI,author,issued,container-title&query={}",
        urlencode(&query)
    );
    match get_json(&url) {
        Ok(body) => {
            let items = crossref_items(&body);
            with_items(format!("Crossref 搜到 {} 篇", items.len()), json!(items))
        }
        Err(err) => fail(format!("Crossref 检索失败：{err}")),
    }
}

fn crossref_items(body: &Value) -> Vec<Value> {
    body.pointer("/message/items")
        .and_then(|one| one.as_array())
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    let title = row
                        .pointer("/title/0")
                        .and_then(|one| one.as_str())
                        .unwrap_or("");
                    let year = row
                        .pointer("/issued/date-parts/0/0")
                        .and_then(|one| one.as_i64())
                        .unwrap_or(0);
                    let venue = row
                        .pointer("/container-title/0")
                        .and_then(|one| one.as_str())
                        .unwrap_or("");
                    json!({
                        "title": title,
                        "doi": row.get("DOI").and_then(|one| one.as_str()).unwrap_or(""),
                        "year": year,
                        "venue": venue,
                        "url": format!("https://doi.org/{}", row.get("DOI").and_then(|one| one.as_str()).unwrap_or("")),
                        "source": "Crossref",
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn semantic_scholar_search(args: &Value) -> Value {
    let query = arg_str(args, "query");
    if query.is_empty() {
        return fail("要传 query");
    }
    let limit = arg_i64(args, "limit", 5).clamp(1, 20);
    let url = format!(
        "https://api.semanticscholar.org/graph/v1/paper/search?query={}&limit={limit}&fields=title,year,citationCount,abstract,externalIds,url",
        urlencode(&query)
    );
    match get_json(&url) {
        Ok(body) => {
            let items: Vec<Value> = body
                .get("data")
                .and_then(|one| one.as_array())
                .map(|rows| {
                    rows.iter()
                        .map(|row| {
                            json!({
                                "title": row.get("title").and_then(|one| one.as_str()).unwrap_or(""),
                                "year": row.get("year").and_then(|one| one.as_i64()).unwrap_or(0),
                                "citations": row.get("citationCount").and_then(|one| one.as_i64()).unwrap_or(0),
                                "abstract": row.get("abstract").and_then(|one| one.as_str()).unwrap_or(""),
                                "doi": row.pointer("/externalIds/DOI").and_then(|one| one.as_str()).unwrap_or(""),
                                "url": row.get("url").and_then(|one| one.as_str()).unwrap_or(""),
                                "source": "Semantic Scholar",
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            with_items(format!("Semantic Scholar 搜到 {} 篇", items.len()), json!(items))
        }
        Err(err) => fail(format!("Semantic Scholar 检索失败：{err}")),
    }
}

pub fn doi_lookup(args: &Value) -> Value {
    let doi = arg_str(args, "doi");
    if doi.is_empty() {
        return fail("要传 doi");
    }
    let url = format!("https://api.crossref.org/works/{}", urlencode(&doi));
    match get_json(&url) {
        Ok(body) => {
            let items = crossref_items(&json!({ "message": { "items": [body["message"]] } }));
            with_items(format!("按 DOI 查到了：{doi}"), json!(items))
        }
        Err(err) => fail(format!("DOI 查询失败：{err}")),
    }
}

pub fn literature_search(args: &Value) -> Value {
    let query = arg_str(args, "query");
    if query.is_empty() {
        return fail("要传 query");
    }
    let limit = arg_i64(args, "limit", 4).clamp(1, 10) as usize;
    let mut merged: Vec<Value> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut push_all = |rows: Vec<Value>| {
        for row in rows {
            let title = row
                .get("title")
                .and_then(|one| one.as_str())
                .unwrap_or("")
                .to_lowercase();
            if title.is_empty() || seen.contains(&title) {
                continue;
            }
            seen.push(title);
            merged.push(row);
        }
    };
    push_all(arxiv_search(&json!({ "query": query, "limit": limit }))["items"]
        .as_array()
        .cloned()
        .unwrap_or_default());
    push_all(crossref_search(&json!({ "query": query, "limit": limit }))["items"]
        .as_array()
        .cloned()
        .unwrap_or_default());
    push_all(semantic_scholar_search(&json!({ "query": query, "limit": limit }))["items"]
        .as_array()
        .cloned()
        .unwrap_or_default());
    with_items(
        format!("三家合并去重后 {} 篇（原「{query}」）", merged.len()),
        json!(merged),
    )
}

pub fn pdf_download(store: &Store, args: &Value) -> Value {
    let mut url = arg_str(args, "url");
    let mut title = arg_str(args, "title");
    // 按文献 id 下：从「我的文献库」里取 pdf_url（没有就用 url，arXiv 的 abs 转成 pdf）
    if url.is_empty() {
        let ids = arg_ids(args);
        if ids.is_empty() {
            return fail("要么给 url，要么给文献 ids");
        }
        let papers = crate::papers::papers(store, "", 0, "").unwrap_or_default();
        let Some(paper) = papers
            .into_iter()
            .find(|one| one.get("id").and_then(|v| v.as_i64()).map(|id| ids.contains(&id)).unwrap_or(false))
        else {
            return fail("「我的文献库」里没有这个 id");
        };
        let pdf = paper.get("pdf_url").and_then(|v| v.as_str()).unwrap_or("");
        let page = paper.get("url").and_then(|v| v.as_str()).unwrap_or("");
        url = if !pdf.is_empty() {
            pdf.to_string()
        } else if page.contains("arxiv.org/abs/") {
            page.replace("/abs/", "/pdf/")
        } else {
            page.to_string()
        };
        if title.is_empty() {
            title = paper
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
        }
        if url.is_empty() {
            let id = paper.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
            return fail(format!("文献 #{id} 没有 PDF 地址，得手动找一下"));
        }
    }
    let dir = crate::paths::data_dir().join("papers");
    if std::fs::create_dir_all(&dir).is_err() {
        return fail("建文献目录失败");
    }
    let name = {
        let stem = if title.is_empty() {
            url.split('/').last().unwrap_or("paper.pdf").to_string()
        } else {
            title.clone()
        };
        let cleaned: String = stem
            .chars()
            .map(|ch| if "\\/:*?\"<>|".contains(ch) { '-' } else { ch })
            .collect();
        if cleaned.to_lowercase().ends_with(".pdf") {
            cleaned
        } else {
            format!("{cleaned}.pdf")
        }
    };
    let target = dir.join(&name);
    let response = match agent().get(&url).call() {
        Ok(value) => value,
        Err(err) => return fail(format!("下载失败：{err}")),
    };
    let mut bytes: Vec<u8> = Vec::new();
    use std::io::Read;
    if response.into_reader().read_to_end(&mut bytes).is_err() {
        return fail("下载中断");
    }
    if !bytes.starts_with(b"%PDF") {
        return fail("这个地址返回的不是 PDF（可能需要登录或付费）");
    }
    if std::fs::write(&target, &bytes).is_err() {
        return fail("写文件失败");
    }
    // 顺手把 file_path 记回文献库，文献页就能直接打开
    let path_text = target.display().to_string();
    {
        let conn = store.connection();
        let _ = conn.execute(
            "UPDATE papers SET file_path = ?1 WHERE title = ?2 OR pdf_url = ?3",
            rusqlite::params![path_text, title, url],
        );
    }
    ok(format!(
        "已下载 {} KB 到 {}",
        bytes.len() / 1024,
        path_text
    ))
}

fn load_papers(store: &Store, ids: &[i64]) -> Vec<Value> {
    let all = crate::papers::papers(store, "", 0, "").unwrap_or_default();
    if ids.is_empty() {
        return all.into_iter().take(5).collect();
    }
    all.into_iter()
        .filter(|one| {
            one.get("id")
                .and_then(|value| value.as_i64())
                .map(|id| ids.contains(&id))
                .unwrap_or(false)
        })
        .collect()
}

pub fn citation_format(store: &Store, args: &Value) -> Value {
    let ids = arg_ids(args);
    let style = {
        let text = arg_str(args, "style").to_lowercase();
        if text.is_empty() { "gb7714".to_string() } else { text }
    };
    let papers = load_papers(store, &ids);
    if papers.is_empty() {
        return fail("「我的文献库」里没有对应记录（先搜文献并入库）");
    }
    let lines: Vec<String> = papers
        .iter()
        .map(|one| {
            let base = crate::papers::citation(one);
            match style.as_str() {
                "apa" => base.replace('，', ", "),
                "bibtex" => format!(
                    "@article{{{},\n  title = {{{}}},\n  year = {{{}}}\n}}",
                    one.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
                    one.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                    one.get("year").and_then(|v| v.as_i64()).unwrap_or(0),
                ),
                _ => base,
            }
        })
        .collect();
    with_items(
        format!("按 {style} 生成了 {} 条引用", lines.len()),
        json!(lines),
    )
}

pub fn endnote_export(store: &Store, args: &Value) -> Value {
    let ids = arg_ids(args);
    let style = {
        let text = arg_str(args, "style").to_lowercase();
        if text.is_empty() { "enw".to_string() } else { text }
    };
    let papers = load_papers(store, &ids);
    if papers.is_empty() {
        return fail("「我的文献库」里没有对应记录");
    }
    let dir = crate::paths::data_dir().join("exports");
    if std::fs::create_dir_all(&dir).is_err() {
        return fail("建导出目录失败");
    }
    let path = dir.join(format!(
        "DanLume-{}.{}",
        chrono::Local::now().format("%Y%m%d-%H%M%S"),
        if style == "ris" { "ris" } else { "enw" }
    ));
    let mut body = String::new();
    for one in &papers {
        let title = one.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let authors = one.get("authors").and_then(|v| v.as_str()).unwrap_or("");
        let year = one.get("year").and_then(|v| v.as_i64()).unwrap_or(0);
        let doi = one.get("doi").and_then(|v| v.as_str()).unwrap_or("");
        if style == "ris" {
            body.push_str("TY  - JOUR\n");
            body.push_str(&format!("TI  - {title}\n"));
            body.push_str(&format!("AU  - {authors}\n"));
            body.push_str(&format!("PY  - {year}\n"));
            if !doi.is_empty() {
                body.push_str(&format!("DO  - {doi}\n"));
            }
            body.push_str("ER  - \n\n");
        } else {
            body.push_str("%0 Journal Article\n");
            body.push_str(&format!("%T {title}\n"));
            body.push_str(&format!("%A {authors}\n"));
            body.push_str(&format!("%D {year}\n"));
            if !doi.is_empty() {
                body.push_str(&format!("%R {doi}\n"));
            }
            body.push('\n');
        }
    }
    match std::fs::write(&path, body) {
        Ok(_) => ok(format!(
            "已导出 {} 条到 {}",
            papers.len(),
            path.display()
        )),
        Err(err) => fail(format!("写文件失败：{err}")),
    }
}

pub fn library_open(args: &Value) -> Value {
    let query = arg_str(args, "query");
    let which = {
        let text = arg_str(args, "which").to_lowercase();
        if text.is_empty() { "library".to_string() } else { text }
    };
    let encoded = urlencode(&query);
    let items = json!([
        { "name": "复旦大学图书馆", "url": format!("https://lib.fudan.edu.cn/") },
        { "name": "IEEE Xplore", "url": format!("https://ieeexplore.ieee.org/search/searchresult.jsp?queryText={encoded}") },
        { "name": "ScienceDirect", "url": format!("https://www.sciencedirect.com/search?qs={encoded}") },
        { "name": "Google Scholar", "url": format!("https://scholar.google.com/scholar?q={encoded}") },
    ]);
    with_items(format!("给出了 {} 个检索入口（{which}）", 4), items)
}

// ------------------------------------------------------------------ 邮件 / 压缩包
pub fn mail_send(store: &Store, args: &Value) -> Value {
    let to = arg_str(args, "to");
    if to.is_empty() || !to.contains('@') {
        return fail("要传收件人邮箱");
    }
    let subject = {
        let text = arg_str(args, "subject");
        if text.is_empty() { "DanLume 代发".to_string() } else { text }
    };
    let text = arg_str(args, "text");
    if text.is_empty() {
        return fail("要传正文 text");
    }
    let html = format!(
        "<pre style='white-space:pre-wrap'>{}</pre>",
        text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
    );
    // 签名 + SMTP 认证方式（旧版 sender_name / mail_signature / smtp_security / smtp_oauth）
    if let Err(exc) = crate::mail_identity::prepare(store) {
        return fail(exc);
    }
    let html = crate::mail_identity::with_signature(store, &html);
    match crate::mail::send(store, &to, &subject, &html, &[]) {
        Ok(_) => ok(format!("已发送给 {to}：{subject}")),
        Err(err) => fail(format!("发送失败：{}", crate::mail::explain(err))),
    }
}

/// eLearning 下载：v2 没有 Canvas 会话，就从「作业缓存」里的附件 URL 逐个试。
/// 能下的存到下载目录，需要登录的按「失败」计数并如实说明（旧版也是 saved/skipped/failed 三元组）。
pub fn elearning_download(store: &Store, args: &Value) -> Value {
    let keyword = arg_str(args, "keyword");
    let limit = arg_i64(args, "limit", 40).clamp(1, 200) as usize;
    let items = crate::elearn::cached_items(store);
    if items.is_empty() {
        return with_items(
            "还没有 eLearning 缓存：先在「作业」页读取一次课程".to_string(),
            json!([]),
        );
    }
    let target = {
        let text = store.setting("dir_downloads").unwrap_or_default();
        if text.trim().is_empty() {
            crate::paths::data_dir().join("elearning")
        } else {
            std::path::PathBuf::from(text.trim())
        }
    };
    if std::fs::create_dir_all(&target).is_err() {
        return fail("建下载目录失败");
    }
    let mut pending: Vec<(String, String)> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for item in &items {
        if !keyword.is_empty() {
            let hay = format!(
                "{} {}",
                item.get("course").and_then(|v| v.as_str()).unwrap_or(""),
                item.get("title").and_then(|v| v.as_str()).unwrap_or(""),
            );
            if !hay.contains(&keyword) {
                continue;
            }
        }
        for attachment in item
            .get("attachments")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
        {
            let url = attachment
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let name = attachment
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("附件")
                .to_string();
            if url.is_empty() || seen.contains(&url) {
                continue;
            }
            seen.push(url.clone());
            pending.push((name, url));
            if pending.len() >= limit {
                break;
            }
        }
        if pending.len() >= limit {
            break;
        }
    }
    if pending.is_empty() {
        return with_items("缓存里没有带下载地址的附件".to_string(), json!([]));
    }
    let mut saved: Vec<String> = Vec::new();
    let mut skipped = 0usize;
    let mut failed: Vec<String> = Vec::new();
    for (name, url) in &pending {
        let clean: String = name
            .chars()
            .map(|ch| if "\\/:*?\"<>|".contains(ch) { '-' } else { ch })
            .collect();
        let path = target.join(&clean);
        if path.exists() {
            skipped += 1;
            continue;
        }
        match agent().get(url).call() {
            Ok(response) => {
                let mut bytes: Vec<u8> = Vec::new();
                use std::io::Read;
                if response.into_reader().read_to_end(&mut bytes).is_ok()
                    && std::fs::write(&path, &bytes).is_ok()
                {
                    saved.push(clean);
                } else {
                    failed.push(clean);
                }
            }
            Err(_) => failed.push(clean),
        }
    }
    with_items(
        format!(
            "下载完成：新增 {}、重名跳过 {}、失败 {}（失败的通常需要重新登录 Canvas）→ {}",
            saved.len(),
            skipped,
            failed.len(),
            target.display()
        ),
        json!({ "saved": saved, "skipped": skipped, "failed": failed,
                "target": target.display().to_string() }),
    )
}

pub fn zip_extract(args: &Value) -> Value {
    let archive = arg_str(args, "archive");
    if archive.is_empty() {
        return fail("要传 archive");
    }
    let source = std::path::PathBuf::from(&archive);
    let dest = {
        let given = arg_str(args, "dest");
        if given.is_empty() {
            source.with_extension("")
        } else {
            std::path::PathBuf::from(given)
        }
    };
    match crate::zipx::extract(&source, &dest) {
        Ok(count) => ok(format!(
            "已解压 {count} 个文件到 {}",
            dest.display()
        )),
        Err(err) => fail(err),
    }
}
