use super::*;

fn watch(pattern: &str) -> Watch {
    Watch::new(Regex::new(pattern).unwrap())
}

fn body(notice: Option<String>) -> String {
    let notice = notice.expect("应有通知");
    notice.split_once('\n').unwrap().1.to_owned()
}

#[tokio::test(start_paused = true)]
async fn cooldown_batches_and_cap_stops() {
    let mut w = watch("hit");
    assert_eq!(
        body(w.feed(Stream::Stdout, b"hit 1\nhit 2\n")),
        "stdout: hit 1"
    );
    assert!(w.feed(Stream::Stderr, b"hit 3\n").is_none());
    let due = w.due().expect("冷却内有攒下的命中");
    assert_eq!(due - Instant::now(), NOTIFY_COOLDOWN);

    // 冷却结束前的新命中也进同一批，不越过冷却。
    tokio::time::advance(NOTIFY_COOLDOWN).await;
    assert!(w.feed(Stream::Stdout, b"hit 4\n").is_none());
    let notice = w.flush().unwrap();
    assert!(notice.starts_with("notify_pattern matched 3 lines during cooldown (notice 2/"));
    assert!(notice.ends_with("\nstdout: hit 2\nstderr: hit 3\nstdout: hit 4"));
    assert!(w.due().is_none() && w.flush().is_none());

    // 超出批量只留最近几行。
    for n in 0..NOTIFY_BATCH_LINES + 2 {
        assert!(w
            .feed(Stream::Stdout, format!("hit {n}\n").as_bytes())
            .is_none());
    }
    tokio::time::advance(NOTIFY_COOLDOWN).await;
    let notice = w.flush().unwrap();
    assert!(notice.contains(&format!(
        "{} lines during cooldown, latest {NOTIFY_BATCH_LINES} shown",
        NOTIFY_BATCH_LINES + 2
    )));
    assert!(notice.contains("\nstdout: hit 2\n") && !notice.contains("hit 1\n"));

    for n in 4..=NOTIFY_MAX {
        tokio::time::advance(NOTIFY_COOLDOWN).await;
        let notice = w.feed(Stream::Stdout, b"hit\n").unwrap();
        assert_eq!(
            notice.contains("further matches will not be reported"),
            n == NOTIFY_MAX
        );
    }
    assert!(w.feed(Stream::Stdout, b"hit\n").is_none() && w.due().is_none());
}

#[tokio::test(start_paused = true)]
async fn long_lines_are_matched_in_marked_pieces() {
    // 切点落在 `é` 中间，退到字符边界；这一片仍是合法 UTF-8。
    let mut w = watch("^aé+$");
    let mut long = b"a".to_vec();
    long.extend_from_slice("é".repeat(NOTIFY_LINE_BYTES / 2).as_bytes());
    long.extend_from_slice(b" tail\n");
    let notice = w.feed(Stream::Stdout, &long).unwrap();
    assert_eq!(
        body(Some(notice)),
        format!(
            "stdout ({NOTIFY_LINE_BYTES}-byte piece of a long line): a{}",
            "é".repeat(NOTIFY_LINE_BYTES / 2 - 1)
        )
    );
    assert!(w.lines(Stream::Stdout).buf.is_empty());

    // 超长行的收尾段同样标为片段；之后的普通行恢复整行。
    let mut w = watch("tail|short");
    assert!(w
        .feed(Stream::Stderr, &vec![b'x'; NOTIFY_LINE_BYTES + 1])
        .is_none());
    let notice = w.feed(Stream::Stderr, b"x tail\n").unwrap();
    assert!(notice.contains("piece of a long line") && notice.ends_with(": xx tail"));
    tokio::time::advance(NOTIFY_COOLDOWN).await;
    let notice = w.feed(Stream::Stderr, b"short\n").unwrap();
    assert!(notice.contains("(notice 2/") && !notice.contains("piece"));
    assert!(w.finish(Stream::Stderr).is_none());
}
