use super::*;

#[tokio::test]
async fn same_name_gets_numbered_and_leaves_others_untouched() {
    let dir = std::env::temp_dir().join(format!("mic-input-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let first = save(&dir, "a.txt.part", Source::Bytes(b"keep"))
        .await
        .unwrap();
    let second = save(&dir, "a.txt", Source::Bytes(b"one")).await.unwrap();
    let third = save(&dir, "a.txt", Source::Bytes(b"two")).await.unwrap();

    assert_eq!(second.file_name().unwrap(), "a.txt");
    assert_eq!(third.file_name().unwrap(), "a (2).txt");
    assert_eq!(std::fs::read(&first).unwrap(), b"keep");
    assert_eq!(std::fs::read(&second).unwrap(), b"one");
    assert_eq!(std::fs::read(&third).unwrap(), b"two");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn file_name_keeps_last_segment() {
    assert_eq!(file_name("../../etc/passwd"), "passwd");
    assert_eq!(file_name("C:\\x\\报告.pdf"), "报告.pdf");
    assert_eq!(file_name(".."), "文件");
    assert_eq!(file_name("a\nb"), "文件");
}
