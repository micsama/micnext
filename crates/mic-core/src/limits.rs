//! 构建期旋钮。用户可调项存 SQLite，经 Web 设置修改。

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
pub(crate) const DEFAULT_CONTEXT_WINDOW: u32 = 100_000;
pub(crate) const COMPACTION_BUDGET_PERCENT: u64 = 80;
pub(crate) const ASCII_BYTES_PER_TOKEN: u64 = 3;
pub(crate) const NON_ASCII_TOKENS: u64 = 2;
pub(crate) const IMAGE_TOKENS: u64 = 4096;
pub(crate) const SNAPSHOT_FILES: usize = 8;
pub(crate) const SNAPSHOT_BYTES: usize = 64 * 1024;
/// 标题取材：用户文字合计保留最近部分；每条回复保留开头。
pub(crate) const TITLE_INPUT_CHARS: usize = 2_000;
pub(crate) const TITLE_INPUTS: u32 = 8;
pub(crate) const TITLE_REPLIES: u32 = 2;
pub(crate) const TITLE_REPLY_CHARS: usize = 500;
pub(crate) const TITLE_CHARS: usize = 40;
