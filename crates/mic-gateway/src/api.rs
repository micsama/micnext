use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use axum::Json;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use mic_core::{IncomingPart, ParsedCommand, StopOutcome};
use mic_message::limits::{MAX_FILES_PER_INPUT, MAX_FILE_BYTES};
use mic_message::{FileRef, ImageId, MessageId, SessionId};
use mic_store::{
    ModelId, NewSession, PersonaId, ReasoningEffort, RunId, Session, SessionCursor, SessionKind,
    SessionSummary, TagId, ToolScope,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{
    ApiError, FILE_NOT_FOUND, IMAGE_NOT_FOUND, PERSONA_NOT_FOUND, SESSION_NOT_FOUND,
};
use crate::limits::{CHAT_ID_BYTES, MAX_TEXT_CHARS, PAGE_DEFAULT, PAGE_MAX};
use crate::service::{random_hex, App};
use crate::WEB_CHANNEL;

pub(crate) async fn authorize(
    State(app): State<Arc<App>>,
    req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let given = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    match given {
        Some(t) if constant_time_eq(t.as_bytes(), app.token.as_bytes()) => Ok(next.run(req).await),
        _ => Err(ApiError::Unauthorized),
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListQuery {
    channel: String,
    tag: Option<TagId>,
    before_at: Option<i64>,
    before_id: Option<i64>,
    limit: Option<u32>,
}

#[derive(Serialize)]
struct SessionSchedule {
    id: mic_store::ScheduleId,
    name: String,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SessionSource {
    Channel { channel: String },
    Schedule,
}

#[derive(Serialize)]
pub(crate) struct SessionItem {
    schedule: Option<SessionSchedule>,
    title: Option<String>,
    title_error: Option<String>,
    id: SessionId,
    source: SessionSource,
    created_at: i64,
    last_activity_at: i64,
    preview: Option<String>,
    workdir: String,
    /// 能否从 Web 发消息。
    writable: bool,
    archived_at: Option<i64>,
    /// 下一轮用的人设；已删除时下一轮改用默认人设。
    persona_id: PersonaId,
    /// 下一轮用的模型；`null` = 下次认领时取当时默认。
    model_id: Option<ModelId>,
    reasoning_effort: Option<ReasoningEffort>,
    /// 临时明文开启中：新消息发给模型时不按规则脱敏。
    plaintext: bool,
    pinned_at: Option<i64>,
    unread: bool,
    running: bool,
    tag_ids: Vec<TagId>,
}

#[derive(Serialize)]
pub(crate) struct Cursor {
    before_at: i64,
    before_id: SessionId,
}

impl From<SessionCursor> for Cursor {
    fn from(c: SessionCursor) -> Self {
        Self {
            before_at: c.last_activity_at,
            before_id: c.session_id,
        }
    }
}

/// 列表游标的两个查询参数须同时给出或同时缺省。
pub(crate) fn cursor(
    before_at: Option<i64>,
    before_id: Option<i64>,
) -> Result<Option<SessionCursor>, ApiError> {
    match (before_at, before_id) {
        (Some(at), Some(id)) => Ok(Some(SessionCursor {
            last_activity_at: at,
            session_id: SessionId(id),
        })),
        (None, None) => Ok(None),
        _ => Err(ApiError::BadRequest(
            "before_at 与 before_id 须同时给出".into(),
        )),
    }
}

#[derive(Serialize)]
pub(crate) struct SessionPage {
    items: Vec<SessionItem>,
    next: Option<Cursor>,
}

pub(crate) async fn list_sessions(
    State(app): State<Arc<App>>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Json<SessionPage>, ApiError> {
    let Query(q) = query.map_err(|e| ApiError::BadRequest(format!("查询参数不合法：{e}")))?;
    let before = cursor(q.before_at, q.before_id)?;
    let limit = match q.limit {
        None => PAGE_DEFAULT,
        Some(n @ 1..=PAGE_MAX) => n,
        Some(_) => {
            return Err(ApiError::BadRequest(format!(
                "limit 须在 1 到 {PAGE_MAX} 之间"
            )))
        }
    };
    let limit = NonZeroU32::new(limit).expect("limit 已限定为正");
    let page = app
        .kernel
        .list_root_sessions(&q.channel, q.tag, before, limit)
        .await?;
    Ok(Json(SessionPage {
        items: page
            .items
            .into_iter()
            .map(|s| item(s).expect("list_root_sessions 只返回 Root 会话"))
            .collect(),
        next: page.next.map(Cursor::from),
    }))
}

#[derive(Serialize)]
pub(crate) struct PinnedSessions {
    items: Vec<SessionItem>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PinnedQuery {
    tag: Option<TagId>,
}

pub(crate) async fn list_pinned_sessions(
    State(app): State<Arc<App>>,
    query: Result<Query<PinnedQuery>, QueryRejection>,
) -> Result<Json<PinnedSessions>, ApiError> {
    let Query(q) = query.map_err(|e| ApiError::BadRequest(format!("查询参数不合法：{e}")))?;
    let items = app.kernel.list_pinned_sessions(q.tag).await?;
    Ok(Json(PinnedSessions {
        items: items
            .into_iter()
            .map(|s| item(s).expect("list_pinned_sessions 只返回 Root 会话"))
            .collect(),
    }))
}

/// Web 浏览 Root 与定时任务会话。
pub(crate) fn item(s: SessionSummary) -> Option<SessionItem> {
    let writable = writable(&s.session);
    let source = match &s.session.kind {
        SessionKind::Root { channel, .. } => SessionSource::Channel {
            channel: channel.clone(),
        },
        SessionKind::Triggered { module, .. } if module == mic_store::SCHEDULE_MODULE => {
            SessionSource::Schedule
        }
        _ => return None,
    };
    Some(SessionItem {
        schedule: None,
        title: s.title,
        title_error: s.title_error,
        id: s.session.id,
        source,
        created_at: s.session.created_at,
        last_activity_at: s.last_activity_at,
        preview: s.preview,
        workdir: s.session.pwd,
        writable,
        archived_at: s.session.archived_at,
        persona_id: s.session.persona_id,
        model_id: s.session.model_id,
        reasoning_effort: s.session.reasoning_effort,
        plaintext: s.session.plain_after.is_some(),
        pinned_at: s.pinned_at,
        unread: s.unread,
        running: s.running,
        tag_ids: s.tag_ids,
    })
}

/// Web 自己的会话与定时任务会话可写，其它渠道只读。
pub(crate) fn writable(session: &Session) -> bool {
    matches!(&session.kind, SessionKind::Root { channel, .. } if channel == WEB_CHANNEL)
        || matches!(&session.kind, SessionKind::Triggered { module, .. } if module == mic_store::SCHEDULE_MODULE)
}

pub(crate) async fn get_session(
    State(app): State<Arc<App>>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<Json<SessionDetail>, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::NotFound(SESSION_NOT_FOUND))?;
    let mut item = app
        .kernel
        .session_summary(SessionId(id))
        .await?
        .and_then(item)
        .ok_or(ApiError::NotFound(SESSION_NOT_FOUND))?;
    if matches!(item.source, SessionSource::Schedule) {
        item.schedule = app
            .kernel
            .schedules()
            .await?
            .into_iter()
            .find(|v| v.schedule.session_id == item.id)
            .map(|v| SessionSchedule {
                id: v.schedule.id,
                name: v.schedule.name,
            });
    }
    Ok(Json(SessionDetail {
        item,
        title_generating: app.kernel.title_generating(SessionId(id)).await,
    }))
}

#[derive(Serialize)]
pub(crate) struct SessionDetail {
    #[serde(flatten)]
    item: SessionItem,
    title_generating: bool,
}

pub(crate) async fn generate_title(
    State(app): State<Arc<App>>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let session = session(&app, id).await?;
    app.kernel.generate_session_title(session.id).await?;
    Ok(StatusCode::ACCEPTED)
}

pub(crate) async fn archive_session(
    State(app): State<Arc<App>>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::NotFound(SESSION_NOT_FOUND))?;
    app.kernel.archive_session(SessionId(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PinBody {
    pinned: bool,
}

pub(crate) async fn pin_session(
    State(app): State<Arc<App>>,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<PinBody>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::NotFound(SESSION_NOT_FOUND))?;
    let body = json(body)?;
    app.kernel
        .set_session_pinned(SessionId(id), body.pinned)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SeenBody {
    message_id: MessageId,
}

pub(crate) async fn mark_seen(
    State(app): State<Arc<App>>,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<SeenBody>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::NotFound(SESSION_NOT_FOUND))?;
    let body = json(body)?;
    app.kernel
        .mark_session_seen(SessionId(id), body.message_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SendBody {
    parts: Vec<PartBody>,
}

/// 入站消息片段；图片只收 base64 字节，由内核识别并规整；文件原样落盘。
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum PartBody {
    Text { text: String },
    Image { base64: String },
    File { name: String, base64: String },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateBody {
    parts: Vec<PartBody>,
    persona_id: PersonaId,
    /// 缺省 = 当时的默认模型。
    model_id: Option<ModelId>,
    reasoning_effort: Option<ReasoningEffort>,
}

#[derive(Serialize)]
pub(crate) struct Created {
    session_id: SessionId,
}

pub(crate) async fn create_session(
    State(app): State<Arc<App>>,
    body: Result<Json<CreateBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Created>), ApiError> {
    let body = json(body)?;
    let parts = parts(body.parts)?;
    match app.kernel.persona(body.persona_id).await? {
        None => return Err(ApiError::NotFound(PERSONA_NOT_FOUND)),
        Some(p) if p.deleted => return Err(ApiError::Conflict(PERSONA_DELETED.into())),
        Some(_) => {}
    }
    let pwd = app.kernel.default_workdir().await?;
    let chat = random_hex(CHAT_ID_BYTES);
    let session = app
        .kernel
        .resolve_root_session(
            WEB_CHANNEL,
            &chat,
            NewSession {
                kind: SessionKind::Root {
                    channel: WEB_CHANNEL.to_owned(),
                    chat: chat.clone(),
                },
                parent_session_id: None,
                delivery_target: None,
                pwd,
                tool_scope: ToolScope::All,
                created_at: now_ms(),
                persona: Some(body.persona_id),
                model: body.model_id,
                reasoning_effort: body.reasoning_effort,
            },
        )
        .await?;
    append(&app, session.id, parts).await?;
    Ok((
        StatusCode::CREATED,
        Json(Created {
            session_id: session.id,
        }),
    ))
}

pub(crate) async fn send_message(
    State(app): State<Arc<App>>,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<SendBody>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let session = session(&app, id).await?;
    if !writable(&session) {
        return Err(ApiError::ReadOnly);
    }
    let parts = parts(json(body)?.parts)?;
    append(&app, session.id, parts).await?;
    Ok(StatusCode::ACCEPTED)
}

/// 按 run 停止，防止停到刚接上的下一轮。
pub(crate) async fn stop_run(
    State(app): State<Arc<App>>,
    path: Result<Path<(i64, i64)>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path((session, run)) = path.map_err(|_| ApiError::NotFound(SESSION_NOT_FOUND))?;
    let session = app
        .kernel
        .session(SessionId(session))
        .await?
        .ok_or(ApiError::NotFound(SESSION_NOT_FOUND))?;
    if !writable(&session) {
        return Err(ApiError::ReadOnly);
    }
    match app.kernel.stop_run(session.id, Some(RunId(run))).await? {
        StopOutcome::Stopping => Ok(StatusCode::ACCEPTED),
        StopOutcome::NotRunning => Err(ApiError::Conflict("本次执行已经结束".into())),
    }
}

pub(crate) async fn resume_run(
    State(app): State<Arc<App>>,
    path: Result<Path<(i64, i64)>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path((session, run)) = path.map_err(|_| ApiError::NotFound(SESSION_NOT_FOUND))?;
    let session = app
        .kernel
        .session(SessionId(session))
        .await?
        .ok_or(ApiError::NotFound(SESSION_NOT_FOUND))?;
    if !writable(&session) {
        return Err(ApiError::ReadOnly);
    }
    app.kernel.resume_run(session.id, RunId(run)).await?;
    Ok(StatusCode::ACCEPTED)
}

/// 路径里的会话 id 解析并确认存在。
pub(crate) async fn session(
    app: &App,
    id: Result<Path<i64>, PathRejection>,
) -> Result<Session, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::NotFound(SESSION_NOT_FOUND))?;
    app.kernel
        .session(SessionId(id))
        .await?
        .ok_or(ApiError::NotFound(SESSION_NOT_FOUND))
}

const PERSONA_DELETED: &str = "人设已删除，请先在输入框旁重新选择";

pub(crate) fn json<T: DeserializeOwned>(
    body: Result<Json<T>, JsonRejection>,
) -> Result<T, ApiError> {
    let Json(body) = body.map_err(|e| match e.status() {
        StatusCode::PAYLOAD_TOO_LARGE => ApiError::TooLarge("请求体太大".into()),
        _ => ApiError::BadRequest(format!("请求体不合法：{}", e.body_text())),
    })?;
    Ok(body)
}

/// 文本总长与文件数量、大小在此限定；base64 在此解码，其余校验归内核。
fn parts(parts: Vec<PartBody>) -> Result<Vec<IncomingPart>, ApiError> {
    let chars: usize = parts
        .iter()
        .map(|p| match p {
            PartBody::Text { text } => text.chars().count(),
            PartBody::Image { .. } | PartBody::File { .. } => 0,
        })
        .sum();
    if chars > MAX_TEXT_CHARS {
        return Err(ApiError::TooLarge("消息太长".into()));
    }
    let files = parts
        .iter()
        .filter(|p| matches!(p, PartBody::File { .. }))
        .count();
    if files > MAX_FILES_PER_INPUT {
        return Err(ApiError::Unprocessable(format!(
            "一条消息最多带 {MAX_FILES_PER_INPUT} 个文件"
        )));
    }
    parts
        .into_iter()
        .map(|p| match p {
            PartBody::Text { text } => Ok(IncomingPart::Text(text)),
            PartBody::Image { base64 } => BASE64
                .decode(base64)
                .map(IncomingPart::Image)
                .map_err(|_| ApiError::BadRequest("图片不是合法的 base64".into())),
            PartBody::File { name, base64 } => {
                let bytes = BASE64
                    .decode(base64)
                    .map_err(|_| ApiError::BadRequest("文件不是合法的 base64".into()))?;
                if bytes.len() > MAX_FILE_BYTES {
                    return Err(ApiError::TooLarge(format!(
                        "文件不能超过 {} MB",
                        MAX_FILE_BYTES / 1024 / 1024
                    )));
                }
                Ok(IncomingPart::File { name, bytes })
            }
        })
        .collect()
}

async fn append(app: &App, session: SessionId, parts: Vec<IncomingPart>) -> Result<(), ApiError> {
    if let [IncomingPart::Text(text)] = parts.as_slice() {
        match mic_core::parse_command(text).map_err(mic_core::KernelError::from)? {
            Some(ParsedCommand::Stop) => return Ok(app.kernel.stop_command(session).await?),
            Some(ParsedCommand::Session(command)) => {
                app.kernel
                    .append_session_command(session, app.kernel.owner(), command)
                    .await?;
                return Ok(());
            }
            None => {}
        }
    }
    app.kernel
        .append_user_input(session, app.kernel.owner(), parts)
        .await?;
    Ok(())
}

/// 会话内图片原件；id 不属于该会话与不存在同样返回 404。
pub(crate) async fn get_image(
    State(app): State<Arc<App>>,
    path: Result<Path<(i64, i64)>, PathRejection>,
) -> Result<Response, ApiError> {
    let Path((session, image)) = path.map_err(|_| ApiError::NotFound(IMAGE_NOT_FOUND))?;
    let data = app
        .kernel
        .image(SessionId(session), ImageId(image))
        .await?
        .ok_or(ApiError::NotFound(IMAGE_NOT_FOUND))?;
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, data.format.mime())
        .header(header::CACHE_CONTROL, "no-store")
        .header("X-Content-Type-Options", "nosniff")
        .body(Body::from(data.bytes.to_vec()))
        .expect("响应头静态合法"))
}

/// 消息引用的文件原件；不属于该会话、序号越界与不存在同样返回 404。
pub(crate) async fn get_message_file(
    State(app): State<Arc<App>>,
    path: Result<Path<(i64, i64, usize)>, PathRejection>,
) -> Result<Response, ApiError> {
    let Path((session, message, index)) = path.map_err(|_| ApiError::NotFound(FILE_NOT_FOUND))?;
    let (file, handle) = app
        .kernel
        .message_file(SessionId(session), MessageId(message), index)
        .await?
        .ok_or(ApiError::NotFound(FILE_NOT_FOUND))?;
    Ok(file_response(&file, handle))
}

/// 内容由模型或用户提供：按扩展名给类型，`sandbox` 让直接打开的 HTML 拿不到同源权限。
pub(crate) fn file_response(file: &FileRef, handle: tokio::fs::File) -> Response {
    let name = std::path::Path::new(&file.path)
        .file_name()
        .map_or_else(|| file.path.clone(), |n| n.to_string_lossy().into_owned());
    let mime = mime_guess::from_path(&name).first_or_octet_stream();
    Response::builder()
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CACHE_CONTROL, "no-store")
        .header("X-Content-Type-Options", "nosniff")
        .header(header::CONTENT_SECURITY_POLICY, "sandbox")
        .header(
            header::CONTENT_DISPOSITION,
            format!("inline; filename*=UTF-8''{}", rfc5987(&name)),
        )
        .body(Body::from_stream(tokio_util::io::ReaderStream::new(handle)))
        .expect("响应头均已编码")
}

fn rfc5987(name: &str) -> String {
    name.bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' => (b as char).to_string(),
            b'!' | b'#' | b'$' | b'&' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时间早于 1970")
        .as_millis() as i64
}

#[cfg(test)]
mod tests;
