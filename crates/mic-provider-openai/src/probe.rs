//! 测试服务商连接：列出模型名。

use mic_core::ProbeError;
use serde::de::DeserializeOwned;
use serde::Deserialize;

use crate::client::{Client, Failure};
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
        Ok(match catalog {
            Catalog::Standard => {
                let list: ModelList = fetch(&client, &[]).await?;
                list.data.into_iter().map(|m| m.id).collect::<Vec<_>>()
            }
            Catalog::Codex => {
                let list: CodexModelList =
                    fetch(&client, &[("client_version", CODEX_CLIENT_VERSION)]).await?;
                list.models.into_iter().map(|m| m.slug).collect()
            }
        })
    };
    let names = tokio::time::timeout(PROBE_TIMEOUT, fetch)
        .await
        .map_err(|_| ProbeError::Network("连接超时".into()))??;
    if names.is_empty() && catalog == Catalog::Codex {
        return Err(ProbeError::Unexpected(
            "没有可用模型，可能需要更新 CODEX_CLIENT_VERSION".into(),
        ));
    }
    Ok(names)
}

async fn fetch<T: DeserializeOwned>(
    client: &Client,
    query: &[(&str, &str)],
) -> Result<T, ProbeError> {
    let body = client
        .get("/models", query)
        .await
        .map_err(error::probe)?
        .bytes()
        .await
        .map_err(|e| error::probe(Failure::Network(e)))?;
    serde_json::from_slice(&body).map_err(|_| ProbeError::Unexpected("返回内容不是模型列表".into()))
}
