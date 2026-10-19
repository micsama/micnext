//! 一键更新：原地 fetch + 覆盖到 upstream → build.sh → 请装配根 exec。契约：docs/blueprints/self-update.md。

use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};
use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::error::ApiError;
use crate::limits::{PULL_TIMEOUTS, UPDATE_OUTPUT_BYTES};
use crate::service::App;

/// 本程序构建时的提交；由 build.sh 注入，直接 cargo build 时为 `None`。
pub(crate) const COMMIT: Option<&str> = option_env!("MICNEXT_COMMIT");

/// 构建成功后请装配根停止并 exec 的目标。
pub struct RestartRequest {
    pub executable: PathBuf,
}

pub(crate) struct Updater {
    repo: PathBuf,
    restart: mpsc::Sender<RestartRequest>,
    /// 本进程二进制的字节数，启动时读定（构建会原地替换同一路径）。
    running_size: Option<u64>,
    state: Mutex<Task>,
}

struct Task {
    phase: Phase,
    /// 本次任务全部命令的合并输出末尾。
    output: String,
}

enum Phase {
    Idle,
    Checking,
    Pulling {
        from: String,
        attempt: usize,
    },
    Building {
        from: String,
        to: String,
    },
    Restarting {
        from: String,
        to: String,
        to_size: u64,
    },
    UpToDate {
        commit: String,
    },
    Failed {
        stage: Stage,
        from: Option<String>,
        error: String,
    },
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum Stage {
    Checking,
    Pulling,
    Building,
    Restarting,
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum StatusView<'a> {
    Unavailable,
    Idle {
        running: Option<&'static str>,
        size: Option<u64>,
    },
    Checking {
        output: &'a str,
    },
    Pulling {
        from: &'a str,
        attempt: usize,
        output: &'a str,
    },
    Building {
        from: &'a str,
        to: &'a str,
        from_size: Option<u64>,
        output: &'a str,
    },
    Restarting {
        from: &'a str,
        to: &'a str,
        from_size: Option<u64>,
        to_size: u64,
    },
    UpToDate {
        commit: &'a str,
    },
    Failed {
        stage: Stage,
        from: Option<&'a str>,
        error: &'a str,
        output: &'a str,
    },
}

impl Phase {
    fn running(&self) -> Option<(Stage, Option<&str>)> {
        match self {
            Self::Checking => Some((Stage::Checking, None)),
            Self::Pulling { from, .. } => Some((Stage::Pulling, Some(from))),
            Self::Building { from, .. } => Some((Stage::Building, Some(from))),
            Self::Restarting { from, .. } => Some((Stage::Restarting, Some(from))),
            Self::Idle | Self::UpToDate { .. } | Self::Failed { .. } => None,
        }
    }
}

impl Updater {
    pub(crate) fn new(repo: PathBuf, restart: mpsc::Sender<RestartRequest>) -> Self {
        Self {
            repo,
            restart,
            running_size: std::env::current_exe()
                .and_then(std::fs::metadata)
                .map(|m| m.len())
                .ok(),
            state: Mutex::new(Task {
                phase: Phase::Idle,
                output: String::new(),
            }),
        }
    }

    fn view<R>(&self, f: impl FnOnce(StatusView<'_>) -> R) -> R {
        let state = self.state.lock().unwrap();
        let output = state.output.as_str();
        f(match &state.phase {
            Phase::Idle => StatusView::Idle {
                running: COMMIT,
                size: self.running_size,
            },
            Phase::Checking => StatusView::Checking { output },
            Phase::Pulling { from, attempt } => StatusView::Pulling {
                from,
                attempt: *attempt,
                output,
            },
            Phase::Building { from, to } => StatusView::Building {
                from,
                to,
                from_size: self.running_size,
                output,
            },
            Phase::Restarting { from, to, to_size } => StatusView::Restarting {
                from,
                to,
                from_size: self.running_size,
                to_size: *to_size,
            },
            Phase::UpToDate { commit } => StatusView::UpToDate { commit },
            Phase::Failed { stage, from, error } => StatusView::Failed {
                stage: *stage,
                from: from.as_deref(),
                error,
                output,
            },
        })
    }

    pub(crate) fn start(self: &Arc<Self>) -> Result<(), ApiError> {
        {
            let mut state = self.state.lock().unwrap();
            if state.phase.running().is_some() {
                return Err(ApiError::Conflict("已有更新在进行".into()));
            }
            *state = Task {
                phase: Phase::Checking,
                output: String::new(),
            };
        }
        tracing::info!(repo = %self.repo.display(), "update started");
        tokio::spawn(self.clone().run());
        Ok(())
    }

    async fn run(self: Arc<Self>) {
        let Err(error) = self.steps().await else {
            return;
        };
        let mut state = self.state.lock().unwrap();
        let (stage, from) = state.phase.running().expect("失败只发生在运行阶段");
        let from = from.map(str::to_owned);
        tracing::warn!(?from, %error, "update failed");
        state.phase = Phase::Failed { stage, from, error };
    }

    async fn steps(&self) -> Result<(), String> {
        self.capture(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])
            .await
            .map_err(|e| format!("当前分支没有 upstream：{e}"))?;
        let from = self.capture(&["rev-parse", "HEAD"]).await?;
        tracing::info!(%from, "update pulling");

        self.fetch(&from).await?;
        // 服务器上不手改代码：本地改动与提交一律以 upstream 覆盖，被覆盖的列在输出里。
        let dirty = self
            .capture(&["status", "--porcelain", "--untracked-files=no"])
            .await?;
        if !dirty.is_empty() {
            self.append(&format!("以下本地改动将被覆盖：\n{dirty}"));
        }
        let ahead = self.capture(&["log", "--oneline", "@{u}..HEAD"]).await?;
        if !ahead.is_empty() {
            self.append(&format!("以下本地提交将被丢弃：\n{ahead}"));
        }
        self.append("$ git reset --hard @{u}\n");
        let reset = self.capture(&["reset", "--hard", "@{u}"]).await?;
        self.append(&reset);

        let to = self.capture(&["rev-parse", "HEAD"]).await?;
        if to == from {
            tracing::info!(commit = %to, "update: already up to date");
            self.set(Phase::UpToDate { commit: to });
            return Ok(());
        }
        let mut diff = Command::new("git");
        diff.args(["diff", "--stat", &from, &to]);
        self.stream(diff)
            .await
            .map_err(|e| format!("无法运行 git：{e}"))?;

        self.set(Phase::Building {
            from: from.clone(),
            to: to.clone(),
        });
        tracing::info!(%from, %to, "update building");
        let status = self
            .stream(Command::new("./build.sh"))
            .await
            .map_err(|e| format!("无法运行 build.sh：{e}"))?;
        if !status.success() {
            return Err(format!("构建失败（{status}），旧服务继续运行"));
        }

        let executable = self.repo.join("target/release/micnext");
        let to_size = std::fs::metadata(&executable)
            .map_err(|e| format!("读取构建产物 {} 失败：{e}", executable.display()))?
            .len();
        self.set(Phase::Restarting {
            from: from.clone(),
            to: to.clone(),
            to_size,
        });
        tracing::info!(%from, %to, from_size = ?self.running_size, to_size, "update restarting");
        self.restart
            .try_send(RestartRequest { executable })
            .map_err(|e| format!("无法通知重启：{e}"))
    }

    async fn fetch(&self, from: &str) -> Result<(), String> {
        // 以半次为单位计重试机会。
        let budget = PULL_TIMEOUTS.len() * 2;
        let mut spent = 0;
        let mut attempt = 0;
        while spent < budget {
            attempt += 1;
            let timeout = PULL_TIMEOUTS[spent / 2];
            self.set(Phase::Pulling {
                from: from.to_owned(),
                attempt,
            });
            self.append(&format!(
                "$ git fetch（第 {attempt} 次，剩余 {} 次机会）\n",
                halves(budget - spent)
            ));
            let mut git = Command::new("git");
            git.arg("fetch");
            match tokio::time::timeout(timeout, self.stream(git)).await {
                Ok(Ok(status)) if status.success() => return Ok(()),
                Ok(Ok(status)) => {
                    spent += 1;
                    self.append(&format!("git fetch 失败（{status}），扣半次\n"));
                }
                Ok(Err(e)) => return Err(format!("无法运行 git：{e}")),
                Err(_) => {
                    spent += 2;
                    self.append(&format!(
                        "git fetch {} 秒未完成，已终止\n",
                        timeout.as_secs()
                    ));
                }
            }
        }
        Err(format!(
            "git fetch {} 次重试机会用完仍失败，请检查网络或启动前是否开了代理",
            PULL_TIMEOUTS.len()
        ))
    }

    /// 输出实时并入末尾缓冲；future 被丢弃时子进程随之终止。
    async fn stream(&self, mut cmd: Command) -> std::io::Result<ExitStatus> {
        let mut child = cmd
            .current_dir(&self.repo)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let stdout = child.stdout.take().expect("已设为 piped");
        let stderr = child.stderr.take().expect("已设为 piped");
        let (status, (), ()) = tokio::join!(child.wait(), self.pump(stdout), self.pump(stderr));
        status
    }

    async fn pump(&self, pipe: impl AsyncRead + Unpin) {
        let mut reader = BufReader::new(pipe);
        let mut line = Vec::new();
        while matches!(reader.read_until(b'\n', &mut line).await, Ok(n) if n > 0) {
            self.append(&String::from_utf8_lossy(&line));
            line.clear();
        }
    }

    /// 只取 stdout（去首尾空白）；非零退出时以 stderr 作原因。
    async fn capture(&self, args: &[&str]) -> Result<String, String> {
        let out = Command::new("git")
            .args(args)
            .current_dir(&self.repo)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|e| format!("无法运行 git：{e}"))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(format!("git {} 失败：{}", args.join(" "), stderr.trim()));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    fn set(&self, phase: Phase) {
        self.state.lock().unwrap().phase = phase;
    }

    fn append(&self, text: &str) {
        let mut state = self.state.lock().unwrap();
        let output = &mut state.output;
        output.push_str(text);
        if !output.ends_with('\n') {
            output.push('\n');
        }
        if output.len() > UPDATE_OUTPUT_BYTES {
            let mut cut = output.len() - UPDATE_OUTPUT_BYTES;
            while !output.is_char_boundary(cut) {
                cut += 1;
            }
            output.drain(..cut);
        }
    }
}

/// 半次单位换成可读次数。
fn halves(n: usize) -> String {
    match n % 2 {
        0 => (n / 2).to_string(),
        _ => format!("{}.5", n / 2),
    }
}

pub(crate) async fn status(State(app): State<Arc<App>>) -> Response {
    let headers = [(header::CACHE_CONTROL, "no-store")];
    match &app.updater {
        None => (headers, Json(StatusView::Unavailable)).into_response(),
        Some(u) => u.view(|v| (headers, Json(v)).into_response()),
    }
}

pub(crate) async fn start(State(app): State<Arc<App>>) -> Result<StatusCode, ApiError> {
    let updater = app.updater.as_ref().ok_or_else(|| {
        ApiError::Unavailable(
            "未启用更新：在配置 [gateway] 里加 update_repo = \"<部署仓库绝对路径>\" 后重启".into(),
        )
    })?;
    updater.start()?;
    Ok(StatusCode::ACCEPTED)
}
