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

    pub(crate) fn is_once(&self) -> bool {
        matches!(self, Self::Once(_))
    }

    /// 严格晚于 `after` 的下一次时刻（unix 毫秒）。
    pub(crate) fn next_after(&self, after: i64) -> Option<i64> {
        match self {
            Self::Once(at) => (*at > after).then_some(*at),
            Self::Cron(cron) => cron
                .find_next_occurrence(&local(after), false)
                .ok()
                .map(|t| t.timestamp_millis()),
        }
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
