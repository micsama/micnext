use chrono::{Local, TimeZone};

use super::Rule;

/// 游标带毫秒且略早于整点时，下一次仍是该整点，不会跳过一次。
#[test]
fn cron_next_lands_on_whole_minutes() {
    let rule = Rule::parse("* * * * *").unwrap();
    let minute = Local
        .with_ymd_and_hms(2026, 10, 21, 10, 6, 0)
        .unwrap()
        .timestamp_millis();
    assert_eq!(rule.next_after(minute - 60_000 + 40), Some(minute));
    assert_eq!(rule.next_after(minute - 1), Some(minute));
    assert_eq!(rule.next_after(minute), Some(minute + 60_000));
    assert_eq!(rule.next_after(minute + 500), Some(minute + 60_000));
}
