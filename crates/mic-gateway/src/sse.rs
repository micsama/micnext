//! SSE 响应：把待发事件队列包成流，带心跳。

use std::convert::Infallible;

use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::{self, Stream};
use tokio::sync::mpsc;

use crate::limits::SSE_KEEPALIVE;

pub(crate) fn respond(
    rx: mpsc::Receiver<Event>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let body = stream::unfold(rx, |mut rx| async move {
        let event = rx.recv().await?;
        Some((Ok(event), rx))
    });
    Sse::new(body).keep_alive(KeepAlive::new().interval(SSE_KEEPALIVE))
}
