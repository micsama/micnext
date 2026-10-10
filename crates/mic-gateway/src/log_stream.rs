use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::http::header;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use futures_util::stream;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::error::ApiError;
use crate::limits::{LOG_BATCH, SSE_KEEPALIVE};
use crate::logs::{After, LogRecord};
use crate::service::App;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NoQuery {}

#[derive(Serialize)]
struct Ready {
    retained_count: usize,
    history_trimmed: bool,
}

/// 回放当前保留窗口再跟随；没有续传，重连即整窗重放。
pub(crate) async fn stream(
    State(app): State<Arc<App>>,
    query: Result<Query<NoQuery>, QueryRejection>,
) -> Result<impl IntoResponse, ApiError> {
    query.map_err(|e| ApiError::BadRequest(format!("查询参数不合法：{e}")))?;
    let (tx, rx) = mpsc::channel(LOG_BATCH);
    tokio::spawn(feed(app, tx));
    let body = stream::unfold(rx, |mut rx| async move {
        rx.recv()
            .await
            .map(|event| (Ok::<_, Infallible>(event), rx))
    });
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Sse::new(body).keep_alive(KeepAlive::new().interval(SSE_KEEPALIVE)),
    ))
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
