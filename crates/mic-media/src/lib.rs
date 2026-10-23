//! 图片能力：格式识别、解码校验与规整。不理解渠道协议，不决定失败后的处理；
//! 接口为同步 CPU 计算，异步调用方自行放进 `spawn_blocking`。

mod limits;

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::error::ImageError;
use image::imageops::FilterType;
use image::{DynamicImage, ImageReader, Limits};
use mic_message::limits::MAX_IMAGE_BYTES;
use mic_message::{ImageData, ImageFormat};

use crate::limits::{DECODE_MAX_ALLOC, JPEG_QUALITY, KEEP_BYTES, NORMALIZE_EDGE};

/// 无法规整的原因；由调用方决定拒收还是占位。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MediaError {
    #[error("不是可识别的图片")]
    Unsupported,
    #[error("图片已损坏")]
    Corrupt,
    #[error("图片不能超过 {} MB", MAX_IMAGE_BYTES / 1024 / 1024)]
    TooManyBytes,
    #[error("图片尺寸过大")]
    TooLargeDimensions,
}

/// 规整为入库并发给模型的图片：PNG/JPEG/WebP、长边 ≤ `NORMALIZE_EDGE` 且字节 ≤ `KEEP_BYTES`
/// 的原样保留；否则解码首帧，长边超限等比缩小，有实际透明像素编码 PNG，否则 JPEG。
pub fn normalize(bytes: Vec<u8>) -> Result<ImageData, MediaError> {
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(MediaError::TooManyBytes);
    }
    let mut image = decode(&bytes)?;
    let edge = image.width().max(image.height());
    if let Some(format) = sniff(&bytes) {
        if edge <= NORMALIZE_EDGE && bytes.len() <= KEEP_BYTES {
            return Ok(ImageData {
                format,
                bytes: bytes.into(),
            });
        }
    }
    if edge > NORMALIZE_EDGE {
        image = image.resize(NORMALIZE_EDGE, NORMALIZE_EDGE, FilterType::Lanczos3);
    }
    let mut out = Cursor::new(Vec::new());
    let (format, encoded) = if transparent(&image) {
        let encoded = image.write_to(&mut out, image::ImageFormat::Png);
        (ImageFormat::Png, encoded)
    } else {
        let encoded = DynamicImage::ImageRgb8(image.to_rgb8())
            .write_with_encoder(JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY));
        (ImageFormat::Jpeg, encoded)
    };
    encoded.map_err(|error| corrupt("encode", error))?;
    Ok(ImageData {
        format,
        bytes: out.into_inner().into(),
    })
}

/// 截图常带全不透明的 alpha 通道，按像素判断才不会把它们留成大 PNG。
fn transparent(image: &DynamicImage) -> bool {
    image.color().has_alpha() && image.to_rgba8().pixels().any(|pixel| pixel[3] < u8::MAX)
}

/// 按文件魔数识别；不信任文件名与外部给的 MIME。
fn sniff(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some(ImageFormat::Png)
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(ImageFormat::Jpeg)
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(ImageFormat::WebP)
    } else {
        None
    }
}

/// 格式由内容判断；库不认识的格式为 `Unsupported`。
fn reader(bytes: &[u8]) -> Result<ImageReader<Cursor<&[u8]>>, MediaError> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| corrupt("format", error))?;
    if reader.format().is_none() {
        return Err(MediaError::Unsupported);
    }
    let mut limits = Limits::default();
    limits.max_alloc = Some(DECODE_MAX_ALLOC);
    reader.limits(limits);
    Ok(reader)
}

/// 解码首帧；超内存上限视为尺寸过大。
fn decode(bytes: &[u8]) -> Result<DynamicImage, MediaError> {
    reader(bytes)?.decode().map_err(|e| match e {
        ImageError::Limits(_) => MediaError::TooLargeDimensions,
        error => corrupt("decode", error),
    })
}

fn error_chain(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let detail = cause.to_string();
        if !message.ends_with(&detail) {
            message.push_str(": ");
            message.push_str(&detail);
        }
        source = cause.source();
    }
    message
}

fn corrupt(stage: &'static str, error: impl std::error::Error) -> MediaError {
    tracing::warn!(stage, error = %error_chain(&error), "image processing failed");
    MediaError::Corrupt
}

#[cfg(test)]
mod tests;
