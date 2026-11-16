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
            .diagnostic_query(SqlDb::Main, "SELECT name FROM t_account ORDER BY id".into())
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
        .diagnostic_dry_run(
            SqlDb::Main,
            "UPDATE t_account SET name = 'z' WHERE id > 0".into(),
        )
        .await
        .unwrap();
    assert_eq!(dry.changes, 2);
    assert_eq!(dry.tables, ["t_account"]);
    assert!(dry.ddl.contains("CREATE TABLE t_account"), "{}", dry.ddl);
    assert_eq!(names(store.clone()).await, ["a", "b"], "试运行已回滚");

    let w = store
        .diagnostic_write(
            SqlDb::Main,
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
        let err = store
            .diagnostic_dry_run(SqlDb::Main, sql.into())
            .await
            .unwrap_err();
        assert!(matches!(err, DiagnosticError::Rejected(_)), "{sql}: {err}");
    }

    for at in 2..=(SQL_BACKUP_KEEP as i64 + 2) {
        store
            .diagnostic_write(
                SqlDb::Main,
                "DELETE FROM t_account WHERE id = 99".into(),
                at,
            )
            .await
            .unwrap();
    }
    let backups = dir.join("sql-backups");
    assert_eq!(
        std::fs::read_dir(&backups).unwrap().count(),
        SQL_BACKUP_KEEP
    );
    assert!(!backups.join("micnext-1.db").exists(), "删掉最旧的备份");

    external(&store, &dir).await;
    std::fs::remove_dir_all(&dir).unwrap();
}

/// 额外库：临时打开、只读查询拒写、写入按库名备份、文件缺失只报这次错。
async fn external(store: &Store, dir: &Path) {
    let path = dir.join("other.db");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         CREATE TABLE entries (id INTEGER PRIMARY KEY, amount INTEGER);
         INSERT INTO entries (amount) VALUES (1), (2);",
    )
    .unwrap();
    let db = SqlDb::External {
        name: "other".into(),
        path: path.clone(),
    };

    let r = store
        .diagnostic_query(db.clone(), "SELECT sum(amount) FROM entries".into())
        .await
        .unwrap();
    assert_eq!(r.rows, [[Cell::Integer(3)]]);
    let err = store
        .diagnostic_query(db.clone(), "DELETE FROM entries".into())
        .await
        .unwrap_err();
    assert!(matches!(err, DiagnosticError::Rejected(_)), "{err}");

    let schema = store.diagnostic_schema(db.clone()).await.unwrap();
    assert_eq!(schema[0].name, "entries");
    assert_eq!(schema[0].columns.len(), 2);

    let dry = store
        .diagnostic_dry_run(db.clone(), "UPDATE entries SET amount = 0".into())
        .await
        .unwrap();
    assert_eq!(
        (dry.changes, dry.tables.as_slice()),
        (2, ["entries".to_owned()].as_slice())
    );

    let w = store
        .diagnostic_write(db.clone(), "DELETE FROM entries WHERE id = 1".into(), 1)
        .await
        .unwrap();
    assert_eq!((w.changes, w.backup.as_str()), (1, "other-1.db"));
    assert!(dir.join("sql-backups/other-1.db").exists());
    let main = std::fs::read_dir(dir.join("sql-backups"))
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("micnext-")
        })
        .count();
    assert_eq!(main, SQL_BACKUP_KEEP, "主库备份不受影响");
    let n: i64 = conn
        .query_row("SELECT count(*) FROM entries", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1, "对方连接看得到写入");

    drop(conn);
    std::fs::rename(&path, dir.join("moved.db")).unwrap();
    for wal in ["other.db-wal", "other.db-shm"] {
        let _ = std::fs::remove_file(dir.join(wal));
    }
    let err = store
        .diagnostic_query(db, "SELECT 1".into())
        .await
        .unwrap_err();
    assert!(matches!(err, DiagnosticError::Open(_)), "{err}");
    assert!(!path.exists(), "不创建新库");
}
