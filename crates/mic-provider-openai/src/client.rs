//! HTTP 发送：地址与鉴权头来自已解析配置，不重试；非 2xx 读体分类。

use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{AsHeaderName, HeaderMap, CONTENT_TYPE, RETRY_AFTER};
use reqwest::{RequestBuilder, Response, StatusCode};
use serde::{Deserialize, Serialize};

use crate::limits::CONNECT_TIMEOUT;

#[derive(Clone)]
pub(crate) struct Client {
    http: reqwest::Client,
    base: Arc<str>,
}

/// 发送失败：网络层错误或非 2xx 响应。
pub(crate) enum Failure {
    Network(reqwest::Error),
    Http(HttpFailure),
}

/// 非 2xx 响应：状态、Retry-After 与上游结构化错误，不含原始响应体。
pub(crate) struct HttpFailure {
    pub(crate) status: StatusCode,
    pub(crate) retry_after: Option<Duration>,
    pub(crate) error: Option<ApiError>,
}

#[derive(Deserialize)]
struct WrappedError {
    error: ApiError,
}

#[derive(Deserialize)]
pub(crate) struct ApiError {
    pub(crate) code: Option<String>,
    pub(crate) message: String,
}

impl Client {
    pub(crate) fn build(base: &str, headers: HeaderMap) -> Result<Self, reqwest::Error> {
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .default_headers(headers)
            .build()?;
        Ok(Self {
            http,
            base: base.into(),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    pub(crate) async fn post(
        &self,
        path: &str,
        body: &impl Serialize,
        headers: HeaderMap,
    ) -> Result<Response, Failure> {
        send(self.http.post(self.url(path)).headers(headers).json(body)).await
    }

    pub(crate) async fn get(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<Response, Failure> {
        send(self.http.get(self.url(path)).query(query)).await
    }
}

async fn send(req: RequestBuilder) -> Result<Response, Failure> {
    let resp = req.send().await.map_err(Failure::Network)?;
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let retry_after = retry_after(resp.headers());
    let path = resp.url().path().to_owned();
    let content_type = header_str(resp.headers(), CONTENT_TYPE);
    let cf_mitigated = header_str(resp.headers(), "cf-mitigated");
    let error = resp
        .bytes()
        .await
        .ok()
        .and_then(|b| serde_json::from_slice::<WrappedError>(&b).ok())
        .map(|w| w.error);
    tracing::warn!(
        status = status.as_u16(),
        path,
        content_type,
        cf_mitigated,
        code = error.as_ref().and_then(|e| e.code.as_deref()),
        message = error.as_ref().map(|e| e.message.as_str()),
        "upstream http failure"
    );
    Err(Failure::Http(HttpFailure {
        status,
        retry_after,
        error,
    }))
}

fn header_str(headers: &HeaderMap, name: impl AsHeaderName) -> Option<String> {
    Some(headers.get(name)?.to_str().ok()?.to_owned())
}

/// 只认秒数形式；HTTP 日期形式视为未给。
fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    let secs = headers
        .get(RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()?;
    Some(Duration::from_secs(secs))
}
