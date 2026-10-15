use std::sync::Arc;

use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::http::header;
use axum::response::sse::Event;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::error::ApiError;
use crate::limits::LOG_BATCH;
use crate::logs::{After, LogRecord};
use crate::service::App;
use crate::sse;
use crate::ticket::StreamTarget;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NoQuery {}

#[derive(Serialize)]
struct Ready {
    retained_count: usize,
    history_trimmed: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StreamQuery {
    ticket: Option<String>,
}

/// 回放当前保留窗口再跟随；没有续传，重连即整窗重放。
pub(crate) async fn stream(
    State(app): State<Arc<App>>,
    query: Result<Query<StreamQuery>, QueryRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let Query(q) = query.map_err(|e| ApiError::BadRequest(format!("查询参数不合法：{e}")))?;
    app.tickets
        .redeem(q.ticket.as_deref(), StreamTarget::Logs)?;
    let (tx, rx) = mpsc::channel(LOG_BATCH);
    tokio::spawn(feed(app, tx));
    Ok(([(header::CACHE_CONTROL, "no-store")], sse::respond(rx)))
}

/// 之前启动的最近日志与当前存储状态。
pub(crate) async fn history(
    State(app): State<Arc<App>>,
    query: Result<Query<NoQuery>, QueryRejection>,
) -> Result<impl IntoResponse, ApiError> {
    query.map_err(|e| ApiError::BadRequest(format!("查询参数不合法：{e}")))?;
    let logs = app.logs.clone();
    let history = tokio::task::spawn_blocking(move || logs.history())
        .await
        .expect("日志历史查询不 panic")
        .map_err(|e| ApiError::Unavailable(format!("读取日志库失败：{e}")))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(history)))
}

/// 客户端断开、进程停止或实时阶段未读记录被淘汰时返回，流随之结束。
async fn feed(app: Arc<App>, tx: mpsc::Sender<Event>) {
    // 先订阅再取快照：之后追加的记录一定能按 seq 读到。
    let mut latest = app.logs.subscribe();
    let snapshot = app.logs.snapshot();
    let ready = Ready {
        retained_count: snapshot.records.len(),
        history_trimmed: snapshot.trimmed,
    };
    if !send(&app, &tx, json_event("ready", &ready)).await {
        return;
    }
    let mut last = snapshot.latest;
    for record in snapshot.records {
        if !send(&app, &tx, log_event(&record)).await {
            return;
        }
    }
    loop {
        let records = match app.logs.after(last, LOG_BATCH) {
            After::Records(records) => records,
            After::Evicted => return,
        };
        if records.is_empty() {
            tokio::select! {
                () = app.stop.cancelled() => return,
                () = tx.closed() => return,
                changed = latest.changed() => if changed.is_err() { return },
            }
            continue;
        }
        for record in records {
            last = record.seq;
            if !send(&app, &tx, log_event(&record)).await {
                return;
            }
        }
    }
}

async fn send(app: &App, tx: &mpsc::Sender<Event>, event: Event) -> bool {
    tokio::select! {
        () = app.stop.cancelled() => false,
        sent = tx.send(event) => sent.is_ok(),
    }
}

fn log_event(record: &LogRecord) -> Event {
    json_event("log", record)
}

fn json_event<T: Serialize>(name: &str, data: &T) -> Event {
    Event::default()
        .event(name)
        .json_data(data)
        .expect("SSE 数据可序列化")
}
