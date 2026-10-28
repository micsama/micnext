//! 构建期旋钮。用户可调项存 SQLite，经 Web 设置修改。

use std::num::NonZeroU32;
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
pub(crate) const DEFAULT_CONTEXT_WINDOW: u32 = 200_000;
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
/// 冷时整理：距上次对话调用超过此闲置时长，run 开始前整理历史；按模型名前缀匹配，
/// 未列出的取缺省。经验值，宁可晚整理。
pub(crate) const IDLE_TRIM_AFTER: &[(&str, Duration)] = &[("gpt-", Duration::from_secs(3 * 3600))];
pub(crate) const IDLE_TRIM_DEFAULT: Duration = Duration::from_secs(24 * 3600);
/// 冷时整理：上下文占用超过此百分比改为直接压缩。
pub(crate) const IDLE_COMPACT_PERCENT: u64 = 50;
/// skill：加载结果里列出的文件数上限；description 字符上限（同开放规范）。
pub(crate) const SKILL_FILES_LISTED: usize = 50;
pub(crate) const SKILL_DESCRIPTION_CHARS: usize = 1024;
/// 标题取材：用户文字合计保留最近部分；每条回复保留开头。
pub(crate) const TITLE_INPUT_CHARS: usize = 2_000;
pub(crate) const TITLE_INPUTS: u32 = 8;
pub(crate) const TITLE_REPLIES: u32 = 2;
pub(crate) const TITLE_REPLY_CHARS: usize = 500;
pub(crate) const TITLE_CHARS: usize = 40;
/// 输入预测：取最近几轮，每条文字保留结尾；输出上限与整次调用时限。
pub(crate) const PREDICTION_ROUNDS: NonZeroU32 = NonZeroU32::new(3).unwrap();
pub(crate) const PREDICTION_MESSAGES: NonZeroU32 = NonZeroU32::new(40).unwrap();
pub(crate) const PREDICTION_SOURCE_CHARS: usize = 1_000;
pub(crate) const PREDICTION_CHARS: usize = 60;
pub(crate) const PREDICTION_TIMEOUT: Duration = Duration::from_secs(20);
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
/// 单次交付（`send_file` / push）的文件数与单个文件上限。
pub(crate) const MAX_SEND_FILES: usize = 8;
pub(crate) const MAX_SEND_FILE_BYTES: u64 = 100 * 1024 * 1024;
/// 输出增长的列表刷新通知至多这么频繁。
pub(crate) const JOB_NOTIFY_INTERVAL: Duration = Duration::from_secs(1);
/// 结束超过这么久的任务默认不列出。
pub(crate) const JOB_HIDE_AFTER: Duration = Duration::from_secs(14 * 24 * 3600);
/// 子 run 的工具轮次上限（与全局 `max_turns` 取小；用尽后另有一次总结）与墙钟上限。
pub(crate) const AGENT_MAX_TURNS: u32 = 12;
pub(crate) const AGENT_RUN_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// 子 run 交付的文本上限，超出截断并指向子会话。
pub(crate) const AGENT_DELIVERY_MAX: usize = 32 * 1024;
/// 定时任务：到点时上下文占用超过此百分比先压缩；低于对话阈值，因每次触发都重读整段历史。
pub(crate) const SCHEDULE_COMPACT_PERCENT: u64 = 30;
/// 定时任务：周期规则相邻两次触发的最小间隔。
pub(crate) const SCHEDULE_MIN_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// 任务会话 run 的墙钟上限；轮次上限同子 agent。
pub(crate) const SCHEDULE_RUN_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// 连续这么多次系统级失败后自动暂停。
pub(crate) const SCHEDULE_FAIL_PAUSE: usize = 3;
/// `schedules` 查看单个任务时列出的触发数。
pub(crate) const SCHEDULE_FIRES_SHOWN: u32 = 10;
/// 到点后迟于此即记为错过，不再执行。
pub(crate) const SCHEDULE_LATE_GRACE: Duration = Duration::from_secs(30);
/// 调度循环至少这么频繁对一次墙钟，免受系统休眠与改时影响。
pub(crate) const SCHEDULE_TICK_MAX: Duration = Duration::from_secs(60);
/// 合并错过时最多枚举的次数。
pub(crate) const SCHEDULE_MISSED_SCAN: u32 = 100_000;
/// SQL 写入把关等待 AI 答复的上限。
pub(crate) const SQL_REVIEW_TIMEOUT: Duration = Duration::from_secs(60);
