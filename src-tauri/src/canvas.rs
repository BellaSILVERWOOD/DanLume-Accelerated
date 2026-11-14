//! Canvas（eLearning）接口：课程 / 作业 / 文件 / 下载（对应旧版 `elearning.py`）。
//!
//! 鉴权与教务同理，需要"第二跳"：ID 会话 → `elearning.fudan.edu.cn` 服务会话，
//! 之后按 Canvas REST API 取数据（旧版用的就是这几个端点）。
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::store::Store;
use crate::uis;

pub const BASE: &str = "https://elearning.fudan.edu.cn";
pub const COURSES: &str =
    "/api/v1/courses?enrollment_state=active&per_page=100";
pub const ASSIGNMENTS: &str =
    "/api/v1/courses/{cid}/assignments?per_page=100";
pub const FILES: &str =
    "/api/v1/courses/{cid}/files?per_page=100&sort=created_at&order=desc";
/// dash 页右侧 To Do（最近要交什么）——旧版 `elearning.TODO_API`
pub const TODO: &str = "/api/v1/users/self/todo?per_page=50";

fn agent(url: &str) -> ureq::Agent {
    crate::net::with_proxy(
        ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(30))
            .redirects(5)
            .user_agent(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                 (KHTML, like Gecko) Chrome/120.0 Safari/537.36",
            )
,
        crate::get_store(),
        url,
    )
    .build()
}

/// 建立（或复用）Canvas 会话：先请求首页走完整 SSO
pub fn ensure_session(store: &Store) -> Result<uis::Jar, String> {
    let (final_url, jar) = uis::visit_with_session(store, BASE)?;
    if final_url.contains("id.fudan.edu.cn") {
        return Err(format!(
            "会话对 Canvas 无效（被重定向到统一身份认证）：先在「打开登录窗口」里登录，再点「抓取会话」。落地地址：{final_url}"
        ));
    }
    Ok(jar)
}

fn get_json(jar: &uis::Jar, path: &str) -> Result<Value, String> {
    let url = format!("{BASE}{path}");
    let response = agent(&url)
        .get(&url)
        .set("Cookie", &jar.to_header())
        .set("Accept", "application/json")
        .call()
        .map_err(|exc| format!("请求 {path} 失败：{exc}"))?;
    let text = response
        .into_string()
        .map_err(|exc| format!("{path} 内容读不出来：{exc}"))?;
    serde_json::from_str(&text).map_err(|_| {
        let head: String = text.chars().take(160).collect();
        format!("{path} 返回的不是 JSON（可能没登录）：{head}")
    })
}

pub fn courses(store: &Store, keyword: &str) -> Result<Vec<Value>, String> {
    let jar = ensure_session(store)?;
    let value = get_json(&jar, COURSES)?;
    let rows = value.as_array().cloned().unwrap_or_default();
    let filtered: Vec<Value> = rows
        .into_iter()
        .filter(|one| {
            keyword.is_empty()
                || one
                    .get("name")
                    .and_then(|value| value.as_str())
                    .map(|name| name.contains(keyword))
                    .unwrap_or(false)
        })
        .map(|one| {
            json!({
                "id": one.get("id").cloned().unwrap_or(json!(0)),
                "name": one.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                "course_code": one.get("course_code").and_then(|v| v.as_str()).unwrap_or(""),
            })
        })
        .collect();
    Ok(filtered)
}

pub fn assignments(store: &Store, course_id: &str) -> Result<Vec<Value>, String> {
    let jar = ensure_session(store)?;
    let value = get_json(&jar, &ASSIGNMENTS.replace("{cid}", course_id))?;
    Ok(value.as_array().cloned().unwrap_or_default())
}

pub fn files(store: &Store, course_id: &str) -> Result<Vec<Value>, String> {
    let jar = ensure_session(store)?;
    let value = get_json(&jar, &FILES.replace("{cid}", course_id))?;
    Ok(value.as_array().cloned().unwrap_or_default())
}

/// To Do 里各种 type 的中文叫法（与旧版 `elearning.TODO_KINDS` 一致）
fn todo_kind(kind: &str) -> &'static str {
    match kind.trim() {
        "submitting" => "要交",
        "grading" | "not_graded" => "待评分",
        "peer_review" => "互评",
        _ => "作业",
    }
}

/// UTC 的 `due_at` → 本地 `YYYY-MM-DD HH:MM`（旧版 `utils.local_datetime`）
fn local_due(value: &str) -> String {
    let text = value.trim();
    if text.is_empty() {
        return String::new();
    }
    if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(text) {
        return parsed
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string();
    }
    text.replace('T', " ").chars().take(16).collect()
}

/// 把 `/users/self/todo` 的返回整理成「最近作业」的行
/// （与旧版 `elearning.todo_rows` 逐字段对齐：title / course / due / due_at / url / kind）。
pub fn todo_rows(data: &Value) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut seen: Vec<(String, String)> = Vec::new();
    let Some(items) = data.as_array() else {
        return out;
    };
    for item in items {
        let body = item
            .get("assignment")
            .or_else(|| item.get("quiz"))
            .cloned()
            .unwrap_or_else(|| json!({}));
        let title = body
            .get("name")
            .or_else(|| body.get("title"))
            .or_else(|| item.get("name"))
            .and_then(|one| one.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if title.is_empty() {
            continue;
        }
        let course = item
            .get("context_name")
            .or_else(|| item.get("course_name"))
            .and_then(|one| one.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let key = (title.clone(), course.clone());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        let due_at = body
            .get("due_at")
            .or_else(|| item.get("due_at"))
            .and_then(|one| one.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        out.push(json!({
            "title": title,
            "course": course,
            "due": local_due(&due_at),
            "due_at": due_at,
            "url": item
                .get("html_url")
                .or_else(|| body.get("html_url"))
                .and_then(|one| one.as_str())
                .unwrap_or(""),
            "kind": todo_kind(item.get("type").and_then(|one| one.as_str()).unwrap_or("")),
        }));
    }
    out.sort_by(|a, b| {
        let left = a.get("due").and_then(|one| one.as_str()).unwrap_or("9999-99-99");
        let right = b.get("due").and_then(|one| one.as_str()).unwrap_or("9999-99-99");
        left.cmp(right).then_with(|| {
            let l = a.get("title").and_then(|one| one.as_str()).unwrap_or("");
            let r = b.get("title").and_then(|one| one.as_str()).unwrap_or("");
            l.cmp(r)
        })
    });
    out
}

/// 拉一次 eLearning 的 To Do（最近作业）。旧版 `elearning.todo_items` 的等价。
pub fn todo_items(store: &Store) -> Result<Vec<Value>, String> {
    let jar = ensure_session(store)?;
    let value = get_json(&jar, TODO)?;
    Ok(todo_rows(&value))
}

/// 诊断：课程接口通不通、返回几条，并把课程清单记下来给「下载课程文件」选课用。
///
/// 默认课程不再死取第一门：有的课（实测 119758「2026本科生入学教育考试」）文件接口直接
/// 回 401「用户无权执行该操作」，拿它当默认会让「下载课程文件」每次都失败。
/// 所以这里往后找一门真能列出文件的课，并记进 `canvas_last_course`。
pub fn probe(store: &Store) -> Value {
    match courses(store, "") {
        Ok(rows) => {
            let list: Vec<Value> = rows
                .iter()
                .map(|one| {
                    json!({
                        "id": json_id_text(one.get("id")),
                        "name": one.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    })
                })
                .collect();
            if let Ok(text) = serde_json::to_string(&list) {
                let _ = store.set_setting("canvas_course_list", &text);
            }
            let mut chosen: Option<(String, String, usize)> = None;
            for one in list.iter().take(8) {
                let id = one.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let name = one.get("name").and_then(|v| v.as_str()).unwrap_or("");
                if id.is_empty() {
                    continue;
                }
                match files(store, id) {
                    Ok(file_rows) if !file_rows.is_empty() => {
                        chosen = Some((id.to_string(), name.to_string(), file_rows.len()));
                        break;
                    }
                    Ok(_) => {}
                    Err(exc) => crate::uis_log(&format!("[canvas] 课程 {id} 的文件读不到：{exc}")),
                }
            }
            let current = match &chosen {
                Some((id, name, count)) => {
                    let _ = store.set_setting("canvas_last_course", id);
                    let _ = store.set_setting("canvas_last_course_name", name);
                    crate::uis_log(&format!("[canvas] 默认课程选了 {name}（{count} 个文件）"));
                    json!({ "id": id, "name": name, "files": count })
                }
                None => {
                    // 一门都列不出文件：还是记下第一门，界面上至少能选
                    if let Some(first) = list.first() {
                        if let Some(id) = first.get("id").and_then(|v| v.as_str()) {
                            let _ = store.set_setting("canvas_last_course", id);
                        }
                        if let Some(name) = first.get("name").and_then(|v| v.as_str()) {
                            let _ = store.set_setting("canvas_last_course_name", name);
                        }
                    }
                    json!(null)
                }
            };
            json!({
                "ok": true,
                "courses": list.len(),
                "names": list.iter().take(5).map(|one| {
                    one.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string()
                }).collect::<Vec<_>>(),
                "list": list,
                "current": current,
            })
        }
        Err(exc) => json!({ "ok": false, "error": exc }),
    }
}

/// 把课程名压成可以当目录名的字符串：
/// 非法字符（`\ / : * ? " < > |`）换成 `-`，控制字符换成空格，
/// 去掉首尾的空白和点，超长截断到 60 个字符。
///
/// 纯函数：只做字符串处理，可能返回空串（交给 `course_dir_name` 兜底）。
fn sanitize_dir_name(name: &str) -> String {
    let replaced: String = name
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .map(|ch| if "\\/:*?\"<>|".contains(ch) { '-' } else { ch })
        .collect();
    // 先收首尾空白，再去掉首尾的点，最后再收一次空白（` . 线性代数 . ` → `线性代数`）
    let trimmed = replaced.trim().trim_matches('.').trim();
    let cut: String = trimmed.chars().take(60).collect();
    cut.trim().trim_matches('.').trim().to_string()
}

/// 课程名 → 课程目录名：安全化之后为空就退回 `course_<课程号>`
fn course_dir_name(name: &str, course_id: &str) -> String {
    let safe = sanitize_dir_name(name);
    if !safe.is_empty() {
        return safe;
    }
    let id = sanitize_dir_name(course_id);
    if id.is_empty() {
        "course".to_string()
    } else {
        format!("course_{id}")
    }
}

/// 文件名清理（下载与迁移共用）：非法字符换成 `-`，其余原样保留
fn sanitize_file_name(name: &str) -> String {
    name.chars()
        .map(|ch| if "\\/:*?\"<>|".contains(ch) { '-' } else { ch })
        .collect::<String>()
        .trim()
        .to_string()
}

/// 下载基准目录：`dir_downloads` 为空时用 `data_dir()/elearning`，相对路径补成绝对路径
fn downloads_root(store: &Store) -> PathBuf {
    let text = store.setting("dir_downloads").unwrap_or_default();
    let base = if text.trim().is_empty() {
        crate::paths::data_dir().join("elearning")
    } else {
        PathBuf::from(text.trim())
    };
    if base.is_absolute() {
        base
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(&base))
            .unwrap_or(base)
    }
}

/// 课程 id 在 Canvas JSON 里可能是数字也可能是字符串，统一成字符串再比
fn json_id_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.trim().to_string(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

/// 课程名解析：
/// a) 先问一次课程列表；b) 拿不到就用上次「诊断 Canvas」记下的课；
/// c) 都不行返回空串，由 `course_dir_name` 退回 `course_<id>`。
fn resolve_course_name(store: &Store, course_id: &str) -> String {
    let wanted = course_id.trim();
    if let Ok(rows) = courses(store, "") {
        if let Some(one) = rows.iter().find(|one| json_id_text(one.get("id")) == wanted) {
            let name = one.get("name").and_then(|value| value.as_str()).unwrap_or("");
            if !name.trim().is_empty() {
                return name.to_string();
            }
        }
    }
    // 远程列表拿不到（没登录 / 断网）时，用上次诊断记下的那门课
    if store
        .setting("canvas_last_course")
        .unwrap_or_default()
        .trim()
        == wanted
    {
        let name = store.setting("canvas_last_course_name").unwrap_or_default();
        if !name.trim().is_empty() {
            return name;
        }
    }
    String::new()
}

/// 下载某门课的文件到 `<下载目录>/<课程名>/`（重名跳过），返回 saved/skipped/failed
pub fn download_course(store: &Store, course_id: &str, limit: usize) -> Value {
    let jar = match ensure_session(store) {
        Ok(jar) => jar,
        Err(exc) => return json!({ "ok": false, "error": exc }),
    };
    let base = downloads_root(store);
    // 每门课一个子目录：<base>/<课程名>/<文件名>
    let course_name = resolve_course_name(store, course_id);
    let folder = course_dir_name(&course_name, course_id);
    // 解析不到课程名时，返回值里的 course 就用兜底的目录名（course_<id>）
    let label = if course_name.trim().is_empty() {
        folder.clone()
    } else {
        course_name
    };
    let target = base.join(&folder);
    // 先确认这门课的文件能列出来再建目录：否则读不到文件时会在下载目录里留一个空文件夹
    let list = match files(store, course_id) {
        Ok(rows) => rows,
        // 401「用户无权执行该操作」= 这门课不开放文件（实测入学教育考试那门就是这样），
        // 换门课就行，不要说成「没登录」
        Err(exc) => {
            return json!({
                "ok": false,
                "error": format!("这门课的文件读不到（{exc}）；换一门课再试"),
            })
        }
    };
    if list.is_empty() {
        return json!({ "ok": false, "error": "这门课没有可下载的文件（换一门课再试）" });
    }
    let _ = std::fs::create_dir_all(&target);
    let mut saved: Vec<String> = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    let mut skipped = 0usize;
    for file in list.into_iter().take(limit.max(1)) {
        let name = file
            .get("display_name")
            .and_then(|v| v.as_str())
            .or_else(|| file.get("filename").and_then(|v| v.as_str()))
            .unwrap_or("file");
        let url = file
            .get("url")
            .and_then(|v| v.as_str())
            .or_else(|| file.get("preview_url").and_then(|v| v.as_str()))
            .unwrap_or("");
        if url.is_empty() {
            failed.push(name.to_string());
            continue;
        }
        let clean = sanitize_file_name(name);
        let clean = if clean.is_empty() { "file".to_string() } else { clean };
        let path = target.join(&clean);
        if path.exists() {
            skipped += 1;
            continue;
        }
        let response = agent(url).get(url).set("Cookie", &jar.to_header()).call();
        match response {
            Ok(res) => {
                let mut bytes: Vec<u8> = Vec::new();
                use std::io::Read;
                if res.into_reader().read_to_end(&mut bytes).is_ok()
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
    json!({
        "ok": true,
        "course": label,
        "saved": saved.len(),
        "skipped": skipped,
        "failed": failed.len(),
        "files": saved,
        "target": target.display().to_string(),
    })
}

/// 「读取我的课程」时缓存的官方文件清单：`{"安全文件名": "课程名"}`。
/// 这个键由 Python 版写（`elearning.save_remote_file_map`），Rust 也认它，
/// 这样两个版本共用一份缓存——不用每次都重新联网拉 Canvas 清单。
fn remote_file_map(json_text: &str) -> HashMap<String, String> {
    let mut out: HashMap<String, String> = HashMap::new();
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(json_text) else {
        return out;
    };
    for (name, course) in map {
        let name = sanitize_file_name(name.trim());
        let course = course.as_str().unwrap_or("").trim().to_string();
        if !name.is_empty() && !course.is_empty() {
            out.insert(name, course);
        }
    }
    out
}

/// 旧版文件索引 `elearning_file_index` → 文件名 → 课程名。
///
/// 只收「平铺」条目：`name` 里没有 `/`（有斜杠说明已经在课程子目录里），
/// `course` 非空。解析不了就当没有索引。
fn index_courses(index_json: &str) -> HashMap<String, String> {
    let mut out: HashMap<String, String> = HashMap::new();
    let Ok(value) = serde_json::from_str::<Value>(index_json) else {
        return out;
    };
    let Some(items) = value.as_array() else {
        return out;
    };
    for item in items {
        let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("").trim();
        let course = item.get("course").and_then(|v| v.as_str()).unwrap_or("").trim();
        if name.is_empty() || course.is_empty() || name.contains('/') {
            continue;
        }
        out.entry(name.to_string()).or_insert_with(|| course.to_string());
    }
    out
}

/// 规划一个文件的落点：
/// - 目标不存在 → `Some(<课程目录>/<文件名>)`
/// - 目标已存在且大小相同 → `None`（同一份，去重时删掉根目录那份）
/// - 目标已存在但大小不同 → `Some(<课程目录>/<主名> (2).<ext>)`，(2) 也占了就 (3)、(4)…
fn plan_move(from: &Path, target_dir: &Path, name: &str) -> Option<PathBuf> {
    let target = target_dir.join(name);
    if !target.exists() {
        return Some(target);
    }
    // 大小一致就当成同一份；读不到大小时宁可改名也不删，别丢文件
    let same_size = match (std::fs::metadata(from), std::fs::metadata(&target)) {
        (Ok(left), Ok(right)) => left.len() == right.len(),
        _ => false,
    };
    if same_size {
        return None;
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .and_then(|one| one.to_str())
        .unwrap_or(name);
    let ext = path.extension().and_then(|one| one.to_str()).unwrap_or("");
    for index in 2..1000 {
        let candidate = if ext.is_empty() {
            format!("{stem} ({index})")
        } else {
            format!("{stem} ({index}).{ext}")
        };
        let candidate = target_dir.join(candidate);
        if !candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

/// 把文件挪走：先试 rename，跨盘失败就复制 + 删源；删不掉源文件也不丢数据
fn move_file(from: &Path, to: &Path) -> Result<(), String> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to).map_err(|exc| format!("复制失败：{exc}"))?;
    std::fs::remove_file(from).map_err(|exc| format!("复制成功但删不掉原文件：{exc}"))
}

/// 一次迁移的结果（交给 `migrate_downloads` 拼 JSON / 回写索引）
#[derive(Default)]
struct MoveOutcome {
    moved: Vec<(String, String)>,
    deduped: Vec<String>,
    unresolved: Vec<String>,
    failed: Vec<(String, String)>,
    /// 旧平铺文件名 → 新的「课程目录/文件名」，用于回写索引
    renamed: HashMap<String, String>,
}

/// 纯文件操作：把 `base` 下直接放着的文件按索引（+ 可选缓存 / 远程兜底）搬进课程子目录。
///
/// 认课程的顺序：`elearning_file_index` → `elearning_remote_file_map`
/// （Python 版「读取我的课程」时缓存的官方文件清单，键是安全化后的文件名）→
/// `remote_lookup`（现拉一次 Canvas 清单，没登录就不该先撞网络）。
fn migrate_files<F>(
    base: &Path,
    index_text: &str,
    cached_map: &str,
    mut remote_lookup: F,
) -> MoveOutcome
where
    F: FnMut(&str) -> Option<String>,
{
    let mut out = MoveOutcome::default();
    let index = index_courses(index_text);
    let cached = remote_file_map(cached_map);
    // 下载目录还没建（全新安装）就算没有要整理的东西
    if !base.exists() {
        return out;
    }
    let entries = match std::fs::read_dir(base) {
        Ok(rows) => rows,
        Err(exc) => {
            out.failed
                .push((base.display().to_string(), format!("读不到下载目录：{exc}")));
            return out;
        }
    };
    // 只要直接位于基准目录下的文件；子文件夹原样不动
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        match entry.file_type() {
            Ok(kind) if kind.is_file() => files.push((name, path)),
            Ok(_) => continue,
            Err(exc) => out.failed.push((name, format!("读不到文件类型：{exc}"))),
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));

    for (name, path) in files {
        let clean = sanitize_file_name(&name);
        if clean.is_empty() {
            out.failed.push((name, "文件名清理后为空".to_string()));
            continue;
        }
        // a) 先看旧索引；b) 再看「读取课程」缓存的官方清单；c) 最后才联网
        let mut course = index.get(&name).cloned();
        if course.is_none() {
            course = cached.get(&clean).cloned().or_else(|| cached.get(&name).cloned());
        }
        if course.is_none() {
            course = remote_lookup(&clean);
        }
        let course = match course {
            Some(text) if !text.trim().is_empty() => text.trim().to_string(),
            _ => {
                out.unresolved.push(name);
                continue;
            }
        };
        let folder = course_dir_name(&course, "");
        let target_dir = base.join(&folder);
        if let Err(exc) = std::fs::create_dir_all(&target_dir) {
            out.failed
                .push((name, format!("建不了课程目录：{exc}")));
            continue;
        }
        match plan_move(&path, &target_dir, &clean) {
            // 目标已有一份同样大小的：删掉根目录这份
            None => match std::fs::remove_file(&path) {
                Ok(_) => {
                    out.deduped.push(name.clone());
                    out.renamed.insert(name, format!("{folder}/{clean}"));
                }
                Err(exc) => out
                    .failed
                    .push((name, format!("删不掉重复文件：{exc}"))),
            },
            Some(target) => {
                let final_name = target
                    .file_name()
                    .map(|one| one.to_string_lossy().to_string())
                    .unwrap_or_else(|| clean.clone());
                match move_file(&path, &target) {
                    Ok(_) => {
                        out.moved.push((name.clone(), course));
                        out.renamed.insert(name, format!("{folder}/{final_name}"));
                    }
                    Err(exc) => out.failed.push((name, exc)),
                }
            }
        }
    }
    out
}

/// 回写索引：只改「刚搬走的平铺文件」对应的 `name`，其它条目原样保留。
/// 没有任何改动时返回 `None`（不必写库）。
fn rewrite_index(index_text: &str, renamed: &HashMap<String, String>) -> Option<String> {
    let mut value: Value = serde_json::from_str(index_text).ok()?;
    let items = value.as_array_mut()?;
    let mut changed = false;
    for item in items.iter_mut() {
        let name = item
            .get("name")
            .and_then(|one| one.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let Some(new_name) = renamed.get(&name) else {
            continue;
        };
        if let Some(obj) = item.as_object_mut() {
            obj.insert("name".to_string(), Value::String(new_name.clone()));
            changed = true;
        }
    }
    if !changed {
        return None;
    }
    // serde_json 默认原样输出 UTF-8，中文不会被转义成 \uXXXX
    serde_json::to_string(&value).ok()
}

/// 兜底：Canvas 每门课的文件清单 → 「清理后的文件名 → 课程名」
/// （会话不可用就返回 Err，交给调用方当成"没有远程信息"，不刷错误）
fn canvas_file_courses(store: &Store) -> Result<HashMap<String, String>, String> {
    let mut out: HashMap<String, String> = HashMap::new();
    for course in courses(store, "")? {
        let id = json_id_text(course.get("id"));
        let name = course.get("name").and_then(|one| one.as_str()).unwrap_or("");
        if id.is_empty() || name.trim().is_empty() {
            continue;
        }
        let rows = match files(store, &id) {
            Ok(rows) => rows,
            // 单门课取不到不影响别的课
            Err(_) => continue,
        };
        for file in rows {
            let file_name = file
                .get("display_name")
                .and_then(|one| one.as_str())
                .or_else(|| file.get("filename").and_then(|one| one.as_str()))
                .unwrap_or("");
            let clean = sanitize_file_name(file_name);
            if clean.is_empty() {
                continue;
            }
            out.entry(clean).or_insert_with(|| name.to_string());
        }
    }
    Ok(out)
}

/// 一次性整理：把下载根目录下平铺的旧文件搬进 `<课程目录>/`，不递归进已有子文件夹。
///
/// 归属优先级：`elearning_file_index` → Canvas 文件清单；都判断不了就留在原地并记进
/// `unresolved`。任何一步出错都只是记 `failed`，文件不会被删。
pub fn migrate_downloads(store: &Store) -> Value {
    let base = downloads_root(store);
    let index_text = store.setting("elearning_file_index").unwrap_or_default();
    // 「读取我的课程」时缓存的官方文件清单（Python 版写的，两个版本共用）
    let cached_text = store
        .setting("elearning_remote_file_map")
        .unwrap_or_default();
    // Canvas 清单很贵：第一次真正需要时才请求，失败也只记一次空表
    let mut cache: Option<HashMap<String, String>> = None;
    let mut lookup = |name: &str| -> Option<String> {
        if cache.is_none() {
            cache = Some(canvas_file_courses(store).unwrap_or_default());
        }
        cache
            .as_ref()
            .and_then(|map| map.get(name).cloned())
    };
    let outcome = migrate_files(&base, &index_text, &cached_text, &mut lookup);
    drop(lookup);
    // 现拉的官方清单顺手缓存起来（Python 版用的是同一个键），下次整理就不用再联网
    if cached_text.trim().is_empty() {
        if let Some(map) = cache.as_ref() {
            if !map.is_empty() {
                if let Ok(text) = serde_json::to_string(map) {
                    let _ = store.set_setting("elearning_remote_file_map", &text);
                }
            }
        }
    }
    // 索引回写：搬走的平铺文件改成「课程目录/文件名」
    if !outcome.renamed.is_empty() {
        if let Some(text) = rewrite_index(&index_text, &outcome.renamed) {
            let _ = store.set_setting("elearning_file_index", &text);
        }
    }
    json!({
        "ok": true,
        "moved": outcome
            .moved
            .iter()
            .map(|(name, course)| json!([name, course]))
            .collect::<Vec<_>>(),
        "deduped": outcome.deduped,
        "unresolved": outcome.unresolved,
        "failed": outcome
            .failed
            .iter()
            .map(|(name, why)| json!([name, why]))
            .collect::<Vec<_>>(),
        "root": base.display().to_string(),
    })
}

/// 下载根目录下还「没归类」的文件 + 可以归进去的课程目录。
///
/// 用户 2026-11-11 报「有些作业没有归类」：从浏览器里手动下载的那些文件（比如
/// `03《物种起源》.pdf`、Canvas 用附件 id 当文件名的 `34FAF30F@...png`）不在
/// `elearning_remote_file_map` 里，自动整理认不出来，就永远留在根目录。
/// 自动认不出的时候得让人自己指定，这个命令给出「候选文件 + 候选课程」。
pub fn unclassified(store: &Store) -> Value {
    let base = downloads_root(store);
    let mut files: Vec<Value> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&base) {
        let mut rows: Vec<(String, u64)> = Vec::new();
        for entry in entries.flatten() {
            match entry.file_type() {
                Ok(kind) if kind.is_file() => {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let size = entry.metadata().map(|one| one.len()).unwrap_or(0);
                    rows.push((name, size));
                }
                _ => continue,
            }
        }
        rows.sort_by(|left, right| left.0.cmp(&right.0));
        for (name, size) in rows {
            files.push(json!({ "name": name, "size": size }));
        }
    }
    // 候选课程目录：已经存在的子文件夹（包括没有文件在里面的）+ 官方清单里的课程名
    let mut courses: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&base) {
        for entry in entries.flatten() {
            if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                courses.push(entry.file_name().to_string_lossy().to_string());
            }
        }
    }
    let cached_text = store
        .setting("elearning_remote_file_map")
        .unwrap_or_default();
    for (_name, course) in remote_file_map(&cached_text) {
        let folder = course_dir_name(&course, "");
        if !folder.is_empty() {
            courses.push(folder);
        }
    }
    courses.sort();
    courses.dedup();
    json!({
        "ok": true,
        "root": base.display().to_string(),
        "files": files,
        "courses": courses,
    })
}

/// 把根目录里的一个文件挪进指定课程目录（用户手动归类）。
///
/// `course` 既可以是课程名（`GECC20002.01 生命进化论`），也可以是已经存在的目录名。
pub fn classify(store: &Store, path: &str, course: &str) -> Result<Value, String> {
    let base = downloads_root(store);
    let source = PathBuf::from(path);
    if !source.is_file() {
        return Err("这个文件不在了".to_string());
    }
    let name = source
        .file_name()
        .map(|one| one.to_string_lossy().to_string())
        .ok_or_else(|| "认不出文件名".to_string())?;
    let clean = sanitize_file_name(&name);
    let folder = course_dir_name(course, "");
    if folder.is_empty() || course.trim().is_empty() {
        return Err("先选一个课程目录".to_string());
    }
    let target_dir = base.join(&folder);
    std::fs::create_dir_all(&target_dir).map_err(|exc| format!("建不了课程目录：{exc}"))?;
    // 已经在目标目录里就不动了
    if let (Some(parent), true) = (source.parent(), true) {
        if let (Ok(left), Ok(right)) = (parent.canonicalize(), target_dir.canonicalize()) {
            if left == right {
                return Ok(json!({ "ok": true, "moved": false, "target": source.display().to_string() }));
            }
        }
    }
    let target = match plan_move(&source, &target_dir, &clean) {
        Some(target) => target,
        None => {
            // 目标目录里已有同名同大小的一份：删掉根目录这份
            std::fs::remove_file(&source).map_err(|exc| format!("删不掉重复文件：{exc}"))?;
            return Ok(json!({
                "ok": true,
                "moved": false,
                "deduped": true,
                "target": target_dir.join(&clean).display().to_string(),
            }));
        }
    };
    move_file(&source, &target)?;
    // 索引里如果有这个名字，跟着改路径，作业页「打开」才不会失效
    let index_text = store.setting("elearning_file_index").unwrap_or_default();
    if !index_text.trim().is_empty() {
        let mut renamed: HashMap<String, String> = HashMap::new();
        renamed.insert(name, format!("{folder}/{}", target.file_name().map(|one| one.to_string_lossy().to_string()).unwrap_or(clean.clone())));
        if let Some(text) = rewrite_index(&index_text, &renamed) {
            let _ = store.set_setting("elearning_file_index", &text);
        }
    }
    Ok(json!({
        "ok": true,
        "moved": true,
        "course": course,
        "target": target.display().to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_match_python_version() {
        assert_eq!(COURSES, "/api/v1/courses?enrollment_state=active&per_page=100");
        assert!(ASSIGNMENTS.contains("{cid}"));
        assert!(FILES.contains("sort=created_at"));
        assert_eq!(
            BASE,
            "https://elearning.fudan.edu.cn"
        );
    }

    /// 课程目录名：非法字符一律换成 `-`，中文和括号原样保留
    #[test]
    fn course_dir_name_replaces_illegal_chars() {
        assert_eq!(
            course_dir_name(r#"A/B:C*D?E"F<G>H|I\J"#, "42"),
            "A-B-C-D-E-F-G-H-I-J"
        );
        assert_eq!(course_dir_name("高等数学（上）", "42"), "高等数学（上）");
        assert_eq!(course_dir_name("Data\nStructure", "42"), "Data Structure");
    }

    /// 课程目录名：首尾空白和点会被去掉，剩不下东西就退回 course_<id>
    #[test]
    fn course_dir_name_trims_spaces_and_dots() {
        assert_eq!(course_dir_name("  . 线性代数 . ", "42"), "线性代数");
        assert_eq!(course_dir_name("...", "42"), "course_42");
        assert_eq!(course_dir_name("   ", "42"), "course_42");
        assert_eq!(course_dir_name("", "12345"), "course_12345");
        // 课程号本身也会被安全化，极端情况退回 course
        assert_eq!(course_dir_name("", "a/b"), "course_a-b");
        assert_eq!(course_dir_name("", ""), "course");
    }

    /// 课程目录名：超长截断到 60 个字符，且不会以点 / 空格结尾
    #[test]
    fn course_dir_name_truncates_long_names() {
        let long = "课".repeat(80);
        let name = course_dir_name(&long, "7");
        assert_eq!(name.chars().count(), 60);

        // 第 60 个字符正好是点，截断后必须被去掉
        let dotted = format!("{}.{}", "长".repeat(59), "长".repeat(20));
        let name = course_dir_name(&dotted, "7");
        assert_eq!(name.chars().count(), 59);
        assert!(!name.ends_with('.'));
    }

    /// 临时目录：temp_dir 下按「pid + 纳秒」建唯一目录，测完自己删
    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|one| one.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "danlume-canvas-{tag}-{}-{nanos}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }

    /// 手动归类：把下载根目录里的文件挪进指定课程目录
    #[test]
    fn classify_moves_file_into_course_folder() {
        let base = temp_dir("classify");
        std::fs::create_dir_all(base.join("生命进化论")).unwrap();
        let file = base.join("03《物种起源》.pdf");
        std::fs::write(&file, b"pdf-bytes").unwrap();

        // 直接调文件级的归类逻辑（不碰数据库）：路线与 classify() 一致
        let folder = course_dir_name("生命进化论", "");
        let target_dir = base.join(&folder);
        let target = plan_move(&file, &target_dir, &sanitize_file_name("03《物种起源》.pdf"))
            .expect("目标位置");
        move_file(&file, &target).unwrap();

        assert!(!file.exists(), "根目录那份应该没了");
        assert_eq!(
            std::fs::read(base.join("生命进化论").join("03《物种起源》.pdf")).unwrap(),
            b"pdf-bytes".to_vec()
        );
    }

    /// 索引解析：只认「平铺」（name 里没有 `/`）且 course 非空的条目
    #[test]
    fn index_courses_keeps_flat_entries_with_course() {
        let raw = r#"[
            {"course":"高等数学","assignment":"第一次作业","name":"a.pdf"},
            {"course":"高等数学","assignment":"","name":"高等数学/old.pdf"},
            {"course":"","assignment":"","name":"b.pdf"},
            {"course":"线性代数","assignment":""},
            {"course":42,"name":"c.pdf"},
            {"course":"线性代数","assignment":"","name":"z.pdf"}
        ]"#;
        let map = index_courses(raw);
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("a.pdf").map(String::as_str), Some("高等数学"));
        assert_eq!(map.get("z.pdf").map(String::as_str), Some("线性代数"));
        // 已经搬进课程目录的、课程名为空的都不要
        assert!(!map.contains_key("高等数学/old.pdf"));
        assert!(!map.contains_key("b.pdf"));
        assert!(!map.contains_key("c.pdf"));
        // 空串 / 坏 JSON / 不是数组 → 空表，不 panic
        assert!(index_courses("").is_empty());
        assert!(index_courses("{不是 JSON").is_empty());
        assert!(index_courses(r#"{"name":"a.pdf"}"#).is_empty());
    }

    /// 冲突规划：没有就搬；同大小去重；不同大小编号 (2)、(3)…
    #[test]
    fn plan_move_dedupes_or_renames() {
        let root = temp_dir("plan");
        let target = root.join("高等数学");
        std::fs::create_dir_all(&target).unwrap();

        // 目标不存在 → 直接搬过去
        std::fs::write(root.join("a.pdf"), b"aaa").unwrap();
        assert_eq!(
            plan_move(&root.join("a.pdf"), &target, "a.pdf"),
            Some(target.join("a.pdf"))
        );

        // 目标已存在且大小相同 → None（去重）
        std::fs::write(root.join("b.pdf"), b"bbb").unwrap();
        std::fs::write(target.join("b.pdf"), b"xyz").unwrap();
        assert_eq!(plan_move(&root.join("b.pdf"), &target, "b.pdf"), None);

        // 目标已存在但大小不同 → b (2).pdf
        std::fs::write(root.join("c.pdf"), b"ccccc").unwrap();
        std::fs::write(target.join("c.pdf"), b"c").unwrap();
        assert_eq!(
            plan_move(&root.join("c.pdf"), &target, "c.pdf"),
            Some(target.join("c (2).pdf"))
        );

        // (2) 也被占了 → (3)
        std::fs::write(target.join("c (2).pdf"), b"c").unwrap();
        assert_eq!(
            plan_move(&root.join("c.pdf"), &target, "c.pdf"),
            Some(target.join("c (3).pdf"))
        );

        // 没有扩展名的文件名也能编号
        std::fs::write(root.join("README"), b"long text").unwrap();
        std::fs::write(target.join("README"), b"x").unwrap();
        assert_eq!(
            plan_move(&root.join("README"), &target, "README"),
            Some(target.join("README (2)"))
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 整个迁移流程（索引版）：a 搬走、b 去重、c 改名、d 留在原地
    #[test]
    fn migrate_files_moves_dedupes_and_reports() {
        let base = temp_dir("migrate");
        let math = base.join("高等数学");
        std::fs::create_dir_all(&math).unwrap();
        std::fs::write(base.join("a.pdf"), b"aaa").unwrap();
        std::fs::write(base.join("b.pdf"), b"bbb").unwrap();
        std::fs::write(math.join("b.pdf"), b"bbb").unwrap();
        std::fs::write(base.join("c.pdf"), b"ccccc").unwrap();
        std::fs::write(math.join("c.pdf"), b"c").unwrap();
        std::fs::write(base.join("d.pdf"), b"ddd").unwrap();

        let index = r#"[
            {"course":"高等数学","assignment":"第一次作业","name":"a.pdf"},
            {"course":"高等数学","assignment":"第一次作业","name":"b.pdf"},
            {"course":"高等数学","assignment":"第一次作业","name":"c.pdf"}
        ]"#;
        // 不联网：缓存为空、兜底查询直接返回 None
        let outcome = migrate_files(&base, index, "", |_name: &str| -> Option<String> { None });

        assert!(outcome.failed.is_empty(), "不该有失败：{:?}", outcome.failed);
        assert_eq!(outcome.moved.len(), 2);
        assert!(outcome
            .moved
            .iter()
            .any(|(name, course)| name == "a.pdf" && course == "高等数学"));
        assert!(outcome
            .moved
            .iter()
            .any(|(name, course)| name == "c.pdf" && course == "高等数学"));
        assert_eq!(outcome.deduped, vec!["b.pdf".to_string()]);
        assert_eq!(outcome.unresolved, vec!["d.pdf".to_string()]);

        // 文件真的动了，而且内容没变
        assert!(!base.join("a.pdf").exists());
        assert_eq!(std::fs::read(math.join("a.pdf")).unwrap(), b"aaa".to_vec());
        assert!(!base.join("b.pdf").exists());
        assert!(!base.join("c.pdf").exists());
        assert_eq!(
            std::fs::read(math.join("c (2).pdf")).unwrap(),
            b"ccccc".to_vec()
        );
        // 识别不出来的留在根目录
        assert!(base.join("d.pdf").exists());

        // 回写用的相对路径
        assert_eq!(
            outcome.renamed.get("a.pdf").map(String::as_str),
            Some("高等数学/a.pdf")
        );
        assert_eq!(
            outcome.renamed.get("c.pdf").map(String::as_str),
            Some("高等数学/c (2).pdf")
        );
        assert!(!outcome.renamed.contains_key("d.pdf"));

        let _ = std::fs::remove_dir_all(&base);
    }

    /// 「读取我的课程」缓存的官方清单：没有本地索引、也不联网，也能归类
    #[test]
    fn cached_remote_map_works_offline() {
        let base = temp_dir("cached-map");
        std::fs::write(base.join("3-Matrix.pdf"), b"m").unwrap();
        std::fs::write(base.join("谁也认不出来.pdf"), b"x").unwrap();
        let cached = r#"{"3-Matrix.pdf": "线性代数A"}"#;
        let outcome =
            migrate_files(&base, "", cached, |_name: &str| -> Option<String> { None });
        println!("缓存归类结果：moved={:?} unresolved={:?}", outcome.moved, outcome.unresolved);
        assert_eq!(outcome.moved.len(), 1);
        assert_eq!(outcome.unresolved, vec!["谁也认不出来.pdf".to_string()]);
        assert!(base.join("线性代数A").join("3-Matrix.pdf").is_file());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 索引回写：只动搬走的条目，别的原样保留、中文不转义
    #[test]
    fn rewrite_index_updates_only_moved_entries() {
        let raw = r#"[
            {"course":"高等数学","assignment":"第一次作业","name":"a.pdf"},
            {"course":"高等数学","assignment":"第一次作业","name":"高等数学/old.pdf"},
            {"course":"线性代数","assignment":"","name":"z.pdf"}
        ]"#;
        let mut renamed: HashMap<String, String> = HashMap::new();
        renamed.insert("a.pdf".to_string(), "高等数学/a.pdf".to_string());
        let text = rewrite_index(raw, &renamed).expect("有改动就该写回");
        assert!(text.contains("高等数学/a.pdf"));
        // 其它条目不变
        assert!(text.contains("高等数学/old.pdf"));
        assert!(text.contains("z.pdf"));
        assert!(text.contains("第一次作业"));
        // 中文保持原样，没被转义
        assert!(!text.contains("\\u"));
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed.as_array().map(|one| one.len()), Some(3));

        // 没命中任何条目 → None（不写库）
        let mut none: HashMap<String, String> = HashMap::new();
        none.insert("nope.pdf".to_string(), "x/nope.pdf".to_string());
        assert!(rewrite_index(raw, &none).is_none());
        assert!(rewrite_index("{坏 JSON", &renamed).is_none());
    }

    /// 真联网：cargo test --lib -- --ignored --nocapture canvas_real_probe
    #[test]
    #[ignore]
    fn canvas_real_probe() {
        let store = crate::testdata::real_store("canvas");
        println!("Canvas 诊断：{}", probe(&store));
    }
}
