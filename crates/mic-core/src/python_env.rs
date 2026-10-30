//! 命令里的 Python：数据目录下共享的 uv venv；没有 uv 或创建失败时回退系统 python。
//! 契约：docs/blueprints/python-env.md。

use std::path::{Path, PathBuf};

use crate::limits::{PYTHON_ENV_TIMEOUT, UV_DEFAULT_INDEX};

const PYTHON_DIR: &str = "python";

/// 启动时确定，之后不变。
pub(crate) struct PythonEnv {
    pub(crate) dir: PathBuf,
    /// 注入命令的 `PATH`、`VIRTUAL_ENV`，进程未设时加 `UV_DEFAULT_INDEX`。
    pub(crate) vars: Vec<(String, String)>,
}

pub(crate) async fn prepare(data_dir: &Path) -> Option<PythonEnv> {
    let dir = data_dir.join(PYTHON_DIR);
    if let Err(reason) = ensure(&dir).await {
        tracing::warn!(dir = %dir.display(), reason, "commands fall back to system python");
        return None;
    }
    let bin = std::iter::once(dir.join("bin"));
    let inherited = std::env::var_os("PATH");
    let path = std::env::join_paths(bin.chain(inherited.iter().flat_map(std::env::split_paths)))
        .expect("venv 路径与继承的 PATH 不含分隔符");
    let mut vars = vec![
        ("PATH".to_owned(), path.to_string_lossy().into_owned()),
        ("VIRTUAL_ENV".to_owned(), dir.display().to_string()),
    ];
    if std::env::var_os("UV_DEFAULT_INDEX").is_none() {
        vars.push(("UV_DEFAULT_INDEX".to_owned(), UV_DEFAULT_INDEX.to_owned()));
    }
    Some(PythonEnv { dir, vars })
}

async fn ensure(dir: &Path) -> Result<(), String> {
    let uv = std::env::var_os("PATH")
        .iter()
        .flat_map(std::env::split_paths)
        .map(|p| p.join("uv"))
        .find(|p| p.is_file())
        .ok_or("uv not found on PATH")?;
    if dir.join("pyvenv.cfg").is_file() {
        return Ok(());
    }
    let run = tokio::process::Command::new(&uv)
        .arg("venv")
        .arg("--quiet")
        .arg(dir)
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(PYTHON_ENV_TIMEOUT, run)
        .await
        .map_err(|_| format!("uv venv timed out after {}s", PYTHON_ENV_TIMEOUT.as_secs()))?
        .map_err(|e| format!("failed to run {}: {e}", uv.display()))?;
    if !output.status.success() {
        return Err(format!(
            "uv venv exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}
