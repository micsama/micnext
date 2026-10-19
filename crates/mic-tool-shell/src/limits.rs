//! build 后不改的旋钮。

use std::time::Duration;

/// 前台 `timeoutMs` 缺省值。
pub(crate) const DEFAULT_TIMEOUT_MS: u64 = 120_000;
/// 前台 `timeoutMs` 上限；超过按此执行。
pub(crate) const MAX_TIMEOUT_MS: u64 = 600_000;
/// 后台 `timeoutMs` 缺省值；后台不设上限。
pub(crate) const BACKGROUND_TIMEOUT_MS: u64 = 600_000;
/// 前台 stdout、stderr 各自保留的尾部字节数。
pub(crate) const MAX_OUTPUT_BYTES: usize = 64_000;
/// 后台 `notify_pattern`：每路按行匹配，超过此字节数的行按片段匹配。
pub(crate) const NOTIFY_LINE_BYTES: usize = 4096;
/// 后台 `notify_pattern`：两次通知的最短间隔，期间命中丢弃。
pub(crate) const NOTIFY_COOLDOWN: Duration = Duration::from_secs(10);
/// 后台 `notify_pattern`：每个任务最多通知次数。
pub(crate) const NOTIFY_MAX: u32 = 10;
