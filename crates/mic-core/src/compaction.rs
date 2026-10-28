use mic_message::{
    Compaction, ContentPart, ExecOutcome, Message, MessageBody, MessageId, ModelView, ReplyBlock,
    SessionCommand, ToolResultOutcome, Trim,
};
use mic_store::{
    ContextUsage, ModelCallContext, ModelCallOutcome, ModelCallPurpose, RunState, Session,
    SessionKind, StoreError,
};
use serde::Serialize;

use crate::attempt::{stream_attempt, Facts};
use crate::context_usage::{estimate_tokens, message_tokens};
use crate::limits::{
    COMPACTION_BUDGET_PERCENT, DEFAULT_CONTEXT_WINDOW, ELIDE_TOOL_TOKENS, IDLE_COMPACT_PERCENT,
    IDLE_TRIM_AFTER, IDLE_TRIM_DEFAULT,
};
use crate::run::{now_ms, Engine, Exec};
use crate::skills::SKILL;
use crate::{request, ModelRequest, ModelResponse, Provider, StopReason, ToolChoice};

const SUMMARY_PROMPT: &str = "Summarize the earlier conversation for continuing the work. Do not continue the task or call tools. Output only the summary itself, without analysis or preamble. Include: 用户诉求、关键结论与决定、涉及文件、错误与修正、未完成事项、当前进度. Copy hard facts verbatim: file paths, exact error messages, explicit user constraints, unfinished tasks. Incorporate the earlier summary cumulatively. The user's messages will be quoted verbatim after compaction; do not restate them word for word. Finish with an optional <required_files> block, one file path per line, closed by </required_files>. List only files essential to continuing the current task. The additional user input is for focus; it will be processed after compaction.";

/// 定时任务会话：每次触发都重读历史，摘要按事项而非按次记录。
const SCHEDULE_SUMMARY_PROMPT: &str = "Summarize the earlier runs of this scheduled task for its next runs. Do not continue the task or call tools. Output only the summary itself, without analysis or preamble. Include: 任务目标、用户在此会话中给过的修正与约束、已经推送或报告过的事项（后续不再重复）、待跟进的状态. Merge runs that found nothing new into one line giving only the latest check time and conclusion. Copy hard facts verbatim: identifiers, links, file paths, exact error messages, explicit user constraints. Incorporate the earlier summary cumulatively. The user's messages will be quoted verbatim after compaction; do not restate them word for word. Finish with an optional <required_files> block, one file path per line, closed by </required_files>. List only files essential to the next runs. The additional user input is for focus; it will be processed after compaction.";

/// 当前上下文占用，口径同自动压缩判定。契约：docs/blueprints/chat-run-stats.md §二。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Occupancy {
    pub used_tokens: u64,
    pub window: u32,
    pub trigger_percent: u8,
}

/// 会话未选模型、所选模型已删除或当前边界后尚无调用时为 `None`。
pub(crate) async fn occupancy(
    engine: &Engine,
    session: &Session,
) -> Result<Option<Occupancy>, StoreError> {
    Ok(usage(engine, session).await?.map(|(o, ..)| o))
}

/// 占用连同其来源调用的用量，以及当前模型是否支持图片。
pub(crate) async fn usage(
    engine: &Engine,
    session: &Session,
) -> Result<Option<(Occupancy, ContextUsage, bool)>, StoreError> {
    let Some(model_id) = session.model_id else {
        return Ok(None);
    };
    let Some(model) = engine.store.model(model_id).await? else {
        return Ok(None);
    };
    let Some(usage) = engine.store.context_usage(session.id).await? else {
        return Ok(None);
    };
    let settings = engine.store.run_settings(session.id).await?;
    let occupancy = Occupancy {
        used_tokens: usage.input_tokens + usage.output_tokens,
        window: model
            .context_window
            .map_or(DEFAULT_CONTEXT_WINDOW, |w| w.get()),
        trigger_percent: settings.compaction.trigger_percent,
    };
    Ok(Some((occupancy, usage, model.images)))
}

impl Exec<'_> {
    /// 压缩请求的历史，连同窗口内已换成占位的消息。
    async fn history_request(
        &self,
    ) -> Result<(ModelRequest, ModelCallContext, Vec<MessageId>), StoreError> {
        let mut window = self.engine.store.context_window(self.session.id).await?;
        let elided = std::mem::take(&mut window.elided);
        window
            .messages
            .retain(|m| !self.pending_inputs.contains(&m.id));
        let through = window.messages.iter().map(|m| m.id).max();
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
        let images = self.window_images(&mut window).await?;
        let channel_prompt = self.engine.channel_prompt(self.session);
        let req = request::build(
            self.session,
            window,
            images,
            channel_prompt,
            self.skills.as_deref(),
            &self.tools,
            self.settings,
        );
        Ok((req, context, elided))
    }

    /// 每步工具结果落盘后判定：本步上游用量达到触发比例即压缩；已停止不再发起。
    pub(crate) async fn compact_if_full(
        &self,
        provider: &dyn Provider,
    ) -> Result<Result<(), RunState>, StoreError> {
        if self.stop.is_cancelled() {
            return Ok(Ok(()));
        }
        let Some(usage) = self.engine.store.context_usage(self.session.id).await? else {
            return Ok(Ok(()));
        };
        let trigger = u64::from(self.context_window)
            * u64::from(self.settings.compaction.trigger_percent)
            / 100;
        if usage.input_tokens + usage.output_tokens < trigger {
            return Ok(Ok(()));
        }
        self.compact(provider).await
    }

    /// 普通对话 run 开始前：闲置超过模型阈值即整理历史，占用过半直接压缩。
    /// 契约：docs/blueprints/cold-trim.md。
    pub(crate) async fn tidy_if_idle(
        &self,
        provider: &dyn Provider,
    ) -> Result<Result<(), RunState>, StoreError> {
        let Some(last) = self
            .engine
            .store
            .last_conversation_at(self.session.id)
            .await?
        else {
            return Ok(Ok(()));
        };
        let model = provider.model();
        let idle = IDLE_TRIM_AFTER
            .iter()
            .find(|(prefix, _)| model.starts_with(prefix))
            .map_or(IDLE_TRIM_DEFAULT, |(_, after)| *after);
        if now_ms() - last < idle.as_millis() as i64 {
            return Ok(Ok(()));
        }
        if let Some(usage) = self.engine.store.context_usage(self.session.id).await? {
            if (usage.input_tokens + usage.output_tokens) * 100
                > u64::from(self.context_window) * IDLE_COMPACT_PERCENT
            {
                return self.compact(provider).await;
            }
        }
        self.trim().await.map(Ok)
    }

    /// 轻整理：本 run 输入之前、尚未占位的可省略内容换成占位，不调用模型。
    pub(crate) async fn trim(&self) -> Result<(), StoreError> {
        let window = self.engine.store.context_window(self.session.id).await?;
        let (mut outputs, mut skills) = (0, 0);
        let mut elided = Vec::new();
        for m in &window.messages {
            if self.pending_inputs.contains(&m.id) || window.elided.binary_search(&m.id).is_ok() {
                continue;
            }
            match elidable(m) {
                Some(Elidable::Output) => outputs += 1,
                Some(Elidable::Skill) => skills += 1,
                None => continue,
            }
            elided.push(m.id);
        }
        if elided.is_empty() {
            return Ok(());
        }
        let counts: Vec<String> = [(outputs, "条工具输出"), (skills, "个 skill 正文")]
            .into_iter()
            .filter(|(n, _)| *n > 0)
            .map(|(n, unit)| format!("{n} {unit}"))
            .collect();
        let text = format!("长时间未活动，已整理历史：省略 {}。", counts.join("、"));
        let trim = Trim {
            base: window.boundary_id,
            elided,
        };
        let publish = self.engine.events.publisher().await;
        for message in self
            .engine
            .store
            .commit_trim(self.session.id, self.run.id, trim, text, now_ms())
            .await?
        {
            publish.appended(self.session.id, self.channel, message);
        }
        Ok(())
    }

    /// 自动压缩与 `/compact` 共用：附加文字取本步待处理输入。
    pub(crate) async fn compact(
        &self,
        provider: &dyn Provider,
    ) -> Result<Result<(), RunState>, StoreError> {
        let (mut req, mut context, carried) = self.history_request().await?;
        let retention = retain(
            &req.messages,
            &carried,
            self.settings.compaction.keep_percent,
        );
        let budget = u64::from(self.context_window) * COMPACTION_BUDGET_PERCENT / 100;
        let before = req.messages.len();
        while estimate_tokens(&req) > budget && !req.messages.is_empty() {
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
        let pending = self
            .engine
            .store
            .messages_by_ids(self.pending_inputs.clone())
            .await?;
        context.instruction = pending.iter().map(|m| m.id).collect();
        context.keep_from = retention.keep_from;
        context.elide = retention.elide.iter().map(|m| m.id).collect();
        req.messages.push(instruction_note(
            self.session,
            extra_text(&pending),
            retention.kept_replies,
            &retention.elide,
        ));
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
                            let files = paths;
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
                                        keep_from: retention.keep_from,
                                        files,
                                        elided: retention.elided,
                                        quoted: retention.quoted,
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

/// 压缩请求末尾的指令消息；执行与 Trace 重建共用。
pub(crate) fn instruction_note(
    session: &Session,
    extra: Option<String>,
    kept_replies: usize,
    elide: &[Message],
) -> Message {
    let scheduled = matches!(&session.kind, SessionKind::Triggered { module, .. } if module == mic_store::SCHEDULE_MODULE);
    let mut text = String::from(if scheduled {
        SCHEDULE_SUMMARY_PROMPT
    } else {
        SUMMARY_PROMPT
    });
    if kept_replies > 0 {
        text += &format!("\n\nThe last {kept_replies} assistant replies and their tool results will be kept verbatim after compaction; summarize what comes before them and do not restate them.");
    }
    if !elide.is_empty() {
        text += "\n\nThese kept tool results will be replaced by placeholders after compaction. Extract from them what is needed to continue the work:";
        for m in elide {
            match &m.body {
                MessageBody::ToolResult {
                    tool_name,
                    tool_call_id,
                    ..
                } => text += &format!("\n- {tool_name} (tool_call_id {tool_call_id})"),
                MessageBody::Completion {
                    tool_name, exec_id, ..
                } => text += &format!("\n- {tool_name} (exec_id {exec_id})"),
                _ => unreachable!("只替换工具结果"),
            }
        }
    }
    if let Some(extra) = extra {
        text += &format!("\n\nAdditional user input:\n{extra}");
    }
    Message {
        id: MessageId(0),
        session_id: session.id,
        body: MessageBody::HarnessNote { text },
        created_at: now_ms(),
        delivered_at: None,
    }
}

/// 压缩后留下什么。契约：docs/blueprints/archive/compaction-retention.md §三。
#[derive(Debug)]
pub(crate) struct Retention {
    pub keep_from: Option<MessageId>,
    pub kept_replies: usize,
    /// 保留段内换成占位的消息：沿用上一边界的与本次新增的。
    pub elided: Vec<MessageId>,
    /// 本次新增替换、需要摘要提取要点的工具输出原文，按 id；skill 正文可重新加载，不在此列。
    pub elide: Vec<Message>,
    pub quoted: Vec<MessageId>,
}

/// `messages` 为请求顺序的完整历史（不含待回答输入），`carried` 为窗口内已替换的消息；
/// 预算 = 替换后整段历史估算 × `percent`。
pub(crate) fn retain(messages: &[Message], carried: &[MessageId], percent: u8) -> Retention {
    let starts: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| matches!(m.body, MessageBody::Reply { .. }))
        .map(|(i, _)| i)
        .collect();
    let newest = starts.last().copied().unwrap_or(messages.len());
    let fresh = |i: usize| {
        i < newest && !carried.contains(&messages[i].id) && elidable(&messages[i]).is_some()
    };
    let size = |range: std::ops::Range<usize>| -> u64 {
        range
            .map(|i| {
                if fresh(i) {
                    let mut m = messages[i].clone();
                    m.elide_output();
                    message_tokens(&m)
                } else {
                    message_tokens(&messages[i])
                }
            })
            .sum()
    };
    let budget = size(0..messages.len()) * u64::from(percent) / 100;
    let mut keep = starts.len();
    if let Some(&last) = starts.last() {
        keep -= 1;
        let mut used = size(last..messages.len());
        while keep > 0 {
            let step = size(starts[keep - 1]..starts[keep]);
            if used + step > budget {
                break;
            }
            used += step;
            keep -= 1;
        }
    }
    let keep_from = starts.get(keep).map(|&i| messages[i].id);
    let segment = starts.get(keep).map_or(messages.len(), |&i| i)..messages.len();
    let mut elided = Vec::new();
    let mut elide = Vec::new();
    for i in segment {
        if carried.contains(&messages[i].id) {
            elided.push(messages[i].id);
        } else if fresh(i) {
            elided.push(messages[i].id);
            if elidable(&messages[i]) == Some(Elidable::Output) {
                elide.push(messages[i].clone());
            }
        }
    }
    elided.sort();
    elide.sort_by_key(|m| m.id);
    let keep_position = starts.get(keep).copied().unwrap_or(messages.len());
    let mut quoted: Vec<MessageId> = messages
        .iter()
        .take(keep_position)
        .filter(|m| matches!(m.body, MessageBody::UserInput { .. }))
        .map(|m| m.id)
        .collect();
    quoted.sort();
    Retention {
        keep_from,
        kept_replies: starts.len() - keep,
        elided,
        elide,
        quoted,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Elidable {
    /// 超过阈值的完成态工具输出。
    Output,
    /// skill 正文：`skill` 工具结果或 `/name` 注入的用户输入。
    Skill,
}

/// 可换成占位的旧内容；压缩保留段与冷时整理共用。
pub(crate) fn elidable(message: &Message) -> Option<Elidable> {
    match &message.body {
        MessageBody::ToolResult {
            tool_name,
            outcome: ToolResultOutcome::Terminal(ExecOutcome::Completed { .. }),
            ..
        } if tool_name == SKILL => Some(Elidable::Skill),
        MessageBody::ToolResult {
            outcome: ToolResultOutcome::Terminal(ExecOutcome::Completed { .. }),
            ..
        }
        | MessageBody::Completion {
            outcome: ExecOutcome::Completed { .. },
            ..
        } => (message_tokens(message) > ELIDE_TOOL_TOKENS).then_some(Elidable::Output),
        MessageBody::UserInput { parts, .. } => parts
            .iter()
            .any(|p| matches!(p, ContentPart::Skill { .. }))
            .then_some(Elidable::Skill),
        _ => None,
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
    // 清单只认结尾的那一块；正文里提到标签名不算。
    let body = text.trim_end();
    let paths = match body
        .strip_suffix("</required_files>")
        .and_then(|head| head.rfind("<required_files>").map(|start| (head, start)))
    {
        Some((head, start)) => {
            let mut paths = Vec::new();
            for path in head[start + "<required_files>".len()..]
                .lines()
                .map(str::trim)
                .filter(|p| !p.is_empty())
            {
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

#[cfg(test)]
mod tests;
