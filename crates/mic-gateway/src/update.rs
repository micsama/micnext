//! 一键更新：原地 pull → build.sh → 请装配根 exec。契约：docs/blueprints/self-update.md。

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

/// 构建成功后请装配根停止并 exec 的目标。
pub struct RestartRequest {
    pub executable: PathBuf,
}

pub(crate) struct Updater {
    repo: PathBuf,
    restart: mpsc::Sender<RestartRequest>,
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
    Idle,
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
        output: &'a str,
    },
    Restarting {
        from: &'a str,
        to: &'a str,
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
            Phase::Idle => StatusView::Idle,
            Phase::Checking => StatusView::Checking { output },
            Phase::Pulling { from, attempt } => StatusView::Pulling {
                from,
                attempt: *attempt,
                output,
            },
            Phase::Building { from, to } => StatusView::Building { from, to, output },
            Phase::Restarting { from, to } => StatusView::Restarting { from, to },
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
        let dirty = self.capture(&["status", "--porcelain"]).await?;
        if !dirty.is_empty() {
            self.append(&dirty);
            return Err("工作区有未提交修改或未跟踪文件，请先在服务器上处理".into());
        }
        self.capture(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])
            .await
            .map_err(|e| format!("当前分支没有 upstream：{e}"))?;
        let from = self.capture(&["rev-parse", "HEAD"]).await?;
        tracing::info!(%from, "update pulling");

        self.pull(&from).await?;

        let to = self.capture(&["rev-parse", "HEAD"]).await?;
        if to == from {
            tracing::info!(commit = %to, "update: already up to date");
            self.set(Phase::UpToDate { commit: to });
            return Ok(());
        }

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

        self.set(Phase::Restarting {
            from: from.clone(),
            to: to.clone(),
        });
        tracing::info!(%from, %to, "update restarting");
        let executable = self.repo.join("target/release/micnext");
        self.restart
            .try_send(RestartRequest { executable })
            .map_err(|e| format!("无法通知重启：{e}"))
    }

    async fn pull(&self, from: &str) -> Result<(), String> {
        let total = PULL_TIMEOUTS.len();
        for (i, timeout) in PULL_TIMEOUTS.into_iter().enumerate() {
            let attempt = i + 1;
            self.set(Phase::Pulling {
                from: from.to_owned(),
                attempt,
            });
            self.append(&format!(
                "$ git pull --ff-only（第 {attempt}/{total} 次）\n"
            ));
            let mut git = Command::new("git");
            git.args(["pull", "--ff-only"]);
            match tokio::time::timeout(timeout, self.stream(git)).await {
                Ok(Ok(status)) if status.success() => return Ok(()),
                Ok(Ok(status)) => self.append(&format!("git pull 失败（{status}）\n")),
                Ok(Err(e)) => return Err(format!("无法运行 git：{e}")),
                Err(_) => self.append(&format!(
                    "git pull {} 秒未完成，已终止\n",
                    timeout.as_secs()
                )),
            }
        }
        Err(format!(
            "git pull 连续 {total} 次失败，请检查网络或启动前是否开了代理"
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
