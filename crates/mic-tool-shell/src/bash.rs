use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use mic_message::{ContentPart, ExecOutcome};
use mic_tool::{BackgroundTask, OutputSink, Tool, ToolContext, ToolError, ToolOutput};
use nix::sys::signal::{killpg, Signal};
use nix::unistd::Pid;
use regex::Regex;
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, ChildStderr, ChildStdout, Command};

use crate::limits::{BACKGROUND_TIMEOUT_MS, DEFAULT_TIMEOUT_MS, MAX_OUTPUT_BYTES, MAX_TIMEOUT_MS};
use watch::{Stream, Watch};

pub(crate) struct Bash;

const DESCRIPTION: &str = "Execute a bash command (`bash -c`) and return its stdout/stderr. \
    Each call runs in a fresh shell: no state (cwd, variables, functions) persists between calls \
    — pass `workdir` instead of using `cd`. Non-zero exits are reported as `[exit code: N]`. \
    Long output is truncated to its tail. Set `background` for long-running work (builds, \
    servers): the call returns at once with an exec_id, stdout and stderr go to a log you read \
    with `job_output`, and the exit arrives later as a completion. Background commands must not \
    daemonize themselves. With `notify_pattern`, stdout and stderr lines are matched separately \
    while the job runs (lines over 512 bytes are matched in 512-byte pieces) and a match \
    arrives as a job-notice: at most one per 10 seconds (matches in between are dropped) and at \
    most 10 per job.";

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Args {
    /// The bash command to execute.
    command: String,
    /// Label shown in the UI, written in the language the user writes in. Foreground: what the command does in active voice, 5-10 words, e.g. "git status" → "Show working tree status". Background: a short name for the job, 2-5 words, e.g. "bun run dev" → "Web dev server".
    description: String,
    /// Timeout in milliseconds; the command is killed on expiry. Foreground: defaults to 120000, values above 600000 are capped at 600000. Background: defaults to 600000, no cap, 0 = no timeout (for servers).
    #[serde(rename = "timeoutMs")]
    timeout_ms: Option<u64>,
    /// Working directory for this command. Defaults to the session workspace; a relative path is resolved against it.
    workdir: Option<String>,
    /// Run in the background and return immediately. Defaults to false.
    #[serde(default)]
    background: bool,
    /// Background only. A Rust regex (no lookaround or backreferences) matched against each output line while the job runs, e.g. "Listening on|ERROR"; a match is sent to you early as a job-notice.
    notify_pattern: Option<String>,
}

impl Tool for Bash {
    type Args = Args;

    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        DESCRIPTION
    }

    async fn execute(&self, args: Args, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        if args.command.trim().is_empty() {
            return Err(ToolError::input("command must be a non-empty string"));
        }
        if args.description.trim().is_empty() {
            return Err(ToolError::input("description must be a non-empty string"));
        }
        let timeout = parse_timeout(args.timeout_ms, args.background)?;
        let watch = parse_pattern(args.notify_pattern, args.background)?;
        let workdir = match args.workdir {
            Some(dir) => ctx.cwd().join(dir),
            None => ctx.cwd().to_path_buf(),
        };
        if !workdir.is_dir() {
            return Err(ToolError::input(format!(
                "workdir is not an existing directory: \"{}\"",
                workdir.display()
            )));
        }
        if !args.background {
            let timeout_ms = timeout.expect("前台必有超时");
            let run = run(&args.command, workdir, timeout_ms).await?;
            return Ok(ToolOutput::Completed(vec![ContentPart::Text {
                text: run.render(timeout_ms),
            }]));
        }
        let command = args.command;
        let exec_id = ctx
            .background()
            .dispatch(BackgroundTask {
                description: args.description,
                timeout: timeout.map(Duration::from_millis),
                run: Box::new(move |sink| Box::pin(run_background(command, workdir, watch, sink))),
            })
            .await?;
        Ok(ToolOutput::Dispatched { exec_id })
    }
}

/// 毫秒；`None` = 不限时（只有后台允许）。
fn parse_timeout(timeout_ms: Option<u64>, background: bool) -> Result<Option<u64>, ToolError> {
    Ok(match (timeout_ms, background) {
        (None, false) => Some(DEFAULT_TIMEOUT_MS),
        (None, true) => Some(BACKGROUND_TIMEOUT_MS),
        (Some(0), false) => return Err(ToolError::input("timeoutMs must be a positive integer")),
        (Some(0), true) => None,
        (Some(ms), false) => Some(ms.min(MAX_TIMEOUT_MS)),
        (Some(ms), true) => Some(ms),
    })
}

fn parse_pattern(pattern: Option<String>, background: bool) -> Result<Option<Watch>, ToolError> {
    let Some(pattern) = pattern else {
        return Ok(None);
    };
    if !background {
        return Err(ToolError::input("notify_pattern requires background=true"));
    }
    if pattern.is_empty() {
        return Err(ToolError::input("notify_pattern must be a non-empty regex"));
    }
    let pattern = Regex::new(&pattern)
        .map_err(|e| ToolError::input(format!("invalid notify_pattern: {e}")))?;
    Ok(Some(Watch::new(pattern)))
}

struct Run {
    status: ExitStatus,
    timed_out: bool,
    stdout: Tail,
    stderr: Tail,
}

struct Spawned {
    child: Child,
    group: ProcessGroup,
    stdout: ChildStdout,
    stderr: ChildStderr,
}

fn spawn(command: &str, workdir: PathBuf) -> Result<Spawned, ToolError> {
    let mut child = Command::new("bash")
        .arg("-c")
        .arg(command)
        .current_dir(workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| ToolError::dependency(format!("failed to start bash: {e}")))?;
    let group = ProcessGroup(Pid::from_raw(
        child.id().expect("刚启动的子进程尚未被回收") as i32
    ));
    let stdout = child.stdout.take().expect("stdout 已设为 piped");
    let stderr = child.stderr.take().expect("stderr 已设为 piped");
    Ok(Spawned {
        child,
        group,
        stdout,
        stderr,
    })
}

/// 主进程退出即杀进程组：残留的后台子进程若还握着管道，读端也能随之结束。
/// 返回 (状态, 是否超时)。
async fn wait(
    child: &mut Child,
    group: &ProcessGroup,
    timeout_ms: Option<u64>,
) -> std::io::Result<(ExitStatus, bool)> {
    let result = match timeout_ms {
        None => child.wait().await.map(|s| (s, false)),
        Some(ms) => match tokio::time::timeout(Duration::from_millis(ms), child.wait()).await {
            Ok(status) => status.map(|s| (s, false)),
            Err(_) => {
                group.kill();
                child.wait().await.map(|s| (s, true))
            }
        },
    };
    group.kill();
    result
}

async fn run(command: &str, workdir: PathBuf, timeout_ms: u64) -> Result<Run, ToolError> {
    let Spawned {
        mut child,
        group,
        stdout,
        stderr,
    } = spawn(command, workdir)?;
    let (waited, stdout, stderr) = tokio::join!(
        wait(&mut child, &group, Some(timeout_ms)),
        read_tail(stdout),
        read_tail(stderr)
    );
    let (status, timed_out) =
        waited.map_err(|e| ToolError::dependency(format!("failed to wait for bash: {e}")))?;
    Ok(Run {
        status,
        timed_out,
        stdout,
        stderr,
    })
}

/// 超时由 core 计；这里只跑到退出，stdout/stderr 按读到的顺序合并写入 sink。
async fn run_background(
    command: String,
    workdir: PathBuf,
    watch: Option<Watch>,
    mut sink: Box<dyn OutputSink>,
) -> ExecOutcome {
    let Spawned {
        mut child,
        group,
        stdout,
        stderr,
    } = match spawn(&command, workdir) {
        Ok(spawned) => spawned,
        Err(e) => return failed(e),
    };
    let pumped = async {
        let result = pump(stdout, stderr, watch, &mut *sink).await;
        if result.is_err() {
            group.kill();
        }
        result
    };
    let (waited, pumped) = tokio::join!(wait(&mut child, &group, None), pumped);
    if let Err(e) = pumped {
        return failed(ToolError::dependency(format!(
            "failed to write output: {e}"
        )));
    }
    match waited {
        Ok((status, _)) => ExecOutcome::Completed {
            output: vec![ContentPart::Text {
                text: exit_marker(status).unwrap_or_else(|| "[exit code: 0]".to_owned()),
            }],
        },
        Err(e) => failed(ToolError::dependency(format!(
            "failed to wait for bash: {e}"
        ))),
    }
}

fn failed(e: ToolError) -> ExecOutcome {
    ExecOutcome::Failed {
        kind: e.kind,
        message: e.message,
    }
}

/// 读错误视同 EOF，同前台。先写日志再匹配；通知落盘后才继续读。
async fn pump(
    mut stdout: ChildStdout,
    mut stderr: ChildStderr,
    mut watch: Option<Watch>,
    sink: &mut dyn OutputSink,
) -> std::io::Result<()> {
    let (mut out_buf, mut err_buf) = ([0u8; 8192], [0u8; 8192]);
    let (mut out_open, mut err_open) = (true, true);
    while out_open || err_open {
        let (stream, read) = tokio::select! {
            r = stdout.read(&mut out_buf), if out_open => (Stream::Stdout, r),
            r = stderr.read(&mut err_buf), if err_open => (Stream::Stderr, r),
        };
        let (buf, open) = match stream {
            Stream::Stdout => (&out_buf, &mut out_open),
            Stream::Stderr => (&err_buf, &mut err_open),
        };
        let notice = match read {
            Ok(n) if n > 0 => {
                sink.write(&buf[..n]).await?;
                watch.as_mut().and_then(|w| w.feed(stream, &buf[..n]))
            }
            _ => {
                *open = false;
                watch.as_mut().and_then(|w| w.finish(stream))
            }
        };
        if let Some(text) = notice {
            sink.notify(&text).await?;
        }
    }
    Ok(())
}

/// 丢弃时对整个进程组发 SIGKILL：覆盖取消（future 被丢弃）这一路径。
struct ProcessGroup(Pid);

impl ProcessGroup {
    fn kill(&self) {
        // 进程组已空时返回 ESRCH，无需处理。
        let _ = killpg(self.0, Signal::SIGKILL);
    }
}

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        self.kill();
    }
}

/// 一路输出的尾部。
struct Tail {
    bytes: Vec<u8>,
    truncated: bool,
}

async fn read_tail(mut stream: impl AsyncRead + Unpin) -> Tail {
    let mut bytes = Vec::new();
    let mut truncated = false;
    let mut buf = [0u8; 8192];
    // 读错误视同 EOF：已读到的部分照常返回。
    while let Ok(n) = stream.read(&mut buf).await {
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&buf[..n]);
        if bytes.len() > 2 * MAX_OUTPUT_BYTES {
            bytes.drain(..bytes.len() - MAX_OUTPUT_BYTES);
            truncated = true;
        }
    }
    if bytes.len() > MAX_OUTPUT_BYTES {
        bytes.drain(..bytes.len() - MAX_OUTPUT_BYTES);
        truncated = true;
    }
    Tail { bytes, truncated }
}

impl Tail {
    fn text(&self) -> String {
        if !self.truncated {
            return String::from_utf8_lossy(&self.bytes).into_owned();
        }
        // 截断点可能落在多字节字符中间，跳过开头的续字节。
        let start = self
            .bytes
            .iter()
            .position(|b| (b & 0xC0) != 0x80)
            .unwrap_or(self.bytes.len());
        format!(
            "[output truncated; showing the last {MAX_OUTPUT_BYTES} bytes]\n{}",
            String::from_utf8_lossy(&self.bytes[start..])
        )
    }
}

impl Run {
    fn render(&self, timeout_ms: u64) -> String {
        let mut body = self.stdout.text();
        let stderr = self.stderr.text();
        if !stderr.is_empty() {
            if !body.is_empty() && !body.ends_with('\n') {
                body.push('\n');
            }
            body.push_str("[stderr]\n");
            body.push_str(&stderr);
        }
        if body.is_empty() {
            body.push_str("(no output)");
        }

        let mut markers = Vec::new();
        if self.timed_out {
            markers.push(format!("[timed out after {timeout_ms}ms]"));
        }
        markers.extend(exit_marker(self.status));
        if !markers.is_empty() {
            if !body.ends_with('\n') {
                body.push('\n');
            }
            body.push_str(&markers.join("\n"));
        }
        body
    }
}

/// 正常退出（code 0）为 `None`。
fn exit_marker(status: ExitStatus) -> Option<String> {
    match (status.signal(), status.code()) {
        (Some(sig), _) => Some(format!("[killed by signal: {}]", signal_name(sig))),
        (None, Some(code)) if code != 0 => Some(format!("[exit code: {code}]")),
        _ => None,
    }
}

fn signal_name(sig: i32) -> String {
    Signal::try_from(sig).map_or_else(|_| sig.to_string(), |s| s.as_str().to_owned())
}

mod watch;

#[cfg(test)]
mod tests;
