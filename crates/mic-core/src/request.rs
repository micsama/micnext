//! 由上下文窗口组装模型请求。契约：docs/blueprints/run-execution.md §4.4。

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use mic_message::{
    header_legend, ContentPart, ImageData, ImageId, Message, MessageBody, ModelView, ReplyBlock,
};
use mic_store::{ContextWindow, RunSettings};
use mic_tool::ToolHandle;
use serde::Serialize;

use crate::context_usage::text_tokens;
use crate::limits::{
    ASCII_BYTES_PER_TOKEN, NON_ASCII_TOKENS, QUOTE_FULL_TOKENS, QUOTE_HEAD_TOKENS,
    QUOTE_TAIL_TOKENS,
};
use crate::{ModelRequest, ToolChoice};

const BASE_PROMPT: &str = include_str!("prompts/system.md");

static INSTRUCTIONS: LazyLock<String> =
    LazyLock::new(|| format!("{}\n\n{}", BASE_PROMPT.trim_end(), header_legend()));

static INSTRUCTIONS_BLOCK: LazyLock<String> =
    LazyLock::new(|| block("instructions", &INSTRUCTIONS));

/// `instructions` 块，设置页只读展示。
pub(crate) fn base_prompt() -> &'static str {
    &INSTRUCTIONS_BLOCK
}

fn block(tag: &str, body: &str) -> String {
    format!("<{tag}>\n{body}\n</{tag}>")
}

/// 系统提示的结构：模型请求、trace 预览、上下文构成都由它而来。
/// 契约：docs/blueprints/context-breakdown.md §二。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SystemPrompt {
    pub blocks: Vec<PromptBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PromptBlock {
    pub kind: PromptBlockKind,
    pub parts: Vec<PromptPart>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PromptPart {
    pub kind: PromptPartKind,
    /// 实际发送的完整段落。
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptBlockKind {
    Instructions,
    Persona,
    Context,
}

impl PromptBlockKind {
    fn tag(self) -> &'static str {
        match self {
            Self::Instructions => "instructions",
            Self::Persona => "persona",
            Self::Context => "context",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptPartKind {
    Instructions,
    Persona,
    Environment,
    Skills,
    Summary,
}

impl SystemPrompt {
    /// 发给模型的原文。
    pub fn render(&self) -> String {
        self.blocks
            .iter()
            .map(|b| {
                let body: Vec<&str> = b.parts.iter().map(|p| p.text.as_str()).collect();
                block(b.kind.tag(), &body.join("\n\n"))
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

const IMAGE_PLACEHOLDER: &str = "[图片：当前模型不支持查看]";

pub(crate) fn hide_images(messages: &mut [Message]) {
    for m in messages {
        if let MessageBody::UserInput { parts, .. } = &mut m.body {
            for part in parts
                .iter_mut()
                .filter(|p| matches!(p, ContentPart::Image(_)))
            {
                *part = ContentPart::Text {
                    text: IMAGE_PLACEHOLDER.into(),
                };
            }
        }
    }
}

/// 窗口内消息引用的图片 id。
pub(crate) fn image_ids(window: &ContextWindow) -> Vec<ImageId> {
    window
        .messages
        .iter()
        .filter_map(|m| match &m.body {
            MessageBody::UserInput { parts, .. } => Some(parts),
            _ => None,
        })
        .flatten()
        .filter_map(|p| match p {
            ContentPart::Image(r) => Some(r.id),
            _ => None,
        })
        .collect()
}

pub(crate) fn build(
    session: &mic_store::Session,
    window: ContextWindow,
    images: HashMap<ImageId, ImageData>,
    system: SystemPrompt,
    tools: &[ToolHandle],
) -> ModelRequest {
    ModelRequest {
        tool_choice: ToolChoice::Auto,
        system: system.render(),
        messages: order(window.messages),
        tools: tools.iter().map(|t| t.spec().clone()).collect(),
        images,
        cache_key: format!("micnext-s{}", session.id.0),
        plain_after: window.plain_after,
    }
}

/// 三块顺序固定：instructions → persona → context，保证同一会话前缀稳定。子 agent 不带人设，
/// persona 块只剩用户偏好，空则略去。契约：docs/blueprints/system-prompt-layout.md §三。
pub(crate) fn system_prompt(
    settings: &RunSettings,
    session: &mic_store::Session,
    channel_prompt: Option<&str>,
    skills: Option<&str>,
    env: &[(String, String)],
    window: &ContextWindow,
) -> SystemPrompt {
    use PromptPartKind::*;
    let part = |kind, text: String| PromptPart { kind, text };
    let mut persona = Vec::new();
    if matches!(session.kind, mic_store::SessionKind::Root { .. }) {
        persona.push(part(Persona, settings.persona.prompt.clone()));
    }
    if !settings.general_prompt.is_empty() {
        persona.push(part(
            Persona,
            format!("User preferences:\n{}", settings.general_prompt),
        ));
    }
    let mut context = vec![part(
        Environment,
        format!("Working directory: {}", session.pwd),
    )];
    context.extend(channel_prompt.map(|p| part(Environment, p.to_owned())));
    if !env.is_empty() {
        let names: Vec<String> = env.iter().map(|(name, _)| format!("`${name}`")).collect();
        context.push(part(
            Environment,
            format!(
                "Environment variables the user set for commands: {}. Use them directly; their values reach you masked as `<NAME>`.",
                names.join(", ")
            ),
        ));
    }
    context.push(part(
        Environment,
        if window.plain_after.is_some() {
            "Credential masking: the user has temporarily turned it off for new messages, so credentials in them reach you as-is. Earlier content stays masked (e.g. `sk-a***wxyz`), and everything will be masked again once the user turns it back on."
        } else {
            "Credential masking: credentials are masked before reaching you (e.g. `sk-a***wxyz`); the real values stay intact locally, so never write a masked value back as if it were real."
        }
        .to_owned(),
    ));
    context.extend(skills.map(|s| part(Skills, s.to_owned())));
    if let Some(summary) = &window.summary {
        context.push(part(
            Summary,
            format!("Summary of the earlier conversation:\n{summary}"),
        ));
    }
    if !window.quoted.is_empty() {
        let lines: Vec<String> = window
            .quoted
            .iter()
            .enumerate()
            .map(|(i, m)| format!("[{}] {}", i + 1, quote(m)))
            .collect();
        context.push(part(
            Summary,
            format!(
                "User messages from the compacted conversation (verbatim, oldest first):\n{}",
                lines.join("\n")
            ),
        ));
    }
    if !window.files.is_empty() {
        context.push(part(
            Summary,
            format!("Files essential to continuing the work (read them as needed; contents are not saved):\n{}", window.files.join("\n")),
        ));
    }
    let blocks = [
        (
            PromptBlockKind::Instructions,
            vec![part(Instructions, INSTRUCTIONS.clone())],
        ),
        (PromptBlockKind::Persona, persona),
        (PromptBlockKind::Context, context),
    ];
    SystemPrompt {
        blocks: blocks
            .into_iter()
            .filter(|(_, parts)| !parts.is_empty())
            .map(|(kind, parts)| PromptBlock { kind, parts })
            .collect(),
    }
}

/// 带回的一条用户原话：附件只写标注；超长的只留首尾。
fn quote(message: &Message) -> String {
    let MessageBody::UserInput { parts, .. } = &message.body else {
        unreachable!("带回的只有 UserInput")
    };
    let text = parts
        .iter()
        .map(|p| match p {
            ContentPart::Text { text } => text.clone(),
            ContentPart::Image(_) => "[图片]".into(),
            ContentPart::File(f) => format!("[文件：{}]", f.path),
            ContentPart::Skill { name, .. } => format!("[skill：{name}]"),
        })
        .collect::<Vec<_>>()
        .join("\n");
    if text_tokens(&text) <= QUOTE_FULL_TOKENS {
        return text;
    }
    let head = token_prefix(text.chars(), QUOTE_HEAD_TOKENS);
    let tail = token_prefix(text.chars().rev(), QUOTE_TAIL_TOKENS);
    let (head, rest) = text.split_at(head);
    let (middle, tail) = rest.split_at(rest.len() - tail);
    format!("{head}……（省略约 {} token）……{tail}", text_tokens(middle))
}

/// 按估算口径，从迭代起点起不超过 `budget` token 的字节数。
fn token_prefix(chars: impl Iterator<Item = char>, budget: u64) -> usize {
    let (mut ascii, mut other, mut bytes) = (0u64, 0u64, 0usize);
    for c in chars {
        if c.is_ascii() {
            ascii += 1;
        } else {
            other += 1;
        }
        if ascii.div_ceil(ASCII_BYTES_PER_TOKEN) + other * NON_ASCII_TOKENS > budget {
            break;
        }
        bytes += c.len_utf8();
    }
    bytes
}

/// 有工具调用未结时，user 视图消息待其结果全部到齐后按原顺序放回，保证调用与结果相邻。
/// 未认领输入已由 store 排除。
pub(crate) fn order(messages: Vec<Message>) -> Vec<Message> {
    let mut out = Vec::with_capacity(messages.len());
    let mut open: HashSet<mic_message::ToolCallRef> = HashSet::new();
    let mut held = Vec::new();
    for m in messages {
        match &m.body {
            MessageBody::Reply { blocks, .. } => {
                open.extend(blocks.iter().enumerate().filter_map(|(index, b)| match b {
                    ReplyBlock::ToolCall { .. } => Some(mic_message::ToolCallRef {
                        reply: m.id,
                        index: index as u32,
                    }),
                    _ => None,
                }));
                out.push(m);
            }
            MessageBody::ToolResult { call, .. } => {
                open.remove(call);
                out.push(m);
                if open.is_empty() {
                    out.append(&mut held);
                }
            }
            _ if !open.is_empty() && matches!(m.model_view(), Some(ModelView::User(_))) => {
                held.push(m)
            }
            _ => out.push(m),
        }
    }
    out.append(&mut held);
    out
}
