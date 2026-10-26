use super::*;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mic-send-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("work/sub")).unwrap();
    dir
}

#[tokio::test]
async fn snapshot_copies_files_apart_from_their_sources() {
    let dir = temp("copy");
    let work = dir.join("work");
    std::fs::write(work.join("图.html"), "<p>1</p>").unwrap();
    std::fs::write(work.join("sub/图.html"), "<p>2</p>").unwrap();
    std::fs::write(work.join("a.bin"), [0u8, 255, 7]).unwrap();
    let paths = ["图.html".into(), "sub/图.html".into(), "a.bin".into()];
    let (refs, saved) = snapshot(&dir.join("files"), PersonId(1), &work, &paths)
        .await
        .unwrap();
    saved.keep();
    std::fs::write(work.join("图.html"), "changed").unwrap();

    let names: Vec<_> = refs
        .iter()
        .map(|f| Path::new(&f.path).file_name().unwrap().to_str().unwrap())
        .collect();
    assert_eq!(names, ["图.html", "图 (2).html", "a.bin"]);
    assert_eq!(std::fs::read(&refs[0].path).unwrap(), b"<p>1</p>");
    assert_eq!(std::fs::read(&refs[2].path).unwrap(), [0u8, 255, 7]);
    assert_eq!(refs[2].size_bytes, 3);
    assert!(refs[0].path.contains("/files/1/"), "{}", refs[0].path);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn any_unusable_path_rejects_the_whole_call() {
    let dir = temp("reject");
    let work = dir.join("work");
    std::fs::write(work.join("ok.txt"), "ok").unwrap();
    let files = dir.join("files");
    for bad in ["missing.txt", "sub"] {
        let paths = ["ok.txt".into(), bad.into()];
        assert!(snapshot(&files, PersonId(1), &work, &paths).await.is_err());
    }
    assert!(snapshot(&files, PersonId(1), &work, &[]).await.is_err());
    assert!(!files.exists());
    std::fs::remove_dir_all(&dir).unwrap();
}
