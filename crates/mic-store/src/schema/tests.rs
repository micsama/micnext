use super::*;

#[test]
fn reasoning_migration_removes_model_choice_without_copying_it() {
    let mut conn = Connection::open_in_memory().unwrap();
    for (_, migrations) in group(CORE_MIGRATIONS.iter().filter(|m| m.version <= 6)) {
        for migration in migrations {
            conn.execute_batch(migration.sql).unwrap();
        }
    }
    conn.execute_batch(
        r#"
        CREATE TABLE schema_migrations (module TEXT PRIMARY KEY, version INTEGER NOT NULL);
        INSERT INTO schema_migrations VALUES ('core', 6);
        INSERT INTO core_endpoints (id, name, kind, config_json, created_at, updated_at)
          VALUES (1, 'test', 'openai', '{}', 0, 0);
        INSERT INTO core_models (id, endpoint_id, name, config_json, created_at, updated_at)
          VALUES (1, 1, 'test', '{"max_tokens":128,"reasoning_effort":"high"}', 0, 0);
        INSERT INTO core_sessions (id, kind, channel, chat, pwd, tool_scope, created_at, model_id)
          VALUES (1, 'root', 'web', 'test', '/tmp', '"All"', 0, 1);
        INSERT INTO core_runs (id, session_id, state, created_at) VALUES (1, 1, 'completed', 0);
    "#,
    )
    .unwrap();
    init(&mut conn, &[]).unwrap();
    let model: String = conn
        .query_row("SELECT config_json FROM core_models", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&model).unwrap(),
        serde_json::json!({"max_tokens": 128})
    );
    for table in ["core_sessions", "core_runs"] {
        let effort: Option<String> = conn
            .query_row(&format!("SELECT reasoning_effort FROM {table}"), [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(effort, None);
    }
    assert!(conn
        .execute(
            "UPDATE core_sessions SET model_id = NULL, reasoning_effort = 'high'",
            []
        )
        .is_err());
}
