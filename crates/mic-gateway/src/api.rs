use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use axum::Json;
use mic_message::{ContentPart, MessageId, SessionId};
use mic_store::{
    NewSession, PersonaId, Session, SessionCursor, SessionKind, SessionSummary, ToolScope,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{ApiError, PERSONA_NOT_FOUND, SESSION_NOT_FOUND};
use crate::limits::{CHAT_ID_BYTES, MAX_TEXT_CHARS, PAGE_DEFAULT, PAGE_MAX};
use crate::service::{random_hex, App};
use crate::settings::new_session_workdir;
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
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateBody {
    text: String,
    persona_id: PersonaId,
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
    let text = text(body.text)?;
    match app.kernel.persona(body.persona_id).await? {
        None => return Err(ApiError::NotFound(PERSONA_NOT_FOUND)),
        Some(p) if p.deleted => return Err(ApiError::Conflict(PERSONA_DELETED.into())),
        Some(_) => {}
    }
    let pwd = new_session_workdir(&app).await?;
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
            },
        )
        .await?;
    let message_id = append(&app, session.id, text).await?;
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
    let text = text(json(body)?.text)?;
    let message_id = append(&app, session.id, text).await?;
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
        StatusCode::PAYLOAD_TOO_LARGE => ApiError::TooLarge,
        _ => ApiError::BadRequest(format!("请求体不合法：{}", e.body_text())),
    })?;
    Ok(body)
}

fn text(text: String) -> Result<String, ApiError> {
    if text.trim().is_empty() {
        return Err(ApiError::BadRequest("消息不能为空".into()));
    }
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(ApiError::TooLarge);
    }
    Ok(text)
}

async fn append(app: &App, session: SessionId, text: String) -> Result<MessageId, ApiError> {
    Ok(app
        .kernel
        .append_user_input(
            session,
            app.kernel.owner(),
            vec![ContentPart::Text { text }],
        )
        .await?)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时间早于 1970")
        .as_millis() as i64
}
