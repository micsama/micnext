//! 构建期旋钮。用户可调项在 `config.toml`。

use std::time::Duration;

/// 用到 `max_turns` 的这个百分比时提醒模型收尾。
pub(crate) const MAX_TURNS_WARN_PERCENT: u32 = 80;
/// 单次模型调用的总尝试次数（含首次），只对 `Transient` 重试。
pub(crate) const MAX_MODEL_ATTEMPTS: u32 = 3;
/// 上游没给 `retry_after` 时的首次退避，之后逐次翻倍。
pub(crate) const RETRY_BASE: Duration = Duration::from_secs(2);
pub(crate) const MAX_RETRY_WAIT: Duration = Duration::from_secs(60);
pub(crate) const EVENT_CAPACITY: usize = 1024;
pub(crate) const WAKE_CHANNEL_CAPACITY: usize = 256;
