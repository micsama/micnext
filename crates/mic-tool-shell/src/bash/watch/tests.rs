use std::time::Duration;

use super::*;

fn watch(pattern: &str) -> Watch {
    Watch::new(Regex::new(pattern).unwrap())
}

fn body(notice: Option<String>) -> String {
    let notice = notice.expect("应有通知");
    notice.split_once('\n').unwrap().1.to_owned()
}

#[tokio::test(start_paused = true)]
async fn cooldown_drops_and_cap_stops() {
    let mut w = watch("hit");
    assert!(w.feed(Stream::Stdout, b"hit 1\n").is_some());
    assert!(w.feed(Stream::Stderr, b"hit 2\n").is_none());
    tokio::time::advance(NOTIFY_COOLDOWN - Duration::from_millis(1)).await;
    assert!(w.feed(Stream::Stdout, b"hit 3\n").is_none());
    tokio::time::advance(Duration::from_millis(1)).await;
    assert_eq!(body(w.feed(Stream::Stdout, b"hit 4\nhit 5\n")), "hit 4");
    for n in 3..=NOTIFY_MAX {
        tokio::time::advance(NOTIFY_COOLDOWN).await;
        let notice = w.feed(Stream::Stdout, b"hit\n").unwrap();
        assert_eq!(
            notice.contains("further matches will not be reported"),
            n == NOTIFY_MAX
        );
    }
    tokio::time::advance(NOTIFY_COOLDOWN).await;
    assert!(w.feed(Stream::Stdout, b"hit\n").is_none());
}

#[tokio::test(start_paused = true)]
async fn long_lines_are_matched_in_marked_pieces() {
    // 切点落在 `é` 中间，退到字符边界；这一片仍是合法 UTF-8。
    let mut w = watch("^aé+$");
    let mut long = b"a".to_vec();
    long.extend_from_slice("é".repeat(NOTIFY_LINE_BYTES / 2).as_bytes());
    long.extend_from_slice(b" tail\n");
    let notice = w.feed(Stream::Stdout, &long).unwrap();
    assert!(notice.starts_with(&format!(
        "stdout matched notify_pattern (a {NOTIFY_LINE_BYTES}-byte piece of a long line):\n"
    )));
    assert_eq!(
        body(Some(notice)),
        format!("a{}", "é".repeat(NOTIFY_LINE_BYTES / 2 - 1))
    );
    assert!(w.lines(Stream::Stdout).buf.is_empty());

    // 超长行的收尾段同样标为片段；之后的普通行恢复整行。
    let mut w = watch("tail|short");
    assert!(w
        .feed(Stream::Stderr, &vec![b'x'; NOTIFY_LINE_BYTES + 1])
        .is_none());
    let notice = w.feed(Stream::Stderr, b"x tail\n").unwrap();
    assert!(notice.contains("piece of a long line") && notice.ends_with("\nxx tail"));
    tokio::time::advance(NOTIFY_COOLDOWN).await;
    let notice = w.feed(Stream::Stderr, b"short\n").unwrap();
    assert!(notice.contains("(a line)"));
    assert!(w.finish(Stream::Stderr).is_none());
}
