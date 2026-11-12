//! 临时诊断（session-load-diagnostics），排查完整体删除：前端上报的慢会话加载，记一条 WARN 进运行日志。
//! 契约：docs/blueprints/session-load-diagnostics.md。

use std::fmt::Write as _;

use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use crate::api::json;
use crate::error::ApiError;
use crate::limits::LOAD_REPORT_UA_CHARS;

/// 前端各阶段相对打开时刻的 ms；未到达的阶段为 null。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Report {
    session_id: i64,
    reconnect: bool,
    /// 上报时距打开的耗时；未就绪即放弃时也有。
    elapsed: u32,
    session: Option<u32>,
    open: Option<u32>,
    first: Option<u32>,
    ready: Option<u32>,
    painted: Option<u32>,
    server: Option<ServerTiming>,
    net: Option<Net>,
    events: u32,
    chars: u32,
    max_gap: u32,
    lag: u32,
    hidden_at_start: bool,
    visibility_changed: bool,
    running: u32,
    messages: u32,
    ua: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ServerTiming {
    replay_ms: u32,
    state_ms: u32,
    total_ms: u32,
}

/// `getSession` 的 Resource Timing 分段。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Net {
    dns: u32,
    connect: u32,
    tls: u32,
    ttfb: u32,
    download: u32,
    protocol: String,
}

fn put(out: &mut String, name: &str, value: Option<u32>) {
    match value {
        Some(v) => write!(out, "{name}={v} "),
        None => write!(out, "{name}=- "),
    }
    .expect("写 String 不失败");
}

pub(crate) async fn report(
    body: Result<Json<Report>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let r = json(body)?;
    let mut phases = String::new();
    for (name, v) in [
        ("session", r.session),
        ("open", r.open),
        ("first", r.first),
        ("ready", r.ready),
        ("painted", r.painted),
    ] {
        put(&mut phases, name, v);
    }
    let server = r.server.map_or("-".to_owned(), |s| {
        format!(
            "replay={} state={} total={}",
            s.replay_ms, s.state_ms, s.total_ms
        )
    });
    let net = r.net.map_or("-".to_owned(), |n| {
        format!(
            "dns={} connect={} tls={} ttfb={} download={} protocol={}",
            n.dns, n.connect, n.tls, n.ttfb, n.download, n.protocol
        )
    });
    let ua: String = r.ua.chars().take(LOAD_REPORT_UA_CHARS).collect();
    tracing::warn!(
        session_id = r.session_id,
        elapsed = r.elapsed,
        reconnect = r.reconnect,
        phases = phases.trim_end(),
        server = %server,
        net = %net,
        stream = %format!("events={} chars={} max_gap={}", r.events, r.chars, r.max_gap),
        lag = r.lag,
        hidden_at_start = r.hidden_at_start,
        visibility_changed = r.visibility_changed,
        running = r.running,
        messages = r.messages,
        ua = %ua,
        "session load slow"
    );
    Ok(StatusCode::NO_CONTENT)
}
