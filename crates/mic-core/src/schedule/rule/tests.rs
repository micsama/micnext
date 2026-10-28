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

#[test]
fn min_gap_covers_irregular_cron_and_skips_one_off() {
    let now = Local
        .with_ymd_and_hms(2026, 10, 21, 10, 6, 0)
        .unwrap()
        .timestamp_millis();
    let gap = |rule: &str| Rule::parse(rule).unwrap().min_gap(now);
    assert_eq!(gap("* * * * *"), Some(60_000));
    assert_eq!(gap("*/10 * * * *"), Some(600_000));
    assert_eq!(gap("0,3 * * * *"), Some(180_000));
    assert_eq!(gap("2026-11-01 09:00"), None);
}
