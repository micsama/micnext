//! build 后不改的旋钮。

use std::time::Duration;

/// 一条消息的最大字符数。
pub(crate) const MAX_TEXT_CHARS: usize = 100_000;
/// 人设名字（去首尾空白后）的最大字符数。
pub(crate) const PERSONA_NAME_MAX_CHARS: usize = 40;
/// 人设提示词与通用偏好的最大字符数。
pub(crate) const PROMPT_MAX_CHARS: usize = 8000;
/// 设置里单轮模型调用上限的可选上界。
pub(crate) const MAX_TURNS_LIMIT: u32 = 500;
/// 请求体上限。
pub(crate) const MAX_BODY_BYTES: usize = 1024 * 1024;
/// 带图消息路由的请求体上限：4 张 4 MiB 的图经 base64 膨胀后仍可容纳。
pub(crate) const MAX_INPUT_BODY_BYTES: usize = 24 * 1024 * 1024;
/// 会话列表每页条数：缺省与上限。
pub(crate) const PAGE_DEFAULT: u32 = 30;
pub(crate) const PAGE_MAX: u32 = 100;
/// SSE 心跳注释间隔。
pub(crate) const SSE_KEEPALIVE: Duration = Duration::from_secs(15);
/// 单条流待发事件的缓冲；客户端读得慢时回压到内核订阅，落后即断流重连。
pub(crate) const STREAM_BUFFER: usize = 256;
/// 停止后等连接自行关闭的上限，超时直接断开。
pub(crate) const SHUTDOWN_GRACE: Duration = Duration::from_secs(3);
/// token 与 Web 会话 chat id 的随机字节数。
pub(crate) const TOKEN_BYTES: usize = 32;
pub(crate) const CHAT_ID_BYTES: usize = 16;

/// 带内容哈希的构建产物（`/assets/*`）。
pub(crate) const ASSET_CACHE_CONTROL: &str = "public, max-age=31536000, immutable";
/// `index.html` 等无哈希文件：每次都向服务端确认。
pub(crate) const INDEX_CACHE_CONTROL: &str = "no-cache";

/// 模型名与 API key 的长度上限（字符数）。
pub(crate) const MODEL_NAME_MAX_CHARS: usize = 80;
/// 服务商名称最大字符数。
pub(crate) const ENDPOINT_NAME_MAX_CHARS: usize = 40;
pub(crate) const API_KEY_MAX_CHARS: usize = 512;

/// 开发者日志：保留条数、文本总字节、单条文本字节、单条字段数。
pub(crate) const LOG_RECORDS: usize = 10_000;
pub(crate) const LOG_TOTAL_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const LOG_RECORD_BYTES: usize = 16 * 1024;
pub(crate) const LOG_FIELDS: usize = 128;
/// 日志流每批读取条数，也是单连接待发事件上限。
pub(crate) const LOG_BATCH: usize = 128;
/// 诊断 SQL 文本上限。
pub(crate) const SQL_MAX_BYTES: usize = 16 * 1024;
