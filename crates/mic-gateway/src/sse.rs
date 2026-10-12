//! SSE 响应：把待发事件队列包成流，带心跳与批尾空注释。

use std::convert::Infallible;

use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::{self, Stream};
use tokio::sync::mpsc;

use crate::limits::{SSE_KEEPALIVE, SSE_NUDGE};

/// 一批事件后静默 `SSE_NUDGE` 补一条空注释：WebKit 会压住流末尾已到达的数据，
/// 直到同一流上再来字节。
pub(crate) fn respond(
    rx: mpsc::Receiver<Event>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let body = stream::unfold((rx, false), |(mut rx, nudge)| async move {
        let event = if nudge {
            match tokio::time::timeout(SSE_NUDGE, rx.recv()).await {
                Ok(event) => event?,
                Err(_) => return Some((Ok(Event::default().comment("")), (rx, false))),
            }
        } else {
            rx.recv().await?
        };
        Some((Ok(event), (rx, true)))
    });
    Sse::new(body).keep_alive(KeepAlive::new().interval(SSE_KEEPALIVE))
}
