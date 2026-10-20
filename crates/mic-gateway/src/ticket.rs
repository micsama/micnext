//! SSE 连接票据：EventSource 不能带 Bearer 头，先用 Bearer 换一张绑定目标流、一次性、短时的票据。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::Json;
use serde::Serialize;

use crate::api::session;
use crate::error::ApiError;
use crate::limits::{TICKET_BYTES, TICKET_TTL};
use crate::service::{random_hex, App};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamTarget {
    Session(i64),
    Logs,
    Pushes,
}

/// 未兑换的票据；进程内单实例足够。
#[derive(Default)]
pub(crate) struct Tickets(Mutex<HashMap<String, (StreamTarget, Instant)>>);

impl Tickets {
    fn issue(&self, target: StreamTarget) -> String {
        let now = Instant::now();
        let ticket = random_hex(TICKET_BYTES);
        let mut map = self.0.lock().expect("票据表锁不中毒");
        map.retain(|_, (_, expires)| *expires > now);
        map.insert(ticket.clone(), (target, now + TICKET_TTL));
        ticket
    }

    /// 兑换即作废；缺失、过期、已用或目标不符都是 401。
    pub(crate) fn redeem(
        &self,
        ticket: Option<&str>,
        target: StreamTarget,
    ) -> Result<(), ApiError> {
        let entry = ticket.and_then(|t| self.0.lock().expect("票据表锁不中毒").remove(t));
        match entry {
            Some((t, expires)) if t == target && expires > Instant::now() => Ok(()),
            _ => Err(ApiError::Unauthorized),
        }
    }
}

#[derive(Serialize)]
struct Issued {
    ticket: String,
}

fn issued(ticket: String) -> impl IntoResponse {
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(Issued { ticket }),
    )
}

pub(crate) async fn session_ticket(
    State(app): State<Arc<App>>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let session = session(&app, id).await?;
    Ok(issued(
        app.tickets.issue(StreamTarget::Session(session.id.0)),
    ))
}

pub(crate) async fn logs_ticket(State(app): State<Arc<App>>) -> impl IntoResponse {
    issued(app.tickets.issue(StreamTarget::Logs))
}

pub(crate) async fn pushes_ticket(State(app): State<Arc<App>>) -> impl IntoResponse {
    issued(app.tickets.issue(StreamTarget::Pushes))
}
