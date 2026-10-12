use crate::limits::{ASCII_BYTES_PER_TOKEN, IMAGE_TOKENS, NON_ASCII_TOKENS};
use crate::ModelRequest;
use mic_message::{ContentPart, Message, ModelView, Reasoning, ReplyBlock};
use mic_store::{ContextUsage, ModelId};

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
            blocks
                .iter()
                .map(|block| match block {
                    ReplyBlock::Text { text, .. }
                    | ReplyBlock::Reasoning(Reasoning::Visible { text, .. }) => text_tokens(text),
                    ReplyBlock::Reasoning(Reasoning::Redacted { data }) => text_tokens(data),
                    ReplyBlock::ToolCall { name, args, .. } => {
                        text_tokens(name) + text_tokens(&args.to_string())
                    }
                })
                .sum::<u64>()
                + 8
        }
        None => 0,
    }
}

pub(crate) fn request_tokens(
    req: &ModelRequest,
    usage: Option<&ContextUsage>,
    model: Option<ModelId>,
) -> u64 {
    if let Some(usage) = usage.filter(|u| Some(u.model_id) == model) {
        usage.input_tokens
            + req
                .messages
                .iter()
                .filter(|m| usage.through.is_none_or(|id| m.id > id))
                .map(message_tokens)
                .sum::<u64>()
    } else {
        text_tokens(&req.system)
            + req
                .tools
                .iter()
                .map(|t| {
                    text_tokens(&t.name)
                        + text_tokens(&t.description)
                        + text_tokens(&t.parameters.to_string())
                })
                .sum::<u64>()
            + req.messages.iter().map(message_tokens).sum::<u64>()
    }
}
