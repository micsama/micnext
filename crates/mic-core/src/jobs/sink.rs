//! core 交给后台任务的 sink：输出进日志，中途通知作为 `JobNotice` 进发起会话。

use std::io;
use std::sync::Arc;

use mic_message::MessageBody;
use mic_store::Job;
use mic_tool::{BoxFuture, OutputSink};

use super::output::{self, Writer};
use super::{Jobs, Tree};
use crate::run::now_ms;

pub(super) struct Sink {
    writer: Writer,
    notices: Notices,
}

/// 收尾前封口：输出同 [`output::Seal`]；在途通知落完，之后到达的丢弃，回注必在全部通知之后。
pub(super) struct Seal {
    output: output::Seal,
    gate: Arc<tokio::sync::Mutex<bool>>,
}

#[derive(Clone)]
struct Notices {
    jobs: Arc<Jobs>,
    job: Arc<Job>,
    tree: Tree,
    /// true = 已封口。
    gate: Arc<tokio::sync::Mutex<bool>>,
}

impl Sink {
    pub(super) fn new(
        writer: Writer,
        output: output::Seal,
        jobs: Arc<Jobs>,
        job: Job,
        tree: Tree,
    ) -> (Self, Seal) {
        let gate = Arc::new(tokio::sync::Mutex::new(false));
        let notices = Notices {
            jobs,
            job: Arc::new(job),
            tree,
            gate: gate.clone(),
        };
        (Self { writer, notices }, Seal { output, gate })
    }
}

impl Seal {
    pub(super) async fn seal(self) {
        self.output.seal().await;
        *self.gate.lock().await = true;
    }
}

impl Notices {
    /// 封口后到达的丢弃；落盘与发布在持门期间完成，唤醒在其后。
    async fn send(self, text: String) -> Result<(), mic_store::StoreError> {
        let Self {
            jobs,
            job,
            tree,
            gate,
        } = self;
        let owner = job.owner_session_id;
        {
            let sealed = gate.lock().await;
            if *sealed {
                return Ok(());
            }
            let body = MessageBody::JobNotice {
                person: job.person,
                tool_name: job.tool_name.clone(),
                exec_id: job.id.0.to_string(),
                text,
            };
            let publish = jobs.events.publisher().await;
            let message = jobs.store.append(owner, None, body, now_ms()).await?;
            publish.appended(owner, &tree.channel, message);
        }
        jobs.wake(owner).await
    }
}

impl OutputSink for Sink {
    fn write<'a>(&'a mut self, bytes: &'a [u8]) -> BoxFuture<'a, io::Result<()>> {
        Box::pin(self.writer.write(bytes))
    }

    /// 在独立任务里完成：工具 future 被丢弃时不会半途中断，由封口决定先后。
    fn notify<'a>(&'a mut self, text: &'a str) -> BoxFuture<'a, io::Result<()>> {
        let send = self.notices.clone().send(text.to_owned());
        Box::pin(async move {
            tokio::spawn(send)
                .await
                .map_err(io::Error::other)?
                .map_err(io::Error::other)
        })
    }
}
