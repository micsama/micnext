//! 入站用户输入：校验并把原始字节变成已验证的图片原件。

use mic_message::limits::{MAX_IMAGES_PER_INPUT, MAX_IMAGE_BYTES};
use mic_message::{ImageData, ImageFormat};
use mic_store::NewInputPart;

/// Channel / Gateway 交给内核的原始片段；图片字节尚未验证。
pub enum IncomingPart {
    Text(String),
    Image(Vec<u8>),
}

/// 入站输入的执行处置。
pub enum InputHandling {
    /// 照常执行：发布后唤醒。
    Run,
    /// 只记录：以 held 落盘，不唤醒；进入后续上下文。
    Record,
}

/// 输入被整条拒收，不落任何消息或图片；文案面向用户。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InputError {
    #[error("消息内容为空")]
    Empty,
    #[error("一条消息最多带 {MAX_IMAGES_PER_INPUT} 张图片")]
    ImageLimit,
    #[error("图片不能超过 {} MB", MAX_IMAGE_BYTES / 1024 / 1024)]
    ImageTooLarge,
    #[error("只支持 PNG、JPEG、WebP 图片")]
    InvalidImage,
}

/// 纯文本去空白后为空且无图则拒；图片按魔数识别格式，不解码。
pub(crate) fn validate(parts: Vec<IncomingPart>) -> Result<Vec<NewInputPart>, InputError> {
    let mut has_content = false;
    let mut images = 0;
    let mut out = Vec::with_capacity(parts.len());
    for part in parts {
        out.push(match part {
            IncomingPart::Text(text) => {
                has_content |= !text.trim().is_empty();
                NewInputPart::Text(text)
            }
            IncomingPart::Image(bytes) => {
                images += 1;
                if images > MAX_IMAGES_PER_INPUT {
                    return Err(InputError::ImageLimit);
                }
                if bytes.len() > MAX_IMAGE_BYTES {
                    return Err(InputError::ImageTooLarge);
                }
                let format = ImageFormat::sniff(&bytes).ok_or(InputError::InvalidImage)?;
                has_content = true;
                NewInputPart::Image(ImageData {
                    format,
                    bytes: bytes.into(),
                })
            }
        });
    }
    if !has_content {
        return Err(InputError::Empty);
    }
    Ok(out)
}
