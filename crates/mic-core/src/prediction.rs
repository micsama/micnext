//! 输入预测：对话 run 正常完成后，用杂活模型猜用户的下一句；候选由调用事实推出，不另存状态。

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use mic_message::{ContentPart, Message, MessageBody, ReplyBlock, SessionId};
use mic_store::{
    reply_answer, ClaimedModel, ModelCallContext, ModelCallOutcome, ModelCallPurpose, RunId,
    Session, SessionKind, Store, StoreError,
};
use tokio::sync::mpsc;
use tokio::task::{Id, JoinSet};
use tokio_util::sync::CancellationToken;

use crate::attempt::{stream_attempt, Caller, Facts};
use crate::event::{Events, KernelEventKind};
use crate::limits::{
    PREDICTION_CHARS, PREDICTION_MESSAGES, PREDICTION_ROUNDS, PREDICTION_SOURCE_CHARS,
    PREDICTION_TIMEOUT,
};
use crate::provider::{BuiltProvider, Factories};
use crate::run::now_ms;
use crate::{ModelRequest, ModelResponse, RunError, StopReason, ToolChoice};

#[cfg(test)]
mod tests;

struct Job {
    session: SessionId,
    channel: String,
    run: RunId,
    stop: CancellationToken,
}

#[derive(Clone)]
pub(crate) struct PredictionHandle {
    tx: mpsc::UnboundedSender<Job>,
    /// 每会话至多一个在途预测，记来源 run。
    active: Arc<Mutex<HashMap<SessionId, (RunId, CancellationToken)>>>,
    channels: Arc<HashSet<&'static str>>,
}

impl PredictionHandle {
    /// 对话 run 正常完成后调用；只为声明了预测的渠道的 Root 会话生成。
    pub(crate) fn submit(&self, session: &Session, run: RunId) {
        let SessionKind::Root { channel, .. } = &session.kind else {
            return;
        };
        if !self.channels.contains(channel.as_str()) {
            return;
        }
        let stop = CancellationToken::new();
        let mut active = self.active.lock().expect("prediction state poisoned");
        if let Some((_, old)) = active.insert(session.id, (run, stop.clone())) {
            old.cancel();
        }
        let job = Job {
            session: session.id,
            channel: channel.clone(),
            run,
            stop,
        };
        if self.tx.send(job).is_err() {
            active.remove(&session.id);
        }
    }

    /// 新 run 开始或会话归档时作废在途预测。
    pub(crate) fn cancel(&self, session: SessionId) {
        if let Some((_, stop)) = self
            .active
            .lock()
            .expect("prediction state poisoned")
            .remove(&session)
        {
            stop.cancel();
        }
    }
}

pub(crate) struct PredictionRuntime {
    rx: mpsc::UnboundedReceiver<Job>,
    active: Arc<Mutex<HashMap<SessionId, (RunId, CancellationToken)>>>,
    store: Store,
    factories: Arc<Factories>,
    events: Events,
    tasks: JoinSet<Result<(), StoreError>>,
    owners: HashMap<Id, SessionId>,
}

impl PredictionRuntime {
    pub(crate) fn new(
        store: Store,
        factories: Arc<Factories>,
        events: Events,
        channels: Arc<HashSet<&'static str>>,
    ) -> (PredictionHandle, Self) {
        let (tx, rx) = mpsc::unbounded_channel();
        let active = Arc::new(Mutex::new(HashMap::new()));
        (
            PredictionHandle {
                tx,
                active: active.clone(),
                channels,
            },
            Self {
                rx,
                active,
                store,
                factories,
                events,
                tasks: JoinSet::new(),
                owners: HashMap::new(),
            },
        )
    }

    pub(crate) async fn run(mut self) -> RunError {
        loop {
            tokio::select! {
                Some(job) = self.rx.recv() => {
                    let id = job.session;
                    let store = self.store.clone();
                    let factories = self.factories.clone();
                    let active = self.active.clone();
                    let events = self.events.clone();
                    let task = self.tasks.spawn(async move {
                        let result = generate(&store, &factories, &events, &job).await;
                        let mut active = active.lock().expect("prediction state poisoned");
                        if active.get(&id).is_some_and(|(run, _)| *run == job.run) {
                            active.remove(&id);
                        }
                        result
                    });
                    self.owners.insert(task.id(), id);
                }
                Some(result) = self.tasks.join_next_with_id() => match result {
                    Ok((id, result)) => {
                        self.owners.remove(&id);
                        if let Err(error) = result { return error.into(); }
                    }
                    Err(error) => return RunError::RunPanicked {
                        session_id: self.owners[&error.id()],
                        message: format!("输入预测：{}", crate::scheduler::panic_message(error)),
                    },
                },
            }
        }
    }
}

/// 取材：最近几轮的文字输入与收尾回答。最后一条回复仍带工具调用（被工具打断等用户）时不预测。
fn sources(messages: Vec<Message>) -> Option<Vec<Message>> {
    let last_reply = messages.iter().rev().find_map(|m| match &m.body {
        MessageBody::Reply { blocks, .. } => Some(blocks),
        _ => None,
    })?;
    reply_answer(last_reply)?;
    let kept: Vec<Message> = messages
        .into_iter()
        .filter(|m| match &m.body {
            MessageBody::UserInput { parts, .. } => parts
                .iter()
                .any(|p| matches!(p, ContentPart::Text { text } if !text.trim().is_empty())),
            MessageBody::Reply { blocks, .. } => reply_answer(blocks).is_some(),
            _ => false,
        })
        .collect();
    kept.iter()
        .any(|m| matches!(m.body, MessageBody::UserInput { .. }))
        .then_some(kept)
}

/// 生成与 trace 重建共用：把取材整理成一段对话记录，请模型以用户身份接下一句。
pub(crate) fn prediction_messages(session: SessionId, sources: &[Message]) -> Vec<Message> {
    let tail = |text: &str| {
        let len = text.chars().count();
        text.chars()
            .skip(len.saturating_sub(PREDICTION_SOURCE_CHARS))
            .collect::<String>()
    };
    let transcript = sources
        .iter()
        .filter_map(|message| match &message.body {
            MessageBody::UserInput { parts, .. } => {
                let text = parts
                    .iter()
                    .filter_map(|part| match part {
                        ContentPart::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n");
                Some(format!("用户：{}", tail(&text)))
            }
            MessageBody::Reply { blocks, .. } => {
                Some(format!("AI：{}", tail(&reply_answer(blocks)?)))
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    vec![Message {
        id: mic_message::MessageId(0),
        session_id: session,
        body: MessageBody::HarnessNote {
            text: format!(
                "<对话记录>\n{transcript}\n</对话记录>\n\n\
                 写出用户接下来最可能发的一句话，口吻和记录里的用户一致。\
                 只输出这句话本身，一行，最多 {PREDICTION_CHARS} 字；猜不出就什么都不输出。"
            ),
        },
        created_at: now_ms(),
        delivered_at: None,
    }]
}

async fn generate(
    store: &Store,
    factories: &Factories,
    events: &Events,
    job: &Job,
) -> Result<(), StoreError> {
    let snapshot = store.utility_model_snapshot().await?;
    let model_id = match &snapshot {
        ClaimedModel::Selected(m) => Some(m.id),
        _ => None,
    };
    let Ok(BuiltProvider { provider, effort }) = crate::utility::provider(factories, snapshot)
    else {
        return Ok(());
    };
    let page = store
        .messages_page(job.session, None, PREDICTION_ROUNDS, PREDICTION_MESSAGES)
        .await?;
    let Some(sources) = sources(page.messages) else {
        return Ok(());
    };
    if job.stop.is_cancelled() {
        return Ok(());
    }
    let request = ModelRequest {
        tool_choice: ToolChoice::None,
        system: "你负责预测用户在对话中的下一句输入。".into(),
        messages: prediction_messages(job.session, &sources),
        tools: Vec::new(),
        images: Default::default(),
        cache_key: format!("prediction-{}", job.session.0),
        plain_after: None,
    };
    let mut facts = Facts::start();
    let response = tokio::select! {
        response = tokio::time::timeout(
            PREDICTION_TIMEOUT,
            stream_attempt(provider.as_ref(), request, &mut facts, |_| {}),
        ) => match response {
            Ok(response) => response.map_err(|e| e.to_string()),
            Err(_) => {
                facts.finished_at = now_ms();
                Err(format!("超过 {} 秒未完成", PREDICTION_TIMEOUT.as_secs()))
            }
        },
        () = job.stop.cancelled() => {
            facts.finished_at = now_ms();
            Err("已取消".into())
        }
    };
    let stop = response.as_ref().ok().map(|r| r.stop);
    let output = response.as_ref().ok().map(|r| {
        r.blocks
            .iter()
            .filter_map(|b| match b {
                ReplyBlock::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<String>()
    });
    let result = response.and_then(|r| parse(&r));
    let outcome = match &result {
        Ok(_) => ModelCallOutcome::Replied { blocks: Vec::new() },
        Err(error) => {
            if !job.stop.is_cancelled() {
                tracing::warn!(
                    session = job.session.0,
                    run = job.run.0,
                    purpose = "prediction",
                    model = provider.model(),
                    elapsed_ms = facts.finished_at - facts.started_at,
                    %error,
                    "input prediction failed"
                );
            }
            ModelCallOutcome::Failed {
                error: error.clone(),
            }
        }
    };
    let caller = Caller::new(
        job.session,
        Some(job.run),
        provider.as_ref(),
        model_id,
        effort,
    );
    let call = facts.row(
        &caller,
        ModelCallPurpose::Prediction,
        ModelCallContext {
            instruction: sources.iter().map(|m| m.id).collect(),
            ..Default::default()
        },
        stop,
        outcome,
    );
    store.save_prediction(call, output).await?;
    if let (Ok(Some(text)), false) = (result, job.stop.is_cancelled()) {
        events.emit(
            job.session,
            &job.channel,
            KernelEventKind::PredictionReady {
                run_id: job.run,
                text,
            },
        );
    }
    Ok(())
}

/// 空输出 = 猜不出；其余不合格均为失败。
fn parse(response: &ModelResponse) -> Result<Option<String>, String> {
    if response.stop != StopReason::EndTurn {
        return Err("输出未正常完成".into());
    }
    let mut text = String::new();
    for block in &response.blocks {
        match block {
            ReplyBlock::Text { text: part, .. } => text.push_str(part),
            ReplyBlock::ToolCall { .. } => return Err("输出包含工具调用".into()),
            ReplyBlock::Reasoning(_) => {}
        }
    }
    let text = text.trim();
    if text.contains(['\n', '\r', '\u{85}', '\u{2028}', '\u{2029}'])
        || text.chars().count() > PREDICTION_CHARS
    {
        return Err(format!("须为一行文本，最多 {PREDICTION_CHARS} 字"));
    }
    Ok((!text.is_empty()).then(|| text.to_owned()))
}
