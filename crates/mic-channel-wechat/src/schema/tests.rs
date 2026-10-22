use super::*;

#[test]
fn wechat_baseline_opens_with_core() {
    tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(async {
        let store = mic_store::Store::open_in_memory(MIGRATIONS).await.unwrap();
        store.with_module_tx(|tx| {
            let version: u32 = tx.query_row("SELECT version FROM schema_migrations WHERE module = 'wechat'", [], |r| r.get(0))?;
            assert_eq!(version, 5);
            let tables: u32 = tx.query_row("SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name LIKE 'wechat_%'", [], |r| r.get(0))?;
            assert_eq!(tables, 7);
            Ok(())
        }).await.unwrap();
    });
}

#[test]
fn v5_migrates_old_delivery_and_batches() {
    tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(async {
        let dir = std::env::temp_dir().join(format!("micnext-wechat-v5-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("micnext.db");
        let key = || mic_store::SecretKeyFile(dir.join("secret.key"));

        let store = mic_store::Store::open(&db, &MIGRATIONS[..2], key()).await.unwrap();
        let person = store.ensure_person("me", 0).await.unwrap();
        let session = store
            .create_session(mic_store::NewSession {
                kind: mic_store::SessionKind::Root { channel: "wechat".into(), chat: "u".into() },
                parent_session_id: None,
                delivery_target: None,
                pwd: "/tmp".into(),
                tool_scope: mic_store::ToolScope::All,
                created_at: 0,
                persona: None,
                model: None,
                reasoning_effort: None,
            })
            .await
            .unwrap()
            .id;
        let mut ids = Vec::new();
        for _ in 0..2 {
            let body = mic_message::MessageBody::Notification { source: "t".into(), text: "hi".into() };
            ids.push(store.append(session, None, body, 0).await.unwrap().id.0);
        }
        let (sending, sent) = (ids[0], ids[1]);
        store
            .with_module_tx(move |tx| {
                tx.execute(
                    "INSERT INTO wechat_account VALUES ('u', 'b', 't', 'https://x', ?1, ?2, 'connected', 0)",
                    (person.0, session.0),
                )?;
                tx.execute("UPDATE core_messages SET delivered_at = 1 WHERE id = ?1", [sent])?;
                tx.execute(
                    "INSERT INTO wechat_delivery VALUES (?1, 'u', '[]', 'sending', NULL), (?2, 'u', '[]', 'sent', 1)",
                    (sending, sent),
                )?;
                tx.execute(
                    "INSERT INTO wechat_delivery_attempt VALUES
                       (?1, 0, 1, 'a', 0, 1, NULL, 'timeout'),
                       (?1, 0, 2, 'b', 1, 2, 'ext', NULL),
                       (?1, 1, 1, 'c', 2, 3, NULL, 'timeout'),
                       (?1, 1, 2, 'd', 3, NULL, NULL, NULL)",
                    [sending],
                )?;
                tx.execute(
                    "INSERT INTO wechat_inbound_batch VALUES (1, 'u', '{}', 'interrupted', 0, 1)",
                    [],
                )?;
                Ok(())
            })
            .await
            .unwrap();
        drop(store);

        let store = mic_store::Store::open(&db, MIGRATIONS, key()).await.unwrap();
        store
            .with_module_tx(move |tx| {
                let failure = |id: i64| {
                    tx.query_row(
                        "SELECT delivered_at, delivery_failure FROM core_messages WHERE id = ?1",
                        [id],
                        |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<String>>(1)?)),
                    )
                };
                assert_eq!(failure(sending)?, (None, Some("投递中途中断，可能未送达".into())));
                assert_eq!(failure(sent)?, (Some(1), None));
                let mut stmt = tx.prepare(
                    "SELECT chunk_index, client_id FROM wechat_delivery_chunk ORDER BY chunk_index",
                )?;
                let chunks: Vec<(i64, String)> =
                    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
                assert_eq!(chunks, [(0, "b".into()), (1, "d".into())]);
                let batches: i64 = tx.query_row("SELECT count(*) FROM wechat_inbound_batch", [], |r| r.get(0))?;
                assert_eq!(batches, 0);
                Ok(())
            })
            .await
            .unwrap();
        drop(store);
        std::fs::remove_dir_all(&dir).unwrap();
    });
}
