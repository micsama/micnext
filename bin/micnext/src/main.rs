//! 装配根：读配置、组装模块、信号、日志、`-p` 呈现。
//! 契约：docs/blueprints/mic-core-module.md §四、§七，docs/blueprints/run-execution.md §六。
//! 面向用户的提示与错误用中文直接写 stderr；运行日志走 tracing。

use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use mic_core::{Assembly, KernelEvent, KernelEventKind, Module, OnceOutcome, OneShot};
use mic_message::{ContentPart, ExecOutcome, Message, MessageBody, ReplyBlock, ToolResultOutcome};
use mic_store::RunState;
use tokio::signal::unix::{signal, SignalKind};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::filter::{EnvFilter, LevelFilter};

const DEFAULT_CONFIG: &str = include_str!("default-config.toml");
/// 模型 API key 的主密钥文件名，缺省放在配置文件旁。
const KEY_FILE_NAME: &str = "master.key";
const USAGE: &str = "用法：micnext [--config <配置文件路径>] [-p <提示词>]";
/// `-p` 呈现工具参数与结果时的截断长度（字符）。
const PREVIEW_CHARS: usize = 200;

enum Mode {
    /// 常驻。
    Serve,
    /// `-p`：一次性执行这段提示词。
    Once(String),
}

struct Args {
    config: Option<PathBuf>,
    mode: Mode,
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(code) => code,
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

async fn run() -> Result<ExitCode> {
    let args = parse_args()?;
    // `-p` 调低默认级别，免得日志混进过程输出。
    let level = match args.mode {
        Mode::Serve => LevelFilter::INFO,
        Mode::Once(_) => LevelFilter::WARN,
    };
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(level.into())
                .from_env()
                .context("环境变量 RUST_LOG 格式不对")?,
        )
        .init();

    let (path, text) = load_config(args.config)?;
    let mut config: toml::Table = text
        .parse()
        .with_context(|| format!("配置文件 {} 不是合法的 TOML", path.display()))?;
    default_key_file(&mut config, &path)
        .with_context(|| format!("配置文件 {} 有误", path.display()))?;

    let modules: Vec<Box<dyn Module>> = vec![
        Box::new(mic_gateway::GatewayModule),
        Box::new(mic_provider_openai::OpenAiModule),
        Box::new(mic_tool_shell::ShellModule),
        Box::new(mic_tool_fs::FsModule),
        Box::new(mic_tool_web_fetch::WebFetchModule),
    ];
    #[cfg(feature = "wechat")]
    let modules = {
        let mut modules = modules;
        modules.push(Box::new(mic_channel_wechat::WechatModule));
        modules
    };
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

    match args.mode {
        Mode::Serve => {
            assembly.run(stop).await?;
            tracing::info!("stopped");
            Ok(ExitCode::SUCCESS)
        }
        Mode::Once(prompt) => {
            let pwd = std::env::current_dir().context("无法读取当前目录")?;
            if pwd.to_str().is_none() {
                bail!("当前目录 {} 不是 UTF-8 路径", pwd.display());
            }
            let mut render = Render::default();
            let outcome = assembly
                .run_once(OneShot { prompt, pwd }, |e| render.on(e), stop)
                .await?;
            render.end_line();
            Ok(match outcome {
                OnceOutcome::Finished {
                    state: RunState::Completed,
                    ..
                } => ExitCode::SUCCESS,
                OnceOutcome::Finished { state, .. } => {
                    eprintln!("本轮未成功结束：{state:?}");
                    ExitCode::FAILURE
                }
                OnceOutcome::Stopped { session_id } => {
                    eprintln!("已中断（会话 {}），下次启动时收尾。", session_id.0);
                    ExitCode::from(130)
                }
            })
        }
    }
}

/// `--config` 与 `-p` 顺序任意，各至多一次。
fn parse_args() -> Result<Args> {
    let mut config = None;
    let mut prompt = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(flag) = args.next() {
        let value = |args: &mut dyn Iterator<Item = OsString>| {
            args.next()
                .with_context(|| format!("{} 缺少取值。{USAGE}", flag.to_string_lossy()))
        };
        if flag == "--config" && config.is_none() {
            config = Some(PathBuf::from(value(&mut args)?));
        } else if flag == "-p" && prompt.is_none() {
            let text = value(&mut args)?
                .into_string()
                .map_err(|_| anyhow::anyhow!("-p 的提示词不是 UTF-8"))?;
            if text.trim().is_empty() {
                bail!("-p 的提示词为空。{USAGE}");
            }
            prompt = Some(text);
        } else {
            bail!("参数不对：{}。{USAGE}", flag.to_string_lossy());
        }
    }
    Ok(Args {
        config,
        mode: prompt.map_or(Mode::Serve, Mode::Once),
    })
}

/// `-p` 的过程呈现：正文到 stdout，其余到 stderr。
#[derive(Default)]
struct Render {
    /// stdout / stderr 上各有一段未换行的流式输出。
    text_open: bool,
    reasoning_open: bool,
}

impl Render {
    fn on(&mut self, event: &KernelEvent) {
        match &event.kind {
            KernelEventKind::RunStarted { run_id } => {
                eprintln!("[session {} · run {}]", event.session_id.0, run_id.0)
            }
            KernelEventKind::TextDelta(text) => {
                if self.reasoning_open {
                    eprintln!();
                    self.reasoning_open = false;
                }
                print!("{text}");
                let _ = std::io::stdout().flush();
                self.text_open = true;
            }
            KernelEventKind::ReasoningDelta(text) => {
                eprint!("{text}");
                self.reasoning_open = true;
            }
            KernelEventKind::DraftDiscarded => self.end_line(),
            KernelEventKind::MessageAppended(m) => self.message(m),
            KernelEventKind::RunFinished { .. } => {}
        }
    }

    fn message(&mut self, m: &Message) {
        let line = match &m.body {
            MessageBody::Reply { blocks, .. } => {
                self.end_line();
                for block in blocks {
                    if let ReplyBlock::ToolCall { name, args, .. } = block {
                        eprintln!("▶ {name} {}", preview(&args.to_string()));
                    }
                }
                return;
            }
            MessageBody::ToolResult {
                tool_name, outcome, ..
            } => {
                let status = match outcome {
                    ToolResultOutcome::Terminal(ExecOutcome::Completed { output }) => {
                        let first = match output.first() {
                            Some(ContentPart::Text { text }) => text.lines().next().unwrap_or(""),
                            Some(ContentPart::File(f)) => f.path.as_str(),
                            Some(ContentPart::Image(_)) => "[image]",
                            None => "",
                        };
                        format!("ok {}", preview(first))
                    }
                    ToolResultOutcome::Terminal(ExecOutcome::Failed { kind, message }) => {
                        format!("failed kind={} {}", kind.as_str(), preview(message))
                    }
                    ToolResultOutcome::Terminal(ExecOutcome::Cancelled { message }) => {
                        format!("cancelled {}", preview(message))
                    }
                    ToolResultOutcome::Dispatched { exec_id } => format!("dispatched {exec_id}"),
                };
                format!("◀ {tool_name} {status}")
            }
            MessageBody::HarnessNote { text } => format!("[runtime-note] {text}"),
            MessageBody::Notification { source, text } => {
                format!("[notification {source}] {text}")
            }
            _ => return,
        };
        self.end_line();
        eprintln!("{line}");
    }

    fn end_line(&mut self) {
        if self.text_open {
            println!();
            self.text_open = false;
        }
        if self.reasoning_open {
            eprintln!();
            self.reasoning_open = false;
        }
    }
}

fn preview(s: &str) -> String {
    let first = s.lines().next().unwrap_or("");
    match first.char_indices().nth(PREVIEW_CHARS) {
        Some((i, _)) => format!("{}…", &first[..i]),
        None if first.len() < s.len() => format!("{first}…"),
        None => first.to_owned(),
    }
}

/// 显式 `--config` 缺文件即报错；默认位置缺文件则生成模板并提示。
fn load_config(explicit: Option<PathBuf>) -> Result<(PathBuf, String)> {
    let path = match explicit {
        None => default_config_path()?,
        Some(path) => {
            return match std::fs::read_to_string(&path) {
                Ok(text) => Ok((path, text)),
                Err(e) if e.kind() == ErrorKind::NotFound => {
                    bail!("配置文件 {} 不存在，请检查 --config 路径", path.display())
                }
                Err(e) => Err(e).with_context(|| format!("无法读取配置文件 {}", path.display())),
            };
        }
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

/// 权限 600。
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

/// `[core]` 没写 `key_file` 时补成配置文件旁的 `master.key`。
fn default_key_file(config: &mut toml::Table, config_path: &Path) -> Result<()> {
    let core = config
        .entry("core")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .context("[core] 必须是表")?;
    if !core.contains_key("key_file") {
        let abs = std::path::absolute(config_path).context("无法解析配置文件的绝对路径")?;
        let key = abs.with_file_name(KEY_FILE_NAME);
        let key = key.to_str().context("配置文件路径不是 UTF-8")?;
        core.insert("key_file".into(), toml::Value::String(key.to_owned()));
    }
    Ok(())
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
