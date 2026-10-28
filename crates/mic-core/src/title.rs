use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use mic_message::{ContentPart, Message, MessageBody, ReplyBlock, SessionId};
use mic_store::{
    is_tag_separator, reply_answer, ClaimedModel, ModelCallContext, ModelCallOutcome,
    ModelCallPurpose, Session, Store, StoreError, Tag, TitleCall, TitleOutput, TAGS_PER_SESSION,
};
use tokio::sync::{mpsc, watch};
use tokio::task::{Id, JoinSet};
use tokio_util::sync::CancellationToken;

use crate::attempt::{stream_attempt, Caller, Facts};
use crate::event::{Events, KernelEventKind};
use crate::limits::{TITLE_CHARS, TITLE_INPUT_CHARS, TITLE_REPLY_CHARS};
use crate::provider::{BuiltProvider, Factories};
use crate::run::now_ms;
use crate::{ModelRequest, ModelResponse, ProviderError, RunError, StopReason, ToolChoice};

#[cfg(test)]
mod tests;

#[derive(Debug, thiserror::Error)]
pub enum TitleError {
    #[error("只有主会话可以生成标题")]
    NotRoot,
    #[error("标题正在生成，请稍后再试")]
    Generating,
    #[error("会话还没有文字输入")]
    EmptyInput,
    #[error("服务正在停止，无法生成标题")]
    Stopped,
}

pub(crate) struct Job {
    session: Session,
    sources: Vec<Message>,
    stop: CancellationToken,
    _done: watch::Sender<()>,
}

struct ActiveTitle {
    stop: CancellationToken,
    done: watch::Receiver<()>,
}

#[derive(Clone)]
pub(crate) struct TitleHandle {
    tx: mpsc::UnboundedSender<Job>,
    active: Arc<Mutex<HashMap<SessionId, ActiveTitle>>>,
    events: Events,
}

impl TitleHandle {
    pub(crate) async fn stop(&self, session: SessionId) {
        let task = self
            .active
            .lock()
            .expect("title state poisoned")
            .get(&session)
            .map(|t| (t.stop.clone(), t.done.clone()));
        if let Some((stop, mut done)) = task {
            stop.cancel();
            let _ = done.changed().await;
        }
    }

    pub(crate) fn generating(&self, session: SessionId) -> bool {
        self.active
            .lock()
            .expect("title state poisoned")
            .contains_key(&session)
    }

    pub(crate) fn submit(&self, session: Session, sources: Vec<Message>) -> Result<(), TitleError> {
        let mut active = self.active.lock().expect("title state poisoned");
        if active.contains_key(&session.id) {
            return Err(TitleError::Generating);
        }
        let id = session.id;
        let channel = root_channel(&session).to_owned();
        let stop = CancellationToken::new();
        let (done, rx) = watch::channel(());
        active.insert(
            id,
            ActiveTitle {
                stop: stop.clone(),
                done: rx,
            },
        );
        if self
            .tx
            .send(Job {
                session,
                sources,
                stop,
                _done: done,
            })
            .is_err()
        {
            active.remove(&id);
            return Err(TitleError::Stopped);
        }
        self.events
            .emit(id, &channel, KernelEventKind::SessionChanged);
        Ok(())
    }
}

pub(crate) struct TitleRuntime {
    rx: mpsc::UnboundedReceiver<Job>,
    active: Arc<Mutex<HashMap<SessionId, ActiveTitle>>>,
    store: Store,
    factories: Arc<Factories>,
    events: Events,
    tasks: JoinSet<Result<(), StoreError>>,
    owners: HashMap<Id, SessionId>,
}

impl Drop for TitleRuntime {
    fn drop(&mut self) {
        self.active.lock().expect("title state poisoned").clear();
    }
}

impl TitleRuntime {
    pub(crate) fn new(
        store: Store,
        factories: Arc<Factories>,
        events: Events,
    ) -> (TitleHandle, Self) {
        let (tx, rx) = mpsc::unbounded_channel();
        let active = Arc::new(Mutex::new(HashMap::new()));
        (
            TitleHandle {
                tx,
                active: active.clone(),
                events: events.clone(),
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
                    let id = job.session.id;
                    let store = self.store.clone();
                    let factories = self.factories.clone();
                    let active = self.active.clone();
                    let events = self.events.clone();
                    let task = self.tasks.spawn(async move {
                        if store.session_root(id).await?.archived_at.is_none() && !job.stop.is_cancelled() {
                            generate(&store, &factories, &job).await?;
                        }
                        let mut active = active.lock().expect("title state poisoned");
                        active.remove(&id);
                        events.emit(id, root_channel(&job.session), KernelEventKind::SessionChanged);
                        Ok(())
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
                        message: format!("标题生成：{}", crate::scheduler::panic_message(error)),
                    },
                },
            }
        }
    }
}

fn root_channel(session: &Session) -> &str {
    match &session.kind {
        mic_store::SessionKind::Root { channel, .. } => channel,
        _ => unreachable!("title jobs are Root sessions"),
    }
}

/// 生成与 trace 重建共用取材和末尾任务。
pub(crate) fn title_messages(
    session: SessionId,
    sources: &[Message],
    tags: &[Tag],
) -> Vec<Message> {
    let mut budget = TITLE_INPUT_CHARS;
    let mut messages: Vec<Message> = sources
        .iter()
        .rev()
        .filter_map(|message| {
            let body = match &message.body {
                MessageBody::UserInput { person, parts } => {
                    let text = parts
                        .iter()
                        .filter_map(|part| match part {
                            ContentPart::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    let len = text.chars().count();
                    let keep = len.min(budget);
                    budget -= keep;
                    if keep == 0 {
                        return None;
                    }
                    MessageBody::UserInput {
                        person: *person,
                        parts: vec![ContentPart::Text {
                            text: text.chars().skip(len - keep).collect(),
                        }],
                    }
                }
                MessageBody::Reply {
                    model,
                    blocks,
                    stats,
                } => MessageBody::Reply {
                    stats: stats.clone(),
                    model: model.clone(),
                    blocks: vec![ReplyBlock::Text {
                        text: reply_answer(blocks)?
                            .chars()
                            .take(TITLE_REPLY_CHARS)
                            .collect(),
                        phase: None,
                    }],
                },
                _ => unreachable!("title sources are UserInput or Reply"),
            };
            Some(Message {
                body,
                ..message.clone()
            })
        })
        .collect();
    messages.reverse();
    messages.push(Message {
        id: mic_message::MessageId(0),
        session_id: session,
        body: MessageBody::HarnessNote { text: task(tags) },
        created_at: now_ms(),
        delivered_at: None,
    });
    messages
}

fn task(tags: &[Tag]) -> String {
    const TITLE: &str = "不带 Markdown、引号、前缀或解释";
    if tags.is_empty() {
        return format!("为以上对话生成简短标题，使用用户的语言。只输出一行纯文本，最多 {TITLE_CHARS} 字，{TITLE}。");
    }
    let names = tags
        .iter()
        .map(|t| t.name.as_str())
        .collect::<Vec<_>>()
        .join("、");
    format!(
        "为以上对话生成简短标题并挑选标签，使用用户的语言。\n\
         第一行输出标题：纯文本，最多 {TITLE_CHARS} 字，{TITLE}。\n\
         第二行从这些标签中选 0–{TAGS_PER_SESSION} 个最贴切的，用「、」分隔：{names}。都不贴切就只输出标题这一行。不要输出其他内容。"
    )
}

async fn generate(store: &Store, factories: &Factories, job: &Job) -> Result<(), StoreError> {
    let snapshot = store.utility_model_snapshot().await?;
    let model_id = match &snapshot {
        ClaimedModel::Selected(m) => Some(m.id),
        _ => None,
    };
    let built = crate::utility::provider(factories, snapshot);
    let BuiltProvider { provider, effort } = match built {
        Ok(built) => built,
        Err(error) => {
            tracing::warn!(session = job.session.id.0, purpose = "title", %error, "utility model unavailable");
            return store
                .save_title_result(job.session.id, Err(error.to_string()), None)
                .await;
        }
    };
    let tags = store.title_tag_choices(job.session.id).await?;
    let request = ModelRequest {
        tool_choice: ToolChoice::None,
        system: "你负责为对话生成标题。".into(),
        messages: title_messages(job.session.id, &job.sources, &tags),
        tools: Vec::new(),
        images: Default::default(),
        cache_key: format!("title-{}", job.session.id.0),
        plain_after: None,
    };
    let mut facts = Facts::start();
    let response = tokio::select! {
        response = stream_attempt(provider.as_ref(), request, &mut facts, |_| {}) => response,
        () = job.stop.cancelled() => {
            facts.finished_at = now_ms();
            Err(ProviderError::Protocol { message: "会话已归档，标题生成已停止".into() })
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
    let result = response.and_then(|response| parse_title(&response, &tags));
    if let Err(error) = &result {
        tracing::warn!(
            session = job.session.id.0,
            purpose = "title",
            model = provider.model(),
            elapsed_ms = facts.finished_at - facts.started_at,
            %error,
            "session title generation failed"
        );
    }
    let (result, outcome) = match result {
        Ok(title) => (Ok(title), ModelCallOutcome::Replied { blocks: Vec::new() }),
        Err(error) => {
            let error = error.to_string();
            (Err(error.clone()), ModelCallOutcome::Failed { error })
        }
    };
    let caller = Caller::new(job.session.id, None, provider.as_ref(), model_id, effort);
    store
        .save_title_result(
            job.session.id,
            result,
            Some(TitleCall {
                call: facts.row(
                    &caller,
                    ModelCallPurpose::Title,
                    ModelCallContext {
                        instruction: job.sources.iter().map(|m| m.id).collect(),
                        ..Default::default()
                    },
                    stop,
                    outcome,
                ),
                output,
            }),
        )
        .await
}

fn parse_title(response: &ModelResponse, tags: &[Tag]) -> Result<TitleOutput, ProviderError> {
    let protocol = |message: String| ProviderError::Protocol { message };
    if response.stop != StopReason::EndTurn {
        return Err(protocol("标题输出未正常完成，请重试".into()));
    }
    let mut text = String::new();
    for block in &response.blocks {
        match block {
            ReplyBlock::Text { text: part, .. } => text.push_str(part),
            ReplyBlock::ToolCall { .. } => return Err(protocol("标题输出包含工具调用".into())),
            ReplyBlock::Reasoning(_) => {}
        }
    }
    let mut lines = text
        .trim()
        .split(['\n', '\r', '\u{85}', '\u{2028}', '\u{2029}'])
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let title = lines.next().unwrap_or_default();
    let tag_line = lines.next();
    if title.chars().count() > TITLE_CHARS
        || lines.next().is_some()
        || (tags.is_empty() && tag_line.is_some())
        || title.is_empty()
    {
        return Err(protocol(if tags.is_empty() {
            format!("标题须为一行非空文本，最多 {TITLE_CHARS} 字，请重试")
        } else {
            format!("须输出一行标题（最多 {TITLE_CHARS} 字）和至多一行标签，请重试")
        }));
    }
    if tags.is_empty() {
        return Ok(TitleOutput {
            title: title.into(),
            tags: None,
        });
    }
    let mut chosen = Vec::new();
    for name in tag_line
        .unwrap_or_default()
        .split(is_tag_separator)
        .filter(|n| !n.is_empty())
    {
        let tag = tags
            .iter()
            .find(|t| t.name == name)
            .ok_or_else(|| protocol(format!("标签「{name}」不在词表中，请重试")))?;
        if !chosen.contains(&tag.id) {
            chosen.push(tag.id);
        }
    }
    if chosen.len() > TAGS_PER_SESSION {
        return Err(protocol(format!("标签最多 {TAGS_PER_SESSION} 个，请重试")));
    }
    chosen.sort();
    Ok(TitleOutput {
        title: title.into(),
        tags: Some(chosen),
    })
}
