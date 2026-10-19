//! 会话树的后台任务：列表、读输出、终止。契约：docs/blueprints/archive/async-jobs.md 总则 §四。

use std::sync::Arc;

use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::Json;
use mic_core::{JobId, JobOutput, JobView, StopJobOutcome};
use mic_message::SessionId;
use serde::{Deserialize, Serialize};

use crate::api::{session, writable};
use crate::error::{ApiError, JOB_NOT_FOUND, SESSION_NOT_FOUND};
use crate::service::App;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListQuery {
    #[serde(default)]
    include_old: bool,
}

pub(crate) async fn list(
    State(app): State<Arc<App>>,
    id: Result<Path<i64>, PathRejection>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Json<Vec<JobView>>, ApiError> {
    let session = session(&app, id).await?;
    let Query(q) = query.map_err(|e| ApiError::BadRequest(format!("查询参数不合法：{e}")))?;
    Ok(Json(app.kernel.jobs(session.id, q.include_old).await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OutputQuery {
    offset: Option<u64>,
    limit: Option<u64>,
}

pub(crate) async fn output(
    State(app): State<Arc<App>>,
    path: Result<Path<(i64, i64)>, PathRejection>,
    query: Result<Query<OutputQuery>, QueryRejection>,
) -> Result<Json<JobOutput>, ApiError> {
    let (session, job) = ids(path)?;
    let Query(q) = query.map_err(|e| ApiError::BadRequest(format!("查询参数不合法：{e}")))?;
    Ok(Json(
        app.kernel
            .job_output(session, job, q.offset, q.limit)
            .await?,
    ))
}

#[derive(Serialize)]
pub(crate) struct Stopped {
    outcome: StopJobOutcome,
}

pub(crate) async fn stop(
    State(app): State<Arc<App>>,
    path: Result<Path<(i64, i64)>, PathRejection>,
) -> Result<Json<Stopped>, ApiError> {
    let (session, job) = ids(path)?;
    let s = app
        .kernel
        .session(session)
        .await?
        .ok_or(ApiError::NotFound(SESSION_NOT_FOUND))?;
    if !writable(&s) {
        return Err(ApiError::ReadOnly);
    }
    Ok(Json(Stopped {
        outcome: app.kernel.stop_job(session, job).await?,
    }))
}

fn ids(path: Result<Path<(i64, i64)>, PathRejection>) -> Result<(SessionId, JobId), ApiError> {
    let Path((session, job)) = path.map_err(|_| ApiError::NotFound(JOB_NOT_FOUND))?;
    Ok((SessionId(session), JobId(job)))
}
