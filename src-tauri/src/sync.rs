//! 多端同步（桌面端）：把本地比游标新的日程和删除墓碑推给 `/api/v1/sync`，
//! 再把服务器回来的记录按「后写胜出」合并进本地库。
//!
//! 契约见 `_开发文档.md` §13：稳定 `uid`、UTC ISO 的 `updated_at`、墓碑 `deleted=true`。
//! 游标存在 settings 的 `sync_cursor`（服务器回的 `server_time`）。

use serde_json::{json, Value};

/// 同步一次，返回给前端的摘要
pub fn sync_now() -> Result<Value, String> {
    let store = crate::get_store();
    let cursor = store.setting("sync_cursor").unwrap_or_default();

    let mut items: Vec<Value> = Vec::new();
    for row in store.sync_events_since(&cursor).map_err(|e| e.to_string())? {
        let payload = json!({
            "title": row.title,
            "description": row.description,
            "date": row.date,
            "start_time": row.start_time,
            "end_time": row.end_time,
            "remind_minutes": row.remind_minutes,
            "color": row.color,
            "done": row.done,
        });
        items.push(json!({
            "kind": "event",
            "uid": row.uid,
            "payload": payload.to_string(),
            "updated_at": row.updated_at,
            "deleted": false,
        }));
    }
    for (uid, deleted_at) in store
        .sync_tombstones_since(&cursor)
        .map_err(|e| e.to_string())?
    {
        items.push(json!({
            "kind": "event",
            "uid": uid,
            "payload": "{}",
            "updated_at": deleted_at,
            "deleted": true,
        }));
    }

    let pushed = items.len();
    let body = json!({ "since": cursor, "items": items });
    let reply = crate::account::post_json_auth(&store, "/api/v1/sync", &body)
        .map_err(|e| e.to_string())?;
    let server_time = reply
        .get("server_time")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();

    let mut applied = 0usize;
    if let Some(list) = reply.get("items").and_then(|value| value.as_array()) {
        for item in list {
            if item.get("kind").and_then(|value| value.as_str()) != Some("event") {
                continue;
            }
            let uid = item.get("uid").and_then(|value| value.as_str()).unwrap_or("");
            let updated_at = item
                .get("updated_at")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let deleted = item
                .get("deleted")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            if uid.is_empty() || updated_at.is_empty() {
                continue;
            }
            let payload: Value = item
                .get("payload")
                .and_then(|value| value.as_str())
                .and_then(|text| serde_json::from_str(text).ok())
                .unwrap_or(Value::Null);
            store
                .sync_apply_event(uid, &payload, updated_at, deleted)
                .map_err(|e| e.to_string())?;
            applied += 1;
        }
    }

    if !server_time.is_empty() {
        let _ = store.set_setting("sync_cursor", &server_time);
    }
    Ok(json!({
        "ok": true,
        "pushed": pushed,
        "applied": applied,
        "server_time": server_time,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(name: &str) -> crate::store::Store {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("LUMINA_DATA_DIR", &dir);
        crate::store::Store::open().expect("建临时库失败")
    }

    #[test]
    fn sync_apply_event_inserts_then_updates() {
        let store = temp_store("danlume-sync-apply");
        let payload = json!({
            "title": "同步来的日程",
            "description": "",
            "date": "2026-11-24",
            "start_time": "09:00",
            "end_time": "10:00",
            "remind_minutes": 10,
            "color": "violet",
            "done": false,
        });
        store
            .sync_apply_event("uid-1", &payload, "2026-11-24T01:00:00Z", false)
            .unwrap();
        assert_eq!(store.events_on("2026-11-24").unwrap().len(), 1);
        // 更旧的时间戳不会覆盖
        let older = json!({"title": "旧标题", "date": "2026-11-24"});
        store
            .sync_apply_event("uid-1", &older, "2026-11-23T01:00:00Z", false)
            .unwrap();
        let rows = store.events_on("2026-11-24").unwrap();
        assert_eq!(rows[0].title, "同步来的日程");
        // 更新的会覆盖
        let newer = json!({"title": "新标题", "date": "2026-11-24", "start_time": "11:00", "end_time": "12:00"});
        store
            .sync_apply_event("uid-1", &newer, "2026-11-25T01:00:00Z", false)
            .unwrap();
        let rows = store.events_on("2026-11-24").unwrap();
        assert_eq!(rows[0].title, "新标题");
        // 墓碑删除
        store
            .sync_apply_event("uid-1", &Value::Null, "2026-11-26T01:00:00Z", true)
            .unwrap();
        assert!(store.events_on("2026-11-24").unwrap().is_empty());
    }

    #[test]
    fn delete_records_tombstone() {
        let store = temp_store("danlume-sync-tombstone");
        let id = store
            .add_event(&crate::store::EventInput {
                title: Some("要删的".to_string()),
                description: None,
                date: Some("2026-11-24".to_string()),
                start_time: None,
                end_time: None,
                remind_minutes: None,
                clear_remind: false,
                color: None,
                done: None,
                repeat: None,
            })
            .unwrap();
        assert!(store.delete_event(id).unwrap());
        let tombstones = store.sync_tombstones_since("2000-01-01T00:00:00Z").unwrap();
        assert_eq!(tombstones.len(), 1);
    }
}
