//! 测试服务商连接：`GET {base_url}/models`，取模型 id 列表。

use mic_core::ProbeError;
use serde::Deserialize;

use crate::client::Client;
use crate::error;
use crate::limits::PROBE_TIMEOUT;

/// 兼容服务的模型对象字段不齐，只取 id。
#[derive(Deserialize)]
struct ModelList {
    data: Vec<ModelItem>,
}

#[derive(Deserialize)]
struct ModelItem {
    id: String,
}

pub(crate) async fn list_models(client: Client) -> Result<Vec<String>, ProbeError> {
    let list: ModelList = tokio::time::timeout(PROBE_TIMEOUT, client.models().list_byot())
        .await
        .map_err(|_| ProbeError::Network("连接超时".into()))?
        .map_err(error::probe)?;
    Ok(list.data.into_iter().map(|m| m.id).collect())
}
