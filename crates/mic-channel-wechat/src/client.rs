use aes::cipher::block_padding::Pkcs7;
use aes::cipher::{BlockEncryptMut, KeyInit};
use base64::{engine::general_purpose::STANDARD, Engine};
use md5::{Digest, Md5};
use reqwest::{Client as HttpClient, Response, Url};
use serde::de::DeserializeOwned;
use std::error::Error;
use std::time::Duration;

use crate::account::{Credentials, Token, WechatUserId};
use crate::limits::{
    API_BASE, API_TIMEOUT, APP_ID, BOT_TYPE, CDN_BASE, CDN_UPLOAD_TIMEOUT, CLIENT_VERSION,
    PROTOCOL_VERSION, QR_TIMEOUT,
};
use crate::media::{AttachmentFailure, ImageFailure};
use crate::wire::{self, QrRequest, QrResponse, QrStatus, QrStatusResponse};

#[derive(Debug, thiserror::Error)]
pub(crate) enum ClientError {
    #[error("微信网络连接失败")]
    Network(String),
    #[error("微信请求超时")]
    Timeout(String),
    #[error("微信服务拒绝请求（HTTP {0}）")]
    Rejected(u16),
    #[error("微信服务拒绝请求（ret={ret:?}, errcode={errcode:?}, errmsg={errmsg:?}）")]
    BusinessRejected {
        ret: Option<i32>,
        errcode: Option<i32>,
        errmsg: Option<String>,
    },
    #[error("微信登录已失效")]
    SessionExpired,
    #[error("微信协议不匹配：{0}")]
    Protocol(&'static str),
}

impl ClientError {
    /// 诊断保留脱敏后的完整原因；用户文案仍由 Display 给出。
    pub(crate) fn diagnostic(&self) -> String {
        match self {
            Self::Network(detail) | Self::Timeout(detail) => detail.clone(),
            _ => self.to_string(),
        }
    }
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
    pub cursor: String,
    pub timeout_ms: Option<u32>,
    pub incoming: Vec<IncomingMessage>,
}

pub(crate) struct IncomingMessage {
    pub external_id: wire::MessageId,
    /// `None`：引用无法解析。
    pub references: Vec<Option<QuoteRef>>,
    pub context_token: String,
    pub content: Vec<IncomingContent>,
}

pub(crate) struct QuoteRef {
    pub id: wire::MessageId,
    pub partial: Option<wire::PartialText>,
}

pub(crate) enum IncomingContent {
    Text(String),
    /// 微信服务端的语音转写。
    VoiceText(String),
    Image(Result<CdnSource, ImageFailure>),
    Attachment(Attachment),
    Unsupported(Unsupported),
}

/// 边界一次 parse：下载地址已确定，密钥已解析。
pub(crate) struct CdnSource {
    pub url: Url,
    /// `None` 为 CDN 明文。
    pub key: Option<[u8; 16]>,
}

#[derive(Clone, Copy)]
pub(crate) enum TypingStatus {
    Typing,
    Cancel,
}

/// 入站文件或视频，保存到会话工作目录后以路径交给模型。
pub(crate) struct Attachment {
    pub kind: AttachmentKind,
    pub source: Result<CdnSource, AttachmentFailure>,
}

pub(crate) enum AttachmentKind {
    File { name: Option<String> },
    Video,
}

/// CDN 下载失败：超过调用方给的上限单独区分，不重试。
pub(crate) enum DownloadError {
    Client(ClientError),
    TooLarge,
}

/// 出站消息项：每项单独一条微信消息。
pub(crate) enum Outgoing<'a> {
    Text(&'a str),
    Media { name: &'a str, uploaded: Uploaded },
}

#[derive(Clone, Copy)]
pub(crate) enum MediaKind {
    Image,
    File,
}

/// 已加密上传到 CDN 的文件，凭下载参数与密钥组成消息项。
pub(crate) struct Uploaded {
    kind: MediaKind,
    download_param: String,
    /// 32 位 hex；消息项里再 base64，与 SDK 一致。
    aes_hex: String,
    raw_size: u64,
    cipher_size: u64,
}

/// 当前不能交给模型的入站 item。
pub(crate) enum Unsupported {
    /// 无转写的语音。
    Voice,
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
            .map_err(|error| network_error("client_build", error))
            .inspect_err(
                |error| tracing::warn!(error = %error.diagnostic(), "wechat client build failed"),
            )?;
        Ok(Self { http })
    }

    /// CDN 原样下载；超过 `limit` 字节即中止。
    pub async fn download(
        &self,
        url: &Url,
        limit: usize,
        timeout: Duration,
    ) -> Result<Vec<u8>, DownloadError> {
        let failed = |error| DownloadError::Client(network_error("cdn_download", error));
        let mut response = self
            .http
            .get(url.clone())
            .timeout(timeout)
            .send()
            .await
            .map_err(failed)?;
        if !response.status().is_success() {
            return Err(DownloadError::Client(ClientError::Rejected(
                response.status().as_u16(),
            )));
        }
        if response
            .content_length()
            .is_some_and(|len| len > limit as u64)
        {
            return Err(DownloadError::TooLarge);
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(failed)? {
            if body.len() + chunk.len() > limit {
                return Err(DownloadError::TooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }

    pub async fn qr(&self, tokens: &[Token]) -> Result<Qr, ClientError> {
        let mut random = [0u8; 4];
        getrandom::fill(&mut random).map_err(|error| {
            tracing::error!(%error, "wechat random source failed");
            ClientError::Protocol("随机源不可用")
        })?;
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
        getrandom::fill(&mut random).map_err(|error| {
            tracing::error!(%error, "wechat random source failed");
            ClientError::Protocol("随机源不可用")
        })?;
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
        item: Outgoing<'_>,
    ) -> Result<wire::MessageId, ClientError> {
        let item = match item {
            Outgoing::Text(text) => wire::MessageItem {
                kind: Some(1),
                text_item: Some(wire::TextItem {
                    text: Some(text.to_owned()),
                }),
                ..Default::default()
            },
            Outgoing::Media { name, uploaded } => {
                let media = wire::CdnMedia {
                    encrypt_query_param: Some(uploaded.download_param),
                    aes_key: Some(STANDARD.encode(&uploaded.aes_hex)),
                    encrypt_type: Some(1),
                    full_url: None,
                };
                match uploaded.kind {
                    MediaKind::Image => wire::MessageItem {
                        kind: Some(2),
                        image_item: Some(wire::ImageItem {
                            media: Some(media),
                            thumb_media: None,
                            aeskey: None,
                            url: None,
                            mid_size: Some(uploaded.cipher_size),
                            thumb_size: None,
                            thumb_height: None,
                            thumb_width: None,
                            hd_size: None,
                        }),
                        ..Default::default()
                    },
                    MediaKind::File => wire::MessageItem {
                        kind: Some(4),
                        file_item: Some(wire::FileItem {
                            media: Some(media),
                            file_name: Some(name.to_owned()),
                            md5: None,
                            len: Some(uploaded.raw_size.to_string()),
                        }),
                        ..Default::default()
                    },
                }
            }
        };
        let msg = wire::WechatMessage {
            from_user_id: Some(String::new()),
            to_user_id: Some(credentials.user_id.0.clone()),
            client_id: Some(client_id.to_owned()),
            message_type: Some(2),
            message_state: Some(2),
            context_token: Some(context_token.to_owned()),
            item_list: Some(vec![item]),
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
            _errmsg: errmsg,
        } = response;
        successful(ret, None).map_err(|error| match error {
            ClientError::BusinessRejected { ret, errcode, .. } => ClientError::BusinessRejected {
                ret,
                errcode,
                errmsg,
            },
            error => error,
        })?;
        message_id.ok_or(ClientError::Protocol("发送成功但缺少 message_id"))
    }

    /// 规则同 SDK：AES-128-ECB 加密后传 CDN，无缩略图；只传一次，不重放。
    pub async fn upload(
        &self,
        credentials: &Credentials,
        kind: MediaKind,
        bytes: Vec<u8>,
    ) -> Result<Uploaded, ClientError> {
        let mut random = [0u8; 32];
        getrandom::fill(&mut random).map_err(|error| {
            tracing::error!(%error, "wechat random source failed");
            ClientError::Protocol("随机源不可用")
        })?;
        let filekey = hex(&random[..16]);
        let key: [u8; 16] = random[16..].try_into().expect("切片 16 字节");
        let raw_size = bytes.len() as u64;
        let (md5, cipher) = tokio::task::spawn_blocking(move || {
            let md5 = hex(&Md5::digest(&bytes));
            let len = bytes.len();
            let mut buf = bytes;
            buf.resize(len / 16 * 16 + 16, 0);
            let cipher_len = Encryptor::new(&key.into())
                .encrypt_padded_mut::<Pkcs7>(&mut buf, len)
                .expect("缓冲已按 PKCS7 留足")
                .len();
            buf.truncate(cipher_len);
            (md5, buf)
        })
        .await
        .expect("加密不 panic");
        let cipher_size = cipher.len() as u64;
        let aes_hex = hex(&key);
        let response = self
            .authenticated(credentials, "ilink/bot/getuploadurl")?
            .timeout(API_TIMEOUT)
            .json(&wire::GetUploadUrlRequest {
                filekey: &filekey,
                media_type: match kind {
                    MediaKind::Image => 1,
                    MediaKind::File => 3,
                },
                to_user_id: &credentials.user_id.0,
                rawsize: raw_size,
                rawfilemd5: &md5,
                filesize: cipher_size,
                no_need_thumb: true,
                aeskey: &aes_hex,
                base_info: base_info(),
            })
            .send()
            .await
            .map_err(|error| network_error("getuploadurl", error))?;
        let response: wire::GetUploadUrlResponse = decode(response, "getuploadurl").await?;
        successful(response.ret, response.errcode)?;
        let url = match (response.upload_full_url, response.upload_param) {
            (Some(full), _) if !full.trim().is_empty() => {
                Url::parse(full.trim()).map_err(|_| ClientError::Protocol("上传地址无效"))?
            }
            (_, Some(param)) => {
                let mut url =
                    Url::parse(&format!("{CDN_BASE}/upload")).expect("CDN_BASE 为合法 URL");
                url.query_pairs_mut()
                    .append_pair("encrypted_query_param", &param)
                    .append_pair("filekey", &filekey);
                url
            }
            _ => return Err(ClientError::Protocol("缺少上传地址")),
        };
        let response = self
            .http
            .post(url)
            .timeout(CDN_UPLOAD_TIMEOUT)
            .header("Content-Type", "application/octet-stream")
            .body(cipher)
            .send()
            .await
            .map_err(|error| network_error("cdn_upload", error))?;
        if response.status() != reqwest::StatusCode::OK {
            let status = response.status().as_u16();
            let message = response
                .headers()
                .get("x-error-message")
                .and_then(|value| value.to_str().ok());
            tracing::warn!(status, message, "wechat cdn upload rejected");
            return Err(ClientError::Rejected(status));
        }
        let download_param = response
            .headers()
            .get("x-encrypted-param")
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty())
            .ok_or(ClientError::Protocol("上传成功但缺少 x-encrypted-param"))?
            .to_owned();
        Ok(Uploaded {
            kind,
            download_param,
            aes_hex,
            raw_size,
            cipher_size,
        })
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

type Encryptor = ecb::Encryptor<aes::Aes128>;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn base_info() -> wire::BaseInfo {
    wire::BaseInfo {
        channel_version: PROTOCOL_VERSION,
        bot_agent: format!("micnext/{}", env!("CARGO_PKG_VERSION")),
    }
}

fn parse_updates(
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
        if message.from_user_id.as_deref() != Some(user.0.as_str())
            || message
                .group_id
                .as_ref()
                .is_some_and(|group| !group.is_empty())
            || message.message_type != Some(1)
        {
            continue;
        }
        // 单条消息残缺只丢这一条，不拖垮整个入站连接。
        let (Some(context_token), Some(external_id)) = (
            message
                .context_token
                .clone()
                .filter(|token| !token.is_empty()),
            message.message_id,
        ) else {
            tracing::error!(
                has_message_id = message.message_id.is_some(),
                "wechat inbound message missing context_token or message_id, skipped"
            );
            continue;
        };
        let mut references = Vec::new();
        let mut content = Vec::new();
        for item in message.item_list.iter().flatten() {
            if let Some(reference) = &item.ref_msg {
                references.push(
                    parse_quote(reference)
                        .inspect_err(|reason| {
                            tracing::error!(reason, "wechat inbound quote unparsable");
                        })
                        .ok(),
                );
            }
            match item.kind {
                Some(1) => match item.text_item.as_ref().and_then(|item| item.text.as_ref()) {
                    Some(text) if text.trim().is_empty() => {}
                    Some(text) => content.push(IncomingContent::Text(text.clone())),
                    None => {
                        tracing::error!("wechat inbound text item missing text");
                        content.push(IncomingContent::Unsupported(Unsupported::Other));
                    }
                },
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
                Some(2) => content.push(IncomingContent::Image(parse_image(
                    item.image_item.as_ref(),
                ))),
                Some(4) => content.push(IncomingContent::Attachment(Attachment {
                    kind: AttachmentKind::File {
                        name: item
                            .file_item
                            .as_ref()
                            .and_then(|file| file.file_name.clone()),
                    },
                    source: parse_attachment(
                        item.file_item.as_ref().and_then(|file| file.media.as_ref()),
                    ),
                })),
                Some(5) => content.push(IncomingContent::Attachment(Attachment {
                    kind: AttachmentKind::Video,
                    source: parse_attachment(
                        item.video_item
                            .as_ref()
                            .and_then(|video| video.media.as_ref()),
                    ),
                })),
                Some(0 | 11 | 12) => content.push(IncomingContent::Unsupported(Unsupported::Other)),
                kind => {
                    tracing::warn!(?kind, "wechat inbound item type unknown");
                    content.push(IncomingContent::Unsupported(Unsupported::Other));
                }
            }
        }
        incoming.push(IncomingMessage {
            external_id,
            references,
            context_token,
            content,
        });
    }
    Ok(Updates {
        cursor,
        timeout_ms,
        incoming,
    })
}

fn parse_quote(reference: &wire::RefMessage) -> Result<QuoteRef, &'static str> {
    let id = reference
        .message_item
        .as_ref()
        .and_then(|item| item.msg_id.as_deref())
        .ok_or("引用缺少 message_item.msg_id")?;
    if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("引用 msg_id 不是十进制 uint64");
    }
    let id = wire::MessageId(id.parse().map_err(|_| "引用 msg_id 超出 uint64")?);
    let partial = reference
        .partial_text
        .as_ref()
        .map(|partial| {
            if partial.start.is_empty()
                || partial.end.is_empty()
                || partial.quotemd5.len() != 32
                || !partial
                    .quotemd5
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
            {
                return Err("部分引用锚点或 MD5 不合法");
            }
            let mut partial = partial.clone();
            partial.quotemd5.make_ascii_lowercase();
            Ok(partial)
        })
        .transpose()?;
    Ok(QuoteRef { id, partial })
}

/// 与 SDK 一致：ret/errcode 缺失视为成功（实测 getupdates 成功响应不带 ret），出现则须为 0。
fn successful(ret: Option<i32>, errcode: Option<i32>) -> Result<(), ClientError> {
    if ret == Some(-14) || errcode == Some(-14) {
        return Err(ClientError::SessionExpired);
    }
    if ret.is_some_and(|ret| ret != 0) || errcode.is_some_and(|code| code != 0) {
        return Err(ClientError::BusinessRejected {
            ret,
            errcode,
            errmsg: None,
        });
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
    let error = error.without_url();
    let timeout = error.is_timeout();
    let mut message = error.to_string();
    let mut source = error.source();
    let mut io_kind = None;
    let mut os_error = None;
    let mut tls_handshake_eof = false;
    while let Some(cause) = source {
        message.push_str(&format!(": {cause}"));
        if let Some(io) = cause.downcast_ref::<std::io::Error>() {
            io_kind = Some(io.kind());
            os_error = io.raw_os_error();
        }
        tls_handshake_eof |= cause.to_string() == "tls handshake eof";
        source = cause.source();
    }
    let detail = format!(
        "endpoint={endpoint}, timeout={timeout}, connect={}, io_kind={io_kind:?}, os_error={os_error:?}, tls_handshake_eof={tls_handshake_eof}: {message}",
        error.is_connect(),
    );
    if timeout {
        ClientError::Timeout(detail)
    } else {
        ClientError::Network(detail)
    }
}

async fn decode<T: DeserializeOwned>(
    response: Response,
    endpoint: &'static str,
) -> Result<T, ClientError> {
    if !response.status().is_success() {
        return Err(ClientError::Rejected(response.status().as_u16()));
    }
    let body = response
        .bytes()
        .await
        .map_err(|error| network_error(endpoint, error))?;
    let mut deserializer = serde_json::Deserializer::from_slice(&body);
    serde_path_to_error::deserialize(&mut deserializer).map_err(|error| {
        // 不记 serde 错误文本：invalid type/value 会带出原值。
        tracing::warn!(
            endpoint,
            path = %error.path(),
            category = ?error.inner().classify(),
            line = error.inner().line(),
            column = error.inner().column(),
            body_len = body.len(),
            "wechat response decode failed"
        );
        ClientError::Protocol("响应字段或枚举不匹配")
    })
}

/// 规则同 SDK：密钥优先 `image_item.aeskey`（hex），否则 `media.aes_key`，都没有为明文。
fn parse_image(item: Option<&wire::ImageItem>) -> Result<CdnSource, ImageFailure> {
    let item = item.ok_or(ImageFailure::Unparsable)?;
    let media = item.media.as_ref().ok_or(ImageFailure::Unparsable)?;
    let url = cdn_url(media).ok_or(ImageFailure::Unparsable)?;
    let key = match (&item.aeskey, &media.aes_key) {
        (Some(hex), _) => Some(hex_key(hex.as_bytes()).ok_or(ImageFailure::Unparsable)?),
        (None, Some(b64)) => Some(media_key(b64).ok_or(ImageFailure::Unparsable)?),
        (None, None) => None,
    };
    Ok(CdnSource { url, key })
}

/// 规则同 SDK：文件、视频只认 `media.aes_key`，不接受明文。
fn parse_attachment(media: Option<&wire::CdnMedia>) -> Result<CdnSource, AttachmentFailure> {
    let media = media.ok_or(AttachmentFailure::Unparsable)?;
    let url = cdn_url(media).ok_or(AttachmentFailure::Unparsable)?;
    let key = media
        .aes_key
        .as_deref()
        .and_then(media_key)
        .ok_or(AttachmentFailure::Unparsable)?;
    Ok(CdnSource {
        url,
        key: Some(key),
    })
}

/// 地址优先 `full_url`，否则 CDN 下载地址拼 `encrypted_query_param`。
fn cdn_url(media: &wire::CdnMedia) -> Option<Url> {
    match (&media.full_url, &media.encrypt_query_param) {
        (Some(full), _) => Url::parse(full).ok(),
        (None, Some(param)) => {
            let mut url = Url::parse(&format!("{CDN_BASE}/download")).expect("CDN_BASE 为合法 URL");
            url.query_pairs_mut()
                .append_pair("encrypted_query_param", param);
            Some(url)
        }
        (None, None) => None,
    }
}

/// `media.aes_key`：base64 后为 16 字节原文或 32 位 hex 文本。
fn media_key(b64: &str) -> Option<[u8; 16]> {
    let raw = STANDARD.decode(b64).ok()?;
    match raw.len() {
        16 => raw.try_into().ok(),
        _ => hex_key(&raw),
    }
}

fn hex_key(hex: &[u8]) -> Option<[u8; 16]> {
    if hex.len() != 32 {
        return None;
    }
    let mut key = [0u8; 16];
    for (byte, pair) in key.iter_mut().zip(hex.chunks(2)) {
        let pair = std::str::from_utf8(pair).ok()?;
        *byte = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(key)
}

#[cfg(test)]
mod tests;
