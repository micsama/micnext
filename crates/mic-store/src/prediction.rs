use mic_message::{MessageBody, SessionId, SidePurpose};
use rusqlite::OptionalExtension;

use crate::context::{insert_call, insert_side_output};
use crate::row::{self, MESSAGE_COLS, MESSAGE_FROM, MESSAGE_NEXT};
use crate::{NewModelCall, Prediction, RunId, Store, StoreError};

impl Store {
    /// 一次预测调用：`call.run_id` 为来源 run，取材消息在 `call.context.instruction`；`output` 为模型原文。
    pub async fn save_prediction(
        &self,
        call: NewModelCall,
        output: Option<String>,
    ) -> Result<(), StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let call_id = insert_call(&tx, &call)?;
            if let Some(text) = output {
                insert_side_output(&tx, &call, call_id, SidePurpose::Prediction, text)?;
            }
            tx.commit()?;
            Ok(())
        })
        .await
    }

    /// 最新 run 的成功预测的非空输出。
    pub async fn current_prediction(
        &self,
        session: SessionId,
    ) -> Result<Option<Prediction>, StoreError> {
        self.call(move |conn| {
            let found = conn
                .query_row(
                    &format!(
                        "SELECT {MESSAGE_COLS}, c.run_id FROM {MESSAGE_FROM}
                         WHERE m.session_id = ?1 AND m.kind = 'SideOutput'
                           AND c.purpose = 'prediction' AND c.error IS NULL
                           AND c.run_id = (SELECT max(id) FROM core_runs WHERE session_id = ?1)
                         ORDER BY m.id DESC LIMIT 1"
                    ),
                    [session.0],
                    |r| Ok((row::message(r)?, RunId(r.get(MESSAGE_NEXT)?))),
                )
                .optional()?;
            Ok(found.and_then(|(message, run_id)| match message.body {
                MessageBody::SideOutput { text, .. } => {
                    let text = text.trim();
                    (!text.is_empty()).then(|| Prediction {
                        run_id,
                        text: text.to_owned(),
                    })
                }
                _ => unreachable!("查询限定了 SideOutput"),
            }))
        })
        .await
    }
}
