use base64::{engine::general_purpose::STANDARD, Engine};
use reqwest::{Client as HttpClient, Response, Url};
use serde::de::DeserializeOwned;
use std::error::Error;
use std::time::Duration;

use crate::account::{Credentials, Token, WechatUserId};
use crate::limits::{
    API_BASE, API_TIMEOUT, APP_ID, BOT_TYPE, CLIENT_VERSION, PROTOCOL_VERSION, QR_TIMEOUT,
};
use crate::wire::{self, QrRequest, QrResponse, QrStatus, QrStatusResponse};

#[derive(Debug, thiserror::Error)]
pub(crate) enum ClientError {
    #[error("微信网络连接失败")]
    Network,
    #[error("微信请求超时")]
    Timeout,
    #[error("微信服务拒绝请求（HTTP {0}）")]
    Rejected(u16),
    #[error("微信服务拒绝请求")]
    BusinessRejected,
    #[error("微信登录已失效")]
    SessionExpired,
    #[error("微信协议不匹配：{0}")]
    Protocol(&'static str),
}

pub(crate) struct Client {
    http: HttpClient,
}

pub(crate) struct Qr {
    pub token: String,
    pub content: String,
}

pub(crate) enum LoginStatus {
    Waiting,
    Scanned,
    Redirect(Url),
    NeedsCode,
    VerificationBlocked,
    ExistingBinding,
    Expired,
    Confirmed(Credentials),
}

pub(crate) struct Updates {
    pub snapshot: wire::GetUpdatesResponse,
    pub cursor: String,
    pub timeout_ms: Option<u32>,
    pub incoming: Vec<IncomingMessage>,
}

pub(crate) struct IncomingMessage {
    pub context_token: String,
    pub content: Vec<IncomingContent>,
}

pub(crate) enum IncomingContent {
    Text(String),
    /// 微信服务端的语音转写。
    VoiceText(String),
    Unsupported(Unsupported),
}

#[derive(Clone, Copy)]
pub(crate) enum TypingStatus {
    Typing,
    Cancel,
}

/// 当前不能交给模型的入站 item，保留类型与文件名。
pub(crate) enum Unsupported {
    Image,
    /// 无转写的语音。
    Voice,
    File {
        name: Option<String>,
    },
    Video,
    Other,
}

impl Client {
    pub fn new() -> Result<Self, ClientError> {
        let http = HttpClient::builder()
            .user_agent(format!(
                "micnext/{} iLink/{PROTOCOL_VERSION}",
                env!("CARGO_PKG_VERSION")
            ))
            .timeout(QR_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| ClientError::Protocol("HTTP client 初始化失败"))?;
        Ok(Self { http })
    }

    pub async fn qr(&self, tokens: &[Token]) -> Result<Qr, ClientError> {
        let mut random = [0u8; 4];
        getrandom::fill(&mut random).map_err(|_| ClientError::Protocol("随机源不可用"))?;
        let uin = STANDARD.encode(u32::from_be_bytes(random).to_string());
        let body = QrRequest {
            local_token_list: tokens.iter().map(Token::expose).collect(),
        };
        let response = self
            .http
            .post(format!("{API_BASE}ilink/bot/get_bot_qrcode"))
            .query(&[("bot_type", BOT_TYPE)])
            .header("iLink-App-Id", APP_ID)
            .header("iLink-App-ClientVersion", CLIENT_VERSION)
            .header("AuthorizationType", "ilink_bot_token")
            .header("X-WECHAT-UIN", uin)
            .json(&body)
            .send()
            .await
            .map_err(|error| network_error("get_bot_qrcode", error))?;
        let response: QrResponse = decode(response, "get_bot_qrcode").await?;
        successful(response.ret, None)?;
        if response.qrcode.trim().is_empty() || response.qrcode_img_content.trim().is_empty() {
            return Err(ClientError::Protocol("二维码字段为空"));
        }
        Ok(Qr {
            token: response.qrcode,
            content: response.qrcode_img_content,
        })
    }

    fn authenticated(
        &self,
        credentials: &Credentials,
        endpoint: &str,
    ) -> Result<reqwest::RequestBuilder, ClientError> {
        let url = credentials
            .base_url
            .join(endpoint)
            .map_err(|_| ClientError::Protocol("API 地址无效"))?;
        let mut random = [0u8; 4];
        getrandom::fill(&mut random).map_err(|_| ClientError::Protocol("随机源不可用"))?;
        Ok(self
            .http
            .post(url)
            .header("iLink-App-Id", APP_ID)
            .header("iLink-App-ClientVersion", CLIENT_VERSION)
            .header("AuthorizationType", "ilink_bot_token")
            .header(
                "X-WECHAT-UIN",
                STANDARD.encode(u32::from_be_bytes(random).to_string()),
            )
            .bearer_auth(credentials.token.expose()))
    }

    pub async fn updates(
        &self,
        credentials: &Credentials,
        cursor: &str,
        timeout_ms: u32,
    ) -> Result<Updates, ClientError> {
        let response = self
            .authenticated(credentials, "ilink/bot/getupdates")?
            .timeout(Duration::from_millis(timeout_ms.into()))
            .json(&wire::GetUpdatesRequest {
                get_updates_buf: cursor,
                base_info: base_info(),
            })
            .send()
            .await
            .map_err(|error| network_error("getupdates", error))?;
        parse_updates(decode(response, "getupdates").await?, &credentials.user_id)
    }

    pub async fn send(
        &self,
        credentials: &Credentials,
        context_token: &str,
        client_id: &str,
        text: &str,
    ) -> Result<wire::MessageId, ClientError> {
        let msg = wire::WechatMessage {
            from_user_id: Some(String::new()),
            to_user_id: Some(credentials.user_id.0.clone()),
            client_id: Some(client_id.to_owned()),
            message_type: Some(2),
            message_state: Some(2),
            context_token: Some(context_token.to_owned()),
            item_list: Some(vec![wire::MessageItem {
                kind: Some(1),
                text_item: Some(wire::TextItem {
                    text: Some(text.to_owned()),
                }),
                ..Default::default()
            }]),
            ..Default::default()
        };
        let response = self
            .authenticated(credentials, "ilink/bot/sendmessage")?
            .timeout(API_TIMEOUT)
            .json(&wire::SendMessageRequest {
                msg,
                base_info: base_info(),
            })
            .send()
            .await
            .map_err(|error| network_error("sendmessage", error))?;
        let response: wire::SendMessageResponse = decode(response, "sendmessage").await?;
        let wire::SendMessageResponse {
            message_id,
            ret,
            _errmsg: _,
        } = response;
        successful(ret, None)?;
        message_id.ok_or(ClientError::Protocol("发送成功但缺少 message_id"))
    }

    /// 取 typing ticket；成功但未下发 ticket 时为 `None`。
    pub async fn typing_ticket(
        &self,
        credentials: &Credentials,
        context_token: &str,
    ) -> Result<Option<String>, ClientError> {
        let response = self
            .authenticated(credentials, "ilink/bot/getconfig")?
            .timeout(API_TIMEOUT)
            .json(&wire::GetConfigRequest {
                ilink_user_id: &credentials.user_id.0,
                context_token,
                base_info: base_info(),
            })
            .send()
            .await
            .map_err(|error| network_error("getconfig", error))?;
        let response: wire::GetConfigResponse = decode(response, "getconfig").await?;
        successful(response.ret, None)?;
        Ok(response.typing_ticket.filter(|ticket| !ticket.is_empty()))
    }

    pub async fn typing(
        &self,
        credentials: &Credentials,
        ticket: &str,
        status: TypingStatus,
    ) -> Result<(), ClientError> {
        let response = self
            .authenticated(credentials, "ilink/bot/sendtyping")?
            .timeout(API_TIMEOUT)
            .json(&wire::SendTypingRequest {
                ilink_user_id: &credentials.user_id.0,
                typing_ticket: ticket,
                status: match status {
                    TypingStatus::Typing => 1,
                    TypingStatus::Cancel => 2,
                },
                base_info: base_info(),
            })
            .send()
            .await
            .map_err(|error| network_error("sendtyping", error))?;
        let response: wire::SendTypingResponse = decode(response, "sendtyping").await?;
        successful(response.ret, None)
    }

    pub async fn poll(
        &self,
        base: &Url,
        qr: &Qr,
        code: Option<&str>,
    ) -> Result<LoginStatus, ClientError> {
        let url = base
            .join("ilink/bot/get_qrcode_status")
            .map_err(|_| ClientError::Protocol("轮询地址无效"))?;
        let mut request = self
            .http
            .get(url)
            .query(&[("qrcode", qr.token.as_str())])
            .header("iLink-App-Id", APP_ID)
            .header("iLink-App-ClientVersion", CLIENT_VERSION);
        if let Some(code) = code {
            request = request.query(&[("verify_code", code)]);
        }
        let response = request
            .send()
            .await
            .map_err(|error| network_error("get_qrcode_status", error))?;
        let response: QrStatusResponse = decode(response, "get_qrcode_status").await?;
        successful(response.ret, None)?;
        Ok(match response.status {
            QrStatus::Wait => LoginStatus::Waiting,
            QrStatus::Scaned => LoginStatus::Scanned,
            QrStatus::NeedVerifycode => LoginStatus::NeedsCode,
            QrStatus::VerifyCodeBlocked => LoginStatus::VerificationBlocked,
            QrStatus::BindedRedirect => LoginStatus::ExistingBinding,
            QrStatus::Expired => LoginStatus::Expired,
            QrStatus::ScanedButRedirect => {
                let host = required(response.redirect_host, "缺少 redirect_host")?;
                let url = https_base(&format!("https://{host}/"))?;
                if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
                    return Err(ClientError::Protocol("redirect_host 不是主机地址"));
                }
                LoginStatus::Redirect(url)
            }
            QrStatus::Confirmed => LoginStatus::Confirmed(Credentials {
                user_id: WechatUserId(required(response.ilink_user_id, "缺少 ilink_user_id")?),
                bot_id: required(response.ilink_bot_id, "缺少 ilink_bot_id")?,
                token: Token::new(required(response.bot_token, "缺少 bot_token")?),
                base_url: https_base(&required(response.baseurl, "缺少 baseurl")?)?,
            }),
        })
    }
}

fn base_info() -> wire::BaseInfo {
    wire::BaseInfo {
        channel_version: PROTOCOL_VERSION,
        bot_agent: format!("micnext/{}", env!("CARGO_PKG_VERSION")),
    }
}

pub(crate) fn parse_updates(
    snapshot: wire::GetUpdatesResponse,
    user: &WechatUserId,
) -> Result<Updates, ClientError> {
    successful(snapshot.ret, snapshot.errcode)?;
    let cursor = snapshot
        .get_updates_buf
        .as_ref()
        .ok_or(ClientError::Protocol("缺少 get_updates_buf"))?
        .clone();
    let timeout_ms = snapshot.longpolling_timeout_ms;
    if timeout_ms == Some(0) {
        return Err(ClientError::Protocol("长轮询 timeout 为零"));
    }
    let mut incoming = Vec::new();
    for message in snapshot.msgs.iter().flatten() {
        validate_message(message)?;
        if message.from_user_id.as_deref() != Some(user.0.as_str())
            || message
                .group_id
                .as_ref()
                .is_some_and(|group| !group.is_empty())
            || message.message_type != Some(1)
        {
            continue;
        }
        let context_token = required(message.context_token.clone(), "入站缺少 context_token")?;
        // TODO: 引用入站 ID 对应关系验证后删除。
        tracing::debug!(
            message_id = ?message.message_id.map(|id| id.0.to_string()),
            item_msg_ids = ?message
                .item_list
                .iter()
                .flatten()
                .filter_map(|item| item.msg_id.as_deref())
                .collect::<Vec<_>>(),
            "wechat inbound identity"
        );
        let mut content = Vec::new();
        for item in message.item_list.iter().flatten() {
            // TODO: 引用回复实测样本采集，V2 引用实现后删除。
            if let Some(reference) = &item.ref_msg {
                tracing::debug!(
                    ref_msg = %serde_json::to_string(reference).expect("引用可序列化"),
                    "wechat inbound quote"
                );
            }
            match item.kind {
                Some(1) => {
                    let text = item
                        .text_item
                        .as_ref()
                        .and_then(|item| item.text.as_ref())
                        .ok_or(ClientError::Protocol("文本 item 缺少 text"))?;
                    if !text.trim().is_empty() {
                        content.push(IncomingContent::Text(text.clone()));
                    }
                }
                Some(3) => match item
                    .voice_item
                    .as_ref()
                    .and_then(|voice| voice.text.as_ref())
                {
                    Some(text) if !text.trim().is_empty() => {
                        content.push(IncomingContent::VoiceText(text.clone()))
                    }
                    _ => content.push(IncomingContent::Unsupported(Unsupported::Voice)),
                },
                Some(2) => content.push(IncomingContent::Unsupported(Unsupported::Image)),
                Some(4) => content.push(IncomingContent::Unsupported(Unsupported::File {
                    name: item
                        .file_item
                        .as_ref()
                        .and_then(|file| file.file_name.clone())
                        .filter(|name| !name.trim().is_empty()),
                })),
                Some(5) => content.push(IncomingContent::Unsupported(Unsupported::Video)),
                Some(0 | 11 | 12) => content.push(IncomingContent::Unsupported(Unsupported::Other)),
                None => return Err(ClientError::Protocol("item 缺少 type")),
                Some(_) => unreachable!("validate_message 已拒绝未知 type"),
            }
        }
        incoming.push(IncomingMessage {
            context_token,
            content,
        });
    }
    Ok(Updates {
        snapshot,
        cursor,
        timeout_ms,
        incoming,
    })
}

fn validate_message(message: &wire::WechatMessage) -> Result<(), ClientError> {
    if message
        .message_type
        .is_some_and(|kind| !matches!(kind, 0..=2))
        || message
            .message_state
            .is_some_and(|state| !matches!(state, 0..=2))
    {
        return Err(ClientError::Protocol("未知 message_type 或 message_state"));
    }
    for item in message.item_list.iter().flatten() {
        validate_item(item)?;
    }
    Ok(())
}

fn validate_item(item: &wire::MessageItem) -> Result<(), ClientError> {
    if item
        .kind
        .is_some_and(|kind| !matches!(kind, 0..=5 | 11 | 12))
    {
        return Err(ClientError::Protocol("未知 item type"));
    }
    if let Some(reference) = &item.ref_msg {
        if let Some(item) = &reference.message_item {
            validate_item(item)?;
        }
    }
    Ok(())
}

/// 与 SDK 一致：ret/errcode 缺失视为成功（实测 getupdates 成功响应不带 ret），出现则须为 0。
fn successful(ret: Option<i32>, errcode: Option<i32>) -> Result<(), ClientError> {
    if ret == Some(-14) || errcode == Some(-14) {
        return Err(ClientError::SessionExpired);
    }
    if ret.is_some_and(|ret| ret != 0) || errcode.is_some_and(|code| code != 0) {
        return Err(ClientError::BusinessRejected);
    }
    Ok(())
}

pub(crate) fn https_base(raw: &str) -> Result<Url, ClientError> {
    let mut url = Url::parse(raw).map_err(|_| ClientError::Protocol("API 地址格式无效"))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(ClientError::Protocol("API 地址须为无用户信息的 HTTPS URL"));
    }
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    Ok(url)
}

pub(crate) fn required(value: Option<String>, field: &'static str) -> Result<String, ClientError> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or(ClientError::Protocol(field))
}

fn network_error(endpoint: &'static str, error: reqwest::Error) -> ClientError {
    if error.is_timeout() {
        ClientError::Timeout
    } else {
        let mut source = error.source();
        let mut io_kind = None;
        let mut os_error = None;
        let mut tls_handshake_eof = false;
        while let Some(cause) = source {
            if let Some(io) = cause.downcast_ref::<std::io::Error>() {
                io_kind = Some(io.kind());
                os_error = io.raw_os_error();
            }
            tls_handshake_eof |= cause.to_string() == "tls handshake eof";
            source = cause.source();
        }
        tracing::warn!(
            endpoint,
            connect = error.is_connect(),
            ?io_kind,
            ?os_error,
            tls_handshake_eof,
            "wechat HTTP transport failed"
        );
        ClientError::Network
    }
}

async fn decode<T: DeserializeOwned>(
    response: Response,
    endpoint: &'static str,
) -> Result<T, ClientError> {
    if !response.status().is_success() {
        tracing::warn!(
            endpoint,
            status = response.status().as_u16(),
            "wechat HTTP rejected"
        );
        return Err(ClientError::Rejected(response.status().as_u16()));
    }
    let body = response
        .bytes()
        .await
        .map_err(|error| network_error(endpoint, error))?;
    let mut deserializer = serde_json::Deserializer::from_slice(&body);
    serde_path_to_error::deserialize(&mut deserializer).map_err(|error| {
        // NOTE: serde 的 invalid type/value 错误会带出原值，可能含消息正文。
        tracing::warn!(
            endpoint,
            path = %error.path(),
            category = ?error.inner().classify(),
            error = %error.inner(),
            body_len = body.len(),
            "wechat response decode failed"
        );
        // NOTE: 网页开发者日志常驻采集 DEBUG，凭据在此遮蔽；消息正文仍可见。
        tracing::debug!(endpoint, body = %redacted(&body), "wechat response raw body");
        ClientError::Protocol("响应字段或枚举不匹配")
    })
}

/// 响应中携带凭据的字段名；值替换为占位。
const SECRET_KEYS: [&str; 2] = ["bot_token", "context_token"];

/// 非 JSON 无法定位凭据，只给长度。
fn redacted(body: &[u8]) -> String {
    fn walk(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, value) in map.iter_mut() {
                    if SECRET_KEYS.contains(&key.as_str()) {
                        *value = serde_json::Value::String("<已隐藏>".into());
                    } else {
                        walk(value);
                    }
                }
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(walk),
            _ => {}
        }
    }
    match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(mut value) => {
            walk(&mut value);
            value.to_string()
        }
        Err(_) => format!("<非 JSON 响应，{} 字节>", body.len()),
    }
}
