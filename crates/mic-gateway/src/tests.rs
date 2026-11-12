use std::path::Path;

/// code-map 里反引号包着、像仓库路径的都须存在；行为是否准确靠改功能时核对。
#[test]
fn code_map_paths_exist() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let map = std::fs::read_to_string(repo.join("docs/guide/code-map.md")).unwrap();
    let missing: Vec<_> = map
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|s| {
            ["crates/", "web/", "bin/", "docs/", "scripts/"]
                .iter()
                .any(|p| s.starts_with(p))
        })
        .map(|s| s.split([':', ' ', '#']).next().unwrap())
        .filter(|path| !repo.join(path).exists())
        .collect();
    assert!(missing.is_empty(), "code-map 里的路径不存在：{missing:?}");
}
