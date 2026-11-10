//! eHall 卡片"会话直连"刷新（对应旧版 `ehall_bridge.py`）。
//!
//! 与粘贴抓取的关系：解析逻辑完全复用 `ehall_grab::store_cache`（同一套正则/字段），
//! 这里只负责"带着 UIS 会话把页面取回来"。取不到就报错并保留原有缓存，不清空。
use serde_json::{json, Value};

use crate::store::Store;
use crate::uis;

const CARDS: [(&str, &str, &str); 6] = [
    ("card", "校园卡余额", "ehall_json_card"),
    ("bus", "校车安排", "ehall_json_bus"),
    ("sports", "体育刷锻", "ehall_json_sports"),
    ("library_people", "图书馆人数", "ehall_json_library_people"),
    ("ehall_todo", "我的 ehall 通知", "ehall_json_ehall_todo"),
    ("empty_room", "空教室", "ehall_json_empty_room"),
];

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(25))
        .redirects(5)
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/120.0 Safari/537.36",
        )
        .build()
}

fn card_url(store: &Store, source_key: &str) -> String {
    store
        .setting(source_key)
        .filter(|text| !text.trim().is_empty())
        .or_else(|| {
            store
                .setting("ehall_url")
                .filter(|text| !text.trim().is_empty())
        })
        .unwrap_or_default()
}

/// 用会话取一页（返回最终 URL 与正文）
fn fetch_page(jar: &uis::Jar, url: &str) -> Result<(String, String), String> {
    let response = agent()
        .get(url)
        .set("Cookie", &jar.to_header())
        .call()
        .map_err(|exc| format!("打开 {url} 失败：{exc}"))?;
    let final_url = response.get_url().to_string();
    let text = response
        .into_string()
        .map_err(|exc| format!("读页面失败：{exc}"))?;
    Ok((final_url, text))
}

/// 刷新全部已配置地址的卡片；返回每张卡的结果
pub fn refresh_all(store: &Store) -> Value {
    let mut results: Vec<Value> = Vec::new();
    let mut need_login = false;
    for (key, title, source_key) in CARDS {
        let url = card_url(store, source_key);
        if url.is_empty() {
            results.push(json!({ "key": key, "title": title, "ok": false, "error": "没填地址" }));
            continue;
        }
        let jar = match uis::visit_with_session(store, &url) {
            Ok((final_url, jar)) => {
                if final_url.contains("id.fudan.edu.cn") {
                    need_login = true;
                    results.push(json!({
                        "key": key, "title": title, "ok": false,
                        "error": "会话无效（被踢到统一身份认证）",
                    }));
                    continue;
                }
                jar
            }
            Err(exc) => {
                results.push(json!({ "key": key, "title": title, "ok": false, "error": exc }));
                continue;
            }
        };
        match fetch_page(&jar, &url) {
            Ok((final_url, text)) => {
                if final_url.contains("id.fudan.edu.cn") {
                    need_login = true;
                    results.push(json!({
                        "key": key, "title": title, "ok": false,
                        "error": "会话无效（页面跳到登录页）",
                    }));
                    continue;
                }
                // 校车卡按方向筛选（旧版 dashboard_bus_directions）
                let text = if key == "bus" {
                    filter_by_directions(&text, &bus_directions(store))
                } else {
                    text
                };
                // 复用粘贴抓取那套解析 + 写缓存
                let stored = crate::ehall_grab::store_cache(store, key, &url, &text);
                let ok = stored.get("ok").and_then(|one| one.as_bool()).unwrap_or(false);
                results.push(json!({
                    "key": key, "title": title, "ok": ok,
                    "value": stored.get("value").cloned().unwrap_or(json!("")),
                    "cache": stored,
                }));
            }
            Err(exc) => {
                results.push(json!({ "key": key, "title": title, "ok": false, "error": exc }));
            }
        }
    }
    let ok_count = results
        .iter()
        .filter(|one| one["ok"] == true)
        .count();
    json!({
        "ok": ok_count > 0,
        "refreshed": ok_count,
        "total": results.len(),
        "need_login": need_login,
        "cards": results,
    })
}


/// 旧版 `dashboard_bus_directions`：校车卡只看指定方向（逗号分隔，任一命中即保留该行）
pub fn bus_directions(store: &Store) -> Vec<String> {
    store
        .setting("dashboard_bus_directions")
        .unwrap_or_default()
        .split([',', '，', ';', '、', '\n'])
        .map(|one| one.trim().to_string())
        .filter(|one| !one.is_empty())
        .collect()
}

/// 按方向关键字过滤页面文本：保留含任一关键字的行（没有关键字就原样返回）
fn filter_by_directions(text: &str, directions: &[String]) -> String {
    if directions.is_empty() {
        return text.to_string();
    }
    let kept: Vec<&str> = text
        .lines()
        .filter(|line| directions.iter().any(|one| line.contains(one)))
        .collect();
    if kept.is_empty() {
        // 一行都没命中：宁可给原文，也不要给出空结果让用户以为没数据
        return text.to_string();
    }
    kept.join("\n")
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_table_matches_dashboard_keys() {
        let keys: Vec<&str> = CARDS.iter().map(|(key, _, _)| *key).collect();
        assert_eq!(
            keys,
            vec!["card", "bus", "sports", "library_people", "ehall_todo", "empty_room"]
        );
        // 每张卡的地址键名要和旧版/现有 dashboard 一致
        assert!(CARDS.iter().all(|(_, _, source)| source.starts_with("ehall_json_")));
    }
}
