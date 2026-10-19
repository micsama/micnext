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
/// 压缩：保留段里超过此估算的完成态工具结果换成占位（最新一步除外）。
pub(crate) const ELIDE_TOOL_TOKENS: u64 = 2_000;
/// 压缩：带回的用户原话不超过此估算放全文，否则只留首尾。
pub(crate) const QUOTE_FULL_TOKENS: u64 = 500;
pub(crate) const QUOTE_HEAD_TOKENS: u64 = 300;
pub(crate) const QUOTE_TAIL_TOKENS: u64 = 150;
pub(crate) const SNAPSHOT_FILES: usize = 8;
pub(crate) const SNAPSHOT_BYTES: usize = 64 * 1024;
/// 标题取材：用户文字合计保留最近部分；每条回复保留开头。
pub(crate) const TITLE_INPUT_CHARS: usize = 2_000;
pub(crate) const TITLE_INPUTS: u32 = 8;
pub(crate) const TITLE_REPLIES: u32 = 2;
pub(crate) const TITLE_REPLY_CHARS: usize = 500;
pub(crate) const TITLE_CHARS: usize = 40;
/// 后台任务输出：至少保留最近这么多字节，按段落盘、整段丢弃。
pub(crate) const JOB_OUTPUT_KEEP: u64 = 1024 * 1024;
pub(crate) const JOB_OUTPUT_SEGMENT: u64 = 256 * 1024;
/// `job_output` 单次读取的缺省与上限字节数。
pub(crate) const JOB_READ_DEFAULT: u64 = 16 * 1024;
pub(crate) const JOB_READ_MAX: u64 = 64 * 1024;
/// 回注里附带的输出尾部字节数。
pub(crate) const JOB_REPORT_TAIL: u64 = 4 * 1024;
/// 任务列表里每项附带的输出尾部字节数。
pub(crate) const JOB_VIEW_TAIL: u64 = 1024;
/// 输出增长的列表刷新通知至多这么频繁。
pub(crate) const JOB_NOTIFY_INTERVAL: Duration = Duration::from_secs(1);
/// 结束超过这么久的任务默认不列出。
pub(crate) const JOB_HIDE_AFTER: Duration = Duration::from_secs(14 * 24 * 3600);
/// 子 run 的工具轮次上限（与全局 `max_turns` 取小；用尽后另有一次总结）与墙钟上限。
pub(crate) const AGENT_MAX_TURNS: u32 = 12;
pub(crate) const AGENT_RUN_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// 子 run 交付的文本上限，超出截断并指向子会话。
pub(crate) const AGENT_DELIVERY_MAX: usize = 32 * 1024;
