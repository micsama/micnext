use std::num::NonZeroU32;

use mic_message::{ContentPart, Message, MessageBody, SessionId};
use rusqlite::{params, Connection};

use crate::context::insert_call;
use crate::row::{self, MESSAGE_COLS, MESSAGE_FROM};
use crate::{
    models, ClaimedModel, ModelId, ModelSettingsError, NewModelCall, Store, StoreError,
    TitleModelSelection,
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

    pub async fn recent_text_inputs(
        &self,
        session: SessionId,
        limit: NonZeroU32,
    ) -> Result<Vec<Message>, StoreError> {
        self.call(move |conn| {
            let mut messages = text_inputs(conn, session, limit, true)?;
            messages.reverse();
            Ok(messages)
        })
        .await
    }

    pub async fn save_title_result(
        &self,
        session: SessionId,
        result: Result<String, String>,
        call: Option<NewModelCall>,
    ) -> Result<(), StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            if let Some(call) = call {
                insert_call(&tx, &call)?;
            }
            match result {
                Ok(title) => {
                    tx.execute(
                        "UPDATE core_sessions SET title = ?1, title_error = NULL WHERE id = ?2",
                        params![title, session.0],
                    )?;
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
