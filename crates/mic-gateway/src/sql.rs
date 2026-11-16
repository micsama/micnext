use std::sync::Arc;

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::Json;
use mic_core::SqlRisk;
use mic_store::{Cell, QueryResult, SqlDb, TableSchema};
use serde::{Deserialize, Serialize};

use crate::api::json;
use crate::error::ApiError;
use crate::limits::SQL_MAX_BYTES;
use crate::service::App;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SqlBody {
    sql: String,
    /// 额外库的名字；不传 = 主库。
    #[serde(default)]
    db: Option<String>,
    /// 写入模式：先备份整库，再在业务连接上执行。
    #[serde(default)]
    write: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReviewBody {
    sql: String,
    #[serde(default)]
    db: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SchemaQuery {
    #[serde(default)]
    db: Option<String>,
}

#[derive(Serialize)]
struct ResultView {
    columns: Vec<String>,
    rows: Vec<Vec<CellView>>,
    truncated: bool,
    elapsed_ms: u64,
    changes: Option<u64>,
    backup: Option<String>,
}

#[derive(Serialize)]
struct ReviewView {
    changes: u64,
    tables: Vec<String>,
    risk: Option<&'static str>,
    reason: Option<String>,
    ai_error: Option<String>,
}

/// 整数以十进制字符串下发，避免 JavaScript 精度丢失。
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum CellView {
    Null,
    Integer { value: String },
    Real { value: String },
    Text { value: String },
    Blob { bytes: u64 },
}

#[derive(Serialize)]
struct TableView {
    name: String,
    group: String,
    description: String,
    columns: Vec<ColumnView>,
}

#[derive(Serialize)]
struct ColumnView {
    name: String,
    decl_type: String,
    secret: bool,
}

pub(crate) async fn query(
    State(app): State<Arc<App>>,
    body: Result<Json<SqlBody>, JsonRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let SqlBody { sql, write, db } = json(body)?;
    check_size(&sql)?;
    let db = target(&app, db)?;
    let view = if write {
        let w = app.kernel.diagnostic_write(db, sql).await?;
        ResultView {
            changes: Some(w.changes),
            backup: Some(w.backup),
            ..result_view(w.result)
        }
    } else {
        result_view(app.kernel.diagnostic_query(db, sql).await?)
    };
    Ok(no_store(view))
}

pub(crate) async fn review(
    State(app): State<Arc<App>>,
    body: Result<Json<ReviewBody>, JsonRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let ReviewBody { sql, db } = json(body)?;
    check_size(&sql)?;
    let review = app.kernel.review_sql(target(&app, db)?, sql).await?;
    let (risk, reason, ai_error) = match review.verdict {
        Ok((risk, reason)) => (
            Some(match risk {
                SqlRisk::Low => "low",
                SqlRisk::Medium => "medium",
                SqlRisk::High => "high",
            }),
            Some(reason),
            None,
        ),
        Err(error) => (None, None, Some(error)),
    };
    Ok(no_store(ReviewView {
        changes: review.changes,
        tables: review.tables,
        risk,
        reason,
        ai_error,
    }))
}

fn check_size(sql: &str) -> Result<(), ApiError> {
    if sql.len() > SQL_MAX_BYTES {
        return Err(ApiError::BadRequest(format!(
            "SQL 最多 {} KiB",
            SQL_MAX_BYTES / 1024
        )));
    }
    Ok(())
}

/// 白名单里的名字解析为目标库。
fn target(app: &App, db: Option<String>) -> Result<SqlDb, ApiError> {
    let Some(name) = db else {
        return Ok(SqlDb::Main);
    };
    let path = app
        .sql_databases
        .get(&name)
        .ok_or_else(|| ApiError::BadRequest(format!("未配置数据库 {name}")))?
        .clone();
    Ok(SqlDb::External { name, path })
}

/// 额外库的名字，按名字排序；空 = 只有主库。
pub(crate) async fn databases(State(app): State<Arc<App>>) -> impl IntoResponse {
    no_store(app.sql_databases.keys().cloned().collect::<Vec<_>>())
}

pub(crate) async fn schema(
    State(app): State<Arc<App>>,
    query: Result<Query<SchemaQuery>, QueryRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let Query(SchemaQuery { db }) =
        query.map_err(|e| ApiError::BadRequest(format!("查询参数不合法：{e}")))?;
    let tables = app.kernel.diagnostic_schema(target(&app, db)?).await?;
    Ok(no_store(
        tables.into_iter().map(table_view).collect::<Vec<_>>(),
    ))
}

fn no_store<T: Serialize>(body: T) -> impl IntoResponse {
    ([(header::CACHE_CONTROL, "no-store")], Json(body))
}

fn result_view(result: QueryResult) -> ResultView {
    ResultView {
        columns: result.columns,
        rows: result
            .rows
            .into_iter()
            .map(|row| row.into_iter().map(cell_view).collect())
            .collect(),
        truncated: result.truncated,
        elapsed_ms: result.elapsed_ms,
        changes: None,
        backup: None,
    }
}

fn cell_view(cell: Cell) -> CellView {
    match cell {
        Cell::Null => CellView::Null,
        Cell::Integer(v) => CellView::Integer {
            value: v.to_string(),
        },
        // NOTE: JSON 无法表示 inf；Debug 格式保留指数写法（1e300）。
        Cell::Real(v) => CellView::Real {
            value: format!("{v:?}"),
        },
        Cell::Text(value) => CellView::Text { value },
        Cell::Blob { bytes } => CellView::Blob { bytes },
    }
}

fn table_view(table: TableSchema) -> TableView {
    TableView {
        name: table.name,
        group: table.group,
        description: table.description,
        columns: table
            .columns
            .into_iter()
            .map(|c| ColumnView {
                name: c.name,
                decl_type: c.decl_type,
                secret: c.secret,
            })
            .collect(),
    }
}
