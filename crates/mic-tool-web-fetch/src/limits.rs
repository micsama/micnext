//! build 后不改的旋钮。

use std::time::Duration;

/// URL 最大长度（字符）。
pub(crate) const MAX_URL_CHARS: usize = 2048;
/// 最多跟随的重定向次数。
pub(crate) const MAX_REDIRECTS: usize = 5;
/// 单次抓取总超时。
pub(crate) const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
/// 响应体最多读取的字节数；超出部分丢弃。
pub(crate) const MAX_BODY_BYTES: usize = 5 * 1024 * 1024;
/// 返回给模型的全文字符上限（含首部与尾注）。
pub(crate) const FETCH_MAX_CHARS: usize = 100_000;
