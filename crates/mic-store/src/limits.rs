//! build 后不改的旋钮。

/// 会话列表预览截取的字符数。
pub(crate) const PREVIEW_CHARS: u32 = 80;
/// 诊断查询：执行时间、返回行数与结果文本字节上限。
pub(crate) const DIAGNOSTIC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
pub(crate) const DIAGNOSTIC_ROWS: usize = 200;
pub(crate) const DIAGNOSTIC_BYTES: usize = 1024 * 1024;
/// 诊断连接上单个字符串/BLOB 的长度上限，拦住 printf/zeroblob 生成的巨值；
/// 须大于库内实际存储的最大值（图片输入上限 24 MiB），否则读不出。
pub(crate) const DIAGNOSTIC_VALUE_BYTES: i32 = 32 * 1024 * 1024;
/// 每执行这么多条 VM 指令检查一次超时。
pub(crate) const DIAGNOSTIC_PROGRESS_OPS: i32 = 1000;
