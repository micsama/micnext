use super::*;
use crate::{Schema, SecretKeyFile};

const SECRET: Schema = Schema {
    module: "t",
    version: "1",
    sql: "CREATE TABLE t_account (id INTEGER PRIMARY KEY, name TEXT, token TEXT);
          INSERT INTO t_account (name, token) VALUES ('a', 'x'), ('b', 'y');",
    secret_columns: &[SecretColumn {
        table: "t_account",
        column: "token",
    }],
};

#[test]
fn write_backs_up_and_dry_run_rolls_back() {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(check());
}

async fn check() {
    let dir = std::env::temp_dir().join(format!("mic-sql-write-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(
        &dir.join("micnext.db"),
        &[SECRET],
        SecretKeyFile(dir.join("key")),
    )
    .await
    .unwrap();
    let names = |store: Store| async move {
        let r = store
            .diagnostic_query("SELECT name FROM t_account ORDER BY id".into())
            .await
            .unwrap();
        r.rows
            .into_iter()
            .map(|row| match &row[0] {
                Cell::Text(t) => t.clone(),
                other => panic!("{other:?}"),
            })
            .collect::<Vec<_>>()
    };

    let dry = store
        .diagnostic_dry_run("UPDATE t_account SET name = 'z' WHERE id > 0".into())
        .await
        .unwrap();
    assert_eq!(dry.changes, 2);
    assert_eq!(dry.tables, ["t_account"]);
    assert!(dry.ddl.contains("CREATE TABLE t_account"), "{}", dry.ddl);
    assert_eq!(names(store.clone()).await, ["a", "b"], "试运行已回滚");

    let w = store
        .diagnostic_write(
            "UPDATE t_account SET name = 'c' WHERE id = 1 RETURNING id".into(),
            1,
        )
        .await
        .unwrap();
    assert_eq!((w.changes, w.result.rows.len()), (1, 1));
    assert!(dir.join("sql-backups").join(&w.backup).exists());
    assert_eq!(names(store.clone()).await, ["c", "b"]);

    for sql in [
        "CREATE TABLE t_x (id INTEGER)",
        "PRAGMA user_version = 3",
        "UPDATE t_account SET token = 'leak'",
    ] {
        let err = store.diagnostic_dry_run(sql.into()).await.unwrap_err();
        assert!(matches!(err, DiagnosticError::Rejected(_)), "{sql}: {err}");
    }

    for at in 2..=(SQL_BACKUP_KEEP as i64 + 2) {
        store
            .diagnostic_write("DELETE FROM t_account WHERE id = 99".into(), at)
            .await
            .unwrap();
    }
    let backups = dir.join("sql-backups");
    assert_eq!(
        std::fs::read_dir(&backups).unwrap().count(),
        SQL_BACKUP_KEEP
    );
    assert!(!backups.join("micnext-1.db").exists(), "删掉最旧的备份");
    std::fs::remove_dir_all(&dir).unwrap();
}
