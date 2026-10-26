use std::num::NonZeroU32;

use mic_message::{
    AssistantPhase, ContentPart, Message, MessageBody, ReplyBlock, SessionId, SidePurpose,
};
use rusqlite::{params, Connection};

use crate::context::insert_call;
use crate::row::{self, MESSAGE_COLS, MESSAGE_FROM};
use crate::tags::replace_session_tags;
use crate::{
    models, ClaimedModel, ModelId, ModelSettingsError, Store, StoreError, TitleCall,
    TitleModelSelection, TitleOutput,
};

impl Store {
    pub async fn title_model(&self) -> Result<TitleModelSelection, StoreError> {
        self.call(|conn| read_selection(conn)).await
    }

    pub async fn set_title_model(
        &self,
        selection: TitleModelSelection,
    ) -> Result<(), ModelSettingsError> {
        self.call_models(move |conn| {
            let tx = conn.transaction()?;
            if let Some(id) = selection.model_id {
                models::live(&tx, id)?;
            }
            tx.execute(
                "UPDATE core_settings SET title_model_id = ?1, title_reasoning_effort = ?2",
                params![
                    selection.model_id.map(|id| id.0),
                    selection.reasoning_effort
                ],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await
    }

    /// 标题用途、模型连接配置与凭据在同一读事务中固定。
    pub async fn title_model_snapshot(&self) -> Result<ClaimedModel, StoreError> {
        let cipher = self.cipher();
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let selection = read_selection(&tx)?;
            let id = selection.model_id.or(models::read_default(&tx)?);
            let model = match id {
                Some(id) => models::read_model(&tx, &cipher, id, selection.reasoning_effort)?,
                None => ClaimedModel::Missing,
            };
            tx.commit()?;
            Ok(model)
        })
        .await
    }

    pub async fn first_text_input(
        &self,
        session: SessionId,
    ) -> Result<Option<Message>, StoreError> {
        self.call(move |conn| {
            Ok(text_inputs(conn, session, NonZeroU32::new(1).unwrap(), false)?.pop())
        })
        .await
    }

    /// 最近 `inputs` 条文字输入与 `replies` 条收尾回复（见 [`reply_answer`]），id 升序。
    pub async fn title_sources(
        &self,
        session: SessionId,
        inputs: NonZeroU32,
        replies: u32,
    ) -> Result<Vec<Message>, StoreError> {
        self.call(move |conn| {
            let mut messages = text_inputs(conn, session, inputs, true)?;
            messages.extend(final_replies(conn, session, replies)?);
            messages.sort_by_key(|m| m.id);
            Ok(messages)
        })
        .await
    }

    pub async fn save_title_result(
        &self,
        session: SessionId,
        result: Result<TitleOutput, String>,
        call: Option<TitleCall>,
    ) -> Result<(), StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            if let Some(TitleCall { call, output }) = call {
                let call_id = insert_call(&tx, &call)?;
                if let Some(text) = output {
                    let body = MessageBody::SideOutput {
                        purpose: SidePurpose::Title,
                        text,
                    };
                    tx.execute(
                        "INSERT INTO core_messages (session_id, model_call_id, payload, created_at)
                         VALUES (?1, ?2, ?3, ?4)",
                        params![session.0, call_id.0, row::payload(&body)?, call.finished_at],
                    )?;
                }
            }
            match result {
                Ok(TitleOutput { title, tags }) => {
                    tx.execute(
                        "UPDATE core_sessions SET title = ?1, title_error = NULL WHERE id = ?2",
                        params![title, session.0],
                    )?;
                    // 生成期间用户手动改过标签，就以手动为准。
                    let manual: bool = tx.query_row(
                        "SELECT tags_manual FROM core_sessions WHERE id = ?1",
                        [session.0],
                        |r| r.get(0),
                    )?;
                    if let (Some(tags), false) = (tags, manual) {
                        replace_session_tags(&tx, session, &tags)?;
                    }
                }
                Err(error) => {
                    tx.execute(
                        "UPDATE core_sessions SET title_error = ?1 WHERE id = ?2",
                        params![error, session.0],
                    )?;
                }
            }
            tx.commit()?;
            Ok(())
        })
        .await
    }
}

fn read_selection(conn: &Connection) -> Result<TitleModelSelection, StoreError> {
    Ok(conn.query_row(
        "SELECT title_model_id, title_reasoning_effort FROM core_settings",
        [],
        |row| {
            Ok(TitleModelSelection {
                model_id: row.get::<_, Option<i64>>(0)?.map(ModelId),
                reasoning_effort: row.get(1)?,
            })
        },
    )?)
}

fn text_inputs(
    conn: &Connection,
    session: SessionId,
    limit: NonZeroU32,
    descending: bool,
) -> Result<Vec<Message>, StoreError> {
    let order = if descending { "DESC" } else { "ASC" };
    let mut query = conn.prepare(&format!("SELECT {MESSAGE_COLS} FROM {MESSAGE_FROM} WHERE m.session_id = ?1 AND m.kind = 'UserInput' ORDER BY m.id {order}"))?;
    let rows = query.query_map([session.0], row::message)?;
    let mut messages = Vec::new();
    for row in rows {
        let message = row?;
        if let MessageBody::UserInput { parts, .. } = &message.body {
            if parts
                .iter()
                .any(|part| matches!(part, ContentPart::Text { text } if !text.trim().is_empty()))
            {
                messages.push(message);
                if messages.len() == limit.get() as usize {
                    break;
                }
            }
        }
    }
    Ok(messages)
}

/// 一轮收尾回复的正文：含工具调用的不算；取非 `Commentary` 的 Text（无阶段标注的协议照取）；无正文为 `None`。
pub fn reply_answer(blocks: &[ReplyBlock]) -> Option<String> {
    let mut parts = Vec::new();
    for block in blocks {
        match block {
            ReplyBlock::ToolCall { .. } => return None,
            ReplyBlock::Text {
                phase: Some(AssistantPhase::Commentary),
                ..
            } => {}
            ReplyBlock::Text { text, .. } => parts.push(text.trim()),
            ReplyBlock::Reasoning(_) => {}
        }
    }
    let text = parts
        .into_iter()
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    (!text.is_empty()).then_some(text)
}

fn final_replies(
    conn: &Connection,
    session: SessionId,
    limit: u32,
) -> Result<Vec<Message>, StoreError> {
    let mut messages = Vec::new();
    if limit == 0 {
        return Ok(messages);
    }
    let mut query = conn.prepare(&format!("SELECT {MESSAGE_COLS} FROM {MESSAGE_FROM} WHERE m.session_id = ?1 AND m.kind = 'Reply' ORDER BY m.id DESC"))?;
    let rows = query.query_map([session.0], row::message)?;
    for row in rows {
        let message = row?;
        if matches!(&message.body, MessageBody::Reply { blocks, .. } if reply_answer(blocks).is_some())
        {
            messages.push(message);
            if messages.len() == limit as usize {
                break;
            }
        }
    }
    Ok(messages)
}
