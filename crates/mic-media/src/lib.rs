//! 图片能力：格式识别、合规判定与规整。不理解渠道协议，不决定失败后的处理；
//! 接口为同步 CPU 计算，异步调用方自行放进 `spawn_blocking`。

mod limits;

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::error::ImageError;
use image::imageops::FilterType;
use image::{DynamicImage, ImageReader, Limits};
use mic_message::limits::MAX_IMAGE_BYTES;
use mic_message::{ImageData, ImageFormat};

use crate::limits::{DECODE_MAX_ALLOC, JPEG_QUALITY, NORMALIZE_EDGE};

pub use crate::limits::MAX_EDGE;

/// 不合规原因；由调用方决定拒收还是规整。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MediaError {
    #[error("不是可识别的 PNG、JPEG、WebP 图片")]
    Unsupported,
    #[error("图片已损坏")]
    Corrupt,
    #[error("图片不能超过 {} MB", MAX_IMAGE_BYTES / 1024 / 1024)]
    TooManyBytes,
    #[error("图片长边不能超过 {MAX_EDGE} 像素")]
    TooLargeDimensions,
}

/// 合规 = 魔数为 PNG/JPEG/WebP ∧ 字节 ≤ `MAX_IMAGE_BYTES` ∧ 长边 ≤ `MAX_EDGE` ∧ 完整解码成功。
/// 合规时原样保留字节。
pub fn inspect(bytes: Vec<u8>) -> Result<ImageData, MediaError> {
    let format = check(&bytes)?;
    Ok(ImageData {
        format,
        bytes: bytes.into(),
    })
}

/// 合规原样返回；否则解码首帧，长边超 `NORMALIZE_EDGE` 等比缩小，
/// 有透明通道编码 PNG，否则 JPEG。
pub fn normalize(bytes: Vec<u8>) -> Result<ImageData, MediaError> {
    match check(&bytes) {
        Ok(format) => {
            return Ok(ImageData {
                format,
                bytes: bytes.into(),
            })
        }
        Err(MediaError::Corrupt) => return Err(MediaError::Corrupt),
        Err(_) => {}
    }
    let mut image = decode(&bytes)?;
    if image.width().max(image.height()) > NORMALIZE_EDGE {
        image = image.resize(NORMALIZE_EDGE, NORMALIZE_EDGE, FilterType::Lanczos3);
    }
    let mut out = Cursor::new(Vec::new());
    let encoded = if image.color().has_alpha() {
        image.write_to(&mut out, image::ImageFormat::Png)
    } else {
        DynamicImage::ImageRgb8(image.to_rgb8())
            .write_with_encoder(JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY))
    };
    encoded.map_err(|_| MediaError::Corrupt)?;
    inspect(out.into_inner())
}

fn check(bytes: &[u8]) -> Result<ImageFormat, MediaError> {
    let format = sniff(bytes).ok_or(MediaError::Unsupported)?;
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(MediaError::TooManyBytes);
    }
    let (width, height) = reader(bytes)?
        .into_dimensions()
        .map_err(|_| MediaError::Corrupt)?;
    if width.max(height) > MAX_EDGE {
        return Err(MediaError::TooLargeDimensions);
    }
    decode(bytes)?;
    Ok(format)
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
        .map_err(|_| MediaError::Corrupt)?;
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
        _ => MediaError::Corrupt,
    })
}
