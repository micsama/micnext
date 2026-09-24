//! build 后不改的旋钮。

use std::time::Duration;

/// 一条消息的最大字符数。
pub(crate) const MAX_TEXT_CHARS: usize = 100_000;
/// 请求体上限。
pub(crate) const MAX_BODY_BYTES: usize = 1024 * 1024;
/// 会话列表每页条数：缺省与上限。
pub(crate) const PAGE_DEFAULT: u32 = 30;
pub(crate) const PAGE_MAX: u32 = 100;
/// SSE 心跳注释间隔。
pub(crate) const SSE_KEEPALIVE: Duration = Duration::from_secs(15);
/// 单条流待发事件的缓冲；客户端读得慢时回压到内核订阅，落后即断流重连。
pub(crate) const STREAM_BUFFER: usize = 256;
/// token 与 Web 会话 chat id 的随机字节数。
pub(crate) const TOKEN_BYTES: usize = 32;
pub(crate) const CHAT_ID_BYTES: usize = 16;

/// 带内容哈希的构建产物（`/assets/*`）。
pub(crate) const ASSET_CACHE_CONTROL: &str = "public, max-age=31536000, immutable";
/// `index.html` 等无哈希文件：每次都向服务端确认。
pub(crate) const INDEX_CACHE_CONTROL: &str = "no-cache";
