use super::*;

#[test]
fn snippet_centers_on_the_earliest_term() {
    let terms = SearchTerms::parse("RUST 生命周期").unwrap();
    let long = format!(
        "{}\n\n讲讲 Rust 的生命周期{}",
        "前".repeat(50),
        "后".repeat(100)
    );
    let s = snippet(&long, &terms);
    assert!(s.starts_with('…') && s.ends_with('…'), "{s}");
    assert!(s.contains("讲讲 Rust 的生命周期"), "{s}");
    assert_eq!(s.chars().count(), SNIPPET_CHARS + 2);
    assert_eq!(snippet("短的 rust", &terms), "短的 rust");
}
