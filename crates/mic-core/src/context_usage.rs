use crate::compaction::Occupancy;
use crate::limits::{ASCII_BYTES_PER_TOKEN, IMAGE_TOKENS, NON_ASCII_TOKENS};
use crate::request::{PromptPartKind, SystemPrompt};
use crate::ModelRequest;
use mic_message::{ContentPart, Message, ModelView, Reasoning, ReplyBlock};
use mic_store::ContextUsage;
use mic_tool::ToolSpec;
use serde::Serialize;

pub(crate) fn text_tokens(text: &str) -> u64 {
    let ascii = text.chars().filter(char::is_ascii).count() as u64;
    ascii.div_ceil(ASCII_BYTES_PER_TOKEN) + (text.chars().count() as u64 - ascii) * NON_ASCII_TOKENS
}

fn parts_tokens(parts: &[ContentPart]) -> u64 {
    parts
        .iter()
        .map(|part| match part {
            ContentPart::Text { text } => text_tokens(text),
            ContentPart::Image(_) => IMAGE_TOKENS,
            ContentPart::File(file) => text_tokens(&file.path),
        })
        .sum()
}

pub(crate) fn message_tokens(message: &Message) -> u64 {
    match message.model_view() {
        Some(ModelView::User(parts)) | Some(ModelView::Tool { output: parts, .. }) => {
            parts_tokens(&parts) + 8
        }
        Some(ModelView::Assistant { blocks, .. }) => {
            blocks.iter().map(block_tokens).sum::<u64>() + 8
        }
        None => 0,
    }
}

fn block_tokens(block: &ReplyBlock) -> u64 {
    match block {
        ReplyBlock::Text { text, .. } | ReplyBlock::Reasoning(Reasoning::Visible { text, .. }) => {
            text_tokens(text)
        }
        ReplyBlock::Reasoning(Reasoning::Redacted { data }) => text_tokens(data),
        ReplyBlock::ToolCall { name, args, .. } => {
            text_tokens(name) + text_tokens(&args.to_string())
        }
    }
}

fn tool_call_tokens(blocks: &[ReplyBlock]) -> u64 {
    blocks
        .iter()
        .filter(|b| matches!(b, ReplyBlock::ToolCall { .. }))
        .map(block_tokens)
        .sum()
}

fn spec_tokens(spec: &ToolSpec) -> u64 {
    text_tokens(&spec.name)
        + text_tokens(&spec.description)
        + text_tokens(&spec.parameters.to_string())
}

/// 按字数粗估整个请求，只供压缩前裁剪历史；占用与触发都用上游用量。
pub(crate) fn estimate_tokens(req: &ModelRequest) -> u64 {
    text_tokens(&req.system)
        + req.tools.iter().map(spec_tokens).sum::<u64>()
        + req.messages.iter().map(message_tokens).sum::<u64>()
}

/// 上次对话调用的上下文构成。契约：docs/blueprints/context-breakdown.md。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextBreakdown {
    pub used_tokens: u64,
    pub window: u32,
    pub trigger_percent: u8,
    /// 固定 6 项，顺序同 [`ContextKind`] 声明。
    pub parts: Vec<ContextPart>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextPart {
    pub kind: ContextKind,
    pub tokens: u64,
    /// 仅 `conversation` 有值：其中的工具调用与结果。
    pub tool_tokens: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextKind {
    Instructions,
    Persona,
    Tools,
    Environment,
    Summary,
    Conversation,
}

const KINDS: [ContextKind; 6] = [
    ContextKind::Instructions,
    ContextKind::Persona,
    ContextKind::Tools,
    ContextKind::Environment,
    ContextKind::Summary,
    ContextKind::Conversation,
];

impl From<PromptPartKind> for ContextKind {
    fn from(kind: PromptPartKind) -> Self {
        match kind {
            PromptPartKind::Instructions => Self::Instructions,
            PromptPartKind::Persona => Self::Persona,
            PromptPartKind::Tools => Self::Tools,
            PromptPartKind::Environment => Self::Environment,
            PromptPartKind::Summary => Self::Summary,
        }
    }
}

/// 估算各类权重后缩放到上游用量：输入按比例分到六类，输出整体归对话。
pub(crate) fn breakdown(
    occupancy: Occupancy,
    usage: &ContextUsage,
    system: &SystemPrompt,
    tools: &[ToolSpec],
    messages: &[Message],
    output: &[ReplyBlock],
) -> ContextBreakdown {
    let mut weights = [0u64; 6];
    for part in system.blocks.iter().flat_map(|b| &b.parts) {
        weights[ContextKind::from(part.kind) as usize] += text_tokens(&part.text);
    }
    weights[ContextKind::Tools as usize] += tools.iter().map(spec_tokens).sum::<u64>();
    let conversation = ContextKind::Conversation as usize;
    weights[conversation] = messages.iter().map(message_tokens).sum();
    let tool_in: u64 = messages
        .iter()
        .map(|m| match m.model_view() {
            Some(ModelView::Tool { .. }) => message_tokens(m),
            Some(ModelView::Assistant { blocks, .. }) => tool_call_tokens(blocks),
            _ => 0,
        })
        .sum();

    let mut tokens = apportion(usage.input_tokens, &weights);
    let output_tokens = usage.output_tokens;
    let tool_tokens = share(tokens[conversation], tool_in, weights[conversation])
        + share(
            output_tokens,
            tool_call_tokens(output),
            output.iter().map(block_tokens).sum(),
        );
    tokens[conversation] += output_tokens;
    ContextBreakdown {
        used_tokens: occupancy.used_tokens,
        window: occupancy.window,
        trigger_percent: occupancy.trigger_percent,
        parts: KINDS
            .iter()
            .zip(tokens)
            .map(|(&kind, tokens)| ContextPart {
                kind,
                tokens,
                tool_tokens: (kind == ContextKind::Conversation).then_some(tool_tokens),
            })
            .collect(),
    }
}

/// 最大余数法：按权重分 `total`，合计严格等于 `total`。
fn apportion(total: u64, weights: &[u64]) -> Vec<u64> {
    let sum: u128 = weights.iter().map(|&w| u128::from(w)).sum();
    let exact: Vec<(u64, u128)> = weights
        .iter()
        .map(|&w| {
            let n = u128::from(total) * u128::from(w);
            ((n / sum) as u64, n % sum)
        })
        .collect();
    let mut out: Vec<u64> = exact.iter().map(|(q, _)| *q).collect();
    let mut order: Vec<usize> = (0..out.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(exact[i].1));
    let rest = total - out.iter().sum::<u64>();
    for &i in order.iter().take(rest as usize) {
        out[i] += 1;
    }
    out
}

fn share(total: u64, part: u64, whole: u64) -> u64 {
    if whole == 0 {
        return 0;
    }
    (u128::from(total) * u128::from(part) / u128::from(whole)) as u64
}
