use super::*;

#[test]
fn wechat_baseline_opens_with_core() {
    tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(async {
        let store = mic_store::Store::open_in_memory(MIGRATIONS).await.unwrap();
        store.with_module_tx(|tx| {
            let version: u32 = tx.query_row("SELECT version FROM schema_migrations WHERE module = 'wechat'", [], |r| r.get(0))?;
            assert_eq!(version, 4);
            let tables: u32 = tx.query_row("SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name LIKE 'wechat_%'", [], |r| r.get(0))?;
            assert_eq!(tables, 7);
            Ok(())
        }).await.unwrap();
    });
}
