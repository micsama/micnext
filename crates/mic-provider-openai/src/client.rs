//! SDK 装配：自写 `Config`（不读环境变量）与无重试传输层（非 2xx 自己读体分类）。

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use async_openai::config::Config;
use async_openai::error::{ApiError, OpenAIError, WrappedError};
use async_openai::middleware::{HttpRequestFactory, ReqwestService};
use reqwest::header::{HeaderMap, RETRY_AFTER};
use reqwest::{Response, StatusCode};
use secrecy::SecretString;

use crate::limits::CONNECT_TIMEOUT;

pub(crate) type Client = async_openai::Client<ApiConfig>;

/// 地址与鉴权头全部来自已解析配置；`api_key` 不参与请求，鉴权头已在 `headers` 里。
#[derive(Clone)]
pub(crate) struct ApiConfig {
    base: String,
    headers: HeaderMap,
    key: SecretString,
}

impl Config for ApiConfig {
    fn headers(&self) -> HeaderMap {
        self.headers.clone()
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn query(&self) -> Vec<(&str, &str)> {
        Vec::new()
    }

    fn api_base(&self) -> &str {
        &self.base
    }

    fn api_key(&self) -> &SecretString {
        &self.key
    }
}

pub(crate) fn build(base: &str, headers: HeaderMap) -> Result<Client, reqwest::Error> {
    let http = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .build()?;
    let cfg = ApiConfig {
        base: base.to_owned(),
        headers,
        key: SecretString::from(String::new()),
    };
    Ok(async_openai::Client::build(http.clone(), cfg)
        .with_http_service(Transport(ReqwestService::new(http))))
}

/// 非 2xx 响应：状态、Retry-After 与上游结构化错误，不含原始响应体。
#[derive(Debug)]
pub(crate) struct HttpFailure {
    pub(crate) status: StatusCode,
    pub(crate) retry_after: Option<Duration>,
    pub(crate) error: Option<ApiError>,
}

impl fmt::Display for HttpFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HTTP {}", self.status.as_u16())
    }
}

impl std::error::Error for HttpFailure {}

#[derive(Clone)]
struct Transport(ReqwestService);

impl tower::Service<HttpRequestFactory> for Transport {
    type Response = Response;
    type Error = OpenAIError;
    type Future = Pin<Box<dyn Future<Output = Result<Response, OpenAIError>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.0.poll_ready(cx)
    }

    fn call(&mut self, req: HttpRequestFactory) -> Self::Future {
        let sent = self.0.call(req);
        Box::pin(async move {
            let resp = sent.await?;
            let status = resp.status();
            if status.is_success() {
                return Ok(resp);
            }
            let retry_after = retry_after(resp.headers());
            let error = resp
                .bytes()
                .await
                .ok()
                .and_then(|b| serde_json::from_slice::<WrappedError>(&b).ok())
                .map(|w| w.error);
            Err(OpenAIError::Boxed(Box::new(HttpFailure {
                status,
                retry_after,
                error,
            })))
        })
    }
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
