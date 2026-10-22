//! 微信入站媒体：CDN 下载、AES-128-ECB 解密；图片交 `mic_media` 规整，文件与视频交内核落盘。

use std::time::Duration;

use aes::cipher::block_padding::Pkcs7;
use aes::cipher::{BlockDecryptMut, KeyInit};
use mic_media::MediaError;

use crate::client::{Attachment, AttachmentKind, CdnSource, Client, DownloadError};
use crate::limits::{
    ATTACHMENT_DOWNLOAD_BYTES, ATTACHMENT_DOWNLOAD_TIMEOUT, CDN_DOWNLOAD_ATTEMPTS,
    IMAGE_DOWNLOAD_BYTES, IMAGE_DOWNLOAD_TIMEOUT,
};

type Decryptor = ecb::Decryptor<aes::Aes128>;

pub(crate) struct MediaContext {
    pub session_id: i64,
    pub external_id: u64,
    pub item_index: usize,
}

/// 图片未能交给模型的原因；该条消息只记录并提示重发。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageFailure {
    /// 缺地址或密钥形状不符。
    Unparsable,
    Download,
    Decrypt,
    Media(MediaError),
    /// 超出单条消息的图片数量上限。
    OverLimit,
}

impl ImageFailure {
    pub(crate) fn placeholder(self) -> &'static str {
        match self {
            Self::Download => "[图片：下载失败]",
            Self::OverLimit => "[图片：超出单条上限]",
            Self::Unparsable | Self::Decrypt | Self::Media(_) => "[图片：无法识别]",
        }
    }

    /// 处置说明里的原因短语。
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::Download => "图片下载失败",
            Self::OverLimit => "图片超出单条数量上限",
            Self::Unparsable | Self::Decrypt | Self::Media(_) => "图片无法识别",
        }
    }
}

/// 文件或视频未能下载的原因；该条消息只记录并提示重发。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttachmentFailure {
    /// 缺地址或 `media.aes_key`。
    Unparsable,
    Download,
    TooLarge,
    Decrypt,
}

impl AttachmentFailure {
    fn phrase(self) -> &'static str {
        match self {
            Self::Download => "下载失败",
            Self::TooLarge => "超过 100 MB",
            Self::Unparsable | Self::Decrypt => "无法识别",
        }
    }

    /// 处置说明里的原因短语。
    pub(crate) fn reason(self, kind: &AttachmentKind) -> &'static str {
        match (kind, self) {
            (AttachmentKind::File { .. }, Self::Download) => "文件下载失败",
            (AttachmentKind::File { .. }, Self::TooLarge) => "文件超过 100 MB",
            (AttachmentKind::File { .. }, Self::Unparsable | Self::Decrypt) => "文件无法识别",
            (AttachmentKind::Video, Self::Download) => "视频下载失败",
            (AttachmentKind::Video, Self::TooLarge) => "视频超过 100 MB",
            (AttachmentKind::Video, Self::Unparsable | Self::Decrypt) => "视频无法识别",
        }
    }
}

/// 附件未能收下时用户与模型看到的占位。
pub(crate) fn attachment_placeholder(kind: &AttachmentKind, failure: AttachmentFailure) -> String {
    let status = failure.phrase();
    match kind {
        AttachmentKind::File { name: Some(name) } => format!("[文件：{name}，{status}]"),
        AttachmentKind::File { name: None } => format!("[文件：{status}]"),
        AttachmentKind::Video => format!("[视频：{status}]"),
    }
}

/// 返回合规图片字节。
pub(crate) async fn fetch(
    client: &Client,
    source: &CdnSource,
    context: &MediaContext,
) -> Result<Vec<u8>, ImageFailure> {
    let bytes = download(
        client,
        source,
        IMAGE_DOWNLOAD_BYTES,
        IMAGE_DOWNLOAD_TIMEOUT,
        context,
    )
    .await
    .map_err(|failure| match failure {
        Fetch::Download | Fetch::TooLarge => ImageFailure::Download,
        Fetch::Decrypt => ImageFailure::Decrypt,
    })?;
    let image = tokio::task::spawn_blocking(move || mic_media::normalize(bytes))
        .await
        .expect("图片规整不 panic")
        .map_err(ImageFailure::Media)?;
    Ok(image.bytes.to_vec())
}

/// 下载解密，返回上游文件名与字节；落盘归内核。
pub(crate) async fn fetch_attachment(
    client: &Client,
    attachment: &Attachment,
    context: &MediaContext,
) -> Result<(String, Vec<u8>), AttachmentFailure> {
    let source = attachment.source.as_ref().map_err(|failure| *failure)?;
    let bytes = download(
        client,
        source,
        ATTACHMENT_DOWNLOAD_BYTES,
        ATTACHMENT_DOWNLOAD_TIMEOUT,
        context,
    )
    .await
    .map_err(|failure| match failure {
        Fetch::Download => AttachmentFailure::Download,
        Fetch::TooLarge => AttachmentFailure::TooLarge,
        Fetch::Decrypt => AttachmentFailure::Decrypt,
    })?;
    let name = match &attachment.kind {
        AttachmentKind::File { name } => name.clone().unwrap_or_else(|| "文件".to_owned()),
        AttachmentKind::Video => "视频.mp4".to_owned(),
    };
    Ok((name, bytes))
}

enum Fetch {
    Download,
    TooLarge,
    Decrypt,
}

/// 仅网络类失败立即重试；超限直接返回。
async fn download(
    client: &Client,
    source: &CdnSource,
    limit: usize,
    timeout: Duration,
    context: &MediaContext,
) -> Result<Vec<u8>, Fetch> {
    let mut attempt = 1;
    let mut bytes = loop {
        match client.download(&source.url, limit, timeout).await {
            Ok(bytes) => break bytes,
            Err(DownloadError::TooLarge) => {
                tracing::warn!(
                    session_id = context.session_id,
                    external_id = context.external_id,
                    item_index = context.item_index,
                    attempt,
                    limit,
                    "wechat cdn download over limit"
                );
                return Err(Fetch::TooLarge);
            }
            Err(DownloadError::Client(error)) if attempt < CDN_DOWNLOAD_ATTEMPTS => {
                tracing::warn!(session_id = context.session_id, external_id = context.external_id, item_index = context.item_index, attempt, error = %error.diagnostic(), "wechat cdn download failed, retrying");
                attempt += 1;
            }
            Err(DownloadError::Client(error)) => {
                tracing::warn!(session_id = context.session_id, external_id = context.external_id, item_index = context.item_index, attempt, error = %error.diagnostic(), "wechat cdn download failed");
                return Err(Fetch::Download);
            }
        }
    };
    if let Some(key) = &source.key {
        let len = Decryptor::new(key.into())
            .decrypt_padded_mut::<Pkcs7>(&mut bytes)
            .map_err(|_| Fetch::Decrypt)?
            .len();
        bytes.truncate(len);
    }
    Ok(bytes)
}
