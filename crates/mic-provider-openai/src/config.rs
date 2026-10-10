use mic_core::{ConfigError, SecretValue};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest::Url;
use serde::{Deserialize, Serialize};

use crate::token::ChatgptToken;

const DEEPSEEK_URL: &str = "https://api.deepseek.com";
const OPENAI_URL: &str = "https://api.openai.com/v1";
const OLLAMA_URL: &str = "http://localhost:11434/v1";
const CHATGPT_URL: &str = "https://chatgpt.com/backend-api/codex";

/// 服务商的配置 JSON；DeepSeek / OpenAI / ChatGPT 地址固定，不接受 `base_url`。
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EndpointConfig {
    preset: Preset,
    base_url: Option<String>,
}

/// 模型的配置 JSON；模型名单独存放。
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelConfig {
    max_tokens: Option<u32>,
    reasoning_effort: Option<ReasoningEffort>,
}

/// 推理强度全集；各预设可用的子集见 [`ModelConfig::canonical`]。
#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ReasoningEffort {
    /// 关闭思考。
    None,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl ReasoningEffort {
    pub(crate) fn sdk(self) -> async_openai::types::chat::ReasoningEffort {
        use async_openai::types::chat::ReasoningEffort as E;
        match self {
            Self::None => E::None,
            Self::Minimal => E::Minimal,
            Self::Low => E::Low,
            Self::Medium => E::Medium,
            Self::High => E::High,
            Self::Xhigh => E::Xhigh,
            Self::Max => E::Max,
        }
    }
}

/// 支持推理的预设不写 `reasoning_effort` 时的取值。
const DEFAULT_EFFORT: ReasoningEffort = ReasoningEffort::Low;

#[derive(Deserialize, Serialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Preset {
    Generic,
    Deepseek,
    Ollama,
    Openai,
    /// ChatGPT 订阅，经 Codex 后端；key 为 Codex CLI 的 access_token。
    Chatgpt,
}

/// 预设决定协议；Chat 内部再按方言区分扩展字段，Responses 按后端区分缓存路由。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Protocol {
    Chat(Dialect),
    Responses(Backend),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Backend {
    OpenAi,
    /// 另需按请求发 `session_id` 头才会缓存。
    Codex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dialect {
    DeepSeek,
    Ollama,
    Generic,
}

/// 模型列表接口的形状。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Catalog {
    /// `GET /models` → `data[].id`。
    Standard,
    /// `GET /models?client_version=` → `models[].slug`。
    Codex,
}

/// 解析完成、可直接用于请求的条目。
pub(crate) struct Resolved {
    pub(crate) model: String,
    pub(crate) protocol: Protocol,
    pub(crate) max_tokens: Option<u32>,
    pub(crate) reasoning_effort: Option<ReasoningEffort>,
}

fn err(field: &str, message: impl Into<String>) -> ConfigError {
    ConfigError {
        field: field.into(),
        message: message.into(),
    }
}

impl EndpointConfig {
    pub(crate) fn parse(json: &str) -> Result<Self, ConfigError> {
        serde_json::from_str(json).map_err(|e| err("config", e.to_string()))
    }

    /// 校验并补全地址，返回规范形态（固定地址的预设不存地址）。
    pub(crate) fn canonical(mut self) -> Result<Self, ConfigError> {
        let base_url = match (self.preset, self.base_url.take()) {
            (Preset::Deepseek | Preset::Openai | Preset::Chatgpt, None) => return Ok(self),
            (Preset::Deepseek | Preset::Openai | Preset::Chatgpt, Some(_)) => {
                return Err(err("base_url", "该服务商地址固定，不需要填写"))
            }
            (Preset::Ollama, None) => OLLAMA_URL.to_owned(),
            (Preset::Generic, None) => {
                return Err(err(
                    "base_url",
                    "通用兼容服务必须填地址，如 https://example.com/v1",
                ))
            }
            (_, Some(url)) => url,
        };
        let base_url = base_url.trim_end_matches('/').to_owned();
        let parsed =
            Url::parse(&base_url).map_err(|e| err("base_url", format!("不是合法地址：{e}")))?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(err(
                "base_url",
                "只接受 http(s) 地址，且不能含账号密码、查询参数或锚点",
            ));
        }
        self.base_url = Some(base_url);
        Ok(self)
    }

    pub(crate) fn key_env(&self) -> Option<&'static str> {
        match self.preset {
            Preset::Deepseek => Some("DEEPSEEK_API_KEY"),
            Preset::Generic | Preset::Openai => Some("OPENAI_API_KEY"),
            Preset::Ollama | Preset::Chatgpt => None,
        }
    }

    pub(crate) fn protocol(&self) -> Protocol {
        match self.preset {
            Preset::Generic => Protocol::Chat(Dialect::Generic),
            Preset::Deepseek => Protocol::Chat(Dialect::DeepSeek),
            Preset::Ollama => Protocol::Chat(Dialect::Ollama),
            Preset::Openai => Protocol::Responses(Backend::OpenAi),
            Preset::Chatgpt => Protocol::Responses(Backend::Codex),
        }
    }

    pub(crate) fn catalog(&self) -> Catalog {
        match self.preset {
            Preset::Chatgpt => Catalog::Codex,
            _ => Catalog::Standard,
        }
    }

    /// 要求 `self` 已是规范形态。
    pub(crate) fn base_url(&self) -> &str {
        match self.preset {
            Preset::Deepseek => DEEPSEEK_URL,
            Preset::Openai => OPENAI_URL,
            Preset::Chatgpt => CHATGPT_URL,
            _ => self.base_url.as_deref().expect("规范形态必有 base_url"),
        }
    }

    /// 有 key 时含鉴权头；ChatGPT 必须有有效 token，并带 `chatgpt-account-id`。
    pub(crate) fn headers(&self, key: Option<SecretValue>) -> Result<HeaderMap, ConfigError> {
        let mut headers = HeaderMap::new();
        if let Preset::Chatgpt = self.preset {
            let token = ChatgptToken::parse(key.as_ref())?;
            let account = HeaderValue::from_str(&token.account_id)
                .map_err(|_| err("api_key", "access_token 里的账户 id 含有非法字符"))?;
            headers.insert("chatgpt-account-id", account);
        }
        if let Some(key) = key {
            let mut auth = HeaderValue::from_str(&format!("Bearer {}", key.expose()))
                .map_err(|_| err("api_key", "API key 含有不能放进请求头的字符"))?;
            auth.set_sensitive(true);
            headers.insert(AUTHORIZATION, auth);
        }
        Ok(headers)
    }
}

impl ModelConfig {
    pub(crate) fn parse(json: &str) -> Result<Self, ConfigError> {
        serde_json::from_str(json).map_err(|e| err("config", e.to_string()))
    }

    /// 校验模型名与参数，返回规范形态。
    pub(crate) fn canonical(
        mut self,
        endpoint: &EndpointConfig,
        name: &str,
    ) -> Result<Self, ConfigError> {
        use ReasoningEffort as E;
        if name.trim().is_empty() {
            return Err(err("name", "模型名不能为空"));
        }
        if let (Preset::Chatgpt, Some(_)) = (endpoint.preset, self.max_tokens) {
            return Err(err("max_tokens", "ChatGPT 订阅不支持限制输出长度，请留空"));
        }
        self.reasoning_effort = match (endpoint.preset, self.reasoning_effort) {
            (Preset::Deepseek | Preset::Openai | Preset::Chatgpt, None) => Some(DEFAULT_EFFORT),
            (Preset::Deepseek, Some(e @ (E::None | E::Low | E::High | E::Max))) => Some(e),
            (Preset::Deepseek, Some(_)) => {
                return Err(err(
                    "reasoning_effort",
                    "DeepSeek 只支持 none / low / high / max",
                ))
            }
            (Preset::Openai | Preset::Chatgpt, effort) | (_, effort @ None) => effort,
            (_, Some(_)) => return Err(err("reasoning_effort", "该服务商不支持推理强度，请留空")),
        };
        Ok(self)
    }
}

/// 要求两份配置都已是规范形态（来自库内已校验的 JSON）。
pub(crate) fn resolve(endpoint: &EndpointConfig, model: ModelConfig, name: &str) -> Resolved {
    Resolved {
        model: name.to_owned(),
        protocol: endpoint.protocol(),
        max_tokens: model.max_tokens,
        reasoning_effort: model.reasoning_effort,
    }
}
