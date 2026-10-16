//! build 后不改的旋钮。

/// 前台 `timeoutMs` 缺省值。
pub(crate) const DEFAULT_TIMEOUT_MS: u64 = 120_000;
/// 前台 `timeoutMs` 上限；超过按此执行。
pub(crate) const MAX_TIMEOUT_MS: u64 = 600_000;
/// 后台 `timeoutMs` 缺省值；后台不设上限。
pub(crate) const BACKGROUND_TIMEOUT_MS: u64 = 600_000;
/// 前台 stdout、stderr 各自保留的尾部字节数。
pub(crate) const MAX_OUTPUT_BYTES: usize = 64_000;
