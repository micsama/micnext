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
use mic_core::IncomingPart;
use mic_message::{ImageId, MessageId, SessionId};
use mic_store::{
    ModelId, NewSession, PersonaId, Session, SessionCursor, SessionKind, SessionSummary, ToolScope,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{
    ApiError, IMAGE_NOT_FOUND, MODEL_NOT_FOUND, PERSONA_NOT_FOUND, SESSION_NOT_FOUND,
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
    before_at: Option<i64>,
    before_id: Option<i64>,
    limit: Option<u32>,
}

#[derive(Serialize)]
pub(crate) struct SessionItem {
    id: SessionId,
    channel: String,
    created_at: i64,
    last_activity_at: i64,
    preview: Option<String>,
    workdir: String,
    /// 能否从 Web 发消息。
    writable: bool,
    /// 下一轮用的人设；已删除时下一轮改用默认人设。
    persona_id: PersonaId,
    /// 下一轮用的模型；`null` = 下次认领时取当时默认。
    model_id: Option<ModelId>,
}

#[derive(Serialize)]
pub(crate) struct Cursor {
    before_at: i64,
    before_id: SessionId,
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
    let before = match (q.before_at, q.before_id) {
        (Some(at), Some(id)) => Some(SessionCursor {
            last_activity_at: at,
            session_id: SessionId(id),
        }),
        (None, None) => None,
        _ => {
            return Err(ApiError::BadRequest(
                "before_at 与 before_id 须同时给出".into(),
            ))
        }
    };
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
        .list_root_sessions(&q.channel, before, limit)
        .await?;
    Ok(Json(SessionPage {
        items: page
            .items
            .into_iter()
            .map(|s| item(s).expect("list_root_sessions 只返回 Root 会话"))
            .collect(),
        next: page.next.map(|c| Cursor {
            before_at: c.last_activity_at,
            before_id: c.session_id,
        }),
    }))
}

/// Web 只浏览 Root 会话，其它种类返回 `None`。
fn item(s: SessionSummary) -> Option<SessionItem> {
    let writable = writable(&s.session);
    let SessionKind::Root { channel, .. } = s.session.kind else {
        return None;
    };
    Some(SessionItem {
        id: s.session.id,
        channel,
        created_at: s.session.created_at,
        last_activity_at: s.last_activity_at,
        preview: s.preview,
        workdir: s.session.pwd,
        writable,
        persona_id: s.session.persona_id,
        model_id: s.session.model_id,
    })
}

/// 只有 Web 自己的会话能从 Web 发消息，其它渠道的只读。
pub(crate) fn writable(session: &Session) -> bool {
    matches!(&session.kind, SessionKind::Root { channel, .. } if channel == WEB_CHANNEL)
}

pub(crate) async fn get_session(
    State(app): State<Arc<App>>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<Json<SessionItem>, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::NotFound(SESSION_NOT_FOUND))?;
    app.kernel
        .session_summary(SessionId(id))
        .await?
        .and_then(item)
        .map(Json)
        .ok_or(ApiError::NotFound(SESSION_NOT_FOUND))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SendBody {
    parts: Vec<PartBody>,
}

/// 入站消息片段；图片只收 base64 字节，格式由内核按魔数识别。
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum PartBody {
    Text { text: String },
    Image { base64: String },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateBody {
    parts: Vec<PartBody>,
    persona_id: PersonaId,
    /// 缺省 = 当时的默认模型。
    model_id: Option<ModelId>,
}

#[derive(Serialize)]
pub(crate) struct Created {
    session_id: SessionId,
    message_id: MessageId,
}

#[derive(Serialize)]
pub(crate) struct Accepted {
    message_id: MessageId,
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
    if let Some(id) = body.model_id {
        if app.kernel.model(id).await?.is_none() {
            return Err(ApiError::NotFound(MODEL_NOT_FOUND));
        }
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
            },
        )
        .await?;
    let message_id = append(&app, session.id, parts).await?;
    Ok((
        StatusCode::CREATED,
        Json(Created {
            session_id: session.id,
            message_id,
        }),
    ))
}

pub(crate) async fn send_message(
    State(app): State<Arc<App>>,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<SendBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Accepted>), ApiError> {
    let session = session(&app, id).await?;
    if !writable(&session) {
        return Err(ApiError::ReadOnly);
    }
    let parts = parts(json(body)?.parts)?;
    let message_id = append(&app, session.id, parts).await?;
    Ok((StatusCode::ACCEPTED, Json(Accepted { message_id })))
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

/// 文本总长受限；图片 base64 在此解码，其余校验归内核。
fn parts(parts: Vec<PartBody>) -> Result<Vec<IncomingPart>, ApiError> {
    let chars: usize = parts
        .iter()
        .map(|p| match p {
            PartBody::Text { text } => text.chars().count(),
            PartBody::Image { .. } => 0,
        })
        .sum();
    if chars > MAX_TEXT_CHARS {
        return Err(ApiError::TooLarge("消息太长".into()));
    }
    parts
        .into_iter()
        .map(|p| match p {
            PartBody::Text { text } => Ok(IncomingPart::Text(text)),
            PartBody::Image { base64 } => BASE64
                .decode(base64)
                .map(IncomingPart::Image)
                .map_err(|_| ApiError::BadRequest("图片不是合法的 base64".into())),
        })
        .collect()
}

async fn append(
    app: &App,
    session: SessionId,
    parts: Vec<IncomingPart>,
) -> Result<MessageId, ApiError> {
    Ok(app
        .kernel
        .append_user_input(session, app.kernel.owner(), parts)
        .await?)
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

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时间早于 1970")
        .as_millis() as i64
}
