//! 微信入站图片：CDN 下载、AES-128-ECB 解密，再交 `mic_media` 规整。

use aes::cipher::block_padding::Pkcs7;
use aes::cipher::{BlockDecryptMut, KeyInit};
use mic_media::MediaError;

use crate::client::{Client, ImageSource};
use crate::limits::IMAGE_DOWNLOAD_ATTEMPTS;

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

/// 返回合规图片字节；仅下载失败会立即重试。
pub(crate) async fn fetch(client: &Client, source: &ImageSource) -> Result<Vec<u8>, ImageFailure> {
    let mut attempt = 1;
    let mut bytes = loop {
        match client.download(&source.url).await {
            Ok(bytes) => break bytes,
            Err(error) if attempt < IMAGE_DOWNLOAD_ATTEMPTS => {
                tracing::warn!(attempt, %error, "wechat image download failed, retrying");
                attempt += 1;
            }
            Err(error) => {
                tracing::warn!(attempt, %error, "wechat image download failed");
                return Err(ImageFailure::Download);
            }
        }
    };
    if let Some(key) = &source.key {
        let len = Decryptor::new(key.into())
            .decrypt_padded_mut::<Pkcs7>(&mut bytes)
            .map_err(|_| ImageFailure::Decrypt)?
            .len();
        bytes.truncate(len);
    }
    let image = tokio::task::spawn_blocking(move || mic_media::normalize(bytes))
        .await
        .expect("图片规整不 panic")
        .map_err(ImageFailure::Media)?;
    Ok(image.bytes.to_vec())
}
