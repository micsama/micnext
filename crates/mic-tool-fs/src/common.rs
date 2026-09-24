//! 各文件工具共用的路径、阻塞执行、错误分类与文本规则（tools-basic §四）。

use std::fs::Permissions;
use std::io::{self, Write as _};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use mic_tool::{ToolContext, ToolError};

use crate::limits::BINARY_SNIFF_BYTES;

/// 相对路径以会话 cwd 为基准，绝对路径原样使用。
pub(crate) fn resolve(ctx: &ToolContext, path: &str) -> Result<PathBuf, ToolError> {
    if path.trim().is_empty() {
        return Err(ToolError::input("path must be a non-empty string"));
    }
    Ok(ctx.cwd().join(path))
}

/// 在 cwd 之下显示为相对路径，否则显示绝对路径。
pub(crate) fn display_path(cwd: &Path, path: &Path) -> String {
    match path.strip_prefix(cwd) {
        Ok(rel) if rel.as_os_str().is_empty() => ".".to_owned(),
        Ok(rel) => rel.display().to_string(),
        Err(_) => path.display().to_string(),
    }
}

/// 阻塞工作的取消标志：调用方 future 被丢弃后置位。
#[derive(Clone)]
pub(crate) struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub(crate) fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

struct CancelOnDrop(Cancel);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0 .0.store(true, Ordering::Relaxed);
    }
}

/// 在阻塞线程池上执行 `f`；丢弃返回的 future 会置位取消标志，`f` 应逐文件检查后尽快返回。
pub(crate) async fn blocking<T: Send + 'static>(f: impl FnOnce(Cancel) -> T + Send + 'static) -> T {
    let cancel = Cancel(Arc::new(AtomicBool::new(false)));
    let guard = CancelOnDrop(cancel.clone());
    let result = tokio::task::spawn_blocking(move || f(cancel)).await;
    drop(guard);
    result.unwrap_or_else(|e| std::panic::resume_unwind(e.into_panic()))
}

/// I/O 失败分类：不存在是参数问题，其余是环境问题。
pub(crate) fn io_error(action: &str, display: &str, e: io::Error) -> ToolError {
    match e.kind() {
        io::ErrorKind::NotFound => {
            ToolError::input(format!("cannot {action} \"{display}\": not found"))
        }
        io::ErrorKind::PermissionDenied => {
            ToolError::dependency(format!("cannot {action} \"{display}\": permission denied"))
        }
        _ => ToolError::dependency(format!("cannot {action} \"{display}\": {e}")),
    }
}

/// 前 `BINARY_SNIFF_BYTES` 字节含 NUL 视为二进制。
pub(crate) fn is_binary(head: &[u8]) -> bool {
    head[..head.len().min(BINARY_SNIFF_BYTES)].contains(&0)
}

/// 整个文件解码为 UTF-8 文本；二进制或非 UTF-8 → `business`。
pub(crate) fn decode(bytes: Vec<u8>, action: &str, display: &str) -> Result<String, ToolError> {
    if is_binary(&bytes) {
        return Err(not_text(action, display, "binary file"));
    }
    String::from_utf8(bytes).map_err(|_| not_text(action, display, "not valid UTF-8"))
}

pub(crate) fn not_text(action: &str, display: &str, why: &str) -> ToolError {
    ToolError::business(format!("cannot {action} \"{display}\": {why}"))
}

/// 同目录临时文件写入后 rename；覆盖时保留原文件权限位，父目录不存在则创建。
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .expect("解析后的文件路径是绝对路径，必有父目录");
    std::fs::create_dir_all(parent)?;
    let existing = match std::fs::metadata(path) {
        Ok(meta) => Some(meta.permissions()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    // 新文件按 0666 创建、由 umask 收窄，与普通新建文件一致；覆盖时显式恢复原权限位。
    let mut tmp = tempfile::Builder::new()
        .permissions(Permissions::from_mode(0o666))
        .tempfile_in(parent)?;
    tmp.write_all(bytes)?;
    if let Some(permissions) = existing {
        tmp.as_file().set_permissions(permissions)?;
    }
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}
