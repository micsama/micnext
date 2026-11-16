//! 开发者诊断：独立只读连接上执行单条只读 SQL，凭据列读作 NULL；
//! 写入模式在业务连接上先备份再执行。额外库每次操作临时打开，规则相同。
//! 契约：docs/blueprints/archive/developer-diagnostics.md §三、docs/blueprints/archive/sql-write.md、
//! docs/blueprints/sql-databases.md。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::time::Instant;

use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use rusqlite::limits::Limit;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, ErrorCode, OpenFlags};

use crate::limits::{
    DIAGNOSTIC_BYTES, DIAGNOSTIC_PROGRESS_OPS, DIAGNOSTIC_ROWS, DIAGNOSTIC_TIMEOUT,
    DIAGNOSTIC_VALUE_BYTES, SQL_BACKUP_KEEP,
};
use crate::{SecretColumn, Store, StoreError};

#[derive(Debug, Clone, PartialEq)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Cell>>,
    /// 下一行会超出行数或文本字节上限，其后结果未返回。
    pub truncated: bool,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WriteResult {
    pub result: QueryResult,
    /// 语句直接改动的行数，不含触发器。
    pub changes: u64,
    /// 执行前备份的文件名，位于数据库同目录的 `sql-backups/`。
    pub backup: String,
}

/// 试运行后已回滚。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DryRun {
    pub changes: u64,
    pub tables: Vec<String>,
    /// 涉及表的建表、索引与触发器语句，每张表前注释表说明。
    pub ddl: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Null,
    Integer(i64),
    Real(f64),
    /// 非 UTF-8 字节按替换字符显示。
    Text(String),
    Blob {
        bytes: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSchema {
    pub name: String,
    pub group: String,
    pub description: String,
    pub columns: Vec<ColumnSchema>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnSchema {
    pub name: String,
    pub decl_type: String,
    pub secret: bool,
}

/// 主库在备份文件名里的名字；额外库不能用。
pub const MAIN_SQL_DB: &str = "micnext";

/// 诊断操作的目标库。额外库来自启动配置的白名单，名字只含字母、数字、下划线。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqlDb {
    Main,
    External { name: String, path: PathBuf },
}

/// 诊断查询错误；文案面向用户。
#[derive(Debug, thiserror::Error)]
pub enum DiagnosticError {
    #[error("已有查询在执行")]
    Busy,
    #[error("查询超过 {} 秒已中断", DIAGNOSTIC_TIMEOUT.as_secs())]
    Timeout,
    #[error("SQL 有误或不允许：{0}")]
    Rejected(String),
    #[error("当前数据库不支持诊断查询")]
    Unavailable,
    #[error("打不开数据库 {0}")]
    Open(String),
    #[error("备份失败，未执行：{0}")]
    Backup(String),
    #[error(transparent)]
    Store(#[from] StoreError),
}

pub(crate) struct Diagnostic {
    conn: Mutex<Connection>,
    secrets: Vec<SecretColumn>,
    backups: PathBuf,
}

impl Diagnostic {
    /// 须在业务连接完成初始化后打开。
    pub(crate) fn open(path: &Path, secrets: Vec<SecretColumn>) -> rusqlite::Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.set_limit(Limit::SQLITE_LIMIT_LENGTH, DIAGNOSTIC_VALUE_BYTES)?;
        let hidden = secrets.clone();
        conn.authorizer(Some(move |ctx: AuthContext<'_>| authorize(&hidden, ctx)))?;
        Ok(Self {
            conn: Mutex::new(conn),
            secrets,
            backups: path.with_file_name("sql-backups"),
        })
    }
}

fn is_secret(secrets: &[SecretColumn], table: &str, column: &str) -> bool {
    secrets
        .iter()
        .any(|s| s.table.eq_ignore_ascii_case(table) && s.column.eq_ignore_ascii_case(column))
}

fn authorize(secrets: &[SecretColumn], ctx: AuthContext<'_>) -> Authorization {
    match ctx.action {
        AuthAction::Select | AuthAction::Function { .. } | AuthAction::Recursive => {
            Authorization::Allow
        }
        AuthAction::Read {
            table_name,
            column_name,
        } if is_secret(secrets, table_name, column_name) => Authorization::Ignore,
        AuthAction::Read { .. } => Authorization::Allow,
        _ => Authorization::Deny,
    }
}

/// 只读白名单之外只多放开增删改，凭据列不能写；记下读写到的表。
fn authorize_write(
    secrets: &[SecretColumn],
    tables: &Mutex<BTreeSet<String>>,
    ctx: AuthContext<'_>,
) -> Authorization {
    let table = match ctx.action {
        AuthAction::Insert { table_name } | AuthAction::Delete { table_name } => table_name,
        AuthAction::Update {
            table_name,
            column_name,
        } if is_secret(secrets, table_name, column_name) => return Authorization::Deny,
        AuthAction::Update { table_name, .. } => table_name,
        AuthAction::Read { table_name, .. } => {
            touch(tables, table_name);
            return authorize(secrets, ctx);
        }
        _ => return authorize(secrets, ctx),
    };
    touch(tables, table);
    Authorization::Allow
}

fn touch(tables: &Mutex<BTreeSet<String>>, table: &str) {
    if !table.starts_with("sqlite_") {
        tables
            .lock()
            .expect("表集合锁中毒")
            .insert(table.to_owned());
    }
}

impl Store {
    /// 只读执行一条 SQL；与其他诊断操作全局互斥。
    pub async fn diagnostic_query(
        &self,
        db: SqlDb,
        sql: String,
    ) -> Result<QueryResult, DiagnosticError> {
        let diagnostic = self.diagnostic()?;
        tokio::task::spawn_blocking(move || {
            let main = diagnostic.gate()?;
            match &db {
                SqlDb::Main => query(&main, &sql),
                SqlDb::External { .. } => {
                    let conn = db.open(false)?;
                    conn.authorizer(Some(|ctx: AuthContext<'_>| authorize(&[], ctx)))
                        .map_err(StoreError::from)?;
                    query(&conn, &sql)
                }
            }
        })
        .await
        .expect("诊断查询任务 panic")
    }

    /// 先备份整库，再在事务里执行一条增删改或查询。
    pub async fn diagnostic_write(
        &self,
        db: SqlDb,
        sql: String,
        at: i64,
    ) -> Result<WriteResult, DiagnosticError> {
        let diagnostic = self.diagnostic()?;
        self.writable(db, move |conn, db| {
            let backup = backup(conn, &diagnostic.backups, db.name(), at)?;
            let (result, changes, _) = execute(conn, db.secrets(&diagnostic), &sql, true)?;
            Ok(WriteResult {
                result,
                changes,
                backup,
            })
        })
        .await
    }

    /// 与写入同样授权执行后回滚，取影响行数和涉及表的 DDL。
    pub async fn diagnostic_dry_run(
        &self,
        db: SqlDb,
        sql: String,
    ) -> Result<DryRun, DiagnosticError> {
        let diagnostic = self.diagnostic()?;
        self.writable(db, move |conn, db| {
            let (_, changes, tables) = execute(conn, db.secrets(&diagnostic), &sql, false)?;
            let tables: Vec<String> = tables.into_iter().collect();
            let ddl = ddl(conn, db, &tables).map_err(StoreError::from)?;
            Ok(DryRun {
                changes,
                tables,
                ddl,
            })
        })
        .await
    }

    /// 全部用户表与视图的列；主库按目录分组并标记凭据列。
    pub async fn diagnostic_schema(&self, db: SqlDb) -> Result<Vec<TableSchema>, DiagnosticError> {
        let diagnostic = self.diagnostic()?;
        let rows = match &db {
            SqlDb::Main => self.call(|conn| Ok(columns(conn)?)).await?,
            SqlDb::External { .. } => {
                let db = db.clone();
                tokio::task::spawn_blocking(move || {
                    Ok::<_, DiagnosticError>(columns(&db.open(false)?).map_err(StoreError::from)?)
                })
                .await
                .expect("诊断表结构任务 panic")?
            }
        };
        let mut tables: Vec<TableSchema> = Vec::new();
        for (table, column, decl_type) in rows {
            let secret = is_secret(db.secrets(&diagnostic), &table, &column);
            let column = ColumnSchema {
                name: column,
                decl_type,
                secret,
            };
            match tables.last_mut() {
                Some(last) if last.name == table => last.columns.push(column),
                _ => {
                    let (group, description) = describe(&db, &table);
                    tables.push(TableSchema {
                        group: group.into(),
                        description: description.into(),
                        name: table,
                        columns: vec![column],
                    })
                }
            }
        }
        Ok(tables)
    }

    fn diagnostic(&self) -> Result<Arc<Diagnostic>, DiagnosticError> {
        self.diagnostic.clone().ok_or(DiagnosticError::Unavailable)
    }

    /// 主库用业务连接，额外库临时以读写方式打开；都先取全局闸门。
    async fn writable<R, F>(&self, db: SqlDb, f: F) -> Result<R, DiagnosticError>
    where
        R: Send + 'static,
        F: FnOnce(&mut Connection, &SqlDb) -> Result<R, DiagnosticError> + Send + 'static,
    {
        let diagnostic = self.diagnostic()?;
        match db {
            SqlDb::Main => {
                self.blocking(move |conn| {
                    let _gate = diagnostic.gate()?;
                    f(conn, &db)
                })
                .await
            }
            SqlDb::External { .. } => tokio::task::spawn_blocking(move || {
                let _gate = diagnostic.gate()?;
                let mut conn = db.open(true)?;
                f(&mut conn, &db)
            })
            .await
            .expect("诊断写入任务 panic"),
        }
    }
}

impl Diagnostic {
    /// 全局同一时间只允许一个诊断操作；守卫同时是主库的只读连接。
    fn gate(&self) -> Result<MutexGuard<'_, Connection>, DiagnosticError> {
        match self.conn.try_lock() {
            Ok(conn) => Ok(conn),
            Err(TryLockError::WouldBlock) => Err(DiagnosticError::Busy),
            Err(TryLockError::Poisoned(_)) => panic!("诊断连接锁中毒"),
        }
    }
}

impl SqlDb {
    fn name(&self) -> &str {
        match self {
            SqlDb::Main => MAIN_SQL_DB,
            SqlDb::External { name, .. } => name,
        }
    }

    fn secrets<'a>(&self, diagnostic: &'a Diagnostic) -> &'a [SecretColumn] {
        match self {
            SqlDb::Main => &diagnostic.secrets,
            SqlDb::External { .. } => &[],
        }
    }

    /// 只用于额外库；不带 CREATE，文件不存在即报错。
    fn open(&self, write: bool) -> Result<Connection, DiagnosticError> {
        let SqlDb::External { name, path } = self else {
            unreachable!("主库不临时打开")
        };
        let failed = |e: rusqlite::Error| DiagnosticError::Open(format!("{name}：{e}"));
        let mode = if write {
            OpenFlags::SQLITE_OPEN_READ_WRITE
        } else {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        };
        let conn = Connection::open_with_flags(path, mode | OpenFlags::SQLITE_OPEN_NO_MUTEX)
            .map_err(failed)?;
        conn.busy_timeout(DIAGNOSTIC_TIMEOUT).map_err(failed)?;
        conn.set_limit(Limit::SQLITE_LIMIT_LENGTH, DIAGNOSTIC_VALUE_BYTES)
            .map_err(failed)?;
        // 打开是惰性的，读一次 schema 让「不是数据库」等问题在这里暴露。
        conn.query_row("SELECT count(*) FROM sqlite_schema", [], |_| Ok(()))
            .map_err(failed)?;
        Ok(conn)
    }
}

fn query(conn: &Connection, sql: &str) -> Result<QueryResult, DiagnosticError> {
    let started = Instant::now();
    let deadline = started + DIAGNOSTIC_TIMEOUT;
    conn.progress_handler(
        DIAGNOSTIC_PROGRESS_OPS,
        Some(move || Instant::now() >= deadline),
    )
    .map_err(StoreError::from)?;
    let result = run(conn, sql, started, false).map_err(|e| classify(e, deadline));
    conn.progress_handler(DIAGNOSTIC_PROGRESS_OPS, None::<fn() -> bool>)
        .map_err(StoreError::from)?;
    result
}

fn columns(conn: &Connection) -> rusqlite::Result<Vec<(String, String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT m.name, p.name, p.type
         FROM sqlite_schema m JOIN pragma_table_info(m.name) p
         WHERE m.type IN ('table', 'view') AND m.name NOT LIKE 'sqlite_%'
         ORDER BY m.name, p.cid",
    )?;
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect();
    rows
}

/// 分组与说明只有主库登记；额外库平铺。
fn describe(db: &SqlDb, table: &str) -> (&'static str, &'static str) {
    match db {
        SqlDb::Main => catalog(table).map_or(("其他", "未登记的表或视图"), |entry| {
            (entry.1, entry.2)
        }),
        SqlDb::External { .. } => ("表与视图", ""),
    }
}

fn catalog(table: &str) -> Option<&'static (&'static str, &'static str, &'static str)> {
    crate::schema::TABLE_CATALOG
        .iter()
        .find(|entry| entry.0 == table)
}

/// 返回结果、直接改动行数与涉及的表；`commit` 为假时回滚。
fn execute(
    conn: &mut Connection,
    secrets: &[SecretColumn],
    sql: &str,
    commit: bool,
) -> Result<(QueryResult, u64, BTreeSet<String>), DiagnosticError> {
    let started = Instant::now();
    let deadline = started + DIAGNOSTIC_TIMEOUT;
    let tx = conn.transaction().map_err(StoreError::from)?;
    let tables = Arc::new(Mutex::new(BTreeSet::new()));
    let (hidden, seen) = (secrets.to_vec(), tables.clone());
    tx.authorizer(Some(move |ctx: AuthContext<'_>| {
        authorize_write(&hidden, &seen, ctx)
    }))
    .map_err(StoreError::from)?;
    tx.progress_handler(
        DIAGNOSTIC_PROGRESS_OPS,
        Some(move || Instant::now() >= deadline),
    )
    .map_err(StoreError::from)?;
    let result = run(&tx, sql, started, true).map_err(|e| classify(e, deadline));
    let changes = tx.changes();
    // 回滚与提交本身也要经过授权，须先卸下。
    tx.authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .map_err(StoreError::from)?;
    tx.progress_handler(DIAGNOSTIC_PROGRESS_OPS, None::<fn() -> bool>)
        .map_err(StoreError::from)?;
    let result = result?;
    if commit {
        tx.commit().map_err(StoreError::from)?;
    }
    let tables = std::mem::take(&mut *tables.lock().expect("表集合锁中毒"));
    Ok((result, changes, tables))
}

fn backup(conn: &Connection, dir: &Path, name: &str, at: i64) -> Result<String, DiagnosticError> {
    let failed =
        |e: &dyn std::fmt::Display| DiagnosticError::Backup(format!("{}：{e}", dir.display()));
    std::fs::create_dir_all(dir).map_err(|e| failed(&e))?;
    let file = format!("{name}-{at}.db");
    let path = dir.join(&file);
    let target = path.to_str().ok_or_else(|| failed(&"路径不是 UTF-8"))?;
    conn.execute("VACUUM INTO ?1", [target])
        .map_err(|e| failed(&e))?;
    prune(dir, name).map_err(|e| failed(&e))?;
    Ok(file)
}

/// 每个库只留最近的若干份备份；库名不含 `-`，前缀不会串库。
fn prune(dir: &Path, name: &str) -> std::io::Result<()> {
    let prefix = format!("{name}-");
    let mut stamps: Vec<i64> = std::fs::read_dir(dir)?
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            name.strip_prefix(&prefix)?
                .strip_suffix(".db")?
                .parse()
                .ok()
        })
        .collect();
    stamps.sort_unstable();
    let stale = stamps.len().saturating_sub(SQL_BACKUP_KEEP);
    for at in &stamps[..stale] {
        std::fs::remove_file(dir.join(format!("{name}-{at}.db")))?;
    }
    Ok(())
}

fn ddl(conn: &Connection, db: &SqlDb, tables: &[String]) -> rusqlite::Result<String> {
    let mut stmt = conn.prepare(
        "SELECT sql FROM sqlite_schema WHERE tbl_name = ?1 AND sql IS NOT NULL
         ORDER BY type <> 'table', name",
    )?;
    let mut out = String::new();
    for table in tables {
        match describe(db, table).1 {
            "" => out.push_str(&format!("-- {table}\n")),
            description => out.push_str(&format!("-- {table}：{description}\n")),
        }
        for sql in stmt.query_map([table], |r| r.get::<_, String>(0))? {
            out.push_str(&sql?);
            out.push_str(";\n");
        }
        out.push('\n');
    }
    Ok(out)
}

/// `drain` 时截断后仍执行到底，保证写语句完整生效。
fn run(
    conn: &Connection,
    sql: &str,
    started: Instant,
    drain: bool,
) -> rusqlite::Result<QueryResult> {
    let mut stmt = conn.prepare(sql)?;
    let columns: Vec<String> = stmt.column_names().into_iter().map(str::to_owned).collect();
    let width = columns.len();
    let mut rows = Vec::new();
    let mut bytes = 0;
    let mut truncated = false;
    let mut cursor = stmt.query([])?;
    while let Some(row) = cursor.next()? {
        let mut size = 0;
        for i in 0..width {
            size += 8 + match row.get_ref(i)? {
                ValueRef::Text(v) => v.len(),
                _ => 0,
            };
        }
        if truncated || rows.len() == DIAGNOSTIC_ROWS || bytes + size > DIAGNOSTIC_BYTES {
            truncated = true;
            if drain {
                continue;
            }
            break;
        }
        bytes += size;
        let mut cells = Vec::with_capacity(width);
        for i in 0..width {
            cells.push(match row.get_ref(i)? {
                ValueRef::Null => Cell::Null,
                ValueRef::Integer(v) => Cell::Integer(v),
                ValueRef::Real(v) => Cell::Real(v),
                ValueRef::Text(v) => Cell::Text(String::from_utf8_lossy(v).into_owned()),
                ValueRef::Blob(v) => Cell::Blob {
                    bytes: v.len() as u64,
                },
            });
        }
        rows.push(cells);
    }
    Ok(QueryResult {
        columns,
        rows,
        truncated,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

/// 用户 SQL 引起的 SQLite 错误都归为 Rejected；进度回调中断按截止时间判为超时。
fn classify(error: rusqlite::Error, deadline: Instant) -> DiagnosticError {
    match error.sqlite_error_code() {
        Some(ErrorCode::OperationInterrupted) if Instant::now() >= deadline => {
            DiagnosticError::Timeout
        }
        _ => DiagnosticError::Rejected(error.to_string()),
    }
}

#[cfg(test)]
mod tests;
