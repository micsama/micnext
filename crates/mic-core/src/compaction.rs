use std::path::PathBuf;

use mic_message::{
    Compaction, ContentPart, FileSnapshot, FileSnapshotOutcome, Message, MessageBody, MessageId,
    ModelView, ReplyBlock, SessionCommand, SessionId, SnapshotExtent,
};
use mic_store::{
    ContextUsage, ModelCallContext, ModelCallOutcome, ModelCallPurpose, RunState, SessionKind,
    StoreError,
};
use mic_tool::ToolContext;
use serde::Serialize;

use crate::attempt::{stream_attempt, Facts};
use crate::context_usage::request_tokens;
use crate::limits::{COMPACTION_BUDGET_PERCENT, SNAPSHOT_BYTES, SNAPSHOT_FILES};
use crate::run::{now_ms, Exec};
use crate::{request, ModelRequest, ModelResponse, Provider, StopReason, ToolChoice};

const SUMMARY_PROMPT: &str = "Summarize the earlier conversation for continuing the work. Do not continue the task or call tools. Include: 用户诉求、关键结论与决定、涉及文件、错误与修正、未完成事项、当前进度. Incorporate the earlier summary cumulatively. Finish with an optional <required_files> block, one file path per line, closed by </required_files>. List only files essential to continuing the current task. The additional user input is for focus; it will be processed after compaction.";

impl Exec<'_> {
    /// 压缩（含历史用量估算）用的窗口请求：本步待处理输入排除在外。
    async fn history_request(
        &self,
    ) -> Result<(ModelRequest, ModelCallContext, Option<ContextUsage>), StoreError> {
        let mut window = self.engine.store.context_window(self.session.id).await?;
        window
            .messages
            .retain(|m| !self.pending_inputs.contains(&m.id));
        let through = window.messages.last().map(|m| m.id);
        let context = ModelCallContext {
            boundary_id: window.boundary_id,
            through,
            excluded: window
                .filtered
                .iter()
                .copied()
                .chain(
                    self.pending_inputs
                        .iter()
                        .copied()
                        .filter(|id| through.is_some_and(|t| *id < t)),
                )
                .collect(),
            ..Default::default()
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
            self.session,
            window,
            images,
            channel_prompt,
            &self.tools,
            self.settings,
            &[],
        );
        Ok((req, context, usage))
    }

    pub(crate) async fn needs_compaction(&self) -> Result<bool, StoreError> {
        let (req, _, usage) = self.history_request().await?;
        Ok(request_tokens(&req, usage.as_ref(), self.model_id)
            >= u64::from(self.context_window) * u64::from(self.settings.compaction.trigger_percent)
                / 100)
    }

    /// 自动压缩与 `/compact` 共用：附加文字取本步待处理输入。
    pub(crate) async fn compact(
        &self,
        provider: &dyn Provider,
    ) -> Result<Result<(), RunState>, StoreError> {
        let (mut req, mut context, _) = self.history_request().await?;
        let budget = u64::from(self.context_window) * COMPACTION_BUDGET_PERCENT / 100;
        let before = req.messages.len();
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
        context.skip = (before - req.messages.len()) as u32;
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
        let pending = self
            .engine
            .store
            .messages_by_ids(self.pending_inputs.clone())
            .await?;
        context.instruction = pending.iter().map(|m| m.id).collect();
        req.messages
            .push(instruction_note(self.session.id, extra_text(&pending)));
        req.tool_choice = ToolChoice::None;
        let caller = self.caller(provider);
        let mut attempt = 1;
        loop {
            let mut facts = Facts::start();
            let streamed = tokio::select! {
                result = stream_attempt(provider, req.clone(), &mut facts, |_| {}) => Some(result),
                () = self.stop.cancelled() => None,
            };
            let Some(streamed) = streamed else {
                facts.finished_at = now_ms();
                facts.usage = None;
                self.engine
                    .store
                    .record_model_call(facts.row(
                        &caller,
                        ModelCallPurpose::Compaction,
                        context,
                        Some(StopReason::Stopped),
                        ModelCallOutcome::Replied { blocks: Vec::new() },
                    ))
                    .await?;
                return Ok(Err(RunState::Stopped));
            };
            match streamed {
                Ok(response) => {
                    let stop = Some(response.stop);
                    match parse_summary(&response) {
                        Ok((summary, paths)) => {
                            let files = self.read_snapshots(paths).await;
                            let call = facts.row(
                                &caller,
                                ModelCallPurpose::Compaction,
                                context,
                                stop,
                                ModelCallOutcome::Replied { blocks: Vec::new() },
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
                            tracing::warn!(
                                session = self.session.id.0,
                                run = self.run.id.0,
                                purpose = "compaction",
                                model = provider.model(),
                                attempt,
                                error = %reason,
                                "compaction output rejected"
                            );
                            self.engine
                                .store
                                .record_model_call(facts.row(
                                    &caller,
                                    ModelCallPurpose::Compaction,
                                    context,
                                    stop,
                                    ModelCallOutcome::Replied { blocks: Vec::new() },
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
                        .record_model_call(facts.row(
                            &caller,
                            ModelCallPurpose::Compaction,
                            context.clone(),
                            None,
                            ModelCallOutcome::Failed {
                                error: error.to_string(),
                            },
                        ))
                        .await?;
                    match self.retry_wait("compaction", provider, &error, attempt) {
                        Some(wait) => {
                            if self.sleep(wait).await {
                                return Ok(Err(RunState::Stopped));
                            }
                            attempt += 1;
                        }
                        None => {
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

/// 压缩指令的附加文字：`/compact <text>` 取命令文字，自动压缩取待处理用户输入的文字。
pub(crate) fn extra_text(messages: &[Message]) -> Option<String> {
    let parts: Vec<String> = messages
        .iter()
        .flat_map(|m| match &m.body {
            MessageBody::Command {
                command: SessionCommand::Compact { text: Some(text) },
                ..
            } => vec![text.clone()],
            _ => match m.model_view() {
                Some(ModelView::User(parts)) => parts
                    .into_iter()
                    .filter_map(|p| match p {
                        ContentPart::Text { text } => Some(text),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            },
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

/// 压缩请求末尾的指令消息。
pub(crate) fn instruction_note(session: SessionId, extra: Option<String>) -> Message {
    let text = match extra {
        Some(text) => format!("{SUMMARY_PROMPT}\n\nAdditional user input:\n{text}"),
        None => SUMMARY_PROMPT.into(),
    };
    Message {
        id: MessageId(0),
        session_id: session,
        body: MessageBody::HarnessNote { text },
        created_at: now_ms(),
        delivered_at: None,
    }
}

fn parse_summary(response: &ModelResponse) -> Result<(String, Vec<String>), String> {
    match response.stop {
        StopReason::MaxTokens | StopReason::ContentFilter | StopReason::Stopped => {
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
