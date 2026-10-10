//! ChatGPT 订阅凭据：Codex CLI 签发的 access_token（JWT），只读声明不验签，服务端会验。

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::alphabet::URL_SAFE;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use base64::engine::{DecodePaddingMode, Engine};
use mic_core::{ConfigError, SecretValue};
use serde::Deserialize;

const PAYLOAD: GeneralPurpose = GeneralPurpose::new(
    &URL_SAFE,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

pub(crate) struct ChatgptToken {
    pub(crate) account_id: String,
}

#[derive(Deserialize)]
struct Claims {
    exp: u64,
    #[serde(rename = "https://api.openai.com/auth")]
    auth: AuthClaims,
}

#[derive(Deserialize)]
struct AuthClaims {
    chatgpt_account_id: String,
}

fn err(message: impl Into<String>) -> ConfigError {
    ConfigError {
        field: "api_key".into(),
        message: message.into(),
    }
}

impl ChatgptToken {
    /// 缺失、格式不符或已过期 → `Err`，不发请求。
    pub(crate) fn parse(key: Option<&SecretValue>) -> Result<Self, ConfigError> {
        let key = key.ok_or_else(|| err("请粘贴 ~/.codex/auth.json 里的 tokens.access_token"))?;
        let claims = key
            .expose()
            .split('.')
            .nth(1)
            .and_then(|p| PAYLOAD.decode(p).ok())
            .and_then(|b| serde_json::from_slice::<Claims>(&b).ok())
            .ok_or_else(|| err("这不是 Codex 的 access_token，请复制 tokens.access_token"))?;
        let expires = UNIX_EPOCH + Duration::from_secs(claims.exp);
        if let Ok(ago) = SystemTime::now().duration_since(expires) {
            return Err(err(format!(
                "access_token 已过期（{}前），请在本机运行一次 codex 后重新复制粘贴",
                elapsed(ago)
            )));
        }
        Ok(Self {
            account_id: claims.auth.chatgpt_account_id,
        })
    }
}

fn elapsed(d: Duration) -> String {
    let m = d.as_secs() / 60;
    match m {
        0..60 => format!("{m} 分钟"),
        60..1440 => format!("{} 小时", m / 60),
        _ => format!("{} 天", m / 1440),
    }
}
