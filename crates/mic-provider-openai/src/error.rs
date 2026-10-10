//! SDK 错误与上游业务错误 → 领域错误（provider-sdk-responses §六）。

use std::time::Duration;

use async_openai::error::{OpenAIError, StreamError};
use mic_core::{ProbeError, ProviderError};
use reqwest::StatusCode;

use crate::client::HttpFailure;

pub(crate) fn protocol(message: impl Into<String>) -> ProviderError {
    ProviderError::Protocol {
        message: message.into(),
    }
}

pub(crate) fn transient(message: impl Into<String>) -> ProviderError {
    ProviderError::Transient {
        message: message.into(),
        retry_after: None,
    }
}

pub(crate) fn provider(e: OpenAIError) -> ProviderError {
    match e {
        OpenAIError::Boxed(b) => match b.downcast::<HttpFailure>() {
            Ok(f) => http(*f),
            Err(other) => protocol(other.to_string()),
        },
        OpenAIError::Reqwest(e) => transient(format!("请求失败：{}", e.without_url())),
        OpenAIError::StreamError(e) => match *e {
            StreamError::EventStream(reason) => transient(format!("流中途断开：{reason}")),
            other => protocol(other.to_string()),
        },
        OpenAIError::JSONDeserialize(e, _) => protocol(format!("上游数据无法解析：{e}")),
        other => protocol(other.to_string()),
    }
}

fn http(f: HttpFailure) -> ProviderError {
    let status = f.status.as_u16();
    let (code, message) = match f.error {
        Some(e) => (e.code, Some(e.message)),
        None => (None, None),
    };
    let kind = match code.as_deref() {
        Some(c) => classify_code(c),
        None => None,
    }
    .unwrap_or(match status {
        401..=403 => Kind::Account,
        429 | 500..=599 => Kind::Transient,
        _ => Kind::Rejected,
    });
    let note = match (kind, status) {
        (Kind::Account, 402) => "余额或额度不足，请充值",
        (Kind::Account, _) => "key 无效、无权限或额度不足，请检查 API key 与账户",
        (Kind::Transient, 429) => "请求过于频繁",
        (Kind::Transient, _) => "服务端错误",
        (Kind::Rejected, _) => "请求被拒绝",
    };
    let mut detail = format!("{note}（HTTP {status}");
    if let Some(c) = &code {
        detail.push_str(&format!("，{c}"));
    }
    detail.push('）');
    if let Some(m) = message {
        detail.push_str(&format!("：{m}"));
    }
    kind.into_error(detail, f.retry_after)
}

/// 流内业务错误（Responses `response.failed` / `error` 事件）。
pub(crate) fn upstream(code: Option<&str>, message: &str) -> ProviderError {
    let kind = code.and_then(classify_code).unwrap_or(Kind::Rejected);
    let detail = match code {
        Some(c) => format!("{c}：{message}"),
        None => message.to_owned(),
    };
    kind.into_error(detail, None)
}

#[derive(Clone, Copy)]
enum Kind {
    Account,
    Transient,
    Rejected,
}

impl Kind {
    fn into_error(self, message: String, retry_after: Option<Duration>) -> ProviderError {
        match self {
            Self::Account => ProviderError::Account { message },
            Self::Transient => ProviderError::Transient {
                message,
                retry_after,
            },
            Self::Rejected => ProviderError::Rejected { message },
        }
    }
}

fn classify_code(code: &str) -> Option<Kind> {
    match code {
        "insufficient_quota" | "invalid_api_key" => Some(Kind::Account),
        "rate_limit_exceeded" | "server_error" => Some(Kind::Transient),
        _ => None,
    }
}

pub(crate) fn probe(e: OpenAIError) -> ProbeError {
    match e {
        OpenAIError::Boxed(b) => match b.downcast::<HttpFailure>() {
            Ok(f) if matches!(f.status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) => {
                ProbeError::Auth
            }
            Ok(f) => ProbeError::Unexpected(match f.error {
                Some(e) => format!("HTTP {}：{}", f.status.as_u16(), e.message),
                None => format!("HTTP {}", f.status.as_u16()),
            }),
            Err(other) => ProbeError::Unexpected(other.to_string()),
        },
        OpenAIError::Reqwest(e) => ProbeError::Network(if e.is_timeout() {
            "连接超时".to_owned()
        } else if e.is_connect() {
            "无法建立连接".to_owned()
        } else {
            e.without_url().to_string()
        }),
        OpenAIError::JSONDeserialize(..) => ProbeError::Unexpected("返回内容不是模型列表".into()),
        other => ProbeError::Unexpected(other.to_string()),
    }
}
