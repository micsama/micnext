//! 装配根：读配置、组装模块、信号、日志。契约：docs/blueprints/mic-core-module.md §四、§七。
//! 面向用户的提示与错误用中文直接写 stderr；运行日志走 tracing。

use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use mic_core::{Assembly, Module};
use tokio::signal::unix::{signal, SignalKind};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::filter::{EnvFilter, LevelFilter};

const DEFAULT_CONFIG: &str = include_str!("default-config.toml");

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("错误：{e}");
            for cause in e.chain().skip(1) {
                eprintln!(
                    "  原因：{}",
                    cause.to_string().trim_end().replace('\n', "\n        ")
                );
            }
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env()
                .context("环境变量 RUST_LOG 格式不对")?,
        )
        .init();

    let (path, text) = load_config()?;
    let config: toml::Table = text
        .parse()
        .with_context(|| format!("配置文件 {} 不是合法的 TOML", path.display()))?;

    let modules: Vec<Box<dyn Module>> = Vec::new();
    let assembly = Assembly::new(modules, config)
        .with_context(|| format!("配置文件 {} 有误", path.display()))?;

    let stop = CancellationToken::new();
    let mut int = signal(SignalKind::interrupt()).context("无法注册 SIGINT 处理")?;
    let mut term = signal(SignalKind::terminate()).context("无法注册 SIGTERM 处理")?;
    let trigger = stop.clone();
    tokio::spawn(async move {
        tokio::select! {
            _ = int.recv() => {}
            _ = term.recv() => {}
        }
        tracing::info!("stopping");
        trigger.cancel();
    });

    assembly.run(stop).await?;
    tracing::info!("stopped");
    Ok(())
}

/// 显式 `--config` 缺文件即报错；默认位置缺文件则生成模板并提示。
fn load_config() -> Result<(PathBuf, String)> {
    let mut args = std::env::args_os().skip(1);
    let path = match (args.next(), args.next(), args.next()) {
        (None, _, _) => default_config_path()?,
        (Some(flag), Some(path), None) if flag == "--config" => {
            let path = PathBuf::from(path);
            return match std::fs::read_to_string(&path) {
                Ok(text) => Ok((path, text)),
                Err(e) if e.kind() == ErrorKind::NotFound => {
                    bail!("配置文件 {} 不存在，请检查 --config 路径", path.display())
                }
                Err(e) => Err(e).with_context(|| format!("无法读取配置文件 {}", path.display())),
            };
        }
        _ => bail!("参数不对。用法：micnext [--config <配置文件路径>]"),
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok((path, text)),
        Err(e) if e.kind() == ErrorKind::NotFound => {
            write_default_config(&path)?;
            eprintln!(
                "未找到配置文件，已生成默认配置：\n  {}\n按需编辑后重启生效。\n",
                path.display()
            );
            Ok((path, DEFAULT_CONFIG.to_owned()))
        }
        Err(e) => Err(e).with_context(|| format!("无法读取配置文件 {}", path.display())),
    }
}

/// 权限 600：以后会存模型 key。
fn write_default_config(path: &Path) -> Result<()> {
    let ctx = || format!("无法生成默认配置 {}", path.display());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(ctx)?;
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .and_then(|mut f| f.write_all(DEFAULT_CONFIG.as_bytes()))
        .with_context(ctx)
}

/// `$XDG_CONFIG_HOME/micnext/config.toml` 或 `~/.config/micnext/config.toml`。
fn default_config_path() -> Result<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(d) if !d.is_empty() => PathBuf::from(d),
        _ => PathBuf::from(
            std::env::var_os("HOME")
                .filter(|h| !h.is_empty())
                .context("环境变量 HOME 未设置")?,
        )
        .join(".config"),
    };
    Ok(base.join("micnext/config.toml"))
}
