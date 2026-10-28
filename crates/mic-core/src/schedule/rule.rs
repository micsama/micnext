//! 规则：5 段 cron 或 `YYYY-MM-DD HH:MM` 一次性时刻，服务器本地时区。

use std::str::FromStr;

use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use croner::Cron;

pub(crate) enum Rule {
    Cron(Box<Cron>),
    Once(i64),
}

impl Rule {
    /// 失败文案面向模型，英文。
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        if let Ok(at) = NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M") {
            let at = Local
                .from_local_datetime(&at)
                .earliest()
                .ok_or_else(|| format!("{text} does not exist in the server's time zone"))?;
            return Ok(Self::Once(at.timestamp_millis()));
        }
        if text.split_whitespace().count() != 5 {
            return Err(
                "when must be a 5-field cron expression or a one-off time YYYY-MM-DD HH:MM".into(),
            );
        }
        Cron::from_str(text)
            .map(|c| Self::Cron(Box::new(c)))
            .map_err(|e| format!("invalid cron expression: {e}"))
    }

    /// 严格晚于 `after` 的下一次时刻（unix 毫秒）。croner 保留输入的亚秒部分，
    /// 故先取整到秒，结果才落在整点上。
    pub(crate) fn next_after(&self, after: i64) -> Option<i64> {
        match self {
            Self::Once(at) => (*at > after).then_some(*at),
            Self::Cron(cron) => cron
                .find_next_occurrence(&local(after.div_euclid(1000) * 1000), false)
                .ok()
                .map(|t| t.timestamp_millis()),
        }
    }

    /// 从 `after` 起接下来一批触发里相邻两次的最短间隔（毫秒）；一次性时刻为空。
    /// cron 的密集段总在连续的触发里出现，取前 1000 次足够。
    pub(crate) fn min_gap(&self, after: i64) -> Option<i64> {
        let Self::Cron(_) = self else { return None };
        let mut at = self.next_after(after)?;
        let mut gap = i64::MAX;
        for _ in 0..1000 {
            let Some(next) = self.next_after(at) else {
                break;
            };
            gap = gap.min(next - at);
            at = next;
        }
        Some(gap)
    }
}

fn local(ms: i64) -> DateTime<Local> {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .expect("unix 毫秒在本地时区唯一")
}

/// 给用户看的本地时刻。
pub(crate) fn show(ms: i64) -> String {
    local(ms).format("%Y-%m-%d %H:%M").to_string()
}

/// 给模型看的本地时刻，带 UTC 偏移以表明服务器时区。
pub(crate) fn show_zoned(ms: i64) -> String {
    local(ms).format("%Y-%m-%d %H:%M %:z").to_string()
}

#[cfg(test)]
mod tests;
