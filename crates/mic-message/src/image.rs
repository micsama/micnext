//! 用户入站图片：消息里只存引用，字节由 Store 持有。

use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// 由 `mic-store` 插入时用 SQLite rowid 回填。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ImageId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageFormat {
    Png,
    Jpeg,
    WebP,
}

impl ImageFormat {
    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::WebP => "image/webp",
        }
    }

    /// 按文件魔数识别；不信任文件名与浏览器给的 MIME。
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
            Some(Self::Png)
        } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Some(Self::Jpeg)
        } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            Some(Self::WebP)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageRef {
    pub id: ImageId,
}

/// 图片原件；克隆只增引用计数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageData {
    pub format: ImageFormat,
    pub bytes: Arc<[u8]>,
}
