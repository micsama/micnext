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
    assert_eq!(
        applied_version(&conn, "core").unwrap(),
        Some(CORE_MIGRATIONS.last().unwrap().version)
    );
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
    let known = CORE_MIGRATIONS.last().unwrap().version;
    conn.execute(
        "UPDATE schema_migrations SET version = ?1 WHERE module = 'core'",
        [known + 1],
    )
    .unwrap();
    assert!(matches!(
        init(&mut conn, &[]),
        Err(StoreError::SchemaAhead { db, known: actual, .. }) if db == known + 1 && actual == known
    ));
}

fn core_at_version(version: u32) -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE schema_migrations (module TEXT PRIMARY KEY, version INTEGER NOT NULL);",
    )
    .unwrap();
    for migration in CORE_MIGRATIONS.iter().filter(|m| m.version <= version) {
        conn.execute_batch(migration.sql).unwrap();
    }
    set_version(&conn, "core", version);
    conn
}

fn old_compactions() -> Connection {
    let conn = core_at_version(20);
    conn.execute("INSERT INTO core_sessions (id, kind, channel, chat, pwd, tool_scope, created_at, persona_id, archived_at) VALUES (1, 'root', 'web', 'a', '/tmp', '\"All\"', 1, 1, 9)", []).unwrap();
    let files = serde_json::json!([
        {"path": "a.rs", "outcome": {"Read": {"text": "frozen text", "extent": "Truncated"}}},
        {"path": "b.rs", "outcome": {"Unavailable": {"reason": "missing"}}}
    ]);
    for (id, files, revoked) in [(1, files, Some(8)), (2, serde_json::json!([]), None)] {
        let payload = serde_json::json!({"kind": "Boundary", "boundary": {"Compaction": {
            "summary": "summary", "keep_from": null, "files": files, "elided": [], "quoted": []
        }}});
        conn.execute("INSERT INTO core_messages (id, session_id, payload, created_at, compaction_revoked_at) VALUES (?1, 1, ?2, 3, ?3)", params![id, payload.to_string(), revoked]).unwrap();
    }
    conn
}

#[test]
fn compaction_paths_migrate_revoked_and_empty_boundaries() {
    let mut conn = old_compactions();
    init(&mut conn, &[]).unwrap();
    for (id, expected) in [(1, vec!["a.rs", "b.rs"]), (2, vec![])] {
        let (payload, at, revoked): (String, i64, Option<i64>) = conn.query_row(
            "SELECT payload, created_at, compaction_revoked_at FROM core_messages WHERE id = ?1", [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).unwrap();
        let mic_message::MessageBody::Boundary {
            boundary: mic_message::ContextBoundary::Compaction(result),
        } = serde_json::from_str(&payload).unwrap()
        else {
            panic!("boundary expected")
        };
        assert_eq!(result.files, expected);
        assert_eq!(result.summary, "summary");
        assert_eq!(at, 3);
        assert_eq!(revoked, if id == 1 { Some(8) } else { None });
        assert!(!payload.contains("frozen text") && !payload.contains("outcome"));
    }
}

#[test]
fn malformed_old_compaction_rolls_back_migration() {
    let mut conn = old_compactions();
    conn.execute("UPDATE core_messages SET payload = json_set(payload, '$.boundary.Compaction.files', json('[{\"path\":3}]')) WHERE id = 2", []).unwrap();
    assert!(init(&mut conn, &[]).is_err());
    assert_eq!(applied_version(&conn, "core").unwrap(), Some(20));
    let payload: String = conn
        .query_row("SELECT payload FROM core_messages WHERE id = 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(payload.contains("frozen text"));
}

#[test]
fn dropping_job_report_pointer_keeps_completion_and_run_pointer() {
    let mut conn = core_at_version(21);
    conn.execute_batch("INSERT INTO core_persons VALUES (1, 'owner', 1);
        INSERT INTO core_sessions (id, kind, channel, chat, pwd, tool_scope, created_at, persona_id)
        VALUES (1, 'root', 'web', 'a', '/tmp', '\"All\"', 1, 1);
        INSERT INTO core_runs (id, session_id, state, created_at, finished_at) VALUES (1, 1, 'completed', 1, 2);").unwrap();
    let payload = serde_json::to_string(&mic_message::MessageBody::Completion {
        person: mic_message::PersonId(1),
        tool_name: "bash".into(),
        exec_id: "1".into(),
        outcome: mic_message::ExecOutcome::Completed { output: vec![] },
    })
    .unwrap();
    conn.execute(
        "INSERT INTO core_messages (id, session_id, payload, created_at) VALUES (1, 1, ?1, 3)",
        [&payload],
    )
    .unwrap();
    conn.execute_batch("INSERT INTO core_jobs (id, kind, owner_session_id, person_id, origin_run_id, origin_tool_call_id, tool_name, description, created_at, state, finished_at, report_message_id)
        VALUES (1, 'tool', 1, 1, 1, 'call_0', 'bash', 'test', 1, 'completed', 3, 1);
        UPDATE core_runs SET report_message_id = 1 WHERE id = 1;").unwrap();
    let reply = serde_json::json!({"kind":"Reply", "model":"test", "stats":{"model_ms":0,"first_output_ms":null,"usage":null}, "blocks":[{"ToolCall":{"id":"call_0","name":"bash","args":{}}}]});
    conn.execute("INSERT INTO core_messages (id, session_id, run_id, payload, created_at) VALUES (2, 1, 1, ?1, 0)", [reply.to_string()]).unwrap();
    init(&mut conn, &[]).unwrap();
    let actual: String = conn
        .query_row("SELECT payload FROM core_messages WHERE id = 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(actual, payload);
    let run_report: i64 = conn
        .query_row(
            "SELECT report_message_id FROM core_runs WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(run_report, 1);
    assert!(conn
        .prepare("SELECT report_message_id FROM core_jobs")
        .is_err());
    assert!(!conn
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .exists([])
        .unwrap());
}

fn old_tool_occurrences() -> Connection {
    let conn = core_at_version(22);
    conn.execute_batch(
        "INSERT INTO core_persons VALUES (1, 'owner', 1);
        INSERT INTO core_sessions (id, kind, channel, chat, pwd, tool_scope, created_at, persona_id)
        VALUES (1, 'root', 'web', 'a', '/tmp', '\"All\"', 1, 1);
        INSERT INTO core_runs (id, session_id, state, created_at) VALUES (1, 1, 'executing', 1);",
    )
    .unwrap();
    let tool = |id| serde_json::json!({"ToolCall":{"id":id,"name":if id == "agent" { "agent" } else { "bash" },"args":{}}});
    let reply = |blocks| serde_json::json!({"kind":"Reply", "model":"test", "stats":{"model_ms":0,"first_output_ms":null,"usage":null}, "blocks":blocks});
    let result = |id, outcome| serde_json::json!({"kind":"ToolResult","tool_name":"bash","tool_call_id":id,"outcome":outcome,"started_at":1});
    let messages = [
        reply(serde_json::json!([{"Text":{"text":"before","phase":null}}, tool("call_0")])),
        result(
            "call_0",
            serde_json::json!({"Terminal":{"Completed":{"output":[]}}}),
        ),
        reply(serde_json::json!([tool("call_0")])),
        reply(serde_json::json!([tool("other"), tool("agent")])),
        result("other", serde_json::json!({"Dispatched":{"exec_id":"2"}})),
    ];
    for (i, payload) in messages.into_iter().enumerate() {
        let id = i as i64 + 1;
        conn.execute("INSERT INTO core_messages (id, session_id, run_id, payload, created_at) VALUES (?1, 1, 1, ?2, ?1)", params![id, payload.to_string()]).unwrap();
    }
    conn.execute_batch(r#"INSERT INTO core_jobs (id, kind, owner_session_id, person_id, origin_run_id, origin_tool_call_id, tool_name, description, created_at, state)
        VALUES (1, 'tool', 1, 1, 1, 'call_0', 'bash', 'pending', 3, 'running'),
               (2, 'tool', 1, 1, 1, 'other', 'bash', 'dispatched', 4, 'running');
        INSERT INTO core_sessions (id, kind, parent_tool_call_id, parent_session_id, pwd, tool_scope, created_at, persona_id)
        VALUES (2, 'task', 'agent', 1, '/tmp', '"All"', 4, 1);
        INSERT INTO core_jobs (id, kind, owner_session_id, person_id, origin_run_id, origin_tool_call_id, tool_name, description, created_at, child_session_id)
        VALUES (3, 'agent', 1, 1, 1, 'agent', 'agent', 'child', 4, 2);"#).unwrap();
    conn
}

#[test]
fn tool_identity_migrates_reused_ids_and_pending_jobs() {
    let mut conn = old_tool_occurrences();
    init(&mut conn, &[]).unwrap();
    let source: (i64, i64) = conn.query_row("SELECT json_extract(payload, '$.call.reply'), json_extract(payload, '$.call.index') FROM core_messages WHERE id = 2", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(source, (1, 1));
    let child = conn
        .query_row(
            &format!(
                "SELECT {} FROM core_sessions WHERE id = 2",
                crate::row::SESSION_COLS
            ),
            [],
            crate::row::session,
        )
        .unwrap();
    assert_eq!(child.kind, crate::SessionKind::Task);
    assert_eq!(child.parent_session_id, Some(mic_message::SessionId(1)));
    assert!(conn
        .prepare("SELECT parent_tool_call_id FROM core_sessions")
        .is_err());
    assert!(conn
        .prepare("SELECT origin_tool_call_id FROM core_jobs")
        .is_err());
    for (job, expected) in [(1, (3, 0)), (2, (4, 0)), (3, (4, 1))] {
        let actual = conn
            .query_row(
                "SELECT origin_reply_id, origin_call_index FROM core_jobs WHERE id = ?1",
                [job],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
            )
            .unwrap();
        assert_eq!(actual, expected);
    }
    conn.execute("INSERT INTO core_jobs (kind, owner_session_id, person_id, origin_run_id, tool_name, description, created_at, state, origin_reply_id, origin_call_index) VALUES ('tool', 1, 1, 1, 'bash', 'first occurrence', 6, 'running', 1, 1)", []).unwrap();
    assert!(!conn
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .exists([])
        .unwrap());
    assert_eq!(
        conn.query_row("SELECT count(*) FROM core_messages", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        5
    );
}

#[test]
fn ambiguous_tool_identity_rolls_back_without_losing_history() {
    let mut conn = old_tool_occurrences();
    conn.execute("UPDATE core_messages SET payload = json_insert(payload, '$.blocks[#]', json('{\"ToolCall\":{\"id\":\"call_0\",\"name\":\"bash\",\"args\":{}}}')) WHERE id = 1", []).unwrap();
    assert!(init(&mut conn, &[]).is_err());
    assert_eq!(applied_version(&conn, "core").unwrap(), Some(22));
    assert_eq!(
        conn.query_row("SELECT count(*) FROM core_jobs", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert!(conn
        .prepare("SELECT origin_reply_id FROM core_jobs")
        .is_err());
    let call: Option<i64> = conn
        .query_row(
            "SELECT json_extract(payload, '$.call.reply') FROM core_messages WHERE id = 2",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(call.is_none());
}
