//! AI 工具层：完全对照旧版 `_lumina/lumina/ai_tools.py` 的 41 个工具。
//!
//! 三层：
//! 1. `catalog()`：工具清单（名字 / 中文名 / 说明 / 参数 schema / 风险档 / 默认开关），
//!    与旧版 `_SPECS` 逐条对应；
//! 2. `definitions()`：按 `ai_tools_enabled`（全局）与 `tool_on_<名字>`（单个）过滤后，
//!    给模型看的 OpenAI tools 数组；
//! 3. `dispatch()`：真正执行，返回 `{"ok": bool, "summary": "一句话结果", …}`（与旧版一致）。
use serde_json::{json, Value};

use crate::store::Store;

/// 旧版 `MAX_TOOL_ROUNDS = 6` 已经**不再作为上限**（用户要求每轮对话不限制工具调用）。
/// 这里只留一个极端保护值：正常对话的循环会在「模型不再返回 tool_calls」时自然结束。
pub const MAX_TOOL_ROUNDS: usize = 1000;
/// 全局总开关（与旧版同名）
pub const TOOLS_ENABLED_KEY: &str = "ai_tools_enabled";

/// 每次执行前都要用户点头的工具（旧版是弹窗确认；v2 由模型先问、带 confirm=true 再执行）
pub const NEEDS_CONFIRM: [&str; 7] = [
    "mail_send",
    "setting_set",
    "zip_extract",
    "fs_write",
    "fs_discard",
    "pdf_download",
    "endnote_export",
];

fn p(kind: &str, desc: &str) -> Value {
    json!({ "type": kind, "description": desc })
}

fn spec(name: &str, label: &str, desc: &str, risk: &str, on: bool,
        props: Value, required: &[&str]) -> Value {
    let mut props = props;
    if NEEDS_CONFIRM.contains(&name) {
        if let Some(map) = props.as_object_mut() {
            map.insert(
                "confirm".to_string(),
                p("boolean", "只有在你已明确同意之后，才置 true 再调用一次"),
            );
        }
    }
    json!({
        "name": name,
        "label": label,
        "description": desc,
        "risk": risk,
        "default": on,
        "parameters": { "type": "object", "properties": props, "required": required },
    })
}

/// 41 个工具的清单（顺序、名字、风险档、默认值与旧版一致）
pub fn catalog() -> Vec<Value> {
    let date = || p("string", "日期，格式 YYYY-MM-DD（例如 2026-10-24）");
    let time = || p("string", "时间，24 小时制 HH:MM（例如 09:30）");
    vec![
        // ---------------- 日程与专注 ----------------
        spec("schedule_list", "查日程",
             "查看某个时间段内的日程（默认从今天起 7 天）。回答「我明天有什么安排」这类问题前先调用它。",
             "low", true,
             json!({
                 "date_from": p("string", "开始日期 YYYY-MM-DD，默认今天"),
                 "date_to": p("string", "结束日期 YYYY-MM-DD（含），默认开始日期 +7 天"),
                 "keyword": p("string", "只看标题/备注里包含这个词的日程，可留空"),
             }), &[]),
        spec("schedule_add", "加日程",
             "在日历里新建一条日程。用户说「明天下午三点开组会」这种就调用它。",
             "low", true,
             json!({
                 "title": p("string", "日程标题"),
                 "date": date(),
                 "start_time": time(),
                 "end_time": time(),
                 "description": p("string", "备注，可留空"),
                 "color": p("string", "颜色：violet/teal/amber/rose/sky，默认 violet"),
                 "remind_minutes": p("integer", "提前多少分钟提醒；0=开始时，不填=不提醒"),
             }), &["title", "date"]),
        spec("schedule_update", "改日程",
             "修改一条已有日程（只传要改的字段）。先用 schedule_list 拿到 id。",
             "low", true,
             json!({
                 "event_id": p("integer", "日程 id（schedule_list 返回的 id）"),
                 "title": p("string", "标题"),
                 "date": date(),
                 "start_time": time(),
                 "end_time": time(),
                 "description": p("string", "备注"),
                 "color": p("string", "颜色：violet/teal/amber/rose/sky"),
                 "remind_minutes": p("integer", "提前多少分钟提醒"),
                 "done": p("boolean", "是否标记为已完成"),
             }), &["event_id"]),
        spec("schedule_delete", "删日程",
             "删除一条日程（用户明确要求删掉时才用）。", "mid", true,
             json!({ "event_id": p("integer", "日程 id") }), &["event_id"]),
        spec("countdown_add", "加倒计时",
             "新建重要日倒计时（考试 / 截止日）。", "low", true,
             json!({
                 "title": p("string", "倒计时标题"),
                 "day": date(),
                 "note": p("string", "备注，可留空"),
             }), &["title", "day"]),
        spec("pomodoro_start", "开始专注",
             "启动应用里的专注计时器。", "low", true,
             json!({
                 "minutes": p("integer", "专注多少分钟，默认 25"),
                 "label": p("string", "这一轮专注做什么，可留空"),
             }), &[]),
        // ---------------- 课程与教务 ----------------
        spec("elearning_courses", "读 eLearning",
             "读取 eLearning（Canvas）里的课程、作业和截止时间。需要之前在「作业」页登录过一次。",
             "low", true,
             json!({ "keyword": p("string", "只看课程名或作业名里包含这个词的，可留空") }), &[]),
        spec("elearning_download", "下载课程文件",
             "把 eLearning 的课程文件与作业附件下载到本机下载目录（重名会跳过）。", "mid", true,
             json!({
                 "keyword": p("string", "只下载课程名包含这个词的文件，可留空=全部"),
                 "limit": p("integer", "最多下载几个文件，默认 40"),
             }), &[]),
        spec("timetable_fetch", "抓课表",
             "把课表写进日历：给 .ics 文件路径就导入（旦夕 / 教务都能导出 .ics，重复的课会跳过）。",
             "mid", true,
             json!({
                 "semester": p("string", "学期编号，一般留空，程序会自己找"),
                 "term_start": p("string", "学期第一周周一 YYYY-MM-DD，留空则按文件里的日期算"),
                 "ics_path": p("string", "导出的 .ics 文件路径（v2 走这条通路）"),
             }), &[]),
        // ---------------- 本地文件 ----------------
        spec("fs_list", "看本地目录",
             "列出本机某个目录里的文件（只能在你允许的目录里）。", "low", true,
             json!({
                 "path": p("string", "目录，留空=下载目录"),
                 "limit": p("integer", "最多列多少项，默认 400"),
             }), &[]),
        spec("fs_read", "读本地文件",
             "读取本机上的文本文件内容（只能读你允许的目录）。", "low", true,
             json!({
                 "path": p("string", "文件路径（必填）"),
                 "limit": p("integer", "最多读多少字节，默认按设置"),
             }), &["path"]),
        spec("fs_search", "搜本地文件",
             "在你允许的目录里，按文件内容搜索关键词，返回命中的文件与上下文片段。",
             "low", true,
             json!({
                 "keyword": p("string", "要找的关键词"),
                 "path": p("string", "从哪个目录开始找，留空=下载目录"),
                 "limit": p("integer", "最多返回几条，默认 50"),
             }), &["keyword"]),
        spec("fs_write", "写本地文件",
             "把文本写入本机文件（只有开了这个工具才可用）。用户明确要求保存/新建文件时才调用。",
             "mid", false,
             json!({
                 "path": p("string", "文件路径（必填）"),
                 "content": p("string", "要写入的文本"),
                 "append": p("boolean", "true=追加到末尾，默认覆盖"),
             }), &["path", "content"]),
        spec("fs_discard", "丢弃文件（改名）",
             "把文件「丢弃」：不会真的删除，而是把文件名改成 `[discarded] 原名`。", "mid", false,
             json!({ "path": p("string", "要丢弃的文件路径（必填）") }), &["path"]),
        // ---------------- 只读信息 ----------------
        spec("time_now", "看时间", "返回精确日期/时间/星期/时区（只读）。", "low", true, json!({}), &[]),
        spec("sys_info", "看电脑状况",
             "CPU 核数 / 系统版本 / 本机名 / 数据目录 / 数据库大小 / 本机 IP（只读）。",
             "low", true, json!({}), &[]),
        spec("app_state", "看应用状态",
             "今天日程、未读通知、邮件数、备忘数（只读）。", "low", true, json!({}), &[]),
        spec("memo_search", "搜备忘",
             "在本地备忘里搜关键词（只读）。", "low", true,
             json!({
                 "keyword": p("string", "关键词"),
                 "limit": p("integer", "最多返回几条，默认 10"),
             }), &["keyword"]),
        spec("memo_append", "记到备忘",
             "往备忘里追加一段（没有同名就新建），只写应用内数据。", "low", true,
             json!({
                 "title": p("string", "备忘标题"),
                 "text": p("string", "要记的内容（必填）"),
             }), &["text"]),
        spec("usage_stats", "看用量",
             "本机 API 调用次数 / 会话数 / 工具开关状态（只读）。", "low", true, json!({}), &[]),
        spec("tool_help", "查可用工具",
             "列出现在能用哪些工具（只读）。", "low", true, json!({}), &[]),
        spec("window_list", "看窗口标题",
             "列出当前可见窗口标题（暴露你开着什么，默认关）。", "mid", false,
             json!({ "limit": p("integer", "最多列几个，默认 20") }), &[]),
        spec("flashcards_create", "生成复习卡片",
             "把知识点做成复习卡片（存进备忘，默认关）。", "mid", false,
             json!({
                 "cards": p("array", "[{front, back}] 卡片数组"),
                 "source": p("string", "来源说明，可留空"),
             }), &["cards"]),
        // ---------------- 网络与资料 ----------------
        spec("web_search", "上网搜索",
             "通用网页搜索（DuckDuckGo，失败时用 Bing）。", "low", true,
             json!({
                 "query": p("string", "搜索词"),
                 "limit": p("integer", "返回几条，默认 5"),
             }), &["query"]),
        spec("web_fetch", "读网页",
             "抓网页正文转纯文本（限长）。", "low", true,
             json!({
                 "url": p("string", "网页地址"),
                 "limit": p("integer", "最多多少字符，默认 4000"),
             }), &["url"]),
        spec("video_search", "找课程视频",
             "找 B 站 / MOOC 上的课程视频（用网页搜索实现）。", "low", true,
             json!({
                 "query": p("string", "要找的课程名或知识点"),
                 "limit": p("integer", "返回几条，默认 5"),
             }), &["query"]),
        spec("course_resource_search", "找课件资料",
             "找讲义 / 课件 / 习题：先翻本地课程文件，再上网搜。", "low", true,
             json!({
                 "keyword": p("string", "关键词"),
                 "limit": p("integer", "返回几条，默认 5"),
             }), &["keyword"]),
        spec("resource_push", "推送资源",
             "把资源存进「学习资源」备忘里，方便以后找。", "low", true,
             json!({
                 "items": p("array", "[{title, url, note}] 资源数组"),
                 "kind": p("string", "类型：video/pdf/site，可留空"),
             }), &["items"]),
        // ---------------- 文献与引用 ----------------
        spec("arxiv_search", "搜 arXiv", "arXiv 检索（不用 Key）。", "low", true,
             json!({
                 "query": p("string", "检索词"),
                 "limit": p("integer", "返回几条，默认 5"),
                 "project": p("string", "存进哪个项目，可留空"),
             }), &["query"]),
        spec("crossref_search", "搜 Crossref",
             "Crossref 检索（DOI / 期刊 / 出版社）。", "low", true,
             json!({
                 "query": p("string", "检索词"),
                 "limit": p("integer", "返回几条，默认 5"),
                 "project": p("string", "存进哪个项目，可留空"),
             }), &["query"]),
        spec("semantic_scholar_search", "搜 Semantic Scholar",
             "带引用数与摘要的检索。", "low", true,
             json!({
                 "query": p("string", "检索词"),
                 "limit": p("integer", "返回几条，默认 5"),
                 "project": p("string", "存进哪个项目，可留空"),
             }), &["query"]),
        spec("doi_lookup", "按 DOI 查", "DOI 精确查元数据 + BibTeX。", "low", true,
             json!({
                 "doi": p("string", "形如 10.1145/xxxx 的 DOI"),
                 "project": p("string", "存进哪个项目，可留空"),
             }), &["doi"]),
        spec("literature_search", "搜文献",
             "arXiv + Crossref + Semantic Scholar 一起搜并按标题合并去重。", "low", true,
             json!({
                 "query": p("string", "检索词"),
                 "limit": p("integer", "每个源几条，默认 4"),
                 "project": p("string", "存进哪个项目，可留空"),
             }), &["query"]),
        spec("pdf_download", "下载文献 PDF",
             "把 URL 指向的 PDF 下载到「我的文献库」目录（每次都要你确认）。", "mid", true,
             json!({
                 "ids": p("array", "文献 id 数组（旧版参数名，仍兼容）"),
                 "url": p("string", "直接给 PDF 地址（优先）"),
                 "title": p("string", "保存用的文件名，可留空"),
             }), &["ids"]),
        spec("citation_format", "生成引用",
             "GB/T 7714 / APA / BibTeX：按文献 id 从「我的文献库」生成。", "low", true,
             json!({
                 "ids": p("array", "文献 id 数组"),
                 "style": p("string", "gb7714 / apa / bibtex，默认 gb7714"),
             }), &["ids"]),
        spec("endnote_export", "导出给 EndNote",
             "把文献导出成 .enw / .ris 文件。", "low", true,
             json!({
                 "ids": p("array", "文献 id 数组"),
                 "style": p("string", "enw / ris，默认 enw"),
             }), &["ids"]),
        spec("library_open", "打开数据库",
             "给出图书馆 / IEEE / ScienceDirect 的搜索地址（验证交给你）。", "low", true,
             json!({
                 "query": p("string", "检索词"),
                 "which": p("string", "library / ieee / sciencedirect，默认 library"),
             }), &[]),
        // ---------------- 邮件与设置 ----------------
        spec("mail_send", "发邮件",
             "代发邮件（高风险，默认关）：需要你明确给了 to/subject/text 才会发。", "high", false,
             json!({
                 "to": p("string", "收件人邮箱（必填）"),
                 "subject": p("string", "主题"),
                 "text": p("string", "正文纯文本"),
             }), &["to"]),
        spec("setting_get", "读设置",
             "读白名单里允许修改的设置（只读）。", "low", true,
             json!({ "key": p("string", "设置键，留空=列全部白名单键") }), &[]),
        spec("setting_set", "改设置",
             "改白名单设置（高风险，默认关；密码/账号一律不给改）。", "high", false,
             json!({
                 "key": p("string", "设置键（必填）"),
                 "value": p("string", "新值（必填）"),
             }), &["key", "value"]),
        spec("zip_extract", "解压压缩包",
             "解压到指定目录（防路径穿越，默认关）。", "high", false,
             json!({
                 "archive": p("string", "压缩包路径（必填）"),
                 "dest": p("string", "解压到哪，默认同名目录"),
             }), &["archive"]),
    ]
}

fn spec_of(name: &str) -> Option<Value> {
    catalog().into_iter().find(|one| one["name"] == name)
}

fn default_on(name: &str) -> bool {
    spec_of(name)
        .and_then(|one| one.get("default").and_then(|v| v.as_bool()))
        .unwrap_or(true)
}

fn risk_of(name: &str) -> String {
    spec_of(name)
        .and_then(|one| one.get("risk").and_then(|v| v.as_str()).map(String::from))
        .unwrap_or_else(|| "low".to_string())
}

/// 全局总开关（旧版语义：没设过就是开）
pub fn globally_on(store: &Store) -> bool {
    store
        .setting(TOOLS_ENABLED_KEY)
        .map(|text| text.trim() != "0")
        .unwrap_or(true)
}

/// 单个工具的开关（`tool_on_<name>`，没设过取默认值）
pub fn tool_on(store: &Store, name: &str) -> bool {
    match store.setting(&format!("tool_on_{name}")) {
        Some(text) => match text.trim() {
            "1" => true,
            "0" => false,
            _ => default_on(name),
        },
        None => default_on(name),
    }
}

/// 给模型看的 tools 数组（关掉的工具根本不出现在里面，与旧版一致）
pub fn definitions(store: &Store) -> Vec<Value> {
    if !globally_on(store) {
        return Vec::new();
    }
    catalog()
        .into_iter()
        .filter(|one| {
            let name = one["name"].as_str().unwrap_or_default().to_string();
            tool_on(store, &name)
        })
        .map(|one| {
            json!({
                "type": "function",
                "function": {
                    "name": one["name"],
                    "description": one["description"],
                    "parameters": one["parameters"],
                }
            })
        })
        .collect()
}

// ------------------------------------------------------------------ 执行
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

fn arg_bool(args: &Value, key: &str, fallback: bool) -> bool {
    args.get(key).and_then(|one| one.as_bool()).unwrap_or(fallback)
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

/// 执行一个工具。返回结构固定为 `{ok, summary, …}`。
pub fn dispatch(store: &Store, name: &str, args: &Value) -> Value {
    if !globally_on(store) {
        return fail("AI 工具被全局关掉了（设置 → AI → 工具审核）");
    }
    if !tool_on(store, name) {
        return fail(format!("工具「{name}」是关着的（设置 → AI → 工具审核）"));
    }
    let args = if args.is_null() { json!({}) } else { args.clone() };
    if NEEDS_CONFIRM.contains(&name) && !arg_bool(&args, "confirm", false) {
        return json!({
            "ok": false,
            "needs_confirm": true,
            "summary": format!(
                "「{name}」会真的改动数据 / 发出去，要用户明确同意。\
                 先用一句话问清楚；用户同意后带上 confirm=true 再调用一次"
            ),
        });
    }
    let result = match name {
        // ---------- 日程 ----------
        "schedule_list" => schedule_list(store, &args),
        "schedule_add" => schedule_add(store, &args),
        "schedule_update" => schedule_update(store, &args),
        "schedule_delete" => schedule_delete(store, &args),
        "countdown_add" => countdown_add(store, &args),
        "pomodoro_start" => pomodoro_start(store, &args),
        // ---------- 信息 ----------
        "time_now" => time_now(),
        "sys_info" => sys_info(store),
        "app_state" => app_state(store),
        "memo_search" => memo_search(store, &args),
        "memo_append" => memo_append(store, &args),
        "usage_stats" => usage_stats(store),
        "tool_help" => tool_help(store),
        "window_list" => window_list(&args),
        "flashcards_create" => flashcards_create(store, &args),
        // ---------- 本地文件 ----------
        "fs_list" => fs_list(store, &args),
        "fs_read" => fs_read(store, &args),
        "fs_search" => fs_search(store, &args),
        "fs_write" => fs_write(store, &args),
        "fs_discard" => fs_discard(store, &args),
        // ---------- 网络 / 文献 ----------
        "web_search" => crate::ai_tools_net::web_search(&args),
        "web_fetch" => crate::ai_tools_net::web_fetch(&args),
        "video_search" => crate::ai_tools_net::video_search(&args),
        "course_resource_search" => crate::ai_tools_net::course_resource_search(store, &args),
        "resource_push" => crate::ai_tools_net::resource_push(store, &args),
        "arxiv_search" => crate::ai_tools_net::arxiv_search(&args),
        "crossref_search" => crate::ai_tools_net::crossref_search(&args),
        "semantic_scholar_search" => crate::ai_tools_net::semantic_scholar_search(&args),
        "doi_lookup" => crate::ai_tools_net::doi_lookup(&args),
        "literature_search" => crate::ai_tools_net::literature_search(&args),
        "pdf_download" => crate::ai_tools_net::pdf_download(store, &args),
        "citation_format" => crate::ai_tools_net::citation_format(store, &args),
        "endnote_export" => crate::ai_tools_net::endnote_export(store, &args),
        "library_open" => crate::ai_tools_net::library_open(&args),
        // ---------- 邮件 / 设置 / 压缩包 ----------
        "mail_send" => crate::ai_tools_net::mail_send(store, &args),
        "setting_get" => setting_get(store, &args),
        "setting_set" => setting_set(store, &args),
        "zip_extract" => crate::ai_tools_net::zip_extract(&args),
        // ---------- 教务 ----------
        "elearning_courses" => elearning_courses(store, &args),
        "elearning_download" => crate::ai_tools_net::elearning_download(store, &args),
        "timetable_fetch" => timetable_fetch(store, &args),
        other => fail(format!("没有这个工具：{other}")),
    };
    result
}

// ------------------------------------------------------------------ 日程
fn schedule_list(store: &Store, args: &Value) -> Value {
    let today = crate::today_str();
    let from = {
        let text = arg_str(args, "date_from");
        if text.is_empty() { today.clone() } else { text }
    };
    let to = {
        let text = arg_str(args, "date_to");
        if text.is_empty() {
            let base = chrono::NaiveDate::parse_from_str(&from, "%Y-%m-%d")
                .unwrap_or_else(|_| chrono::Local::now().date_naive());
            (base + chrono::Duration::days(7)).format("%Y-%m-%d").to_string()
        } else {
            text
        }
    };
    let keyword = arg_str(args, "keyword");
    let rows: Vec<Value> = {
        let conn = store.connection();
        let Ok(mut stmt) = conn.prepare(
            "SELECT id, title, description, date, start_time, end_time, done, color
             FROM events WHERE date >= ?1 AND date <= ?2 ORDER BY date, start_time",
        ) else {
            return fail("数据库读不了");
        };
        let mapped = stmt.query_map([&from, &to], |row| {
            Ok(json!({
                "id": row.get::<_, i64>(0)?,
                "title": row.get::<_, String>(1)?,
                "description": row.get::<_, String>(2)?,
                "date": row.get::<_, String>(3)?,
                "start_time": row.get::<_, String>(4)?,
                "end_time": row.get::<_, String>(5)?,
                "done": row.get::<_, i64>(6)? != 0,
                "color": row.get::<_, String>(7)?,
            }))
        });
        match mapped {
            Ok(items) => items.flatten().collect(),
            Err(_) => return fail("查询日程失败"),
        }
    };
    let filtered: Vec<Value> = if keyword.is_empty() {
        rows
    } else {
        rows.into_iter()
            .filter(|one| {
                let hay = format!("{} {}", one["title"].as_str().unwrap_or_default(),
                                  one["description"].as_str().unwrap_or_default());
                hay.contains(&keyword)
            })
            .collect()
    };
    with_items(
        format!("{from} ~ {to} 有 {} 条日程", filtered.len()),
        json!(filtered),
    )
}

fn schedule_add(store: &Store, args: &Value) -> Value {
    let title = arg_str(args, "title");
    let date = arg_str(args, "date");
    if title.is_empty() || date.is_empty() {
        return fail("新建日程至少要 title 和 date");
    }
    let start = {
        let text = arg_str(args, "start_time");
        if text.is_empty() { "09:00".to_string() } else { text }
    };
    let end = {
        let text = arg_str(args, "end_time");
        if text.is_empty() {
            chrono::NaiveTime::parse_from_str(&start, "%H:%M")
                .map(|t| (t + chrono::Duration::hours(1)).format("%H:%M").to_string())
                .unwrap_or_else(|_| "10:00".to_string())
        } else {
            text
        }
    };
    let remind = args
        .get("remind_minutes")
        .and_then(|one| one.as_i64());
    let input = crate::store::EventInput {
        title: Some(title.clone()),
        description: Some(arg_str(args, "description")),
        date: Some(date.clone()),
        start_time: Some(start.clone()),
        end_time: Some(end.clone()),
        remind_minutes: remind,
        clear_remind: false,
        color: Some({
            let text = arg_str(args, "color");
            if text.is_empty() { "violet".to_string() } else { text }
        }),
        done: None,
        repeat: None,
    };
    match store.add_event(&input) {
        Ok(id) => ok(format!("已新建日程 #{id}：{title}（{date} {start}–{end}）")),
        Err(err) => fail(format!("新建日程失败：{err}")),
    }
}

fn schedule_update(store: &Store, args: &Value) -> Value {
    let Some(id) = args.get("event_id").and_then(|one| one.as_i64()) else {
        return fail("要传 event_id");
    };
    let current = match store.get_event(id) {
        Ok(Some(row)) => row,
        Ok(None) => return fail(format!("没有 id={id} 的日程")),
        Err(err) => return fail(format!("读日程失败：{err}")),
    };
    let input = crate::store::EventInput {
        title: Some({
            let text = arg_str(args, "title");
            if text.is_empty() { current.title.clone() } else { text }
        }),
        description: Some({
            let text = arg_str(args, "description");
            if text.is_empty() { current.description.clone() } else { text }
        }),
        date: Some({
            let text = arg_str(args, "date");
            if text.is_empty() { current.date.clone() } else { text }
        }),
        start_time: Some({
            let text = arg_str(args, "start_time");
            if text.is_empty() { current.start_time.clone() } else { text }
        }),
        end_time: Some({
            let text = arg_str(args, "end_time");
            if text.is_empty() { current.end_time.clone() } else { text }
        }),
        remind_minutes: args
            .get("remind_minutes")
            .and_then(|one| one.as_i64())
            .or(current.remind_minutes),
        clear_remind: false,
        color: Some({
            let text = arg_str(args, "color");
            if text.is_empty() { current.color.clone() } else { text }
        }),
        done: args.get("done").and_then(|one| one.as_bool()),
        repeat: None,
    };
    match store.update_event(id, &input) {
        Ok(true) => {
            let mut summary = format!(
                "已修改日程 #{id}：{}",
                input.title.clone().unwrap_or_default()
            );
            if let Some(done) = args.get("done").and_then(|one| one.as_bool()) {
                let flag = if done { 1 } else { 0 };
                let conn = store.connection();
                let _ = conn.execute(
                    "UPDATE events SET done = ?1, updated_at = ?2 WHERE id = ?3",
                    rusqlite::params![flag, crate::now_str(), id],
                );
                summary.push_str(if done { "，已标记完成" } else { "，已标记未完成" });
            }
            ok(summary)
        }
        Ok(false) => fail(format!("没有 id={id} 的日程")),
        Err(err) => fail(format!("修改失败：{err}")),
    }
}

fn schedule_delete(store: &Store, args: &Value) -> Value {
    let Some(id) = args.get("event_id").and_then(|one| one.as_i64()) else {
        return fail("要传 event_id");
    };
    match store.delete_event(id) {
        Ok(true) => ok(format!("已删除日程 #{id}")),
        Ok(false) => fail(format!("没有 id={id} 的日程")),
        Err(err) => fail(format!("删除失败：{err}")),
    }
}

fn countdown_add(store: &Store, args: &Value) -> Value {
    let title = arg_str(args, "title");
    let day = arg_str(args, "day");
    if title.is_empty() || day.is_empty() {
        return fail("要传 title 和 day");
    }
    let conn = store.connection();
    let result = conn.execute(
        "INSERT INTO important_dates(title, date, note, color, kind, created_at)
         VALUES (?1, ?2, ?3, 'rose', 'custom', ?4)",
        rusqlite::params![title, day, arg_str(args, "note"), crate::now_str()],
    );
    match result {
        Ok(_) => {
            let days = chrono::NaiveDate::parse_from_str(&day, "%Y-%m-%d")
                .map(|date| (date - chrono::Local::now().date_naive()).num_days())
                .unwrap_or(0);
            ok(format!("已加倒计时：{title}（{day}，还有 {days} 天）"))
        }
        Err(err) => fail(format!("写倒计时失败：{err}")),
    }
}

fn pomodoro_start(store: &Store, args: &Value) -> Value {
    let minutes = arg_i64(args, "minutes", 25).clamp(1, 180);
    let label = arg_str(args, "label");
    let started = crate::now_str();
    let _ = store.set_setting("pomodoro_minutes", &minutes.to_string());
    let _ = store.set_setting("pomodoro_label", &label);
    let _ = store.set_setting("pomodoro_started_at", &started);
    ok(format!(
        "专注计时已开始：{minutes} 分钟{}{}",
        if label.is_empty() { "" } else { "，内容：" },
        label
    ))
}

// ------------------------------------------------------------------ 信息类
fn time_now() -> Value {
    let now = chrono::Local::now();
    let week = ["一", "二", "三", "四", "五", "六", "日"];
    let weekday = week[now.format("%u").to_string().parse::<usize>().unwrap_or(1) - 1];
    with_items(
        format!("现在是 {}", now.format("%Y-%m-%d %H:%M:%S")),
        json!({
            "date": now.format("%Y-%m-%d").to_string(),
            "time": now.format("%H:%M:%S").to_string(),
            "weekday": format!("星期{weekday}"),
            "timezone": now.format("%:z").to_string(),
        }),
    )
}

fn sys_info(store: &Store) -> Value {
    let _ = store;
    let cores = std::thread::available_parallelism()
        .map(|one| one.get())
        .unwrap_or(0);
    let host = std::env::var("COMPUTERNAME").unwrap_or_default();
    let data_dir = crate::paths::data_dir();
    let db = crate::paths::db_path();
    let db_size = std::fs::metadata(&db).map(|one| one.len()).unwrap_or(0);
    with_items(
        format!("{cores} 核 / {host} / 数据目录 {}", data_dir.display()),
        json!({
            "cpu_cores": cores,
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "host": host,
            "user": std::env::var("USERNAME").unwrap_or_default(),
            "data_dir": data_dir.display().to_string(),
            "db_path": db.display().to_string(),
            "db_bytes": db_size,
        }),
    )
}

fn app_state(store: &Store) -> Value {
    let today = crate::today_str();
    let events = store.events_on(&today).map(|rows| rows.len()).unwrap_or(0);
    let unread = store.notice_unread_count();
    let memos = {
        let conn = store.connection();
        conn.query_row("SELECT COUNT(*) FROM memos", [], |row| row.get::<_, i64>(0))
            .unwrap_or(0)
    };
    let mails = {
        let conn = store.connection();
        conn.query_row("SELECT COUNT(*) FROM mails", [], |row| row.get::<_, i64>(0))
            .unwrap_or(0)
    };
    with_items(
        format!("今天 {events} 条日程、{unread} 条未读通知、{mails} 封发件、{memos} 条备忘"),
        json!({ "today": today, "events": events, "unread_notices": unread,
                "mails": mails, "memos": memos }),
    )
}

fn memo_search(store: &Store, args: &Value) -> Value {
    let keyword = arg_str(args, "keyword");
    if keyword.is_empty() {
        return fail("要传 keyword");
    }
    let limit = arg_i64(args, "limit", 10).max(1) as usize;
    let rows = store.memos().unwrap_or_default();
    let mut hits = Vec::new();
    for row in rows {
        let text = format!(
            "{} {}",
            row.get("title").and_then(|one| one.as_str()).unwrap_or_default(),
            row.get("text").and_then(|one| one.as_str()).unwrap_or_default(),
        );
        if let Some(pos) = text.find(&keyword) {
            let start = pos.saturating_sub(30);
            let snippet: String = text.chars().skip(start).take(90).collect();
            hits.push(json!({
                "id": row.get("id"),
                "title": row.get("title"),
                "snippet": snippet,
            }));
            if hits.len() >= limit {
                break;
            }
        }
    }
    with_items(format!("搜到 {} 条备忘", hits.len()), json!(hits))
}

fn memo_append(store: &Store, args: &Value) -> Value {
    let text = arg_str(args, "text");
    if text.is_empty() {
        return fail("要传 text");
    }
    let title = {
        let given = arg_str(args, "title");
        if given.is_empty() {
            format!("AI 记的备忘 {}", chrono::Local::now().format("%m-%d %H:%M"))
        } else {
            given
        }
    };
    let html = format!("<p>{}</p>", text.replace('<', "&lt;").replace('>', "&gt;"));
    match store.add_memo(&title, &html) {
        Ok(id) => ok(format!("已记到备忘 #{id}：{title}")),
        Err(err) => fail(format!("写备忘失败：{err}")),
    }
}

fn usage_stats(store: &Store) -> Value {
    let sessions = {
        let conn = store.connection();
        conn.query_row("SELECT COUNT(*) FROM chat_sessions", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap_or(0)
    };
    let on = catalog()
        .iter()
        .filter(|one| {
            let name = one["name"].as_str().unwrap_or_default();
            tool_on(store, name)
        })
        .count();
    with_items(
        format!("历史会话 {sessions} 条，已开启工具 {on} 个"),
        json!({ "sessions": sessions, "tools_on": on, "tools_total": catalog().len() }),
    )
}

fn tool_help(store: &Store) -> Value {
    let rows: Vec<Value> = catalog()
        .into_iter()
        .map(|one| {
            let name = one["name"].as_str().unwrap_or_default().to_string();
            json!({
                "name": name,
                "label": one["label"],
                "risk": one["risk"],
                "on": tool_on(store, &name),
            })
        })
        .collect();
    let on = rows.iter().filter(|one| one["on"] == true).count();
    with_items(format!("共 {} 个工具，开着 {on} 个", rows.len()), json!(rows))
}

/// 枚举当前可见窗口的标题（对应旧版 `window_list`；默认关，因为会暴露你开着什么）
fn window_list(args: &Value) -> Value {
    let limit = arg_i64(args, "limit", 20).clamp(1, 100) as usize;
    let titles = collect_window_titles();
    if titles.is_empty() {
        return fail("没枚举到窗口（可能被系统策略拦了）");
    }
    let rows: Vec<Value> = titles
        .into_iter()
        .take(limit)
        .map(|one| json!({ "title": one }))
        .collect();
    with_items(format!("当前有 {} 个可见窗口", rows.len()), json!(rows))
}

/// 前台窗口标题（专注离开检测用；拿不到就返回空串）
#[cfg(windows)]
pub fn foreground_title() -> String {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
    };
    unsafe {
        let hwnd = GetForegroundWindow();
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buffer = vec![0u16; (len + 1) as usize];
        let written = GetWindowTextW(hwnd, &mut buffer);
        if written <= 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buffer[..written as usize])
    }
}

#[cfg(not(windows))]
pub fn foreground_title() -> String {
    String::new()
}

#[cfg(windows)]
fn collect_window_titles() -> Vec<String> {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible,
    };

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let out = &mut *(lparam.0 as *mut Vec<String>);
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return BOOL(1);
        }
        let mut buffer = vec![0u16; (len + 1) as usize];
        let written = GetWindowTextW(hwnd, &mut buffer);
        if written > 0 {
            let title = String::from_utf16_lossy(&buffer[..written as usize]);
            let title = title.trim().to_string();
            if !title.is_empty() && !out.contains(&title) {
                out.push(title);
            }
        }
        BOOL(1)
    }

    let mut titles: Vec<String> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(callback), LPARAM(&mut titles as *mut Vec<String> as isize));
    }
    titles
}

#[cfg(not(windows))]
fn collect_window_titles() -> Vec<String> {
    Vec::new()
}

fn flashcards_create(store: &Store, args: &Value) -> Value {
    let cards = args
        .get("cards")
        .and_then(|one| one.as_array())
        .cloned()
        .unwrap_or_default();
    if cards.is_empty() {
        return fail("要传 cards（[{front, back}]）");
    }
    let mut html = String::new();
    for (index, card) in cards.iter().enumerate() {
        html.push_str(&format!(
            "<p><b>{}. {}</b><br>{}</p>",
            index + 1,
            card.get("front").and_then(|one| one.as_str()).unwrap_or(""),
            card.get("back").and_then(|one| one.as_str()).unwrap_or(""),
        ));
    }
    let title = format!(
        "复习卡片（{} 张）{}",
        cards.len(),
        if arg_str(args, "source").is_empty() {
            String::new()
        } else {
            format!(" · {}", arg_str(args, "source"))
        }
    );
    match store.add_memo(&title, &html) {
        Ok(id) => ok(format!("已生成 {} 张复习卡片（备忘 #{id}）", cards.len())),
        Err(err) => fail(format!("写卡片失败：{err}")),
    }
}

// ------------------------------------------------------------------ 本地文件
fn allowed_roots(store: &Store) -> Vec<std::path::PathBuf> {
    let mut roots = Vec::new();
    if let Some(text) = store.setting("dir_downloads") {
        if !text.trim().is_empty() {
            roots.push(std::path::PathBuf::from(text.trim()));
        }
    }
    if let Some(text) = store.setting("dir_workspace") {
        if !text.trim().is_empty() {
            roots.push(std::path::PathBuf::from(text.trim()));
        }
    }
    roots.push(crate::paths::data_dir());
    roots.push(crate::paths::data_dir().join("elearning"));
    roots
}

fn guard_path(store: &Store, path: &str) -> Result<std::path::PathBuf, String> {
    let raw = std::path::PathBuf::from(path);
    let target = if raw.is_absolute() {
        raw
    } else {
        crate::paths::data_dir().join(raw)
    };
    let canonical = target
        .canonicalize()
        .unwrap_or_else(|_| target.clone());
    for root in allowed_roots(store) {
        if let Ok(base) = root.canonicalize() {
            if canonical.starts_with(&base) {
                return Ok(target);
            }
        }
    }
    Err(format!(
        "这个路径不在允许的目录里（下载目录 / 工作区 / 数据目录）：{}",
        canonical.display()
    ))
}

fn fs_list(store: &Store, args: &Value) -> Value {
    let path = {
        let given = arg_str(args, "path");
        if given.is_empty() {
            store
                .setting("dir_downloads")
                .filter(|text| !text.trim().is_empty())
                .unwrap_or_else(|| crate::paths::data_dir().display().to_string())
        } else {
            given
        }
    };
    match guard_path(store, &path) {
        Ok(_) => {}
        Err(err) => return fail(err),
    }
    let limit = arg_i64(args, "limit", 400).max(1) as usize;
    match crate::docs::list_dir(&path) {
        Ok(entries) => {
            let rows: Vec<Value> = entries
                .iter()
                .take(limit)
                .map(|one| {
                    json!({
                        "name": one.name,
                        "kind": one.kind,
                        "size": one.size,
                        "is_dir": one.is_dir,
                        "modified": one.modified,
                    })
                })
                .collect();
            with_items(format!("{} 里有 {} 项", path, rows.len()), json!(rows))
        }
        Err(err) => fail(err),
    }
}

fn fs_read(store: &Store, args: &Value) -> Value {
    let path = arg_str(args, "path");
    if path.is_empty() {
        return fail("要传 path");
    }
    if let Err(err) = guard_path(store, &path) {
        return fail(err);
    }
    let limit = arg_i64(args, "limit", 60_000).max(1) as usize;
    match crate::docs::read_text(&path, limit) {
        Ok(text) => with_items(
            format!("读了 {} 个字符", text.chars().count()),
            json!({ "path": path, "text": text }),
        ),
        Err(err) => fail(err),
    }
}

fn fs_search(store: &Store, args: &Value) -> Value {
    let keyword = arg_str(args, "keyword");
    if keyword.is_empty() {
        return fail("要传 keyword");
    }
    let root = {
        let given = arg_str(args, "path");
        if given.is_empty() {
            store
                .setting("dir_downloads")
                .filter(|text| !text.trim().is_empty())
                .unwrap_or_else(|| crate::paths::data_dir().display().to_string())
        } else {
            given
        }
    };
    if let Err(err) = guard_path(store, &root) {
        return fail(err);
    }
    let limit = arg_i64(args, "limit", 50).max(1) as usize;
    let mut hits = Vec::new();
    let mut stack = vec![std::path::PathBuf::from(&root)];
    while let Some(dir) = stack.pop() {
        if hits.len() >= limit {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let big = entry.metadata().map(|one| one.len()).unwrap_or(0) > 2_000_000;
            if big {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|one| one.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if !matches!(
                ext.as_str(),
                "txt" | "md" | "py" | "js" | "ts" | "json" | "rs" | "c" | "cpp" | "h"
                    | "tex" | "csv" | "html" | "htm" | "log" | "toml" | "yaml" | "yml"
            ) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if let Some(pos) = text.find(&keyword) {
                let start = pos.saturating_sub(40);
                let snippet: String = text.chars().skip(start).take(120).collect();
                hits.push(json!({
                    "path": path.display().to_string(),
                    "snippet": snippet,
                }));
                if hits.len() >= limit {
                    break;
                }
            }
        }
    }
    with_items(format!("在 {root} 里搜到 {} 个文件", hits.len()), json!(hits))
}

fn fs_write(store: &Store, args: &Value) -> Value {
    let path = arg_str(args, "path");
    let content = args
        .get("content")
        .and_then(|one| one.as_str())
        .unwrap_or_default()
        .to_string();
    if path.is_empty() {
        return fail("要传 path");
    }
    if let Err(err) = guard_path(store, &path) {
        return fail(err);
    }
    let append = arg_bool(args, "append", false);
    let result = if append {
        use std::io::Write;
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut file| file.write_all(content.as_bytes()))
    } else {
        std::fs::write(&path, content.as_bytes())
    };
    match result {
        Ok(_) => ok(format!(
            "已{} {}（{} 字节）",
            if append { "追加写入" } else { "写入" },
            path,
            content.len()
        )),
        Err(err) => fail(format!("写文件失败：{err}")),
    }
}

fn fs_discard(_store: &Store, args: &Value) -> Value {
    let path = arg_str(args, "path");
    if path.is_empty() {
        return fail("要传 path");
    }
    let source = std::path::PathBuf::from(&path);
    if !source.exists() {
        return fail(format!("文件不存在：{path}"));
    }
    let name = source
        .file_name()
        .map(|one| one.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let target = source.with_file_name(format!("[discarded] {name}"));
    match std::fs::rename(&source, &target) {
        Ok(_) => ok(format!(
            "已丢弃（改名）：{} → {}；想彻底删就去 {} 里删",
            name,
            target.display(),
            source.parent().map(|one| one.display().to_string()).unwrap_or_default()
        )),
        Err(err) => fail(format!("改名失败：{err}")),
    }
}

// ------------------------------------------------------------------ 设置
fn setting_whitelist() -> Vec<&'static str> {
    vec![
        "theme_mode", "theme_accent", "theme_bg", "theme_alpha", "theme_blur",
        "theme_glass", "theme_browser", "ai_model", "ai_temperature", "ai_thinking",
        "ai_max_tokens", "dir_downloads", "dir_workspace", "weather_city",
        "notices_source", "dashboard_refresh_minutes", "remind_enabled",
        "remind_advance_minutes", "quiet_hours", "auto_backup", "language",
        "week_start", "pomodoro_minutes", "pomodoro_label",
    ]
}

fn setting_get(store: &Store, args: &Value) -> Value {
    let key = arg_str(args, "key");
    if key.is_empty() {
        let rows: Vec<Value> = setting_whitelist()
            .iter()
            .map(|one| json!({ "key": one, "value": store.setting(one).unwrap_or_default() }))
            .collect();
        return with_items(format!("白名单共 {} 个设置", rows.len()), json!(rows));
    }
    if !setting_whitelist().contains(&key.as_str()) {
        return fail(format!("「{key}」不在可读白名单里（密码 / 账号一律不给）"));
    }
    with_items(
        format!("{key} = {}", store.setting(&key).unwrap_or_default()),
        json!({ "key": key, "value": store.setting(&key).unwrap_or_default() }),
    )
}

fn setting_set(store: &Store, args: &Value) -> Value {
    let key = arg_str(args, "key");
    let value = arg_str(args, "value");
    if key.is_empty() {
        return fail("要传 key");
    }
    if !setting_whitelist().contains(&key.as_str()) {
        return fail(format!(
            "「{key}」不在可改白名单里（密码 / 账号 / 密钥一律不给改）"
        ));
    }
    match store.set_setting(&key, &value) {
        Ok(_) => ok(format!("已把 {key} 改成「{value}」")),
        Err(err) => fail(format!("改设置失败：{err}")),
    }
}

// ------------------------------------------------------------------ 教务缓存
fn elearning_courses(store: &Store, args: &Value) -> Value {
    let keyword = arg_str(args, "keyword");
    let items = crate::elearn::cached_items(store);
    if items.is_empty() {
        return with_items(
            "还没有 eLearning 缓存：先在「作业」页读取一次课程".to_string(),
            json!([]),
        );
    }
    let rows: Vec<Value> = items
        .into_iter()
        .filter(|one| {
            keyword.is_empty()
                || one
                    .get("course")
                    .and_then(|value| value.as_str())
                    .map(|text| text.contains(&keyword))
                    .unwrap_or(false)
                || one
                    .get("title")
                    .and_then(|value| value.as_str())
                    .map(|text| text.contains(&keyword))
                    .unwrap_or(false)
        })
        .collect();
    // 旧版 elearning_status：记住「上次读到什么」，作业页也读这个键显示状态
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
    let _ = store.set_setting(
        "elearning_status",
        &format!("{stamp} 读到 {} 条课程作业", rows.len()),
    );
    with_items(format!("读到 {} 个课程作业", rows.len()), json!(rows))
}

/// 课表：v2 走 .ics 导入（旧版还有一条要内嵌浏览器会话的抓取路径，这里没有那个会话）
fn timetable_fetch(store: &Store, args: &Value) -> Value {
    let path = {
        let given = arg_str(args, "ics_path");
        if !given.is_empty() {
            given
        } else {
            crate::paths::data_dir()
                .join("timetable.ics")
                .display()
                .to_string()
        }
    };
    if !std::path::Path::new(&path).exists() {
        return fail(format!(
            "没找到课表文件：{path}。把旦夕 / 教务导出的 .ics 放到这里，或者用 ics_path 指定路径"
        ));
    }
    match crate::ics::sync(store, &path) {
        Ok(report) => ok(report.summary()),
        Err(err) => fail(err),
    }
}

// ------------------------------------------------------------------ 测试
#[cfg(test)]
mod tests {
    use super::*;

    fn real_store() -> Store {
        std::env::set_var(
            "LUMINA_DATA_DIR",
            r"E:\FIles\Lena‘s Files\Work\znw\Agent\data",
        );
        Store::open().expect("打不开数据库")
    }

    #[test]
    fn catalog_has_all_41_tools() {
        let all = catalog();
        println!("工具数：{}", all.len());
        assert_eq!(all.len(), 41, "工具数量要和旧版一致");
        for name in ["schedule_list", "web_search", "zip_extract", "mail_send"] {
            assert!(all.iter().any(|one| one["name"] == name), "缺工具 {name}");
        }
    }

    #[test]
    fn high_risk_tools_are_off_by_default() {
        for name in ["mail_send", "setting_set", "zip_extract", "fs_write", "fs_discard"] {
            assert!(!default_on(name), "{name} 默认应该是关的");
            assert_eq!(risk_of(name), if name == "fs_write" || name == "fs_discard" { "mid" } else { "high" });
        }
    }

    #[test]
    fn definitions_respect_switches() {
        let store = real_store();
        let defs = definitions(&store);
        println!("给模型的工具数：{}", defs.len());
        assert!(defs.len() <= 41);
        if globally_on(&store) {
            assert!(!defs.is_empty());
            // 高风险默认关：不应该出现在默认的工具列表里
            let names: Vec<String> = defs
                .iter()
                .map(|one| one["function"]["name"].as_str().unwrap_or_default().to_string())
                .collect();
            if !tool_on(&store, "mail_send") {
                assert!(!names.contains(&"mail_send".to_string()));
            }
        }
    }

    #[test]
    fn dispatch_reads_real_data() {
        let store = real_store();
        let state = dispatch(&store, "app_state", &json!({}));
        println!("app_state: {state}");
        assert_eq!(state["ok"], true);

        let help = dispatch(&store, "tool_help", &json!({}));
        println!("tool_help: {}", help["summary"]);
        assert_eq!(help["ok"], true);

        let now = dispatch(&store, "time_now", &json!({}));
        assert_eq!(now["ok"], true);
        println!("time_now: {}", now["summary"]);

        let events = dispatch(&store, "schedule_list", &json!({}));
        println!("schedule_list: {}", events["summary"]);
        assert_eq!(events["ok"], true);
    }

    #[test]
    fn window_enumeration_works() {
        let titles = collect_window_titles();
        println!("枚举到 {} 个窗口，前 5 个：", titles.len());
        for one in titles.iter().take(5) {
            println!("   · {one}");
        }
        #[cfg(windows)]
        assert!(!titles.is_empty(), "Windows 上应该至少能枚举到几个窗口");
    }

    #[test]
    fn risky_tools_need_confirm() {
        let store = real_store();
        // pdf_download 默认就是开着的，用它测确认门，避免别的测试同时改开关
        if !tool_on(&store, "pdf_download") {
            println!("本机把 pdf_download 关了，跳过");
            return;
        }
        let result = dispatch(&store, "pdf_download", &json!({ "ids": [1] }));
        println!("没带 confirm：{result}");
        assert_eq!(result["ok"], false);
        assert_eq!(result["needs_confirm"], true);
    }

    #[test]
    fn disabled_tool_is_refused() {
        let store = real_store();
        if tool_on(&store, "zip_extract") {
            println!("本机开着 zip_extract，跳过这条断言");
            return;
        }
        let result = dispatch(&store, "zip_extract", &json!({ "archive": "x.zip" }));
        assert_eq!(result["ok"], false);
        println!("被拒：{}", result["summary"]);
    }
}
