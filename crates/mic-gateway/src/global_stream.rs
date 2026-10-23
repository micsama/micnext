//! 通知中心、任务页与侧栏共用的全局事件流。
use crate::error::ApiError;
use crate::limits::STREAM_BUFFER;
use crate::service::App;
use crate::sse;
use crate::ticket::StreamTarget;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::http::header;
use axum::response::{sse::Event, IntoResponse};
use mic_core::KernelEventKind;
use mic_message::MessageBody;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StreamQuery {
    ticket: Option<String>,
}

#[derive(Serialize)]
struct Changed {
    id: i64,
}

/// 只传变化事实的 id，客户端按需重拉。
pub(crate) async fn stream(
    State(app): State<Arc<App>>,
    query: Result<Query<StreamQuery>, QueryRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let Query(q) = query.map_err(|e| ApiError::BadRequest(format!("查询参数不合法：{e}")))?;
    app.tickets
        .redeem(q.ticket.as_deref(), StreamTarget::Global)?;
    let mut events = app.kernel.subscribe();
    let (tx, rx) = mpsc::channel(STREAM_BUFFER);
    tokio::spawn(async move {
        loop {
            let event = tokio::select! {
                () = app.stop.cancelled() => return,
                () = tx.closed() => return,
                event = events.recv() => match event {
                    Ok(event) => event,
                    Err(lagged) => return tracing::warn!(%lagged, "global stream lagged"),
                },
            };
            let (name, id) = match event.kind {
                KernelEventKind::PushCreated { push_id } => ("push_created", push_id.0),
                KernelEventKind::PushDeliveryChanged { push_id } => {
                    ("push_delivery_changed", push_id.0)
                }
                KernelEventKind::ScheduleChanged { schedule } => ("schedule_changed", schedule.0),
                KernelEventKind::MessageAppended(ref m)
                    if matches!(
                        m.body,
                        MessageBody::Reply { .. } | MessageBody::Notification { .. }
                    ) =>
                {
                    ("session_activity", event.session_id.0)
                }
                KernelEventKind::InputsClaimed { .. }
                | KernelEventKind::RunFinished { .. }
                | KernelEventKind::SessionChanged => ("session_activity", event.session_id.0),
                _ => continue,
            };
            let event = Event::default()
                .event(name)
                .json_data(Changed { id })
                .expect("SSE 数据可序列化");
            if tx.send(event).await.is_err() {
                return;
            }
        }
    });
    Ok(([(header::CACHE_CONTROL, "no-store")], sse::respond(rx)))
}
