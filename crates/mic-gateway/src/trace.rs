//! trace 与看板的只读接口。契约：docs/blueprints/run-trace.md §六。

use std::sync::Arc;

use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::Json;
use mic_message::{Message, MessageId, SessionId};
use mic_store::{Dashboard, ModelCallId, Peers, RunId, TraceRunPage, TraceSession};
use serde::Deserialize;

use crate::error::{ApiError, SESSION_NOT_FOUND};
use crate::limits::TRACE_RUNS_DEFAULT;
use crate::log_store::StoredRecord;
use crate::service::App;

type Id = Result<Path<i64>, PathRejection>;

fn id(id: Id) -> Result<i64, ApiError> {
    id.map(|Path(id)| id)
        .map_err(|e| ApiError::BadRequest(format!("路径参数不合法：{e}")))
}

pub(crate) async fn sessions(
    State(app): State<Arc<App>>,
) -> Result<Json<Vec<TraceSession>>, ApiError> {
    Ok(Json(app.kernel.trace_sessions().await?))
}

pub(crate) async fn usage(
    State(app): State<Arc<App>>,
    session: Id,
) -> Result<Json<mic_store::SessionUsageView>, ApiError> {
    Ok(Json(
        app.kernel.session_usage(SessionId(id(session)?)).await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunsQuery {
    after: Option<i64>,
    limit: Option<u32>,
}

pub(crate) async fn runs(
    State(app): State<Arc<App>>,
    session: Id,
    query: Result<Query<RunsQuery>, QueryRejection>,
) -> Result<Json<TraceRunPage>, ApiError> {
    let session = SessionId(id(session)?);
    let Query(q) = query.map_err(|e| ApiError::BadRequest(format!("查询参数不合法：{e}")))?;
    let limit = match q.limit {
        None => TRACE_RUNS_DEFAULT,
        Some(n @ 1..=100) => n,
        Some(_) => return Err(ApiError::BadRequest("limit 须在 1 到 100 之间".into())),
    };
    Ok(Json(
        app.kernel
            .trace_runs(session, q.after.map(RunId), limit)
            .await?,
    ))
}

#[derive(serde::Serialize)]
pub(crate) struct SystemPrompt {
    text: String,
    blocks: Vec<mic_core::PromptBlock>,
    tools: Vec<mic_core::ToolSpec>,
}

pub(crate) async fn system(
    State(app): State<Arc<App>>,
    session: Id,
) -> Result<Json<SystemPrompt>, ApiError> {
    let session = SessionId(id(session)?);
    let not_found = |e| match e {
        mic_core::KernelError::SessionNotFound => ApiError::NotFound(SESSION_NOT_FOUND),
        e => e.into(),
    };
    let prompt = app.kernel.system_prompt(session).await.map_err(not_found)?;
    let tools = app.kernel.session_tools(session).await.map_err(not_found)?;
    Ok(Json(SystemPrompt {
        text: prompt.render(),
        blocks: prompt.blocks,
        tools,
    }))
}

pub(crate) async fn message(
    State(app): State<Arc<App>>,
    message: Id,
) -> Result<Json<Message>, ApiError> {
    app.kernel
        .message(MessageId(id(message)?))
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound("消息不存在"))
}

pub(crate) async fn call_input(
    State(app): State<Arc<App>>,
    call: Id,
) -> Result<Json<Vec<Message>>, ApiError> {
    app.kernel
        .call_input(ModelCallId(id(call)?))
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound("该调用没有会话输入"))
}

pub(crate) async fn run_logs(
    State(app): State<Arc<App>>,
    run: Id,
) -> Result<Json<Vec<StoredRecord>>, ApiError> {
    let run = id(run)?;
    let logs = app.logs.clone();
    let records = tokio::task::spawn_blocking(move || logs.by_run(run))
        .await
        .expect("日志查询不 panic")
        .map_err(|e| ApiError::Unavailable(format!("读取日志库失败：{e}")))?;
    Ok(Json(records))
}

pub(crate) async fn peers(State(app): State<Arc<App>>) -> Result<Json<Peers>, ApiError> {
    Ok(Json(app.kernel.call_peers().await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DashboardQuery {
    days: Option<u32>,
}

pub(crate) async fn dashboard(
    State(app): State<Arc<App>>,
    query: Result<Query<DashboardQuery>, QueryRejection>,
) -> Result<Json<Dashboard>, ApiError> {
    let Query(q) = query.map_err(|e| ApiError::BadRequest(format!("查询参数不合法：{e}")))?;
    let days = match q.days {
        None => 7,
        Some(n @ 1..=366) => n,
        Some(_) => return Err(ApiError::BadRequest("days 须在 1 到 366 之间".into())),
    };
    Ok(Json(app.kernel.dashboard(days).await?))
}
