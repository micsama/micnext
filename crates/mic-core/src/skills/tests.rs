use super::*;

fn write(root: &Path, dir: &str, file: &str, text: &str) {
    let path = root.join(dir).join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn names_follow_the_spec() {
    for ok in ["a", "log-analysis", "pdf2"] {
        assert!(valid_name(ok), "{ok}");
    }
    for bad in ["", "-a", "a-", "a--b", "Log", "a_b", &"a".repeat(65)] {
        assert!(!valid_name(bad), "{bad}");
    }
}

#[tokio::test]
async fn scan_load_and_expand() {
    let tmp = tempdir();
    let root = tmp.join("skills");
    write(
        &root,
        "zeta",
        SKILL_FILE,
        "---\nname: zeta\ndescription: |\n  last\n  one\nlicense: MIT\n---\nbody z\n",
    );
    write(
        &root,
        "alpha",
        SKILL_FILE,
        "---\nname: alpha\ndescription: first\n---\n\nbody a\n",
    );
    write(&root, "alpha", "scripts/run.py", "");
    write(
        &root,
        "wrong",
        SKILL_FILE,
        "---\nname: other\ndescription: x\n---\n",
    );
    write(
        &root,
        "clear",
        SKILL_FILE,
        "---\nname: clear\ndescription: x\n---\n",
    );
    std::fs::create_dir_all(root.join("empty")).unwrap();

    let list = scan(&root).unwrap();
    let names: Vec<_> = list.skills.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["alpha", "zeta"]);
    assert_eq!(list.skills[1].description, "last one");
    let invalid: Vec<_> = list.invalid.iter().map(|i| i.dir.as_str()).collect();
    assert_eq!(invalid, ["clear", "empty", "wrong"]);

    let loaded = load(&root, "alpha").unwrap();
    assert!(loaded.starts_with("body a\n\nSkill directory: "));
    assert!(loaded.ends_with("\n- scripts/run.py"));
    assert!(matches!(
        load(&root, "nope"),
        Err(LoadError::NotFound { .. })
    ));
    assert!(matches!(
        load(&root, "wrong"),
        Err(LoadError::Invalid { .. })
    ));
    assert_eq!(scan(&tmp.join("missing")).unwrap(), SkillList::default());

    let skills = Skills::new(root.clone());
    let catalog = skills.catalog(&list).unwrap();
    assert!(catalog.contains(&format!(
        "<skill name=\"zeta\" dir=\"{}\">last one</skill>",
        root.join("zeta").display()
    )));
    assert!(matches!(
        skills.expand("/alpha  do it\nnow").await,
        Some(Named::Loaded { name, rest, .. }) if name == "alpha" && rest == "do it\nnow"
    ));
    assert!(skills.expand("/alpha-x hi").await.is_none());
    assert!(skills.expand("/nope").await.is_none());
    assert!(matches!(
        skills.expand("/wrong").await,
        Some(Named::Failed { name, .. }) if name == "wrong"
    ));
    std::fs::remove_dir_all(&tmp).unwrap();
}

fn tempdir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mic-skills-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
