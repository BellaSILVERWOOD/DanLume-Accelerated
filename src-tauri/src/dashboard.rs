//! 仪表板取数：与旧版 `lumina/dashboard.py` 的 `upcoming_events()` 同语义，
//! 包括「已经开始的写进行中」这条。
use chrono::{Duration, Local, NaiveDate, NaiveDateTime, NaiveTime};
use serde::Serialize;

use crate::store::{EventRow, Store};

#[derive(Debug, Clone, Serialize)]
pub struct UpcomingItem {
    #[serde(flatten)]
    pub event: EventRow,
    pub day: String,
    pub begin: String,
    pub finish: String,
    pub starts: String,
    pub ongoing: bool,
    pub minutes_until: i64,
}

fn parse_dt(day: &str, clock: &str) -> Option<NaiveDateTime> {
    let date = NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
    let time = NaiveTime::parse_from_str(clock, "%H:%M").ok()?;
    Some(NaiveDateTime::new(date, time))
}

/// 从现在起 `horizon_days` 天内还没结束的日程；`ongoing` = 已经开始还没结束
pub fn upcoming_events(store: &Store, limit: usize, horizon_days: i64) -> Vec<UpcomingItem> {
    let now = Local::now().naive_local();
    let today = now.date();
    let start = today.format("%Y-%m-%d").to_string();
    let end = (today + Duration::days(horizon_days.max(1)))
        .format("%Y-%m-%d")
        .to_string();

    let index = match store.event_index(&start, &end) {
        Ok(value) => value,
        Err(_) => return Vec::new(),
    };

    let mut items: Vec<UpcomingItem> = Vec::new();
    for (day, events) in index {
        for event in events {
            let begin = event.start_time.chars().take(5).collect::<String>();
            let finish = event.end_time.chars().take(5).collect::<String>();
            let Some(starts) = parse_dt(&day, &begin) else {
                continue;
            };
            let mut ends = if finish.is_empty() {
                starts
            } else {
                parse_dt(&day, &finish).unwrap_or(starts)
            };
            if ends < starts {
                // 跨夜：23:40 → 00:10
                ends += Duration::days(1);
            }
            if ends < now {
                continue;
            }
            items.push(UpcomingItem {
                day: day.clone(),
                begin,
                finish,
                starts: starts.format("%Y-%m-%dT%H:%M:%S").to_string(),
                ongoing: starts <= now && now < ends,
                minutes_until: (starts - now).num_minutes(),
                event,
            });
        }
    }
    items.sort_by(|a, b| a.starts.cmp(&b.starts));
    items.truncate(limit.max(1));
    items
}

/// 「第 N 周」：旧版显示的是**当年的 ISO 周数**（例如 10-30 是第 44 周）
pub fn week_number(_store: &Store) -> i64 {
    use chrono::Datelike;

    let today = Local::now().date_naive();
    today.iso_week().week() as i64
}
