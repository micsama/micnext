//! build 后不改的旋钮。

use std::time::Duration;

/// 建连超时。
pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// 两次收到数据之间的最长间隔（含等待首字节）。
pub(crate) const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
/// 测试连接的整体超时。
pub(crate) const PROBE_TIMEOUT: Duration = Duration::from_secs(15);
