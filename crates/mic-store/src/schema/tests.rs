use super::*;

const MODULE: Schema = Schema {
    module: "demo",
    version: "3",
    sql: "CREATE TABLE demo_t (id INTEGER PRIMARY KEY);",
    secret_columns: &[],
};

#[test]
fn current_schema_initializes_and_reopens() {
    let mut conn = Connection::open_in_memory().unwrap();
    init(&mut conn, &[]).unwrap();
    // 模块可以之后启用，只直接创建它的当前完整结构。
    init(&mut conn, &[MODULE]).unwrap();
    conn.execute("INSERT INTO demo_t VALUES (1)", []).unwrap();
    init(&mut conn, &[MODULE]).unwrap();
    assert_eq!(applied_version(&conn, "core").unwrap(), Some(28));
    assert_eq!(applied_version(&conn, "demo").unwrap(), Some(3));
    assert_eq!(
        conn.query_row("SELECT count(*) FROM demo_t", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(!conn
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .exists([])
        .unwrap());
}

#[test]
fn different_versions_are_rejected_without_changing_data() {
    for version in [22, 29] {
        let mut conn = Connection::open_in_memory().unwrap();
        init(&mut conn, &[]).unwrap();
        conn.execute(
            "UPDATE schema_migrations SET version = ?1 WHERE module = 'core'",
            [version],
        )
        .unwrap();
        assert!(
            matches!(init(&mut conn, &[MODULE]), Err(StoreError::SchemaMismatch { db, expected: 28, .. }) if db == version)
        );
        assert_eq!(applied_version(&conn, "core").unwrap(), Some(version));
        assert_eq!(applied_version(&conn, "demo").unwrap(), None);
        assert_eq!(
            conn.query_row("SELECT count(*) FROM core_settings", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}
