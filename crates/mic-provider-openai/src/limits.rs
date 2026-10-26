//! build 后不改的旋钮。

use std::time::Duration;

/// 建连超时。
pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// 两次收到数据之间的最长间隔（含等待首字节）。
pub(crate) const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
/// 测试连接的整体超时。
pub(crate) const PROBE_TIMEOUT: Duration = Duration::from_secs(30);
/// 向 Codex 后端声明的客户端版本；过旧时模型列表为空。
pub(crate) const CODEX_CLIENT_VERSION: &str = "0.161.0";
/// 联网搜索返回的条数。
pub(crate) const SEARCH_COUNT: u32 = 10;
/// 联网搜索单次请求的整体超时。
pub(crate) const SEARCH_TIMEOUT: Duration = Duration::from_secs(30);
