//! 通知中心、任务页与侧栏共用的全局事件流。
use crate::limits::STREAM_BUFFER;
use crate::service::App;
use crate::sse;
use axum::extract::State;
use axum::http::header;
use axum::response::{sse::Event, IntoResponse};
use mic_core::KernelEventKind;
use mic_message::MessageBody;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Serialize)]
struct Changed {
    id: i64,
}

/// 只传变化事实的 id，客户端按需重拉。
pub(crate) async fn stream(State(app): State<Arc<App>>) -> impl IntoResponse {
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
                        MessageBody::Reply { .. }
                            | MessageBody::Notification { .. }
                            | MessageBody::Attachment { .. }
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
    ([(header::CACHE_CONTROL, "no-store")], sse::respond(rx))
}
