use super::*;

const NEXT: Migration = Migration {
    module: "demo",
    version: 3,
    sql: "CREATE TABLE demo_t (id INTEGER PRIMARY KEY);",
    secret_columns: &[],
};

fn tables(conn: &Connection) -> Vec<String> {
    conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn set_version(conn: &Connection, module: &str, version: u32) {
    conn.execute(
        "INSERT INTO schema_migrations (module, version) VALUES (?1, ?2)",
        params![module, version],
    )
    .unwrap();
}

#[test]
fn empty_database_builds_every_baseline() {
    let mut conn = Connection::open_in_memory().unwrap();
    init(&mut conn, &[NEXT]).unwrap();
    let tables = tables(&conn);
    for expected in [
        "core_sessions",
        "core_model_calls",
        "core_model_call_inputs",
        "demo_t",
    ] {
        assert!(tables.contains(&expected.to_owned()), "缺少 {expected}");
    }
    assert_eq!(applied_version(&conn, "core").unwrap(), Some(16));
    assert_eq!(applied_version(&conn, "demo").unwrap(), Some(3));
}

#[test]
fn module_enabled_later_builds_its_baseline() {
    let mut conn = Connection::open_in_memory().unwrap();
    init(&mut conn, &[]).unwrap();
    assert!(!tables(&conn).contains(&"demo_t".to_owned()));
    init(&mut conn, &[NEXT]).unwrap();
    assert!(tables(&conn).contains(&"demo_t".to_owned()));
}

#[test]
fn older_format_asks_for_rebuild_instead_of_migrating() {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE schema_migrations (module TEXT PRIMARY KEY, version INTEGER NOT NULL);",
    )
    .unwrap();
    set_version(&conn, "core", 9);
    match init(&mut conn, &[]) {
        Err(StoreError::SchemaRebuild {
            module,
            version: 9,
            baseline: 10,
        }) => assert_eq!(module, "core"),
        other => panic!("{other:?}"),
    }

    let mut conn = Connection::open_in_memory().unwrap();
    init(&mut conn, &[]).unwrap();
    set_version(&conn, "demo", 2);
    assert!(matches!(
        init(&mut conn, &[NEXT]),
        Err(StoreError::SchemaRebuild { baseline: 3, .. })
    ));
}

#[test]
fn newer_format_is_rejected() {
    let mut conn = Connection::open_in_memory().unwrap();
    init(&mut conn, &[]).unwrap();
    conn.execute(
        "UPDATE schema_migrations SET version = 17 WHERE module = 'core'",
        [],
    )
    .unwrap();
    assert!(matches!(
        init(&mut conn, &[]),
        Err(StoreError::SchemaAhead {
            db: 17,
            known: 16,
            ..
        })
    ));
}
