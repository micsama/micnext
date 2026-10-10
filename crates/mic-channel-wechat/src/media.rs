//! 微信入站媒体：CDN 下载、AES-128-ECB 解密；图片交 `mic_media` 规整，文件与视频存入会话工作目录。

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use aes::cipher::block_padding::Pkcs7;
use aes::cipher::{BlockDecryptMut, KeyInit};
use mic_media::MediaError;
use tokio::io::AsyncWriteExt;

use crate::client::{Attachment, AttachmentKind, CdnSource, Client, DownloadError};
use crate::limits::{
    ATTACHMENT_DOWNLOAD_BYTES, ATTACHMENT_DOWNLOAD_TIMEOUT, CDN_DOWNLOAD_ATTEMPTS,
    IMAGE_DOWNLOAD_BYTES, IMAGE_DOWNLOAD_TIMEOUT,
};

type Decryptor = ecb::Decryptor<aes::Aes128>;

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

/// 文件或视频未能保存的原因；该条消息只记录并提示重发。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttachmentFailure {
    /// 缺地址或 `media.aes_key`。
    Unparsable,
    Download,
    TooLarge,
    Decrypt,
    Save,
}

impl AttachmentFailure {
    fn phrase(self) -> &'static str {
        match self {
            Self::Download => "下载失败",
            Self::TooLarge => "超过 100 MB",
            Self::Unparsable | Self::Decrypt => "无法识别",
            Self::Save => "保存失败",
        }
    }

    /// 处置说明里的原因短语。
    pub(crate) fn reason(self, kind: &AttachmentKind) -> &'static str {
        match (kind, self) {
            (AttachmentKind::File { .. }, Self::Download) => "文件下载失败",
            (AttachmentKind::File { .. }, Self::TooLarge) => "文件超过 100 MB",
            (AttachmentKind::File { .. }, Self::Unparsable | Self::Decrypt) => "文件无法识别",
            (AttachmentKind::File { .. }, Self::Save) => "文件保存失败",
            (AttachmentKind::Video, Self::Download) => "视频下载失败",
            (AttachmentKind::Video, Self::TooLarge) => "视频超过 100 MB",
            (AttachmentKind::Video, Self::Unparsable | Self::Decrypt) => "视频无法识别",
            (AttachmentKind::Video, Self::Save) => "视频保存失败",
        }
    }
}

/// 用户与模型看到的附件占位：成功带绝对路径，失败带原因。
pub(crate) fn attachment_placeholder(
    kind: &AttachmentKind,
    result: Result<&Path, AttachmentFailure>,
) -> String {
    let status = match result {
        Ok(path) => format!("已保存到 {}", path.display()),
        Err(failure) => failure.phrase().to_owned(),
    };
    match kind {
        AttachmentKind::File { name: Some(name) } => format!("[文件：{name}，{status}]"),
        AttachmentKind::File { name: None } => format!("[文件：{status}]"),
        AttachmentKind::Video => format!("[视频：{status}]"),
    }
}

/// 返回合规图片字节。
pub(crate) async fn fetch(client: &Client, source: &CdnSource) -> Result<Vec<u8>, ImageFailure> {
    let bytes = download(client, source, IMAGE_DOWNLOAD_BYTES, IMAGE_DOWNLOAD_TIMEOUT)
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

/// 下载解密后存入 `dir`，返回最终绝对路径；同名不覆盖，失败或取消不留半截文件。
pub(crate) async fn save(
    client: &Client,
    attachment: &Attachment,
    dir: &Path,
) -> Result<PathBuf, AttachmentFailure> {
    let source = attachment.source.as_ref().map_err(|failure| *failure)?;
    let bytes = download(
        client,
        source,
        ATTACHMENT_DOWNLOAD_BYTES,
        ATTACHMENT_DOWNLOAD_TIMEOUT,
    )
    .await
    .map_err(|failure| match failure {
        Fetch::Download => AttachmentFailure::Download,
        Fetch::TooLarge => AttachmentFailure::TooLarge,
        Fetch::Decrypt => AttachmentFailure::Decrypt,
    })?;
    let name = match &attachment.kind {
        AttachmentKind::File { name } => name.as_deref().and_then(file_name).unwrap_or("文件"),
        AttachmentKind::Video => "视频.mp4",
    };
    write(dir, name, &bytes).await.map_err(|error| {
        tracing::warn!(%error, dir = %dir.display(), "wechat attachment save failed");
        AttachmentFailure::Save
    })
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
) -> Result<Vec<u8>, Fetch> {
    let mut attempt = 1;
    let mut bytes = loop {
        match client.download(&source.url, limit, timeout).await {
            Ok(bytes) => break bytes,
            Err(DownloadError::TooLarge) => {
                tracing::warn!(limit, "wechat cdn download over limit");
                return Err(Fetch::TooLarge);
            }
            Err(DownloadError::Client(error)) if attempt < CDN_DOWNLOAD_ATTEMPTS => {
                tracing::warn!(attempt, %error, "wechat cdn download failed, retrying");
                attempt += 1;
            }
            Err(DownloadError::Client(error)) => {
                tracing::warn!(attempt, %error, "wechat cdn download failed");
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

/// 取上游文件名最后一段，去掉控制字符；空、`.`、`..` 视为缺失。
fn file_name(raw: &str) -> Option<&str> {
    let last = raw.rsplit(['/', '\\']).next()?.trim();
    match last {
        "" | "." | ".." => None,
        name if name.chars().any(char::is_control) => None,
        name => Some(name),
    }
}

/// 先以 `create_new` 占住最终名，再写 `.part` 后 rename 覆盖占位。
async fn write(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    tokio::fs::create_dir_all(dir).await?;
    let dir = tokio::fs::canonicalize(dir).await?;
    let (stem, ext) = split_name(name);
    let mut n = 1u32;
    let target = loop {
        let candidate = match (n, ext) {
            (1, _) => dir.join(name),
            (_, Some(ext)) => dir.join(format!("{stem} ({n}).{ext}")),
            (_, None) => dir.join(format!("{stem} ({n})")),
        };
        match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
            .await
        {
            Ok(_) => break candidate,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => n += 1,
            Err(error) => return Err(error),
        }
    };
    let mut part = target.clone().into_os_string();
    part.push(".part");
    let mut guard = Cleanup(vec![target.clone(), PathBuf::from(part)]);
    let mut file = tokio::fs::File::create(&guard.0[1]).await?;
    file.write_all(bytes).await?;
    file.sync_all().await?;
    drop(file);
    tokio::fs::rename(&guard.0[1], &target).await?;
    guard.0.clear();
    Ok(target)
}

/// `报告.pdf` → (`报告`, `pdf`)；`.bashrc`、无扩展名不拆。
fn split_name(name: &str) -> (&str, Option<&str>) {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => (stem, Some(ext)),
        _ => (name, None),
    }
}

/// 未完成写盘时删掉占位与 `.part`；取消即 drop。
struct Cleanup(Vec<PathBuf>);

impl Drop for Cleanup {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}
