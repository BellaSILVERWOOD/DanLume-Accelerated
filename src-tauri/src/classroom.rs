//! 空教室：把「空闲教室查询」页面里的表格解析成「教室 × 节次」的格子。
//!
//! 对应旧版 `ehall_bridge.py` 的 `CLASSROOM_JS` / `build_classroom_rooms` /
//! `parse_classroom_grid`：旧版在 QWebEngine 页面里跑一段 JS 把表格读出来，
//! 这里用子 Webview 的 `eval_with_callback` 做同一件事（`scrape_script()`），
//! 解析仍在 Rust 侧完成，所以「抓取当前页面」不需要用户复制粘贴。
//!
//! 解析规则与旧版一致：
//! * 自动认表格方向（行=教室 / 列=教室 两种排法都认）；
//! * 占用格里的课程名从 `[课程](老师){人数}` 里抠出来，抠不到就写格子原文；
//! * 只留「像教室」的行（H3101 / JB100 / 光华西301），教学楼下拉、框架词一律丢掉。
use serde_json::{json, Value};

/// 节次数（`fdu.DEFAULT_UNIT_TIMES` 是 14 节）
const PERIODS: usize = 14;

/// 页面上的「框架词」，不是教室数据
const CHROME_WORDS: [&str; 19] = [
    "请选择", "全部校区", "全部教学楼", "全部教室", "关键词", "查询", "教学楼", "教室", "节次",
    "校区", "空闲教室", "在线教学", "借用", "多个关键词", "空格分隔", "重置", "打印", "导出",
    "上一页",
];

const FREE_WORDS: [&str; 9] = ["空闲", "空", "可用", "无课", "无", "-", "—", "/", "空教室"];

const CN_PERIODS: [(&str, usize); 14] = [
    ("一", 1),
    ("二", 2),
    ("三", 3),
    ("四", 4),
    ("五", 5),
    ("六", 6),
    ("七", 7),
    ("八", 8),
    ("九", 9),
    ("十", 10),
    ("十一", 11),
    ("十二", 12),
    ("十三", 13),
    ("十四", 14),
];

/// 抓取脚本：读「本表自己的」行和格（`tb.rows`），避免把表头里那个嵌套小表也算进来。
///
/// 返回**对象**（不是 `JSON.stringify`）：`eval_with_callback` 会把结果序列化成 JSON
/// 交给 Rust，直接给对象就少一层转义。
pub fn scrape_script() -> &'static str {
    r#"(function () {
  function tidy(text) { return (text || '').replace(/[ \t\u3000]+/g, ' ').trim(); }
  function readTable(tb) {
    var rows = [];
    try {
      for (var i = 0; i < tb.rows.length; i++) {
        var tr = tb.rows[i];
        var cells = [];
        for (var j = 0; j < tr.cells.length; j++) {
          var td = tr.cells[j];
          var label = tidy(td.innerText).slice(0, 120);
          var title = td.getAttribute('title') || td.getAttribute('data-text') || '';
          cells.push((label || tidy(title)).slice(0, 120));
        }
        if (cells.length) { rows.push(cells); }
      }
    } catch (e) {}
    return rows;
  }
  var candidates = [];
  try {
    var tables = document.querySelectorAll('table');
    for (var t = 0; t < tables.length; t++) {
      var tb = tables[t];
      var rows = readTable(tb);
      if (rows.length < 2) { continue; }
      var width = 0, shortCells = 0, totalCells = 0;
      for (var i = 0; i < rows.length; i++) {
        if (rows[i].length > width) { width = rows[i].length; }
        for (var j = 0; j < rows[i].length; j++) {
          totalCells += 1;
          if ((rows[i][j] || '').length <= 12) { shortCells += 1; }
        }
      }
      var ratio = totalCells ? shortCells / totalCells : 0;
      var roomRows = 0;
      for (var k = 0; k < rows.length; k++) {
        var head = (rows[k][0] || '');
        if (rows[k].length >= 6 && /^[A-Za-z]{0,3}\s*-?\s*\d{2,4}/.test(head)) { roomRows += 1; }
      }
      var score = ratio * 10 + Math.min(width, 20) + Math.min(rows.length, 40) / 10 + roomRows * 6;
      var id = (tb.id || '');
      if (/statusTable_0/i.test(id)) { score += 40; }
      if (/statusTable_1/i.test(id)) { score -= 60; }
      candidates.push({ rows: rows, score: score });
    }
  } catch (e) {}
  candidates.sort(function (a, b) { return b.score - a.score; });
  var rows = candidates.length ? candidates[0].rows : [];
  var headers = [];
  if (rows.length && rows[0].length >= 3) {
    var head = rows[0];
    var timeWords = /节|时间|上午|下午|晚上|周一|周二|周三|周四|周五|周末|:|\d/;
    var hits = 0;
    for (var h = 0; h < head.length; h++) { if (timeWords.test(head[h])) { hits += 1; } }
    if (hits >= Math.max(2, head.length - 1)) { headers = head; rows = rows.slice(1); }
  }
  var lines = [];
  try {
    var text = document.body ? (document.body.innerText || '') : '';
    var all = text.split('\n');
    for (var m = 0; m < all.length && m < 800; m++) {
      var one = tidy(all[m]);
      if (one && one.length <= 40) { lines.push(one); }
    }
  } catch (e) {}
  return {
    u: location.href,
    h: document.title || '',
    headers: headers,
    rows: rows.slice(0, 80),
    lines: lines.slice(0, 200),
    tables: candidates.length
  };
})()"#
}

/// 在一个下拉里按文字选中某一项（原生 select 与可点击的 div/li 都试）。
///
/// 对应旧版 `ehall_bridge.pick_js(value)`：后台页面先把校区/教学楼/教室选好，
/// 再点「查询」，页面才会把表格画出来。
pub fn pick_script(value: &str) -> String {
    let want = serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string());
    PICK_JS.replace("__WANT__", &want)
}

/// 点页面上的「查询」按钮
pub fn click_query_script() -> &'static str {
    CLICK_QUERY_JS
}

const PICK_JS: &str = r#"(function () {
  var want = __WANT__;
  function fire(el) {
    try { el.dispatchEvent(new Event('input', {bubbles: true})); } catch (e) {}
    try { el.dispatchEvent(new Event('change', {bubbles: true})); } catch (e) {}
    try { el.dispatchEvent(new Event('blur', {bubbles: true})); } catch (e) {}
  }
  var found = null;
  var selects = document.querySelectorAll('select');
  for (var i = 0; i < selects.length && !found; i++) {
    var sel = selects[i];
    for (var j = 0; j < sel.options.length; j++) {
      var text = (sel.options[j].text || '').trim();
      var value = (sel.options[j].value || '').trim();
      if (text === want || value === want || (want && text.indexOf(want) >= 0)) {
        sel.selectedIndex = j;
        fire(sel);
        found = {select: i, text: text};
        break;
      }
    }
  }
  if (!found) {
    var nodes = document.querySelectorAll('li, a, div[role="option"], span');
    for (var k = 0; k < nodes.length; k++) {
      var label = (nodes[k].innerText || '').replace(/\s+/g, '');
      if (!label || label.length > 24) { continue; }
      if (label === want || (want && label.indexOf(want) >= 0)) {
        try { nodes[k].click(); } catch (e) {}
        fire(nodes[k]);
        found = {node: k, text: label};
        break;
      }
    }
  }
  return JSON.stringify({ok: !!found, found: found});
})()"#;

const CLICK_QUERY_JS: &str = r#"(function () {
  var words = ['查询', '搜 索', '搜索', '查 询'];
  var nodes = document.querySelectorAll('button, a, input[type="button"], div[role="button"], span, li');
  for (var i = 0; i < nodes.length; i++) {
    var node = nodes[i];
    if (node.children && node.children.length > 0) { continue; }
    var text = (node.innerText || node.value || '').replace(/\s+/g, '');
    for (var w = 0; w < words.length; w++) {
      if (text === words[w] || (words[w].length > 2 && text.indexOf(words[w]) >= 0)) {
        try { node.click(); } catch (e) {}
        return JSON.stringify({ok: true, text: text});
      }
    }
  }
  return JSON.stringify({ok: false, reason: '没找到「查询」按钮'});
})()"#;

fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for ch in text.chars() {
        if ch.is_whitespace() || ch == '\u{3000}' {
            if !space && !out.is_empty() {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(ch);
            space = false;
        }
    }
    out.trim().to_string()
}

/// 第一个 `H:MM` 时刻（分钟数）
fn minutes_of(text: &str) -> Option<usize> {
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        if !chars[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        let mut end = index;
        while end < chars.len() && chars[end].is_ascii_digit() {
            end += 1;
        }
        let digits = end - index;
        if (1..=2).contains(&digits)
            && end < chars.len()
            && (chars[end] == ':' || chars[end] == '：')
        {
            let hour: String = chars[index..end].iter().collect();
            let mut minute = String::new();
            let mut next = end + 1;
            while next < chars.len() && chars[next].is_ascii_digit() && minute.len() < 2 {
                minute.push(chars[next]);
                next += 1;
            }
            if minute.len() == 2 {
                if let (Ok(hour), Ok(minute)) = (hour.parse::<usize>(), minute.parse::<usize>()) {
                    if hour < 24 && minute < 60 {
                        return Some(hour * 60 + minute);
                    }
                }
            }
        }
        index = end.max(index + 1);
    }
    None
}

/// 「第 3 大节」/「3」这种纯节次
fn period_number(text: &str) -> Option<usize> {
    let mut value = tidy(text);
    if let Some(rest) = value.strip_prefix('第') {
        value = rest.trim().to_string();
    }
    let compact: String = value.chars().filter(|ch| !ch.is_whitespace()).collect();
    let compact = compact.trim_end_matches('节').trim_end_matches('大');
    if compact.is_empty() || compact.chars().count() > 2 {
        return None;
    }
    if compact.chars().all(|ch| ch.is_ascii_digit()) {
        return compact.parse::<usize>().ok();
    }
    None
}

fn looks_like_time(text: &str) -> bool {
    let value = tidy(text);
    if value.is_empty() {
        return false;
    }
    if value.contains(':') || value.contains('：') || minutes_of(&value).is_some() {
        return true;
    }
    if period_number(&value).is_some() {
        return true;
    }
    ["上午", "下午", "晚上", "节次", "时间"]
        .iter()
        .any(|word| value.contains(word))
}

fn is_chrome_text(text: &str) -> bool {
    let value = tidy(text);
    if value.is_empty() {
        return true;
    }
    if CHROME_WORDS.iter().any(|word| value.contains(word)) {
        return true;
    }
    // 一长串楼名/选项（空格分隔）当成表单内容
    value.matches(' ').count() >= 6 && value.chars().count() > 30
}

fn is_header_cell(text: &str) -> bool {
    let value = tidy(text);
    !value.is_empty()
        && (value.contains("教室") || value.contains("时间") || value.contains("节次"))
        && minutes_of(&value).is_none()
}

/// 「H3101」「JB100」「光华西301」这种教室名（不是「二教」「江湾」这种楼名）
///
/// 比旧版多挡两类：**小数**（`22.37`，页面上的金额/人数）和**日期**（`2026-10-12`）——
/// 停在 eHall 首页时那张「我的待办」表就是这么被当成教室表的。
fn looks_like_room(text: &str) -> bool {
    let value = tidy(text);
    if value.is_empty() || value.chars().count() > 12 {
        return false;
    }
    if value == "教室" || value == "教室号" || value == "房间" || value == "座位" {
        return false;
    }
    if is_chrome_text(&value) || minutes_of(&value).is_some() {
        return false;
    }
    if value.contains('.') || value.contains('%') || value.contains('％') {
        return false;
    }
    // 日期：第一段是四位年份、至少两段（`2026-10-12` / `2026-10-12 11:24:46` 已被长度挡住）
    if value.matches('-').count() >= 2 {
        let head: String = value.chars().take_while(|ch| ch.is_ascii_digit()).collect();
        if head.len() == 4 {
            return false;
        }
    }
    // 至少要有一个字母+数字，或者两个数字（H3101 / JB100 / 3101）
    let chars: Vec<char> = value.chars().collect();
    let mut digits = 0;
    for (index, ch) in chars.iter().enumerate() {
        if ch.is_ascii_digit() {
            digits += 1;
            if index > 0 && chars[index - 1].is_ascii_alphabetic() {
                return true;
            }
        }
    }
    digits >= 2
}

/// 表头/行首的标签 → 节次下标（0 起）；认不出来 -1
fn slot_index(label: &str) -> i64 {
    let text = tidy(label);
    if text.is_empty() {
        return -1;
    }
    if let Some(number) = period_number(&text) {
        return (number.saturating_sub(1)).min(PERIODS - 1) as i64;
    }
    let numeral = text.trim_matches(|ch: char| " 　节大第".contains(ch));
    if let Some((_, number)) = CN_PERIODS.iter().find(|(key, _)| *key == numeral) {
        return (number.saturating_sub(1)).min(PERIODS - 1) as i64;
    }
    if let Some(start) = minutes_of(&text) {
        let times = crate::timetable_remote::default_unit_times();
        for (index, (_number, begin, finish)) in times.iter().enumerate() {
            let low = minutes_of(begin).unwrap_or(0);
            let high = minutes_of(finish).unwrap_or(low + 45);
            if start >= low && start < high {
                return index as i64;
            }
        }
        let mut best = -1i64;
        let mut distance = 24 * 60usize;
        for (index, (_number, begin, _finish)) in times.iter().enumerate() {
            let gap = minutes_of(begin).unwrap_or(0).abs_diff(start);
            if gap < distance {
                best = index as i64;
                distance = gap;
            }
        }
        return best;
    }
    -1
}

/// 一个格子的原文 → 课程名 / 老师（`[线性代数A](司然){130人}`）
fn course_label(text: &str) -> String {
    let raw = tidy(text);
    if raw.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = raw.chars().collect();
    let mut names: Vec<String> = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '[' {
            let mut end = index + 1;
            while end < chars.len() && chars[end] != ']' {
                end += 1;
            }
            if end < chars.len() {
                let name: String = chars[index + 1..end].iter().collect::<String>().trim().to_string();
                if !name.is_empty() && !names.contains(&name) {
                    names.push(name);
                }
                index = end + 1;
                continue;
            }
        }
        index += 1;
    }
    if !names.is_empty() {
        return names.join("、");
    }
    // 没写方括号：把 `{130人}` 这种尾巴去掉，留前半段当课程名
    let mut short = raw.clone();
    if let Some(position) = short.find('{') {
        short.truncate(position);
    }
    let short = short.trim().to_string();
    if short.chars().count() > 40 {
        short.chars().take(40).collect()
    } else if short.is_empty() {
        raw.chars().take(40).collect()
    } else {
        short
    }
}

fn cell_occupied(text: &str) -> bool {
    let value = tidy(text);
    if value.is_empty() {
        return false;
    }
    if FREE_WORDS.contains(&value.as_str()) {
        return false;
    }
    !is_chrome_text(&value)
}

fn make_cell(text: &str, slot: i64) -> Value {
    let occupied = cell_occupied(text);
    let label = if occupied { course_label(text) } else { String::new() };
    json!({
        "label": label,
        "occupied": occupied,
        "slot": slot,
        "raw": if occupied { tidy(text) } else { String::new() },
    })
}

fn empty_cells() -> Vec<Value> {
    (0..PERIODS)
        .map(|index| {
            json!({ "label": "", "occupied": false, "slot": index as i64, "raw": "" })
        })
        .collect()
}

/// 表格 → 「一行一间教室」。返回 (rooms, periods, transposed)
pub fn build_rooms(table: &[Vec<String>], headers: &[String]) -> (Vec<Value>, Vec<String>, bool) {
    let mut rows: Vec<Vec<String>> = table
        .iter()
        .map(|row| row.iter().map(|cell| tidy(cell)).collect())
        .filter(|row: &Vec<String>| row.iter().any(|cell| !cell.is_empty()))
        .collect();
    let mut headers: Vec<String> = headers.iter().map(|cell| tidy(cell)).collect();
    if rows.is_empty() {
        return (Vec::new(), Vec::new(), false);
    }
    if is_header_cell(rows[0].first().map(|one| one.as_str()).unwrap_or("")) {
        if headers.is_empty() {
            headers = rows[0].clone();
        }
        rows.remove(0);
    }
    if rows.is_empty() {
        return (Vec::new(), Vec::new(), false);
    }

    let header_cells: Vec<String> = if headers.len() > 1 {
        headers[1..].to_vec()
    } else {
        Vec::new()
    };
    let column_head: Vec<String> = if !header_cells.is_empty() {
        header_cells
    } else if rows[0].len() > 1 {
        rows[0][1..].to_vec()
    } else {
        Vec::new()
    };
    let first_col: Vec<String> = rows.iter().map(|row| row.first().cloned().unwrap_or_default()).collect();

    // ① 行 = 教室
    let plain_periods: Vec<String> = if column_head.iter().any(|cell| looks_like_time(cell)) {
        column_head.clone()
    } else {
        Vec::new()
    };
    let mut plain: Vec<Value> = Vec::new();
    for row in &rows {
        let name = row.first().cloned().unwrap_or_default();
        if name.is_empty() || name.chars().count() > 20 || is_chrome_text(&name) {
            continue;
        }
        if is_header_cell(&name) || looks_like_time(&name) {
            continue;
        }
        let mut values: Vec<String> = row[1..].to_vec();
        // 第二列可能是「座位数」（学校页面：JB100 | 258 | 节次…）
        if values.len() >= 3
            && values[0].chars().count() <= 5
            && values[0].chars().all(|ch| ch.is_ascii_digit())
        {
            values.remove(0);
        }
        let mut cells: Vec<Value> = Vec::new();
        for (offset, value) in values.iter().enumerate() {
            let slot = plain_periods
                .get(offset)
                .map(|cell| slot_index(cell))
                .unwrap_or(offset as i64);
            let mut text = tidy(value);
            if !text.is_empty() && text == name {
                text.clear(); // 格子里写的是这间教室自己的名字 → 不是课
            }
            cells.push(make_cell(&text, slot));
        }
        while cells.len() < PERIODS {
            let slot = cells.len() as i64;
            cells.push(json!({ "label": "", "occupied": false, "slot": slot, "raw": "" }));
        }
        plain.push(json!({ "name": name, "cells": cells }));
    }

    // ② 列 = 教室（表头是教室、行首是时间）
    let mut column: Vec<Value> = Vec::new();
    for (offset, name) in column_head.iter().enumerate() {
        if !looks_like_room(name) {
            continue;
        }
        let mut cells = empty_cells();
        for row in &rows {
            if offset + 1 >= row.len() {
                continue;
            }
            let value = tidy(&row[offset + 1]);
            let head = row.first().cloned().unwrap_or_default();
            let mut slot = slot_index(&head);
            if slot < 0 {
                if let Some(position) = first_col.iter().position(|one| one == &head) {
                    slot = position as i64;
                }
            }
            if slot < 0 || !cell_occupied(&value) {
                continue;
            }
            let index = slot as usize;
            if index < cells.len() {
                cells[index] = make_cell(&value, slot);
            }
        }
        column.push(json!({ "name": name, "cells": cells }));
    }

    fn score(rooms: &[Value]) -> f64 {
        if rooms.is_empty() {
            return 0.0;
        }
        let good = rooms
            .iter()
            .filter(|room| looks_like_room(room["name"].as_str().unwrap_or_default()))
            .count();
        good as f64 / rooms.len() as f64
    }

    fn dedupe(rooms: Vec<Value>) -> Vec<Value> {
        let mut order: Vec<String> = Vec::new();
        let mut merged: Vec<Value> = Vec::new();
        let mut busy_counts: Vec<usize> = Vec::new();
        for room in rooms {
            let name = room["name"].as_str().unwrap_or_default().to_string();
            if name.is_empty() {
                continue;
            }
            let busy = room["cells"]
                .as_array()
                .map(|cells| {
                    cells
                        .iter()
                        .filter(|cell| cell["occupied"].as_bool().unwrap_or(false))
                        .count()
                })
                .unwrap_or(0);
            if let Some(index) = order.iter().position(|one| one == &name) {
                if busy > busy_counts[index] {
                    merged[index] = room;
                    busy_counts[index] = busy;
                }
                continue;
            }
            order.push(name);
            merged.push(room);
            busy_counts.push(busy);
        }
        merged
    }

    if !column.is_empty() && score(&column) > score(&plain) {
        let periods: Vec<String> = first_col
            .iter()
            .enumerate()
            .map(|(index, cell)| {
                if cell.is_empty() {
                    format!("第 {} 节", index + 1)
                } else {
                    cell.clone()
                }
            })
            .collect();
        return (dedupe(column), periods, true);
    }
    if !plain.is_empty() {
        return (dedupe(plain), plain_periods, false);
    }
    if !column.is_empty() {
        let periods: Vec<String> = first_col
            .iter()
            .enumerate()
            .map(|(index, cell)| {
                if cell.is_empty() {
                    format!("第 {} 节", index + 1)
                } else {
                    cell.clone()
                }
            })
            .collect();
        return (dedupe(column), periods, true);
    }
    (Vec::new(), Vec::new(), false)
}

/// 「抓回来的一段 JSON」→ 空教室缓存（形状与旧版 `ehall_cache_empty_room` 一致）
pub fn parse(raw: &str, building: &str, room: &str) -> Value {
    let mut data: Value = json!({});
    let text = raw.trim();
    if !text.is_empty() {
        if let Ok(parsed) = serde_json::from_str::<Value>(text) {
            // 有的平台会把结果再包一层字符串
            data = match parsed {
                Value::String(inner) => {
                    serde_json::from_str::<Value>(&inner).unwrap_or(Value::String(inner))
                }
                other => other,
            };
        }
    }
    let mut table: Vec<Vec<String>> = Vec::new();
    if let Some(rows) = data.get("rows").and_then(|value| value.as_array()) {
        for row in rows {
            let cells: Vec<String> = row
                .as_array()
                .map(|cells| {
                    cells
                        .iter()
                        .map(|cell| cell.as_str().unwrap_or_default().trim().to_string())
                        .collect()
                })
                .unwrap_or_default();
            if cells.iter().any(|cell| !cell.is_empty()) {
                table.push(cells);
            }
        }
    }
    let headers: Vec<String> = data
        .get("headers")
        .and_then(|value| value.as_array())
        .map(|cells| {
            cells
                .iter()
                .map(|cell| cell.as_str().unwrap_or_default().trim().to_string())
                .collect()
        })
        .unwrap_or_default();
    let lines: Vec<String> = data
        .get("lines")
        .and_then(|value| value.as_array())
        .map(|rows| rows.iter().take(120).map(|one| one.as_str().unwrap_or_default().to_string()).collect())
        .unwrap_or_default();
    let url = data.get("u").and_then(|value| value.as_str()).unwrap_or_default().to_string();

    let (mut rooms, periods, transposed) = build_rooms(&table, &headers);
    // 只留「像教室」的行：以前抓糊了会把教学楼下拉、金额、日期都当成教室画出来。
    // 一个都不像教室时**清空**（旧版 `rooms = looks` 也是无条件赋值），下面走兜底 / 报错。
    rooms = rooms
        .iter()
        .filter(|one| looks_like_room(one["name"].as_str().unwrap_or_default()))
        .cloned()
        .collect();
    if !room.is_empty() {
        let filtered: Vec<Value> = rooms
            .iter()
            .filter(|one| one["name"].as_str().unwrap_or_default().contains(room))
            .cloned()
            .collect();
        if !filtered.is_empty() {
            rooms = filtered;
        }
    }

    if !rooms.is_empty() {
        rooms.truncate(60);
        let value = format!("{} 间教室", rooms.len());
        return json!({
            "ok": true,
            "from_table": true,
            "rooms": rooms,
            "periods": periods,
            "headers": headers,
            "rows": table.iter().take(60).collect::<Vec<_>>(),
            "lines": lines,
            "transposed": transposed,
            "building": building,
            "room": room,
            "url": url,
            "value": value,
            "error": "",
            "ts": chrono::Local::now().timestamp(),
        });
    }

    // 兜底：只有短行文本（例如「H3101 1-2 空闲」），按行拆成单格。
    // 注意要**至少两段**：只有一段的行（页面上的数字、日期）不是教室行——
    // 抓错页面时（比如停在 eHall 首页）以前会把「22.37」「2026-10-12」当成教室画出来。
    let mut fallback: Vec<Value> = Vec::new();
    for line in &lines {
        if is_chrome_text(line) || line.chars().count() > 40 {
            continue;
        }
        let parts: Vec<String> = line
            .split(|ch: char| ch.is_whitespace() || ch == '|' || ch == '｜' || ch == '\t')
            .filter(|part| !part.trim().is_empty())
            .map(|part| part.trim().to_string())
            .collect();
        if parts.len() < 2 {
            continue;
        }
        let Some(name) = parts.first().cloned() else {
            continue;
        };
        if name.chars().count() > 12 || !looks_like_room(&name) {
            continue;
        }
        let label = parts[1..].join(" ");
        fallback.push(json!({
            "name": name,
            "cells": [{
                "label": if cell_occupied(&label) { label.clone() } else { String::new() },
                "occupied": cell_occupied(&label),
                "slot": 0,
                "raw": label,
            }],
        }));
    }
    if !fallback.is_empty() {
        fallback.truncate(40);
        let value = format!("{} 间教室", fallback.len());
        return json!({
            "ok": true,
            "from_table": false,
            "rooms": fallback,
            "periods": Value::Null,
            "headers": headers,
            "rows": table.iter().take(60).collect::<Vec<_>>(),
            "lines": lines,
            "transposed": transposed,
            "building": building,
            "room": room,
            "url": url,
            "value": value,
            "error": "",
            "ts": chrono::Local::now().timestamp(),
        });
    }

    json!({
        "ok": false,
        "from_table": false,
        "rooms": [],
        "periods": periods,
        "headers": headers,
        "rows": table.iter().take(60).collect::<Vec<_>>(),
        "lines": lines,
        "transposed": transposed,
        "building": building,
        "room": room,
        "url": url,
        "value": "",
        "error": if lines.is_empty() {
            "页面上没找到教室表：确认地址栏是「空闲教室查询」页，再点「查询」"
        } else {
            "抓到的不是教室表（看着像教学楼 / 下拉列表）：确认地址栏是「空闲教室查询」页，再点「查询」"
        },
        "ts": chrono::Local::now().timestamp(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(rows: &[&[&str]]) -> Vec<Vec<String>> {
        rows.iter()
            .map(|row| row.iter().map(|cell| cell.to_string()).collect())
            .collect()
    }

    #[test]
    fn row_per_room_grid() {
        let rows = table(&[
            &["教室", "座位", "第1节", "第2节", "第3节"],
            &["H3101", "60", "本 理学 MATH10031.17 [线性代数A](司然){130人}", "", "空闲"],
            &["H3102", "60", "", "", ""],
            &["二教", "", "", "", ""],
        ]);
        let (rooms, _periods, transposed) = build_rooms(&rows, &[]);
        assert!(!transposed);
        // 楼名（二教）在 build_rooms 里还留着，`parse` 才按「像不像教室」过滤（与旧版一致）
        let names: Vec<&str> = rooms
            .iter()
            .map(|room| room["name"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(names, vec!["H3101", "H3102", "二教"]);
        let first = &rooms[0]["cells"][0];
        assert_eq!(first["occupied"], json!(true));
        assert_eq!(first["label"], json!("线性代数A"));
        assert_eq!(rooms[0]["cells"][2]["occupied"], json!(false));
        assert_eq!(rooms[1]["cells"].as_array().unwrap().len(), PERIODS);

        // 走完整的 parse：楼名被丢掉，只剩真教室
        let raw = r#"{"u":"https://ehall.fudan.edu.cn/x","headers":[],"rows":[
            ["教室","座位","第1节","第2节","第3节"],
            ["H3101","60","本 理学 MATH10031.17 [线性代数A](司然){130人}","","空闲"],
            ["H3102","60","","",""],
            ["二教","","","",""]
        ],"lines":[]}"#;
        let payload = parse(raw, "", "");
        let parsed: Vec<&str> = payload["rooms"]
            .as_array()
            .unwrap()
            .iter()
            .map(|room| room["name"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(parsed, vec!["H3101", "H3102"]);
    }

    #[test]
    fn column_per_room_grid_is_transposed() {
        let rows = table(&[
            &["时间", "H2101", "H2102"],
            &["08:00-08:45", "高等数学", ""],
            &["08:55-09:40", "", "大学物理"],
        ]);
        let (rooms, periods, transposed) = build_rooms(&rows, &[]);
        assert!(transposed);
        assert!(!periods.is_empty());
        assert_eq!(rooms.len(), 2);
        assert_eq!(rooms[0]["name"], json!("H2101"));
        // 第一节落在 slot 0，第二节落在 slot 1
        assert_eq!(rooms[0]["cells"][0]["occupied"], json!(true));
        assert_eq!(rooms[1]["cells"][1]["occupied"], json!(true));
        assert_eq!(rooms[1]["cells"][0]["occupied"], json!(false));
    }

    #[test]
    fn parse_reports_error_when_page_has_no_table() {
        let payload = parse(r#"{"u":"https://ehall.fudan.edu.cn/","rows":[],"lines":["空闲教室查询"]}"#, "", "");
        assert_eq!(payload["ok"], json!(false));
        assert!(!payload["error"].as_str().unwrap_or_default().is_empty());
    }

    #[test]
    fn parse_marks_ok_with_rooms() {
        let raw = r#"{"u":"https://ehall.fudan.edu.cn/x","headers":[],"rows":[["教室","第1节"],["H3101","高数"]],"lines":[]}"#;
        let payload = parse(raw, "三教", "");
        assert_eq!(payload["ok"], json!(true));
        assert_eq!(payload["rooms"][0]["name"], json!("H3101"));
        assert_eq!(payload["value"], json!("1 间教室"));
        assert_eq!(payload["building"], json!("三教"));
    }

    #[test]
    fn slot_index_understands_labels() {
        assert_eq!(slot_index("第 3 节"), 2);
        assert_eq!(slot_index("十二"), 11);
        assert_eq!(slot_index("09:55"), 2);
        assert_eq!(slot_index("随便写的"), -1);
    }

    #[test]
    fn room_names_are_not_buildings() {
        assert!(looks_like_room("H3101"));
        assert!(looks_like_room("JB100"));
        assert!(!looks_like_room("二教"));
        assert!(!looks_like_room("空闲教室"));
        assert!(!looks_like_room("08:00-08:45"));
    }

    /// 停在 eHall 首页时点「抓取当前页面」：那些数字/日期不能被当成教室，
    /// 更不能标记成「可以写缓存」把已查好的结果覆盖掉。
    #[test]
    fn wrong_page_does_not_produce_rooms() {
        let raw = r#"{"u":"https://ehall.fudan.edu.cn/","headers":[],"rows":[],
            "lines":["22.37","2026-10-12","2244.8","我的待办","快速链接"]}"#;
        let payload = parse(raw, "", "");
        assert_eq!(payload["ok"], json!(false));
        assert_eq!(payload["from_table"], json!(false));
        assert_eq!(payload["rooms"], json!([]));
    }

    /// eHall 首页的「我的待办」是一张真表格：只有两行、第一格是金额和日期，
    /// 这种不能被当成教室表（比旧版多挡了小数和日期）。
    #[test]
    fn dashboard_table_is_not_a_classroom_table() {
        let raw = r#"{"u":"https://ehall.fudan.edu.cn/","headers":[],"rows":[
            ["服务名称","申请人","申请时间"],
            ["22.37","2026-10-12","2244.8"]
        ],"lines":["我的待办"]}"#;
        let payload = parse(raw, "", "");
        assert_eq!(payload["ok"], json!(false));
        assert_eq!(payload["rooms"], json!([]));
    }

    /// 正文兜底要至少两段（「H3101 空闲」可以，「22.37」不行）
    #[test]
    fn fallback_lines_need_two_parts() {
        let raw = r#"{"u":"https://ehall.fudan.edu.cn/x","headers":[],"rows":[],
            "lines":["H3101 空闲","22.37","H3102 第 3 节 高数"]}"#;
        let payload = parse(raw, "", "");
        assert_eq!(payload["ok"], json!(true));
        assert_eq!(payload["from_table"], json!(false));
        let names: Vec<&str> = payload["rooms"]
            .as_array()
            .unwrap()
            .iter()
            .map(|room| room["name"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(names, vec!["H3101", "H3102"]);
    }
}
