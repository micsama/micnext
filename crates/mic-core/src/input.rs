//! 入站用户输入：校验并把原始字节变成已验证的图片原件。

use mic_media::MediaError;
use mic_message::limits::MAX_IMAGES_PER_INPUT;
use mic_store::NewInputPart;

/// Channel / Gateway 交给内核的原始片段；图片字节尚未验证。
pub enum IncomingPart {
    Text(String),
    Image(Vec<u8>),
}

/// 输入被整条拒收，不落任何消息或图片；文案面向用户。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InputError {
    #[error("消息内容为空")]
    Empty,
    #[error("一条消息最多带 {MAX_IMAGES_PER_INPUT} 张图片")]
    ImageLimit,
    #[error("{0}")]
    Image(MediaError),
}

/// 纯文本去空白后为空且无图则拒；图片须经 `mic_media::inspect` 判定合规。
pub(crate) async fn validate(parts: Vec<IncomingPart>) -> Result<Vec<NewInputPart>, InputError> {
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
                let image = tokio::task::spawn_blocking(move || mic_media::inspect(bytes))
                    .await
                    .expect("图片检查不 panic")
                    .map_err(InputError::Image)?;
                has_content = true;
                NewInputPart::Image(image)
            }
        });
    }
    if !has_content {
        return Err(InputError::Empty);
    }
    Ok(out)
}
