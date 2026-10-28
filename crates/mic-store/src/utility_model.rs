use rusqlite::{params, Connection};

use crate::{
    models, ClaimedModel, ModelId, ModelSettingsError, Store, StoreError, UtilityModelSelection,
};

impl Store {
    pub async fn utility_model(&self) -> Result<UtilityModelSelection, StoreError> {
        self.call(|conn| read_selection(conn)).await
    }

    pub async fn set_utility_model(
        &self,
        selection: UtilityModelSelection,
    ) -> Result<(), ModelSettingsError> {
        self.call_models(move |conn| {
            let tx = conn.transaction()?;
            if let Some(id) = selection.model_id {
                models::live(&tx, id)?;
            }
            tx.execute(
                "UPDATE core_settings SET utility_model_id = ?1, utility_reasoning_effort = ?2",
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

    /// 杂活用途、模型连接配置与凭据在同一读事务中固定；未选时用默认模型。
    pub async fn utility_model_snapshot(&self) -> Result<ClaimedModel, StoreError> {
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
}

fn read_selection(conn: &Connection) -> Result<UtilityModelSelection, StoreError> {
    Ok(conn.query_row(
        "SELECT utility_model_id, utility_reasoning_effort FROM core_settings",
        [],
        |row| {
            Ok(UtilityModelSelection {
                model_id: row.get::<_, Option<i64>>(0)?.map(ModelId),
                reasoning_effort: row.get(1)?,
            })
        },
    )?)
}
