use md5::{Digest, Md5};
use mic_core::{IncomingPart, Kernel};
use mic_message::{ContentPart, Message, MessageBody, MessageId};
use mic_store::rusqlite::{params, OptionalExtension};

use crate::account::{Account, AccountError};
use crate::client::QuoteRef;
use crate::{delivery, wire};

pub(crate) async fn resolve(
    kernel: &Kernel,
    account: &Account,
    references: Vec<QuoteRef>,
) -> Result<Vec<IncomingPart>, AccountError> {
    let mut out = Vec::with_capacity(references.len());
    let mut history = None;
    for reference in references {
        let text = match incoming(kernel, account, reference.id).await? {
            Some((id, source_start)) => {
                let messages = match &history {
                    Some(messages) => messages,
                    None => {
                        history.insert(kernel.messages_after(account.view.session_id, None).await?)
                    }
                };
                Some(source_text(messages, id, source_start)?)
            }
            None => {
                delivery::quoted_text(kernel, &account.credentials.user_id, reference.id).await?
            }
        };
        let quoted = match text {
            None => "[引用：原消息未找到]".to_owned(),
            Some(text) => match reference.partial {
                None => format!("[引用：{text}]"),
                Some(partial) => match partial_text(&text, &partial) {
                    Some(selected) => format!("[引用：{selected}]"),
                    None => format!("[引用：{text}]\n[引用说明：部分选区未还原，已提供全文。]"),
                },
            },
        };
        out.push(IncomingPart::Text(format!("{quoted}\n")));
    }
    Ok(out)
}

pub(crate) async fn remember(
    kernel: &Kernel,
    account: &Account,
    external_id: wire::MessageId,
    message_id: MessageId,
    source_start: usize,
) -> Result<(), AccountError> {
    let user = account.credentials.user_id.0.clone();
    let external_id = external_id.0.to_string();
    let source_start = i64::try_from(source_start)
        .map_err(|_| AccountError::Invalid("引用原内容位置超出整数范围"))?;
    kernel
        .with_module_tx(move |tx| {
            tx.execute(
                "INSERT INTO wechat_inbound_message
                 (message_id, user_id, external_message_id, source_start)
                 VALUES (?1, ?2, ?3, ?4)",
                params![message_id.0, user, external_id, source_start],
            )?;
            Ok(())
        })
        .await?;
    Ok(())
}

async fn incoming(
    kernel: &Kernel,
    account: &Account,
    external_id: wire::MessageId,
) -> Result<Option<(MessageId, usize)>, AccountError> {
    let user = account.credentials.user_id.0.clone();
    let external_id = external_id.0.to_string();
    let raw = kernel
        .with_module_tx(move |tx| {
            tx.query_row(
                "SELECT message_id, source_start FROM wechat_inbound_message
                 WHERE user_id = ?1 AND external_message_id = ?2
                 ORDER BY message_id LIMIT 1",
                params![user, external_id],
                |row| Ok((MessageId(row.get(0)?), row.get::<_, i64>(1)?)),
            )
            .optional()
        })
        .await?;
    raw.map(|(id, source_start)| {
        let source_start = usize::try_from(source_start)
            .map_err(|_| AccountError::Invalid("引用原内容位置不合法"))?;
        Ok((id, source_start))
    })
    .transpose()
}

fn source_text(
    messages: &[Message],
    id: MessageId,
    source_start: usize,
) -> Result<String, AccountError> {
    let message = messages
        .iter()
        .find(|message| message.id == id)
        .ok_or(AccountError::Invalid("引用关联的原消息不存在或会话不匹配"))?;
    let MessageBody::UserInput { parts, .. } = &message.body else {
        return Err(AccountError::Invalid("引用入站关联不是 UserInput"));
    };
    if source_start >= parts.len() {
        return Err(AccountError::Invalid("引用原内容位置不合法"));
    }
    let mut text = String::new();
    for part in &parts[source_start..] {
        match part {
            ContentPart::Text { text: part } => text.push_str(part),
            _ => return Err(AccountError::Invalid("引用入站包含未支持的内容类型")),
        }
    }
    Ok(text)
}

fn partial_text<'a>(text: &'a str, partial: &wire::PartialText) -> Option<&'a str> {
    let start = nth_index(text, &partial.start, partial.startindex, 0)?;
    let ends = [
        nth_index(text, &partial.end, partial.endindex, 0),
        nth_index(
            text,
            &partial.end,
            partial.endindex,
            start + partial.start.len(),
        ),
    ];
    for end in ends.into_iter().flatten() {
        if end < start {
            continue;
        }
        let candidate = &text[start..end + partial.end.len()];
        if format!("{:x}", Md5::digest(candidate.as_bytes())) == partial.quotemd5 {
            return Some(candidate);
        }
    }
    None
}

fn nth_index(text: &str, anchor: &str, occurrence: u64, from: usize) -> Option<usize> {
    let mut position = from;
    for index in 0..=occurrence {
        position += text[position..].find(anchor)?;
        if index == occurrence {
            return Some(position);
        }
        position += anchor.len();
    }
    None
}
