//! 定时任务页面接口。
use crate::api::json;
use crate::error::ApiError;
use crate::limits::{PAGE_DEFAULT, PAGE_MAX};
use crate::service::App;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use mic_core::{ScheduleFirePage, ScheduleUpdate, ScheduleView};
use mic_store::{FireId, ScheduleId};
use serde::Deserialize;
use std::sync::Arc;

fn id(path: Result<Path<i64>, PathRejection>) -> Result<ScheduleId, ApiError> {
    path.map(|Path(id)| ScheduleId(id))
        .map_err(|_| ApiError::NotFound("定时任务不存在"))
}

pub(crate) async fn list(State(app): State<Arc<App>>) -> Result<Json<Vec<ScheduleView>>, ApiError> {
    Ok(Json(app.kernel.schedules().await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FiresQuery {
    before: Option<i64>,
    limit: Option<u32>,
}

pub(crate) async fn fires(
    State(app): State<Arc<App>>,
    path: Result<Path<i64>, PathRejection>,
    query: Result<Query<FiresQuery>, QueryRejection>,
) -> Result<Json<ScheduleFirePage>, ApiError> {
    let Query(q) = query.map_err(|e| ApiError::BadRequest(format!("查询参数不合法：{e}")))?;
    let limit = match q.limit {
        None => PAGE_DEFAULT,
        Some(n @ 1..=PAGE_MAX) => n,
        Some(_) => {
            return Err(ApiError::BadRequest(format!(
                "limit 须在 1 到 {PAGE_MAX} 之间"
            )))
        }
    };
    Ok(Json(
        app.kernel
            .schedule_fires(id(path)?, q.before.map(FireId), limit)
            .await?,
    ))
}

pub(crate) async fn update(
    State(app): State<Arc<App>>,
    path: Result<Path<i64>, PathRejection>,
    body: Result<Json<ScheduleUpdate>, JsonRejection>,
) -> Result<Json<ScheduleView>, ApiError> {
    Ok(Json(
        app.kernel.update_schedule(id(path)?, json(body)?).await?,
    ))
}

pub(crate) async fn delete(
    State(app): State<Arc<App>>,
    path: Result<Path<i64>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    app.kernel.delete_schedule(id(path)?).await?;
    Ok(StatusCode::NO_CONTENT)
}
