//! build 后不改的旋钮。

use std::time::Duration;

/// `read` 缺省行数，也是上限。
pub(crate) const READ_LIMIT: u64 = 2000;
/// `read` 单行保留的字符数。
pub(crate) const READ_MAX_LINE_CHARS: usize = 2000;
/// `read` 一次输出的行内容字节上限。
pub(crate) const READ_MAX_BYTES: usize = 50 * 1024;
/// `glob` 输出的路径数上限。
pub(crate) const GLOB_MAX_RESULTS: usize = 100;
/// `grep` 输出的匹配数上限。
pub(crate) const GREP_MAX_MATCHES: usize = 250;
/// `grep` 单行保留的字节数。
pub(crate) const GREP_MAX_LINE_BYTES: usize = 2000;
/// `grep` 搜索超时。
pub(crate) const SEARCH_TIMEOUT: Duration = Duration::from_secs(30);
/// 判定二进制时检查的文件头长度。
pub(crate) const BINARY_SNIFF_BYTES: usize = 8 * 1024;
