use super::*;

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "micnext-output-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn segs(dir: PathBuf) -> Segments {
    Segments {
        dir,
        segs: VecDeque::new(),
        file: None,
        total: 0,
    }
}

#[test]
fn keeps_only_recent_output_with_global_offsets() {
    let dir = scratch();
    let mut s = segs(dir.clone());
    let chunk = vec![b'a'; 100_000];
    for _ in 0..30 {
        s.append(&chunk).unwrap();
    }
    s.append(b"END").unwrap();
    let total = 3_000_003;
    let tail = read(&dir, None, 3).unwrap();
    assert_eq!(tail.text, "END");
    assert_eq!(
        (tail.offset, tail.total_bytes, tail.eof),
        (total - 3, total, true)
    );
    // 只保留最近 KEEP 以上、不足 KEEP + 一段。
    assert!(total - tail.available_from >= JOB_OUTPUT_KEEP);
    assert!(total - tail.available_from < JOB_OUTPUT_KEEP + JOB_OUTPUT_SEGMENT);
    assert_eq!(tail.available_from % JOB_OUTPUT_SEGMENT, 0);
    // 早于可读范围的偏移从最早处读，跨段连续。
    let early = read(&dir, Some(0), JOB_OUTPUT_SEGMENT + 10).unwrap();
    assert_eq!(early.offset, tail.available_from);
    assert_eq!(early.text.len() as u64, JOB_OUTPUT_SEGMENT + 10);
    assert!(!early.eof);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn trims_split_utf8_at_both_ends() {
    let dir = scratch();
    let mut s = segs(dir.clone());
    s.append("你好世界".as_bytes()).unwrap();
    // 从「好」的第二个字节开始、到「世」的中间结束。
    let out = read(&dir, Some(4), 5).unwrap();
    assert_eq!(out.text, "世");
    assert_eq!((out.offset, out.next_offset), (6, 9));
    let missing = read(&dir.join("none"), None, 10).unwrap();
    assert_eq!(
        (missing.total_bytes, missing.eof, missing.text.as_str()),
        (0, true, "")
    );
    fs::remove_dir_all(dir).unwrap();
}
