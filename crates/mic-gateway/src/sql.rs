use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::http::header;
use axum::response::IntoResponse;
use axum::Json;
use mic_store::{Cell, QueryResult, TableSchema};
use serde::{Deserialize, Serialize};

use crate::api::json;
use crate::error::ApiError;
use crate::limits::SQL_MAX_BYTES;
use crate::service::App;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SqlBody {
    sql: String,
}

#[derive(Serialize)]
struct ResultView {
    columns: Vec<String>,
    rows: Vec<Vec<CellView>>,
    truncated: bool,
    elapsed_ms: u64,
}

/// 整数以十进制字符串下发，避免 JavaScript 精度丢失。
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum CellView {
    Null,
    Integer { value: String },
    Real { value: f64 },
    Text { value: String },
    Blob { bytes: u64 },
}

#[derive(Serialize)]
struct TableView {
    name: String,
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
    let SqlBody { sql } = json(body)?;
    if sql.len() > SQL_MAX_BYTES {
        return Err(ApiError::BadRequest(format!(
            "SQL 最多 {} KiB",
            SQL_MAX_BYTES / 1024
        )));
    }
    let result = app.kernel.diagnostic_query(sql).await?;
    Ok(no_store(result_view(result)))
}

pub(crate) async fn schema(State(app): State<Arc<App>>) -> Result<impl IntoResponse, ApiError> {
    let tables = app.kernel.diagnostic_schema().await?;
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
    }
}

fn cell_view(cell: Cell) -> CellView {
    match cell {
        Cell::Null => CellView::Null,
        Cell::Integer(v) => CellView::Integer {
            value: v.to_string(),
        },
        Cell::Real(value) => CellView::Real { value },
        Cell::Text(value) => CellView::Text { value },
        Cell::Blob { bytes } => CellView::Blob { bytes },
    }
}

fn table_view(table: TableSchema) -> TableView {
    TableView {
        name: table.name,
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
