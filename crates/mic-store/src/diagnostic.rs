//! 开发者诊断：独立只读连接上执行单条只读 SQL，凭据列读作 NULL。
//! 契约：docs/blueprints/developer-diagnostics.md §三。

use std::path::Path;
use std::sync::{Mutex, TryLockError};
use std::time::Instant;

use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, ErrorCode, OpenFlags};

use crate::limits::{
    DIAGNOSTIC_BYTES, DIAGNOSTIC_PROGRESS_OPS, DIAGNOSTIC_ROWS, DIAGNOSTIC_TIMEOUT,
};
use crate::{SecretColumn, Store, StoreError};

#[derive(Debug, Clone, PartialEq)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Cell>>,
    /// 行数或文本字节达到上限，其后结果未读取。
    pub truncated: bool,
    pub elapsed_ms: u64,
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
    pub columns: Vec<ColumnSchema>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnSchema {
    pub name: String,
    pub decl_type: String,
    pub secret: bool,
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
    #[error(transparent)]
    Store(#[from] StoreError),
}

pub(crate) struct Diagnostic {
    conn: Mutex<Connection>,
    secrets: Vec<SecretColumn>,
}

impl Diagnostic {
    /// 须在业务连接完成迁移后打开。
    pub(crate) fn open(path: &Path, secrets: Vec<SecretColumn>) -> rusqlite::Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let hidden = secrets.clone();
        conn.authorizer(Some(move |ctx: AuthContext<'_>| authorize(&hidden, ctx)))?;
        Ok(Self {
            conn: Mutex::new(conn),
            secrets,
        })
    }

    fn is_secret(&self, table: &str, column: &str) -> bool {
        is_secret(&self.secrets, table, column)
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

impl Store {
    /// 在只读连接上执行一条 SQL；同一时间只允许一个查询。
    pub async fn diagnostic_query(&self, sql: String) -> Result<QueryResult, DiagnosticError> {
        let diagnostic = self
            .diagnostic
            .clone()
            .ok_or(DiagnosticError::Unavailable)?;
        tokio::task::spawn_blocking(move || {
            let conn = match diagnostic.conn.try_lock() {
                Ok(conn) => conn,
                Err(TryLockError::WouldBlock) => return Err(DiagnosticError::Busy),
                Err(TryLockError::Poisoned(_)) => panic!("诊断连接锁中毒"),
            };
            let started = Instant::now();
            let deadline = started + DIAGNOSTIC_TIMEOUT;
            conn.progress_handler(
                DIAGNOSTIC_PROGRESS_OPS,
                Some(move || Instant::now() >= deadline),
            )
            .map_err(StoreError::from)?;
            let result = run(&conn, &sql, started).map_err(|e| classify(e, deadline));
            conn.progress_handler(DIAGNOSTIC_PROGRESS_OPS, None::<fn() -> bool>)
                .map_err(StoreError::from)?;
            result
        })
        .await
        .expect("诊断查询任务 panic")
    }

    /// 全部用户表与视图的列；凭据列标记 `secret`。
    pub async fn diagnostic_schema(&self) -> Result<Vec<TableSchema>, DiagnosticError> {
        let diagnostic = self
            .diagnostic
            .clone()
            .ok_or(DiagnosticError::Unavailable)?;
        let rows = self
            .call(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT m.name, p.name, p.type
                     FROM sqlite_schema m JOIN pragma_table_info(m.name) p
                     WHERE m.type IN ('table', 'view') AND m.name NOT LIKE 'sqlite_%'
                     ORDER BY m.name, p.cid",
                )?;
                let rows = stmt
                    .query_map([], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?;
        let mut tables: Vec<TableSchema> = Vec::new();
        for (table, column, decl_type) in rows {
            let secret = diagnostic.is_secret(&table, &column);
            let column = ColumnSchema {
                name: column,
                decl_type,
                secret,
            };
            match tables.last_mut() {
                Some(last) if last.name == table => last.columns.push(column),
                _ => tables.push(TableSchema {
                    name: table,
                    columns: vec![column],
                }),
            }
        }
        Ok(tables)
    }
}

fn run(conn: &Connection, sql: &str, started: Instant) -> rusqlite::Result<QueryResult> {
    let mut stmt = conn.prepare(sql)?;
    let columns: Vec<String> = stmt.column_names().into_iter().map(str::to_owned).collect();
    let width = columns.len();
    let mut rows = Vec::new();
    let mut bytes = 0;
    let mut truncated = false;
    let mut cursor = stmt.query([])?;
    while let Some(row) = cursor.next()? {
        if rows.len() == DIAGNOSTIC_ROWS || bytes >= DIAGNOSTIC_BYTES {
            truncated = true;
            break;
        }
        let mut cells = Vec::with_capacity(width);
        for i in 0..width {
            let cell = match row.get_ref(i)? {
                ValueRef::Null => Cell::Null,
                ValueRef::Integer(v) => Cell::Integer(v),
                ValueRef::Real(v) => Cell::Real(v),
                ValueRef::Text(v) => {
                    bytes += v.len();
                    Cell::Text(String::from_utf8_lossy(v).into_owned())
                }
                ValueRef::Blob(v) => Cell::Blob {
                    bytes: v.len() as u64,
                },
            };
            bytes += 8;
            cells.push(cell);
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
