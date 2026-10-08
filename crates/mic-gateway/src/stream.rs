use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::{self, Stream};
use mic_core::{EventReceiver, KernelEventKind};
use mic_message::{Message, MessageBody, MessageId, SessionId};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::api::session;
use crate::error::ApiError;
use crate::limits::{SSE_KEEPALIVE, STREAM_BUFFER};
use crate::service::App;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StreamQuery {
    after: Option<i64>,
}

pub(crate) async fn stream(
    State(app): State<Arc<App>>,
    id: Result<Path<i64>, PathRejection>,
    query: Result<Query<StreamQuery>, QueryRejection>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let session = session(&app, id).await?;
    let Query(q) = query.map_err(|e| ApiError::BadRequest(format!("查询参数不合法：{e}")))?;
    // 先订阅再回放：回放期间发生的事件留在订阅里，交接时按 id 去重。
    let events = app.kernel.subscribe();
    let (tx, rx) = mpsc::channel(STREAM_BUFFER);
    tokio::spawn(feed(app, session.id, q.after.map(MessageId), events, tx));
    let body = stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|event| (Ok(event), rx))
    });
    Ok(Sse::new(body).keep_alive(KeepAlive::new().interval(SSE_KEEPALIVE)))
}

/// 客户端断开（发送失败）、订阅落后、进程停止或出错时返回，流随之结束。
async fn feed(
    app: Arc<App>,
    session_id: SessionId,
    after: Option<MessageId>,
    mut events: EventReceiver,
    tx: mpsc::Sender<Event>,
) {
    let replayed = match app.kernel.messages_after(session_id, after).await {
        Ok(messages) => messages,
        Err(e) => return tracing::error!(error = %e, "gateway replay failed"),
    };
    let mut last = after;
    for message in replayed {
        last = Some(message.id);
        if !send(&app, &tx, message_event(&message)).await {
            return;
        }
    }
    let executing_run = match app.kernel.executing_run(session_id).await {
        Ok(run) => run,
        Err(e) => return tracing::error!(error = %e, "gateway replay failed"),
    };
    let ready = Ready {
        executing_run: executing_run.map(|r| r.0),
    };
    if !send(&app, &tx, json_event("ready", &ready)).await {
        return;
    }

    // 已转发增量、尚未以 `Reply` 或 `draft_discarded` 结束的草稿。
    let mut draft_open = false;
    loop {
        let event = tokio::select! {
            () = app.stop.cancelled() => return,
            () = tx.closed() => return,
            event = events.recv() => match event {
                Ok(event) => event,
                Err(lagged) => return tracing::warn!(%lagged, "gateway stream lagged"),
            },
        };
        if event.session_id != session_id {
            continue;
        }
        let out = match event.kind {
            KernelEventKind::MessageAppended(message) => {
                let is_reply = matches!(message.body, MessageBody::Reply { .. });
                if last.is_some_and(|n| message.id <= n) {
                    // 这份草稿已作为稳定消息回放过，收掉接入后转发的那部分。
                    if !(is_reply && draft_open) {
                        continue;
                    }
                    draft_open = false;
                    json_event("draft_discarded", &Empty {})
                } else {
                    if is_reply {
                        draft_open = false;
                    }
                    message_event(&message)
                }
            }
            KernelEventKind::TextDelta(text) => {
                draft_open = true;
                json_event("text_delta", &Delta { text })
            }
            KernelEventKind::ReasoningDelta(text) => {
                draft_open = true;
                json_event("reasoning_delta", &Delta { text })
            }
            KernelEventKind::DraftDiscarded => {
                draft_open = false;
                json_event("draft_discarded", &Empty {})
            }
            KernelEventKind::RunStarted { run_id } => {
                json_event("run_started", &RunStarted { run_id: run_id.0 })
            }
            KernelEventKind::RunFinished { run_id, state } => json_event(
                "run_finished",
                &RunFinished {
                    run_id: run_id.0,
                    state: state.as_str(),
                },
            ),
        };
        if !send(&app, &tx, out).await {
            return;
        }
    }
}

/// 客户端不读时发送会一直等，故同样响应停止；返回 `false` 即该结束。
async fn send(app: &App, tx: &mpsc::Sender<Event>, event: Event) -> bool {
    tokio::select! {
        () = app.stop.cancelled() => false,
        sent = tx.send(event) => sent.is_ok(),
    }
}

#[derive(Serialize)]
struct Ready {
    executing_run: Option<i64>,
}

#[derive(Serialize)]
struct Delta {
    text: String,
}

#[derive(Serialize)]
struct Empty {}

#[derive(Serialize)]
struct RunStarted {
    run_id: i64,
}

#[derive(Serialize)]
struct RunFinished {
    run_id: i64,
    state: &'static str,
}

fn message_event(message: &Message) -> Event {
    json_event("message", message).id(message.id.0.to_string())
}

fn json_event<T: Serialize>(name: &str, data: &T) -> Event {
    Event::default()
        .event(name)
        .json_data(data)
        .expect("SSE 数据可序列化")
}
