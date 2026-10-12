use std::future::poll_fn;
use std::path::PathBuf;

use mic_message::{
    Compaction, ContentPart, FileSnapshot, FileSnapshotOutcome, Message, MessageBody, ReplyBlock,
    SnapshotExtent,
};
use mic_store::{
    ContextUsage, ModelCallContext, ModelCallOutcome, NewModelCall, RunState, SessionKind,
    StoreError,
};
use mic_tool::ToolContext;
use serde::Serialize;

use crate::context_usage::request_tokens;
use crate::limits::{
    COMPACTION_BUDGET_PERCENT, MAX_MODEL_ATTEMPTS, MAX_RETRY_WAIT, RETRY_BASE, SNAPSHOT_BYTES,
    SNAPSHOT_FILES,
};
use crate::run::{now_ms, Exec};
use crate::{
    request, ModelEvent, ModelRequest, ModelResponse, Provider, ProviderError, StopReason,
    ToolChoice,
};

const SUMMARY_PROMPT: &str = "Summarize the earlier conversation for continuing the work. Do not continue the task or call tools. Include: 用户诉求、关键结论与决定、涉及文件、错误与修正、未完成事项、当前进度. Incorporate the earlier summary cumulatively. Finish with an optional <required_files> block, one file path per line, closed by </required_files>. List only files essential to continuing the current task. The additional user input is for focus; it will be processed after compaction.";

impl Exec<'_> {
    async fn history_request(
        &self,
    ) -> Result<(ModelRequest, ModelCallContext, Option<ContextUsage>), StoreError> {
        let mut window = self.engine.store.context_window(self.session.id).await?;
        window
            .messages
            .retain(|m| !self.pending_inputs.contains(&m.id));
        let context = ModelCallContext {
            boundary_id: window.boundary_id,
            through: window.messages.last().map(|m| m.id),
        };
        let usage = window.usage.clone();
        let images = self
            .engine
            .store
            .images(self.session.id, request::image_ids(&window))
            .await?;
        let channel_prompt = match &self.session.kind {
            SessionKind::Root { channel, .. } => {
                self.engine.channel_prompts.get(channel.as_str()).copied()
            }
            _ => None,
        };
        let req = request::build(
            self.session.id,
            window,
            images,
            &self.session.pwd,
            channel_prompt,
            &self.tools,
            self.settings,
        );
        Ok((req, context, usage))
    }

    pub(crate) async fn needs_compaction(&self) -> Result<bool, StoreError> {
        let (req, _, usage) = self.history_request().await?;
        Ok(request_tokens(&req, usage.as_ref(), self.model_id)
            >= u64::from(self.context_window) * u64::from(self.settings.compaction.trigger_percent)
                / 100)
    }

    pub(crate) async fn pending_text(&self) -> Result<Option<String>, StoreError> {
        let window = self.engine.store.context_window(self.session.id).await?;
        let parts: Vec<String> = window
            .messages
            .iter()
            .filter(|m| self.pending_inputs.contains(&m.id))
            .filter_map(|m| m.model_view())
            .flat_map(|v| match v {
                mic_message::ModelView::User(parts) => parts,
                _ => Vec::new(),
            })
            .filter_map(|p| match p {
                ContentPart::Text { text } => Some(text),
                _ => None,
            })
            .collect();
        Ok((!parts.is_empty()).then(|| parts.join("\n\n")))
    }

    pub(crate) async fn compact(
        &self,
        provider: &dyn Provider,
        extra: Option<String>,
    ) -> Result<Result<(), RunState>, StoreError> {
        let (mut req, context, _) = self.history_request().await?;
        let budget = u64::from(self.context_window) * COMPACTION_BUDGET_PERCENT / 100;
        while request_tokens(&req, None, self.model_id) > budget && !req.messages.is_empty() {
            let first_reply = req
                .messages
                .iter()
                .position(|m| matches!(m.body, MessageBody::Reply { .. }));
            let cut = first_reply
                .and_then(|start| {
                    req.messages
                        .iter()
                        .enumerate()
                        .skip(start + 1)
                        .find(|(_, m)| matches!(m.body, MessageBody::Reply { .. }))
                        .map(|(i, _)| i)
                })
                .unwrap_or(req.messages.len());
            req.messages.drain(..cut);
        }
        let steps: Vec<_> = req
            .messages
            .iter()
            .filter(|m| matches!(m.body, MessageBody::Reply { .. }))
            .map(|m| m.id)
            .collect();
        let n = self.settings.compaction.keep_steps as usize;
        let keep_from = if n == 0 {
            None
        } else {
            steps.get(steps.len().saturating_sub(n)).copied()
        };
        let instruction = match &extra {
            Some(text) => format!("{SUMMARY_PROMPT}\n\nAdditional user input:\n{text}"),
            None => SUMMARY_PROMPT.into(),
        };
        req.messages.push(Message {
            id: mic_message::MessageId(0),
            session_id: self.session.id,
            body: MessageBody::HarnessNote { text: instruction },
            created_at: now_ms(),
            delivered_at: None,
        });
        req.tool_choice = ToolChoice::None;
        let mut attempt = 1;
        loop {
            let started_at = now_ms();
            match summarize(provider, req.clone()).await {
                Ok(response) => {
                    let parsed = parse_summary(&response);
                    match parsed {
                        Ok((summary, paths)) => {
                            let files = self.read_snapshots(paths).await;
                            let call = self.compaction_call(
                                provider,
                                started_at,
                                context,
                                ModelCallOutcome::Replied {
                                    usage: response.usage,
                                    blocks: Vec::new(),
                                },
                            );
                            let publish = self.engine.events.publisher().await;
                            let messages = self
                                .engine
                                .store
                                .commit_compaction(
                                    self.session.id,
                                    self.run.id,
                                    Compaction {
                                        summary,
                                        keep_from,
                                        files,
                                        extra,
                                    },
                                    self.pending_inputs.clone(),
                                    call,
                                    now_ms(),
                                )
                                .await?;
                            for message in messages {
                                publish.appended(self.session.id, self.channel, message);
                            }
                            return Ok(Ok(()));
                        }
                        Err(reason) => {
                            self.engine
                                .store
                                .record_model_call(self.compaction_call(
                                    provider,
                                    started_at,
                                    context,
                                    ModelCallOutcome::Failed {
                                        error: reason.clone(),
                                    },
                                ))
                                .await?;
                            self.notify(format!("上下文压缩失败：{reason}。可查看历史，调整输入或清空上下文后重试。" )).await?;
                            return Ok(Err(RunState::ProviderFailed));
                        }
                    }
                }
                Err(error) => {
                    self.engine
                        .store
                        .record_model_call(self.compaction_call(
                            provider,
                            started_at,
                            context,
                            ModelCallOutcome::Failed {
                                error: error.to_string(),
                            },
                        ))
                        .await?;
                    match &error {
                        ProviderError::Transient { retry_after, .. }
                            if attempt < MAX_MODEL_ATTEMPTS =>
                        {
                            let wait = retry_after
                                .unwrap_or(RETRY_BASE * 2u32.pow(attempt - 1))
                                .min(MAX_RETRY_WAIT);
                            tokio::time::sleep(wait).await;
                            attempt += 1;
                        }
                        _ => {
                            self.notify(format!(
                                "上下文压缩失败：{error}。请处理原因后重试，或清空上下文。"
                            ))
                            .await?;
                            return Ok(Err(RunState::ProviderFailed));
                        }
                    }
                }
            }
        }
    }

    fn compaction_call(
        &self,
        provider: &dyn Provider,
        started_at: i64,
        context: ModelCallContext,
        outcome: ModelCallOutcome,
    ) -> NewModelCall {
        NewModelCall {
            session_id: self.session.id,
            run_id: None,
            model: provider.model().into(),
            started_at,
            finished_at: now_ms(),
            context,
            outcome,
        }
    }

    async fn read_snapshots(&self, paths: Vec<String>) -> Vec<FileSnapshot> {
        let read = self.tools.iter().find(|t| t.name() == "read");
        let ctx = ToolContext::new(PathBuf::from(&self.session.pwd));
        let mut remaining = SNAPSHOT_BYTES;
        let mut files = Vec::new();
        for (index, path) in paths.into_iter().enumerate() {
            let outcome = if index >= SNAPSHOT_FILES || remaining == 0 {
                FileSnapshotOutcome::Unavailable {
                    reason: "文件快照限额已用完".into(),
                }
            } else if let Some(read) = read {
                #[derive(Serialize)]
                struct ReadArgs<'a> {
                    file_path: &'a str,
                }
                let args =
                    serde_json::to_value(ReadArgs { file_path: &path }).expect("read 参数可序列化");
                match read.invoke(&args, &ctx).await {
                    Ok(parts) => {
                        let mut text = parts
                            .into_iter()
                            .filter_map(|p| match p {
                                ContentPart::Text { text } => Some(text),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        let extent = if text.len() > remaining {
                            let mut end = remaining;
                            while !text.is_char_boundary(end) {
                                end -= 1;
                            }
                            text.truncate(end);
                            SnapshotExtent::Truncated
                        } else {
                            SnapshotExtent::Complete
                        };
                        remaining -= text.len();
                        FileSnapshotOutcome::Read { text, extent }
                    }
                    Err(error) => FileSnapshotOutcome::Unavailable {
                        reason: error.message,
                    },
                }
            } else {
                FileSnapshotOutcome::Unavailable {
                    reason: "当前会话未开放 read 工具".into(),
                }
            };
            files.push(FileSnapshot { path, outcome });
        }
        files
    }
}

async fn summarize(
    provider: &dyn Provider,
    req: ModelRequest,
) -> Result<ModelResponse, ProviderError> {
    let mut stream = provider.stream(req);
    loop {
        match poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
            Some(Ok(ModelEvent::Finished(response))) => return Ok(response),
            Some(Ok(_)) => {}
            Some(Err(error)) => return Err(error),
            None => {
                return Err(ProviderError::Protocol {
                    message: "stream ended without Finished".into(),
                })
            }
        }
    }
}

fn parse_summary(response: &ModelResponse) -> Result<(String, Vec<String>), String> {
    match response.stop {
        StopReason::MaxTokens | StopReason::ContentFilter => {
            return Err("总结被上游截断，内容不完整".into())
        }
        StopReason::EndTurn | StopReason::ToolUse => {}
    }
    let mut text = String::new();
    for block in &response.blocks {
        match block {
            ReplyBlock::Text { text: part, .. } => text.push_str(part),
            ReplyBlock::ToolCall { name, .. } => {
                tracing::warn!(tool = name, "discarded tool call in compaction output")
            }
            ReplyBlock::Reasoning(_) => {}
        }
    }
    if text.trim().is_empty() {
        return Err("总结正文为空".into());
    }
    let paths = match text.find("<required_files>") {
        Some(start) => {
            let rest = &text[start + "<required_files>".len()..];
            let end = rest.find("</required_files>").ok_or("必读文件清单未闭合")?;
            let mut paths = Vec::new();
            for path in rest[..end].lines().map(str::trim).filter(|p| !p.is_empty()) {
                if !paths.iter().any(|p| p == path) {
                    paths.push(path.to_owned());
                }
            }
            text.truncate(start);
            paths
        }
        None => Vec::new(),
    };
    let summary = text.trim().to_owned();
    if summary.is_empty() {
        return Err("总结正文为空".into());
    }
    Ok((summary, paths))
}
