//! 测试服务商连接：请求 `{base_url}/models`，取模型 id 列表。

use mic_core::{ProbeError, SecretValue};
use reqwest::StatusCode;
use serde::Deserialize;

use crate::config::EndpointConfig;
use crate::limits::{CONNECT_TIMEOUT, PROBE_TIMEOUT};

#[derive(Deserialize)]
struct ModelList {
    data: Vec<ModelItem>,
}

#[derive(Deserialize)]
struct ModelItem {
    id: String,
}

pub(crate) async fn list_models(
    endpoint: &EndpointConfig,
    key: Option<SecretValue>,
) -> Result<Vec<String>, ProbeError> {
    let headers = endpoint
        .headers(key)
        .map_err(|e| ProbeError::Unexpected(e.message))?;
    let client = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(PROBE_TIMEOUT)
        .default_headers(headers)
        .build()
        .map_err(|e| ProbeError::Unexpected(e.without_url().to_string()))?;
    let resp = client
        .get(format!("{}/models", endpoint.base_url()))
        .send()
        .await
        .map_err(|e| {
            let reason = if e.is_timeout() {
                "连接超时".to_owned()
            } else if e.is_connect() {
                "无法建立连接".to_owned()
            } else {
                e.without_url().to_string()
            };
            ProbeError::Network(reason)
        })?;
    match resp.status() {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => return Err(ProbeError::Auth),
        s if !s.is_success() => return Err(ProbeError::Unexpected(format!("HTTP {}", s.as_u16()))),
        _ => {}
    }
    let list: ModelList = resp
        .json()
        .await
        .map_err(|_| ProbeError::Unexpected("返回内容不是模型列表".into()))?;
    Ok(list.data.into_iter().map(|m| m.id).collect())
}
