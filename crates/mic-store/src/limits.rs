//! build 后不改的旋钮。

/// 会话列表预览截取的字符数。
pub(crate) const PREVIEW_CHARS: u32 = 80;
/// 诊断查询：执行时间、返回行数与结果文本字节上限。
pub(crate) const DIAGNOSTIC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
pub(crate) const DIAGNOSTIC_ROWS: usize = 200;
pub(crate) const DIAGNOSTIC_BYTES: usize = 1024 * 1024;
/// 每执行这么多条 VM 指令检查一次超时。
pub(crate) const DIAGNOSTIC_PROGRESS_OPS: i32 = 1000;
