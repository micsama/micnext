use std::collections::BTreeMap;

use mic_core::BoxError;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION};
use serde::Deserialize;

/// `[models.<name>]` 条目（`kind` 已由内核去掉）。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EntryConfig {
    model: String,
    preset: Option<Preset>,
    base_url: Option<String>,
    api_key: Option<String>,
    api_key_env: Option<String>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    max_tokens: Option<u32>,
    reasoning_effort: Option<ReasoningEffort>,
}

/// 推理强度，取值与 DeepSeek 的 `reasoning_effort` 同名；目前只有 DeepSeek 方言支持。
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Preset {
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
    /// 鉴权头与自定义头。
    pub(crate) headers: HeaderMap,
    pub(crate) max_tokens: Option<u32>,
    /// 仅 DeepSeek 方言为 `Some`。
    pub(crate) reasoning_effort: Option<ReasoningEffort>,
}

impl EntryConfig {
    pub(crate) fn resolve(self, name: &str) -> Result<Resolved, BoxError> {
        let (dialect, default_url) = match self.preset {
            Some(Preset::Deepseek) => (Dialect::DeepSeek, Some("https://api.deepseek.com")),
            Some(Preset::Ollama) => (Dialect::Ollama, Some("http://localhost:11434/v1")),
            None => (Dialect::Generic, None),
        };
        let base_url = match (self.base_url.as_deref(), default_url) {
            (Some(url), _) | (None, Some(url)) => url,
            (None, None) => {
                return Err(format!(
                    "模型 `{name}` 没有 preset，必须写 base_url（如 \"https://example.com/v1\"）"
                )
                .into())
            }
        };
        let reasoning_effort = match (dialect, self.reasoning_effort) {
            (Dialect::DeepSeek, effort) => Some(effort.unwrap_or(DEEPSEEK_DEFAULT_EFFORT)),
            (Dialect::Ollama | Dialect::Generic, None) => None,
            (Dialect::Ollama | Dialect::Generic, Some(_)) => {
                return Err(format!(
                    "模型 `{name}` 写了 reasoning_effort，但目前只有 preset = \"deepseek\" 支持它"
                )
                .into())
            }
        };
        let key = resolve_key(name, self.preset, self.api_key, self.api_key_env)?;

        let mut headers = HeaderMap::new();
        let mut auth = HeaderValue::from_str(&format!("Bearer {key}"))
            .map_err(|_| format!("模型 `{name}` 的 API key 含有不能放进请求头的字符"))?;
        auth.set_sensitive(true);
        headers.insert(AUTHORIZATION, auth);
        for (k, v) in &self.headers {
            let (Ok(k), Ok(v)) = (
                HeaderName::from_bytes(k.as_bytes()),
                HeaderValue::from_str(v),
            ) else {
                return Err(format!("模型 `{name}` 的 headers 里 `{k}` 不是合法的请求头").into());
            };
            headers.insert(k, v);
        }

        Ok(Resolved {
            model: self.model,
            dialect,
            url: format!("{}/chat/completions", base_url.trim_end_matches('/')),
            headers,
            max_tokens: self.max_tokens,
            reasoning_effort,
        })
    }
}

fn resolve_key(
    name: &str,
    preset: Option<Preset>,
    api_key: Option<String>,
    api_key_env: Option<String>,
) -> Result<String, BoxError> {
    let how = format!(
        "在 [models.{name}] 写 api_key = \"...\"，或写 api_key_env = \"变量名\" 并设置该环境变量"
    );
    let from_env = |var: &str| match std::env::var(var) {
        Ok(v) if !v.is_empty() => Ok(v),
        _ => Err(format!(
            "模型 `{name}` 读不到 API key：环境变量 {var} 未设置。请设置它，或{how}"
        )),
    };
    match (api_key, api_key_env, preset) {
        (Some(_), Some(_), _) => {
            Err(format!("模型 `{name}` 的 api_key 与 api_key_env 只能写一个").into())
        }
        (Some(key), None, _) => Ok(key),
        (None, Some(var), _) => Ok(from_env(&var)?),
        (None, None, Some(Preset::Deepseek)) => Ok(from_env("DEEPSEEK_API_KEY")?),
        // Ollama 不校验 key，占位即可。
        (None, None, Some(Preset::Ollama)) => Ok("ollama".into()),
        (None, None, None) => Err(format!("模型 `{name}` 缺少 API key：{how}").into()),
    }
}
