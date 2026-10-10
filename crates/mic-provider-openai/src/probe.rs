//! 测试服务商连接：列出模型名。

use async_openai::traits::RequestOptionsBuilder;
use mic_core::ProbeError;
use serde::Deserialize;

use crate::client::Client;
use crate::config::Catalog;
use crate::error;
use crate::limits::{CODEX_CLIENT_VERSION, PROBE_TIMEOUT};

/// 兼容服务的模型对象字段不齐，只取 id。
#[derive(Deserialize)]
struct ModelList {
    data: Vec<ModelItem>,
}

#[derive(Deserialize)]
struct ModelItem {
    id: String,
}

/// Codex 后端的模型目录，只取 slug。
#[derive(Deserialize)]
struct CodexModelList {
    models: Vec<CodexModel>,
}

#[derive(Deserialize)]
struct CodexModel {
    slug: String,
}

pub(crate) async fn list_models(
    client: Client,
    catalog: Catalog,
) -> Result<Vec<String>, ProbeError> {
    let fetch = async {
        match catalog {
            Catalog::Standard => {
                let list: ModelList = client.models().list_byot().await?;
                Ok(list.data.into_iter().map(|m| m.id).collect::<Vec<_>>())
            }
            Catalog::Codex => {
                let list: CodexModelList = client
                    .models()
                    .query(&[("client_version", CODEX_CLIENT_VERSION)])?
                    .list_byot()
                    .await?;
                Ok(list.models.into_iter().map(|m| m.slug).collect())
            }
        }
    };
    let names = tokio::time::timeout(PROBE_TIMEOUT, fetch)
        .await
        .map_err(|_| ProbeError::Network("连接超时".into()))?
        .map_err(error::probe)?;
    if names.is_empty() && catalog == Catalog::Codex {
        return Err(ProbeError::Unexpected(
            "没有可用模型，可能需要更新 CODEX_CLIENT_VERSION".into(),
        ));
    }
    Ok(names)
}
