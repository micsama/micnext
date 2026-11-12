use std::convert::Infallible;
use std::sync::Arc;
use std::time::Instant;

use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::response::sse::{Event, Sse};
use axum::Json;
use futures_util::stream::Stream;
use mic_core::{EventReceiver, KernelEventKind, ReplyDraft};
use mic_message::{Message, MessageId, SessionId};
use mic_store::Resumable;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::api::session;
use crate::error::{ApiError, SESSION_NOT_FOUND};
use crate::limits::{PAGE_MESSAGES, PAGE_ROUNDS, STREAM_BUFFER};
use crate::service::App;
use crate::sse;

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
    let started = Instant::now();
    let Path(id) = id.map_err(|_| ApiError::NotFound(SESSION_NOT_FOUND))?;
    let Query(q) = query.map_err(|e| ApiError::BadRequest(format!("查询参数不合法：{e}")))?;
    let session = session(&app, Ok(Path(id))).await?;
    let (events, cut, draft) = app.kernel.subscribe_from_now(session.id).await?;
    let (tx, rx) = mpsc::channel(STREAM_BUFFER);
    tokio::spawn(feed(
        app,
        session.id,
        q.after.map(MessageId),
        Cut { events, cut, draft },
        started,
        tx,
    ));
    Ok(sse::respond(rx))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PageQuery {
    before: i64,
    /// 定位：一并取回 `[until, before)` 的全部消息。
    until: Option<i64>,
}

#[derive(Serialize)]
pub(crate) struct Page {
    messages: Vec<Message>,
    has_earlier: bool,
}

/// 首次回放之前的历史，一页一页往前取。
pub(crate) async fn page(
    State(app): State<Arc<App>>,
    id: Result<Path<i64>, PathRejection>,
    query: Result<Query<PageQuery>, QueryRejection>,
) -> Result<Json<Page>, ApiError> {
    let session = session(&app, id).await?;
    let Query(q) = query.map_err(|e| ApiError::BadRequest(format!("查询参数不合法：{e}")))?;
    let before = MessageId(q.before);
    let page = match q.until {
        Some(until) => {
            app.kernel
                .messages_page_until(
                    session.id,
                    MessageId(until),
                    before,
                    PAGE_ROUNDS,
                    PAGE_MESSAGES,
                )
                .await?
        }
        None => {
            app.kernel
                .messages_page(session.id, Some(before), PAGE_ROUNDS, PAGE_MESSAGES)
                .await?
        }
    };
    Ok(Json(Page {
        messages: page.messages,
        has_earlier: page.has_earlier,
    }))
}

/// 订阅时刻的交接：切点及之前的消息由回放给出，之后的事件全在 `events` 里；`draft` 是那一刻的草稿。
struct Cut {
    events: EventReceiver,
    cut: Option<MessageId>,
    draft: Option<ReplyDraft>,
}

/// 客户端断开（发送失败）、订阅落后、进程停止或出错时返回，流随之结束。
async fn feed(
    app: Arc<App>,
    session_id: SessionId,
    after: Option<MessageId>,
    Cut {
        mut events,
        cut,
        draft,
    }: Cut,
    started: Instant,
    tx: mpsc::Sender<Event>,
) {
    let replay_at = Instant::now();
    // 首次只回放切点前最近一页并告知是否还有更早的，更早的经 `page` 取；重连从 `after` 续传到切点。
    let replayed = match (cut, after) {
        (None, _) => Ok((Vec::new(), after.is_none().then_some(false))),
        (Some(cut), Some(_)) => {
            app.kernel
                .messages_after(session_id, after)
                .await
                .map(|mut messages| {
                    messages.retain(|m| m.id <= cut);
                    (messages, None)
                })
        }
        (Some(cut), None) => app
            .kernel
            .messages_page(
                session_id,
                Some(MessageId(cut.0 + 1)),
                PAGE_ROUNDS,
                PAGE_MESSAGES,
            )
            .await
            .map(|page| (page.messages, Some(page.has_earlier))),
    };
    let replay_ms = ms(replay_at);
    let (replayed, has_earlier) = match replayed {
        Ok(replayed) => replayed,
        Err(e) => return tracing::error!(error = %crate::error::chain(&e), "gateway replay failed"),
    };
    for message in replayed {
        if !send(&app, &tx, message_event(&message)).await {
            return;
        }
    }
    let state_at = Instant::now();
    let state = tokio::try_join!(
        app.kernel.executing_run(session_id),
        app.kernel.queued_inputs(session_id),
        app.kernel.resumable_run(session_id),
        app.kernel.current_prediction(session_id)
    );
    let state_ms = ms(state_at);
    let (executing_run, queued, resume, prediction) = match state {
        Ok(state) => state,
        Err(e) => return tracing::error!(error = %crate::error::chain(&e), "gateway replay failed"),
    };
    let ready = Ready {
        executing_run: executing_run.map(|r| r.0),
        queued: queued.into_iter().map(|id| id.0).collect(),
        has_earlier,
        resume: resume.map(ResumeView::from),
        prediction: prediction.map(|p| PredictionView {
            run_id: p.run_id.0,
            text: p.text,
        }),
        draft: draft.map(|d| DraftView {
            context_at: d.context_at,
            reasoning: d.reasoning,
            text: d.text,
        }),
        timing: Timing {
            replay_ms,
            state_ms,
            total_ms: ms(started),
        },
    };
    if !send(&app, &tx, json_event("ready", &ready)).await {
        return;
    }

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
            KernelEventKind::SessionChanged => json_event("session_changed", &Empty {}),
            KernelEventKind::JobsChanged => json_event("jobs_changed", &Empty {}),
            KernelEventKind::PushCreated { .. }
            | KernelEventKind::PushDeliveryChanged { .. }
            | KernelEventKind::ScheduleChanged { .. } => continue,
            KernelEventKind::MessageAppended(message) => message_event(&message),
            KernelEventKind::DraftStarted { context_at } => {
                json_event("draft_started", &DraftStarted { context_at })
            }
            KernelEventKind::ModelRetrying { attempt, delay_ms } => {
                json_event("model_retry", &ModelRetry { attempt, delay_ms })
            }
            KernelEventKind::TextDelta(text) => json_event("text_delta", &Delta { text }),
            KernelEventKind::ReasoningDelta(text) => json_event("reasoning_delta", &Delta { text }),
            KernelEventKind::DraftDiscarded => json_event("draft_discarded", &Empty {}),
            KernelEventKind::InputsClaimed { run_id, inputs } => json_event(
                "inputs_claimed",
                &InputsClaimed {
                    run_id: run_id.0,
                    inputs: inputs.into_iter().map(|id| id.0).collect(),
                },
            ),
            KernelEventKind::PredictionReady { run_id, text } => json_event(
                "prediction",
                &PredictionView {
                    run_id: run_id.0,
                    text,
                },
            ),
            KernelEventKind::RunFinished {
                run_id,
                state,
                resume,
            } => json_event(
                "run_finished",
                &RunFinished {
                    run_id: run_id.0,
                    state: state.as_str(),
                    resume: resume.map(ResumeView::from),
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
    /// 排队等待认领的输入 id。
    queued: Vec<i64>,
    /// 首次回放之前是否还有消息；重连续传时为 null。
    has_earlier: Option<bool>,
    resume: Option<ResumeView>,
    /// 最新 run 的输入预测。
    prediction: Option<PredictionView>,
    /// 切点时正在生成的回复，之后的增量接着到达。
    draft: Option<DraftView>,
    timing: Timing,
}

/// 临时诊断（session-load-diagnostics），排查完整体删除。
/// 服务端从收到流请求到发出 `ready` 的耗时，供前端判断慢在服务端还是途中。
#[derive(Serialize)]
struct Timing {
    /// 查回放页（重连时为续传查询）。
    replay_ms: u64,
    /// 查运行状态。
    state_ms: u64,
    /// 含鉴权后的会话查询、订阅、回放写入发送队列。
    total_ms: u64,
}

fn ms(since: Instant) -> u64 {
    since.elapsed().as_millis() as u64
}

#[derive(Serialize)]
struct DraftView {
    context_at: i64,
    reasoning: String,
    text: String,
}

#[derive(Serialize)]
struct DraftStarted {
    context_at: i64,
}

#[derive(Serialize)]
struct PredictionView {
    run_id: i64,
    text: String,
}

/// 可原地接续的 run。
#[derive(Serialize)]
struct ResumeView {
    run_id: i64,
    state: &'static str,
}

impl From<Resumable> for ResumeView {
    fn from(r: Resumable) -> Self {
        Self {
            run_id: r.run_id.0,
            state: r.state.as_str(),
        }
    }
}

#[derive(Serialize)]
struct ModelRetry {
    attempt: u32,
    delay_ms: u64,
}

#[derive(Serialize)]
struct Delta {
    text: String,
}

#[derive(Serialize)]
struct Empty {}

#[derive(Serialize)]
struct InputsClaimed {
    run_id: i64,
    inputs: Vec<i64>,
}

#[derive(Serialize)]
struct RunFinished {
    run_id: i64,
    state: &'static str,
    resume: Option<ResumeView>,
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
