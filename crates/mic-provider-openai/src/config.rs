use mic_core::{ConfigError, SecretValue};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest::Url;
use serde::{Deserialize, Serialize};

const DEEPSEEK_URL: &str = "https://api.deepseek.com";
const OLLAMA_URL: &str = "http://localhost:11434/v1";

/// 服务商的配置 JSON；DeepSeek 地址固定，不接受 `base_url`。
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

/// 推理强度，取值与 DeepSeek 的 `reasoning_effort` 同名；目前只有 DeepSeek 方言支持。
#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ReasoningEffort {
    /// 关闭思考模式。
    None,
    Low,
    High,
    Max,
}

impl ReasoningEffort {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Low => "low",
            Self::High => "high",
            Self::Max => "max",
        }
    }
}

/// DeepSeek 条目不写 `reasoning_effort` 时的取值。
const DEEPSEEK_DEFAULT_EFFORT: ReasoningEffort = ReasoningEffort::Low;

#[derive(Deserialize, Serialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Preset {
    Generic,
    Deepseek,
    Ollama,
}

/// 协议差异只在方言处分支。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dialect {
    DeepSeek,
    Ollama,
    Generic,
}

/// 解析完成、可直接用于请求的条目。
pub(crate) struct Resolved {
    pub(crate) model: String,
    pub(crate) dialect: Dialect,
    /// `{base_url}/chat/completions`。
    pub(crate) url: String,
    /// 有 key 时含鉴权头。
    pub(crate) headers: HeaderMap,
    pub(crate) max_tokens: Option<u32>,
    /// 仅 DeepSeek 方言为 `Some`。
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

    /// 校验并补全地址，返回规范形态（DeepSeek 不存地址）。
    pub(crate) fn canonical(mut self) -> Result<Self, ConfigError> {
        let base_url = match (self.preset, self.base_url.take()) {
            (Preset::Deepseek, None) => return Ok(self),
            (Preset::Deepseek, Some(_)) => {
                return Err(err("base_url", "DeepSeek 地址固定，不需要填写"))
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
            Preset::Generic => Some("OPENAI_API_KEY"),
            Preset::Ollama => None,
        }
    }

    /// `{base_url}`，要求 `self` 已是规范形态。
    pub(crate) fn base_url(&self) -> &str {
        match self.preset {
            Preset::Deepseek => DEEPSEEK_URL,
            _ => self.base_url.as_deref().expect("规范形态必有 base_url"),
        }
    }

    /// 有 key 时含鉴权头。
    pub(crate) fn headers(&self, key: Option<SecretValue>) -> Result<HeaderMap, ConfigError> {
        let mut headers = HeaderMap::new();
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
        if name.trim().is_empty() {
            return Err(err("name", "模型名不能为空"));
        }
        self.reasoning_effort = match (endpoint.preset, self.reasoning_effort) {
            (Preset::Deepseek, effort) => Some(effort.unwrap_or(DEEPSEEK_DEFAULT_EFFORT)),
            (_, None) => None,
            (_, Some(_)) => return Err(err("reasoning_effort", "目前只有 DeepSeek 支持推理强度")),
        };
        Ok(self)
    }
}

/// 要求两份配置都已是规范形态（来自库内已校验的 JSON）。
pub(crate) fn resolve(
    endpoint: &EndpointConfig,
    model: ModelConfig,
    name: &str,
    key: Option<SecretValue>,
) -> Result<Resolved, ConfigError> {
    let dialect = match endpoint.preset {
        Preset::Generic => Dialect::Generic,
        Preset::Deepseek => Dialect::DeepSeek,
        Preset::Ollama => Dialect::Ollama,
    };
    Ok(Resolved {
        model: name.to_owned(),
        dialect,
        url: format!("{}/chat/completions", endpoint.base_url()),
        headers: endpoint.headers(key)?,
        max_tokens: model.max_tokens,
        reasoning_effort: model.reasoning_effort,
    })
}
