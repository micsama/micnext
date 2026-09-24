use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use mic_message::ContentPart;
use mic_tool::{Tool, ToolContext, ToolError};
use nix::sys::signal::{killpg, Signal};
use nix::unistd::Pid;
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

use crate::limits::{DEFAULT_TIMEOUT_MS, MAX_OUTPUT_BYTES, MAX_TIMEOUT_MS};

pub(crate) struct Bash;

const DESCRIPTION: &str = "Execute a bash command (`bash -c`) and return its stdout/stderr. \
    Each call runs in a fresh shell: no state (cwd, variables, functions) persists between calls \
    — pass `workdir` instead of using `cd`. Non-zero exits are reported as `[exit code: N]`. \
    Long output is truncated to its tail. Background execution is not available; \
    long-running commands must finish within the timeout.";

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Args {
    /// The bash command to execute.
    command: String,
    /// Clear, concise description of what this command does in active voice, 5-10 words (shown in the UI). Examples: "ls" → "List files in current directory"; "git status" → "Show working tree status"; "npm install" → "Install package dependencies".
    description: String,
    /// Timeout in milliseconds. Defaults to 120000; values above 600000 are capped at 600000. The command is killed on expiry.
    #[serde(rename = "timeoutMs")]
    timeout_ms: Option<u64>,
    /// Working directory for this command. Defaults to the session workspace; a relative path is resolved against it.
    workdir: Option<String>,
}

impl Tool for Bash {
    type Args = Args;

    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        DESCRIPTION
    }

    fn prompt_hint(&self) -> Option<&str> {
        Some("Check the [exit code: N] marker on every bash result; investigate failures before moving on.")
    }

    async fn execute(&self, args: Args, ctx: &ToolContext) -> Result<Vec<ContentPart>, ToolError> {
        if args.command.trim().is_empty() {
            return Err(ToolError::input("command must be a non-empty string"));
        }
        if args.description.trim().is_empty() {
            return Err(ToolError::input("description must be a non-empty string"));
        }
        let timeout_ms = match args.timeout_ms {
            Some(0) => return Err(ToolError::input("timeoutMs must be a positive integer")),
            Some(ms) => ms.min(MAX_TIMEOUT_MS),
            None => DEFAULT_TIMEOUT_MS,
        };
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
        let run = run(&args.command, workdir, timeout_ms).await?;
        Ok(vec![ContentPart::Text {
            text: run.render(timeout_ms),
        }])
    }
}

struct Run {
    status: ExitStatus,
    timed_out: bool,
    stdout: Tail,
    stderr: Tail,
}

async fn run(command: &str, workdir: PathBuf, timeout_ms: u64) -> Result<Run, ToolError> {
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

    // 主进程退出即杀进程组：残留的后台子进程若还握着管道，读端也能随之结束。
    let wait = async {
        let result =
            match tokio::time::timeout(Duration::from_millis(timeout_ms), child.wait()).await {
                Ok(status) => status.map(|s| (s, false)),
                Err(_) => {
                    group.kill();
                    child.wait().await.map(|s| (s, true))
                }
            };
        group.kill();
        result
    };
    let (waited, stdout, stderr) = tokio::join!(wait, read_tail(stdout), read_tail(stderr));
    let (status, timed_out) =
        waited.map_err(|e| ToolError::dependency(format!("failed to wait for bash: {e}")))?;
    Ok(Run {
        status,
        timed_out,
        stdout,
        stderr,
    })
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
        match (self.status.signal(), self.status.code()) {
            (Some(sig), _) => markers.push(format!("[killed by signal: {}]", signal_name(sig))),
            (None, Some(code)) if code != 0 => markers.push(format!("[exit code: {code}]")),
            _ => {}
        }
        if !markers.is_empty() {
            if !body.ends_with('\n') {
                body.push('\n');
            }
            body.push_str(&markers.join("\n"));
        }
        body
    }
}

fn signal_name(sig: i32) -> String {
    Signal::try_from(sig).map_or_else(|_| sig.to_string(), |s| s.as_str().to_owned())
}
