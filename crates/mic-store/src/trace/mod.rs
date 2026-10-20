//! trace 与看板的只读查询。契约：docs/blueprints/run-trace.md §六。
//! 重试归组、耗时、费用各一处计算，trace 与看板共用。

use std::collections::BTreeMap;

use mic_message::{
    ContentPart, ContextBoundary, ExecOutcome, Message, MessageBody, MessageId, ReplyBlock,
    SessionCommand, SessionId, ToolResultOutcome,
};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

use crate::row::{self, MESSAGE_COLS, MESSAGE_FROM, RUN_COLS};
use crate::{
    ModelCallContext, ModelCallId, ModelCallPurpose, ModelId, ModelPrice, ReasoningEffort, RunId,
    StopReason, Store, StoreError, Usage,
};

const PREVIEW_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TraceSession {
    pub session: SessionId,
    pub kind: TraceSessionKind,
    pub parent: Option<SessionId>,
    /// root：会话标题；task：任务说明。
    pub label: Option<String>,
    pub job: Option<i64>,
    /// 根祖先的 Channel；Triggered 根取触发模块名。
    pub channel: String,
    pub last_run_at: Option<i64>,
    pub failed_runs: u32,
    pub interrupted_runs: u32,
    pub tool_errors: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TraceCall {
    pub id: ModelCallId,
    pub purpose: ModelCallPurpose,
    pub model_id: Option<ModelId>,
    pub model: String,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub started_at: i64,
    pub first_output_at: Option<i64>,
    pub finished_at: i64,
    pub stop: Option<StopReason>,
    pub error: Option<String>,
    pub usage: Option<Usage>,
    /// 按当前单价估算；未知为 `None`。
    pub cost: Option<f64>,
    /// 同一输入的第几次尝试，从 1 起。
    pub attempt: u32,
    /// 这次调用写出的 `Reply` / `Boundary` / `SideOutput`。
    pub reply_id: Option<MessageId>,
    pub boundary_id: Option<MessageId>,
    pub side_id: Option<MessageId>,
    #[serde(skip)]
    key: (Option<i64>, Option<i64>),
    #[serde(skip)]
    day: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolTraceOutcome {
    Completed,
    Failed,
    Cancelled,
    Dispatched,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TraceTool {
    pub name: String,
    pub call_id: String,
    pub started_at: Option<i64>,
    pub outcome: ToolTraceOutcome,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TraceMessage {
    pub id: MessageId,
    pub kind: &'static str,
    pub created_at: i64,
    pub preview: String,
    pub tool: Option<TraceTool>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TraceRun {
    pub id: RunId,
    pub state: &'static str,
    pub created_at: i64,
    pub finished_at: Option<i64>,
    pub calls: Vec<TraceCall>,
    pub messages: Vec<TraceMessage>,
    pub stats: RunStats,
}

/// Trace 的执行合计。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunStats {
    pub total_ms: Option<i64>,
    pub model_ms: i64,
    /// 各步「最后一个已知工具结束 − 该步模型结束」之和。
    pub tool_ms: i64,
    pub steps: u32,
    pub requests: u32,
    pub tools: u32,
    pub first_output_ms: Option<i64>,
    pub output_tps: Option<f64>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// 有调用缺用量时为 false，token 合计只含已知部分。
    pub usage_complete: bool,
    pub cache_read_tokens: u64,
    /// 有调用缺用量或未报缓存读取时为 false。
    pub cache_complete: bool,
    pub cost: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TraceSessionKind {
    Root,
    Task,
    Triggered,
}

/// `tree` 为本会话加全部后代；无后代时为 None。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionUsageView {
    pub own: SessionUsage,
    pub tree: Option<TreeUsage>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TreeUsage {
    pub descendants: u32,
    pub usage: SessionUsage,
}

/// 全会话用量；费用合计已知部分，未知次数单列，沿用 trace 展示口径。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_tokens: Option<u64>,
    pub known_usage: u32,
    pub unknown_usage: u32,
    pub usage_complete: bool,
    pub cache_complete: bool,
    pub cost: Option<f64>,
    pub unknown_cost: u32,
}

fn session_usage(calls: &[TraceCall]) -> SessionUsage {
    let known: Vec<_> = calls.iter().filter_map(|c| c.usage.as_ref()).collect();
    let priced: Vec<_> = calls.iter().filter_map(|c| c.cost).collect();
    let unknown_usage = (calls.len() - known.len()) as u32;
    SessionUsage {
        input_tokens: known.iter().map(|u| u.input_tokens).sum(),
        output_tokens: known.iter().map(|u| u.output_tokens).sum(),
        cache_read_tokens: known.iter().filter_map(|u| u.cache_read_tokens).sum(),
        cache_write_tokens: known.iter().filter_map(|u| u.cache_write_tokens).sum(),
        reasoning_tokens: known
            .iter()
            .any(|u| u.reasoning_tokens.is_some())
            .then(|| known.iter().filter_map(|u| u.reasoning_tokens).sum()),
        known_usage: known.len() as u32,
        unknown_usage,
        usage_complete: unknown_usage == 0,
        cache_complete: !known.is_empty() && known.iter().all(|u| u.cache_read_tokens.is_some()),
        cost: (!priced.is_empty()).then(|| priced.iter().sum()),
        unknown_cost: (calls.len() - priced.len()) as u32,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TraceRunPage {
    pub runs: Vec<TraceRun>,
    pub title_calls: Vec<TraceCall>,
    pub next_after: Option<RunId>,
}

/// 重建输入所需的调用事实。
#[derive(Debug, Clone, PartialEq)]
pub struct CallRecord {
    pub session_id: SessionId,
    pub purpose: ModelCallPurpose,
    pub context: ModelCallContext,
    /// 该调用产出的回复。
    pub reply: Option<MessageId>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelPeers {
    pub purpose: ModelCallPurpose,
    pub model_id: ModelId,
    pub median_ms: i64,
    pub samples: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolPeers {
    pub name: String,
    pub median_ms: i64,
    pub samples: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Peers {
    pub models: Vec<ModelPeers>,
    pub tools: Vec<ToolPeers>,
}

/// 调用聚合；token 只合计已知项，未知另计次数。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct CallStat {
    pub calls: u32,
    pub failed_calls: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub unknown_usage: u32,
    pub cost: f64,
    pub unknown_cost: u32,
    /// 成功调用的平均耗时。
    pub avg_ms: Option<i64>,
    #[serde(skip)]
    ok_ms: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DayStat {
    pub day: String,
    pub runs: u32,
    pub completed_runs: u32,
    #[serde(flatten)]
    pub calls: CallStat,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelStat {
    pub purpose: ModelCallPurpose,
    pub model_id: Option<ModelId>,
    pub model: String,
    #[serde(flatten)]
    pub calls: CallStat,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolStat {
    pub name: String,
    pub calls: u32,
    pub errors: u32,
    pub avg_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Dashboard {
    pub days: Vec<DayStat>,
    pub models: Vec<ModelStat>,
    pub tools: Vec<ToolStat>,
}

/// 调用费用（美元）：公式见 docs/blueprints/run-trace.md §六。任一依据未知则为 `None`。
/// 缓存写入可选：未上报按 0，未设写入价按输入价。
pub fn call_cost(usage: Option<&Usage>, price: &ModelPrice) -> Option<f64> {
    let usage = usage?;
    let read = usage.cache_read_tokens?;
    let write = usage.cache_write_tokens.unwrap_or(0);
    let fresh = usage.input_tokens.saturating_sub(read + write);
    let term = |tokens: u64, price: Option<f64>| match tokens {
        0 => Some(0.0),
        n => price.map(|p| n as f64 * p),
    };
    Some(
        (term(fresh, price.input)?
            + term(read, price.cache_read)?
            + term(write, price.cache_write.or(price.input))?
            + term(usage.output_tokens, price.output)?)
            / 1e6,
    )
}

fn median(mut values: Vec<i64>) -> i64 {
    values.sort_unstable();
    let mid = values.len() / 2;
    if values.len().is_multiple_of(2) {
        values[mid - 1] + (values[mid] - values[mid - 1]) / 2
    } else {
        values[mid]
    }
}

fn mean(values: &[i64]) -> Option<i64> {
    (!values.is_empty()).then(|| values.iter().sum::<i64>() / values.len() as i64)
}

const CALL_SELECT: &str = "SELECT c.id, c.purpose, c.model_id, c.model, c.reasoning_effort,
        c.started_at, c.first_output_at, c.finished_at, c.stop, c.error,
        c.input_tokens, c.output_tokens, c.cache_read_tokens, c.cache_write_tokens, c.reasoning_tokens,
        m.price_input, m.price_cache_read, m.price_cache_write, m.price_output,
        (SELECT id FROM core_messages WHERE model_call_id = c.id AND kind = 'Reply'),
        (SELECT id FROM core_messages WHERE model_call_id = c.id AND kind = 'Boundary'),
        (SELECT id FROM core_messages WHERE model_call_id = c.id AND kind = 'SideOutput'),
        c.context_boundary_id, c.context_through_id,
        strftime('%Y-%m-%d', c.started_at / 1000, 'unixepoch', 'localtime')
 FROM core_model_calls c LEFT JOIN core_models m ON m.id = c.model_id";

fn call(r: &Row<'_>) -> rusqlite::Result<TraceCall> {
    let tokens = |i: usize| r.get::<_, Option<i64>>(i).map(|v| v.map(|n| n as u64));
    let usage = tokens(10)?.map(|input_tokens| -> rusqlite::Result<Usage> {
        Ok(Usage {
            input_tokens,
            output_tokens: tokens(11)?.unwrap_or(0),
            cache_read_tokens: tokens(12)?,
            cache_write_tokens: tokens(13)?,
            reasoning_tokens: tokens(14)?,
        })
    });
    let usage = usage.transpose()?;
    let price = ModelPrice {
        input: r.get(15)?,
        cache_read: r.get(16)?,
        cache_write: r.get(17)?,
        output: r.get(18)?,
    };
    Ok(TraceCall {
        id: ModelCallId(r.get(0)?),
        purpose: r.get(1)?,
        model_id: r.get::<_, Option<i64>>(2)?.map(ModelId),
        model: r.get(3)?,
        reasoning_effort: r.get(4)?,
        started_at: r.get(5)?,
        first_output_at: r.get(6)?,
        finished_at: r.get(7)?,
        stop: r.get(8)?,
        error: r.get(9)?,
        cost: call_cost(usage.as_ref(), &price),
        usage,
        attempt: 1,
        reply_id: r.get::<_, Option<i64>>(19)?.map(MessageId),
        boundary_id: r.get::<_, Option<i64>>(20)?.map(MessageId),
        side_id: r.get::<_, Option<i64>>(21)?.map(MessageId),
        key: (r.get(22)?, r.get(23)?),
        day: r.get(24)?,
    })
}

/// 同一 run 内连续、同目的同输入且上一次失败的调用算同一组重试。
fn number_attempts(calls: &mut [TraceCall]) {
    for i in 1..calls.len() {
        let (before, rest) = calls.split_at_mut(i);
        let (prev, this) = (&before[i - 1], &mut rest[0]);
        if prev.error.is_some() && prev.purpose == this.purpose && prev.key == this.key {
            this.attempt = prev.attempt + 1;
        }
    }
}

fn load_calls(conn: &Connection, filter: &str, arg: i64) -> Result<Vec<TraceCall>, StoreError> {
    let mut calls = conn
        .prepare(&format!("{CALL_SELECT} WHERE {filter} ORDER BY c.id"))?
        .query_map([arg], call)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    number_attempts(&mut calls);
    Ok(calls)
}

/// 已知终点的工具结果：取消与派发的没有可靠终点。
fn tool_end(m: &TraceMessage) -> Option<i64> {
    let t = m.tool.as_ref()?;
    t.started_at?;
    matches!(
        t.outcome,
        ToolTraceOutcome::Completed | ToolTraceOutcome::Failed
    )
    .then_some(m.created_at)
}

fn run_stats(
    created_at: i64,
    finished_at: Option<i64>,
    calls: &[TraceCall],
    messages: &[TraceMessage],
) -> RunStats {
    // 每步以其最后一次尝试为准；工具结果归属它之前最近的回复。
    let step_ends: Vec<&TraceCall> = calls
        .iter()
        .enumerate()
        .filter(|(i, _)| calls.get(i + 1).is_none_or(|next| next.attempt == 1))
        .map(|(_, c)| c)
        .collect();
    let tool_ms = step_ends
        .iter()
        .map(|step| {
            let owned = messages.iter().filter(|m| {
                m.tool.is_some()
                    && step.reply_id.is_some_and(|r| r < m.id)
                    && !step_ends.iter().any(|o| {
                        o.reply_id
                            .is_some_and(|r| step.reply_id < Some(r) && r < m.id)
                    })
            });
            owned
                .filter_map(tool_end)
                .max()
                .map_or(0, |end| (end - step.finished_at).max(0))
        })
        .sum();
    // 失败与被停止的调用拿不到用量，按零计，不影响完整性与费用。
    let settled: Vec<&TraceCall> = calls
        .iter()
        .filter(|c| c.error.is_none() && c.stop != Some(StopReason::Stopped))
        .collect();
    let known: Vec<&Usage> = calls.iter().filter_map(|c| c.usage.as_ref()).collect();
    let usage_complete = settled.iter().all(|c| c.usage.is_some());
    let (speed_tokens, speed_ms) = calls
        .iter()
        .filter_map(|c| match (c.usage.as_ref(), c.first_output_at) {
            (Some(u), Some(first)) if c.finished_at > first => {
                Some((u.output_tokens, c.finished_at - first))
            }
            _ => None,
        })
        .fold((0, 0), |(t, ms), (dt, dms)| (t + dt, ms + dms));
    RunStats {
        total_ms: finished_at.map(|f| f - created_at),
        model_ms: calls.iter().map(|c| c.finished_at - c.started_at).sum(),
        tool_ms,
        steps: step_ends.len() as u32,
        requests: calls.len() as u32,
        tools: messages.iter().filter(|m| m.tool.is_some()).count() as u32,
        first_output_ms: calls
            .iter()
            .filter(|c| c.purpose == ModelCallPurpose::Conversation)
            .find_map(|c| Some(c.first_output_at? - c.started_at)),
        output_tps: (speed_ms > 0).then(|| speed_tokens as f64 * 1000.0 / speed_ms as f64),
        input_tokens: known.iter().map(|u| u.input_tokens).sum(),
        output_tokens: known.iter().map(|u| u.output_tokens).sum(),
        usage_complete,
        cache_read_tokens: known.iter().filter_map(|u| u.cache_read_tokens).sum(),
        cache_complete: usage_complete && known.iter().all(|u| u.cache_read_tokens.is_some()),
        cost: settled.iter().try_fold(0.0, |n, c| Some(n + c.cost?)),
    }
}

fn load_run(conn: &Connection, run: crate::Run) -> Result<TraceRun, StoreError> {
    let messages = conn
        .prepare(&format!(
            "SELECT {MESSAGE_COLS} FROM {MESSAGE_FROM} WHERE m.run_id = ?1 ORDER BY m.id"
        ))?
        .query_map([run.id.0], row::message)?
        .map(|m| m.map(trace_message))
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let calls = load_calls(conn, "c.run_id = ?1", run.id.0)?;
    Ok(TraceRun {
        id: run.id,
        state: run.state.as_str(),
        created_at: run.created_at,
        finished_at: run.finished_at,
        stats: run_stats(run.created_at, run.finished_at, &calls, &messages),
        calls,
        messages,
    })
}

fn clip(text: &str) -> String {
    let flat: String = text.chars().take(PREVIEW_CHARS).collect();
    flat
}

fn parts_text(parts: &[ContentPart]) -> String {
    parts
        .iter()
        .map(|p| match p {
            ContentPart::Text { text } => text.as_str(),
            ContentPart::File(_) => "[文件]",
            ContentPart::Image(_) => "[图片]",
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn trace_message(m: Message) -> TraceMessage {
    let (kind, preview, tool) = match &m.body {
        MessageBody::Command { command, .. } => (
            "Command",
            match command {
                SessionCommand::Compact { text: Some(t) } => format!("/compact {t}"),
                SessionCommand::Compact { text: None } => "/compact".into(),
                SessionCommand::Clear => "/clear".into(),
                SessionCommand::RevokeCompaction { .. } => "撤回压缩".into(),
            },
            None,
        ),
        MessageBody::UserInput { parts, .. } => ("UserInput", parts_text(parts), None),
        MessageBody::Reply { blocks, .. } => {
            let text: Vec<&str> = blocks
                .iter()
                .filter_map(|b| match b {
                    ReplyBlock::Text { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            let preview = if text.is_empty() {
                blocks
                    .iter()
                    .filter_map(|b| match b {
                        ReplyBlock::ToolCall { name, .. } => Some(name.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            } else {
                text.join(" ")
            };
            ("Reply", preview, None)
        }
        MessageBody::ToolResult {
            tool_name,
            tool_call_id,
            outcome,
            started_at,
        } => {
            let (kind, text) = match outcome {
                ToolResultOutcome::Terminal(ExecOutcome::Completed { output }) => {
                    (ToolTraceOutcome::Completed, parts_text(output))
                }
                ToolResultOutcome::Terminal(ExecOutcome::Failed { message, .. }) => {
                    (ToolTraceOutcome::Failed, message.clone())
                }
                ToolResultOutcome::Terminal(ExecOutcome::Cancelled { message }) => {
                    (ToolTraceOutcome::Cancelled, message.clone())
                }
                ToolResultOutcome::Dispatched { .. } => {
                    (ToolTraceOutcome::Dispatched, String::new())
                }
            };
            let error = matches!(kind, ToolTraceOutcome::Failed | ToolTraceOutcome::Cancelled)
                .then(|| clip(&text));
            (
                "ToolResult",
                text,
                Some(TraceTool {
                    name: tool_name.clone(),
                    call_id: tool_call_id.clone(),
                    started_at: *started_at,
                    outcome: kind,
                    error,
                }),
            )
        }
        MessageBody::Completion { tool_name, .. } => ("Completion", tool_name.clone(), None),
        MessageBody::JobNotice { text, .. } => ("JobNotice", text.clone(), None),
        MessageBody::Trigger { text, .. } => ("Trigger", text.clone(), None),
        MessageBody::HarnessNote { text } => ("HarnessNote", text.clone(), None),
        MessageBody::Notification { text, .. } => ("Notification", text.clone(), None),
        MessageBody::Boundary { boundary } => (
            "Boundary",
            match boundary {
                ContextBoundary::Compaction(c) => c.summary.clone(),
                ContextBoundary::UserClear => "清空上下文".into(),
            },
            None,
        ),
        MessageBody::SideOutput { text, .. } => ("SideOutput", text.clone(), None),
    };
    TraceMessage {
        id: m.id,
        kind,
        created_at: m.created_at,
        preview: clip(&preview),
        tool,
    }
}

impl Store {
    /// 所有已记录调用，包括重试、压缩、标题；整树递归含归档、隐藏后代。
    pub async fn session_usage(&self, session: SessionId) -> Result<SessionUsageView, StoreError> {
        self.call(move |conn| {
            let own = session_usage(&load_calls(conn, "c.session_id = ?1", session.0)?);
            let descendants: u32 = conn.query_row(
                "WITH RECURSIVE tree(id) AS (SELECT id FROM core_sessions WHERE parent_session_id = ?1
                 UNION ALL SELECT s.id FROM core_sessions s JOIN tree t ON s.parent_session_id = t.id)
                 SELECT count(*) FROM tree",
                [session.0],
                |r| r.get(0),
            )?;
            let tree = if descendants == 0 {
                None
            } else {
                let calls = load_calls(
                    conn,
                    "c.session_id IN (WITH RECURSIVE tree(id) AS (SELECT ?1 UNION ALL
                     SELECT s.id FROM core_sessions s JOIN tree t ON s.parent_session_id = t.id)
                     SELECT id FROM tree)",
                    session.0,
                )?;
                Some(TreeUsage {
                    descendants,
                    usage: session_usage(&calls),
                })
            };
            Ok(SessionUsageView { own, tree })
        })
        .await
    }

    /// 有已结束 run 的会话及其祖先，扁平返回，前端按 `parent` 建树；统计只算会话自身。
    pub async fn trace_sessions(&self) -> Result<Vec<TraceSession>, StoreError> {
        self.call(|conn| {
            Ok(conn
                .prepare(
                    "WITH RECURSIVE
                     stats AS (
                       SELECT session_id AS id, max(created_at) AS last_run_at,
                         count(CASE WHEN state = 'provider_failed' THEN 1 END) AS failed,
                         count(CASE WHEN state = 'interrupted' THEN 1 END) AS interrupted
                       FROM core_runs WHERE state <> 'executing' GROUP BY session_id),
                     shown(id) AS (
                       SELECT id FROM stats
                       UNION SELECT s.parent_session_id FROM core_sessions s JOIN shown ON s.id = shown.id
                       WHERE s.parent_session_id IS NOT NULL),
                     top(id, top) AS (
                       SELECT id, id FROM shown
                       UNION SELECT top.id, s.parent_session_id FROM top
                       JOIN core_sessions s ON s.id = top.top WHERE s.parent_session_id IS NOT NULL)
                     SELECT s.id, s.kind, s.parent_session_id,
                       CASE s.kind WHEN 'root' THEN s.title WHEN 'task' THEN j.description END,
                       j.id, CASE t.kind WHEN 'triggered' THEN t.trigger_module ELSE t.channel END,
                       st.last_run_at, coalesce(st.failed, 0), coalesce(st.interrupted, 0),
                       (SELECT count(*) FROM core_messages m
                         WHERE m.session_id = s.id AND m.kind = 'ToolResult'
                           AND json_extract(m.payload, '$.outcome.Terminal.Failed') IS NOT NULL)
                     FROM shown JOIN core_sessions s ON s.id = shown.id
                     JOIN top ON top.id = s.id
                       AND NOT EXISTS (SELECT 1 FROM core_sessions p WHERE p.id = top.top AND p.parent_session_id IS NOT NULL)
                     JOIN core_sessions t ON t.id = top.top
                     LEFT JOIN core_jobs j ON j.child_session_id = s.id
                     LEFT JOIN stats st ON st.id = s.id
                     ORDER BY st.last_run_at DESC",
                )?
                .query_map([], |r| {
                    Ok(TraceSession {
                        session: SessionId(r.get(0)?),
                        kind: match r.get_ref(1)?.as_str()? {
                            "root" => TraceSessionKind::Root,
                            "task" => TraceSessionKind::Task,
                            "triggered" => TraceSessionKind::Triggered,
                            other => {
                                return Err(row::corrupt(1, format!("未知 session kind: {other}")))
                            }
                        },
                        parent: r.get::<_, Option<i64>>(2)?.map(SessionId),
                        label: r.get(3)?,
                        job: r.get(4)?,
                        channel: r.get(5)?,
                        last_run_at: r.get(6)?,
                        failed_runs: r.get(7)?,
                        interrupted_runs: r.get(8)?,
                        tool_errors: r.get(9)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .await
    }

    /// 已结束的 run，新到旧，每页若干个完整 run。
    pub async fn trace_runs(
        &self,
        session: SessionId,
        after: Option<RunId>,
        limit: u32,
    ) -> Result<TraceRunPage, StoreError> {
        self.call(move |conn| {
            let mut runs = conn
                .prepare(&format!(
                    "SELECT {RUN_COLS} FROM core_runs
                     WHERE session_id = ?1 AND state <> 'executing' AND (?2 IS NULL OR id > ?2)
                     ORDER BY id ASC LIMIT ?3"
                ))?
                .query_map(
                    params![session.0, after.map(|r| r.0), limit as i64 + 1],
                    row::run,
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let more = runs.len() > limit as usize;
            runs.truncate(limit as usize);
            let next_after = more.then(|| runs.last().expect("limit ≥ 1").id);
            let out = runs
                .into_iter()
                .map(|run| load_run(conn, run))
                .collect::<Result<Vec<_>, _>>()?;
            let title_calls = if after.is_none() {
                load_calls(conn, "c.session_id = ?1 AND c.purpose = 'title'", session.0)?
            } else {
                Vec::new()
            };
            Ok(TraceRunPage {
                runs: out,
                title_calls,
                next_after,
            })
        })
        .await
    }

    pub async fn message(&self, id: MessageId) -> Result<Option<Message>, StoreError> {
        Ok(self.messages_by_ids(vec![id]).await?.pop())
    }

    pub async fn call_record(&self, id: ModelCallId) -> Result<Option<CallRecord>, StoreError> {
        self.call(move |conn| {
            let Some((session_id, purpose, boundary_id, through, skip, reply, keep_from)) = conn
                .query_row(
                    "SELECT session_id, purpose, context_boundary_id, context_through_id, context_skip,
                       (SELECT id FROM core_messages WHERE model_call_id = c.id AND kind = 'Reply'),
                       context_keep_from
                     FROM core_model_calls c WHERE id = ?1",
                    [id.0],
                    |r| {
                        Ok((
                            SessionId(r.get(0)?),
                            r.get::<_, ModelCallPurpose>(1)?,
                            r.get::<_, Option<i64>>(2)?.map(MessageId),
                            r.get::<_, Option<i64>>(3)?.map(MessageId),
                            r.get::<_, u32>(4)?,
                            r.get::<_, Option<i64>>(5)?.map(MessageId),
                            r.get::<_, Option<i64>>(6)?.map(MessageId),
                        ))
                    },
                )
                .optional()?
            else {
                return Ok(None);
            };
            let mut context = ModelCallContext {
                boundary_id,
                through,
                skip,
                keep_from,
                ..Default::default()
            };
            let mut stmt =
                conn.prepare("SELECT message_id, role FROM core_model_call_inputs WHERE call_id = ?1 ORDER BY message_id")?;
            let mut rows = stmt.query([id.0])?;
            while let Some(r) = rows.next()? {
                let (message, role): (i64, String) = (r.get(0)?, r.get(1)?);
                let list = match role.as_str() {
                    "excluded" => &mut context.excluded,
                    "elide" => &mut context.elide,
                    _ => &mut context.instruction,
                };
                list.push(MessageId(message));
            }
            Ok(Some(CallRecord {
                session_id,
                purpose,
                context,
                reply,
            }))
        })
        .await
    }

    /// 全库成功样本的耗时中位数：模型按 `(purpose, model_id)`，工具按名字。
    pub async fn call_peers(&self) -> Result<Peers, StoreError> {
        self.call(|conn| {
            let mut models: BTreeMap<(&'static str, i64), (ModelCallPurpose, Vec<i64>)> =
                BTreeMap::new();
            let mut stmt = conn.prepare(
                "SELECT purpose, model_id, finished_at - started_at FROM core_model_calls
                 WHERE error IS NULL AND model_id IS NOT NULL AND stop IS NOT 'stopped'",
            )?;
            let mut rows = stmt.query([])?;
            while let Some(r) = rows.next()? {
                let purpose: ModelCallPurpose = r.get(0)?;
                models
                    .entry((purpose.as_str(), r.get(1)?))
                    .or_insert_with(|| (purpose, Vec::new()))
                    .1
                    .push(r.get(2)?);
            }
            let mut tools: BTreeMap<String, Vec<i64>> = BTreeMap::new();
            let mut stmt = conn.prepare(
                "SELECT json_extract(payload, '$.tool_name'), created_at - json_extract(payload, '$.started_at')
                 FROM core_messages
                 WHERE kind = 'ToolResult' AND json_extract(payload, '$.started_at') IS NOT NULL
                   AND json_extract(payload, '$.outcome.Terminal.Completed') IS NOT NULL",
            )?;
            let mut rows = stmt.query([])?;
            while let Some(r) = rows.next()? {
                tools.entry(r.get(0)?).or_default().push(r.get(1)?);
            }
            Ok(Peers {
                models: models
                    .into_iter()
                    .map(|((_, id), (purpose, ms))| ModelPeers {
                        purpose,
                        model_id: ModelId(id),
                        samples: ms.len() as u32,
                        median_ms: median(ms),
                    })
                    .collect(),
                tools: tools
                    .into_iter()
                    .map(|(name, ms)| ToolPeers {
                        name,
                        samples: ms.len() as u32,
                        median_ms: median(ms),
                    })
                    .collect(),
            })
        })
        .await
    }

    /// 最近 `days` 天（含今天，按本地日历）的聚合；不读正文。
    pub async fn dashboard(&self, days: u32) -> Result<Dashboard, StoreError> {
        let since = format!("-{} days", days.saturating_sub(1));
        self.call(move |conn| {
            let day =
                |col: &str| format!("strftime('%Y-%m-%d', {col} / 1000, 'unixepoch', 'localtime')");
            let floor = "date('now', 'localtime', ?1)";
            let mut by_day: BTreeMap<String, DayStat> = BTreeMap::new();
            let blank = |d: &str| DayStat {
                day: d.to_owned(),
                runs: 0,
                completed_runs: 0,
                calls: CallStat::default(),
            };
            let mut stmt = conn.prepare(&format!(
                "SELECT {d}, count(*), count(CASE WHEN state = 'completed' THEN 1 END)
                 FROM core_runs WHERE state NOT IN ('executing', 'stopped') AND {d} >= {floor} GROUP BY 1",
                d = day("created_at")
            ))?;
            let mut rows = stmt.query([&since])?;
            while let Some(r) = rows.next()? {
                let d: String = r.get(0)?;
                let e = by_day.entry(d.clone()).or_insert_with(|| blank(&d));
                e.runs = r.get(1)?;
                e.completed_runs = r.get(2)?;
            }
            let mut by_model: BTreeMap<(&'static str, i64, String), ModelStat> = BTreeMap::new();
            let mut stmt = conn.prepare(&format!(
                "{CALL_SELECT} WHERE {d} >= {floor} ORDER BY c.id",
                d = day("c.started_at")
            ))?;
            let calls = stmt
                .query_map([&since], call)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for c in calls.into_iter().filter(|c| c.stop != Some(StopReason::Stopped)) {
                let d = c.day.clone();
                let model_key = (
                    c.purpose.as_str(),
                    c.model_id.map_or(0, |m| m.0),
                    c.model.clone(),
                );
                let stat = by_model.entry(model_key).or_insert_with(|| ModelStat {
                    purpose: c.purpose,
                    model_id: c.model_id,
                    model: c.model.clone(),
                    calls: CallStat::default(),
                });
                let day_stat = by_day.entry(d.clone()).or_insert_with(|| blank(&d));
                for s in [&mut stat.calls, &mut day_stat.calls] {
                    s.add(&c);
                }
            }
            let mut by_tool: BTreeMap<String, (u32, u32, Vec<i64>)> = BTreeMap::new();
            let mut stmt = conn.prepare(&format!(
                "SELECT json_extract(payload, '$.tool_name'),
                        json_extract(payload, '$.outcome.Terminal.Failed') IS NOT NULL,
                        CASE WHEN json_extract(payload, '$.outcome.Terminal.Completed') IS NOT NULL
                             THEN created_at - json_extract(payload, '$.started_at') END
                 FROM core_messages WHERE kind = 'ToolResult' AND {d} >= {floor}",
                d = day("created_at")
            ))?;
            let mut rows = stmt.query([&since])?;
            while let Some(r) = rows.next()? {
                let e = by_tool.entry(r.get(0)?).or_default();
                e.0 += 1;
                e.1 += u32::from(r.get::<_, bool>(1)?);
                e.2.extend(r.get::<_, Option<i64>>(2)?);
            }
            let finish = |mut s: CallStat| {
                s.avg_ms = mean(&s.ok_ms);
                s
            };
            Ok(Dashboard {
                days: by_day
                    .into_values()
                    .map(|mut d| {
                        d.calls = finish(d.calls);
                        d
                    })
                    .collect(),
                models: by_model
                    .into_values()
                    .map(|mut m| {
                        m.calls = finish(m.calls);
                        m
                    })
                    .collect(),
                tools: by_tool
                    .into_iter()
                    .map(|(name, (calls, errors, ms))| ToolStat {
                        name,
                        calls,
                        errors,
                        avg_ms: mean(&ms),
                    })
                    .collect(),
            })
        })
        .await
    }
}

impl CallStat {
    fn add(&mut self, c: &TraceCall) {
        self.calls += 1;
        if c.error.is_some() {
            self.failed_calls += 1;
        } else {
            self.ok_ms.push(c.finished_at - c.started_at);
        }
        match &c.usage {
            Some(u) => {
                self.input_tokens += u.input_tokens;
                self.output_tokens += u.output_tokens;
                self.cache_read_tokens += u.cache_read_tokens.unwrap_or(0);
                self.cache_write_tokens += u.cache_write_tokens.unwrap_or(0);
            }
            None => self.unknown_usage += 1,
        }
        match c.cost {
            Some(cost) => self.cost += cost,
            None => self.unknown_cost += 1,
        }
    }
}

#[cfg(test)]
mod tests;
