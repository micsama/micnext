use crate::limits::{ASCII_BYTES_PER_TOKEN, IMAGE_TOKENS, NON_ASCII_TOKENS};
use crate::ModelRequest;
use mic_message::{ContentPart, Message, ModelView, Reasoning, ReplyBlock};

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

/// 按字数粗估整个请求，只供压缩前裁剪历史；占用与触发都用上游用量。
pub(crate) fn estimate_tokens(req: &ModelRequest) -> u64 {
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
