//! 微信账号备份在边界解析；恢复交给既有账号协调器。

use mic_core::ChannelSetupError;
use serde::{Deserialize, Serialize};

use crate::account::{Credentials, Token, WechatUserId};
use crate::client::{https_base, required};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AccountBackup {
    user_id: String,
    bot_id: String,
    bot_token: String,
    base_url: String,
}

impl From<&Credentials> for AccountBackup {
    fn from(c: &Credentials) -> Self {
        Self {
            user_id: c.user_id.0.clone(),
            bot_id: c.bot_id.clone(),
            bot_token: c.token.expose().to_owned(),
            base_url: c.base_url.to_string(),
        }
    }
}

pub(crate) fn parse(value: serde_json::Value) -> Result<Credentials, ChannelSetupError> {
    let invalid = |reason: &str| ChannelSetupError::BackupInvalid {
        reason: reason.into(),
    };
    let b: AccountBackup =
        serde_json::from_value(value).map_err(|_| invalid("微信账号字段不符"))?;
    Ok(Credentials {
        user_id: WechatUserId(
            required(Some(b.user_id), "user_id 为空").map_err(|e| invalid(&e.to_string()))?,
        ),
        bot_id: required(Some(b.bot_id), "bot_id 为空").map_err(|e| invalid(&e.to_string()))?,
        token: Token::new(
            required(Some(b.bot_token), "bot_token 为空").map_err(|e| invalid(&e.to_string()))?,
        ),
        base_url: https_base(&b.base_url).map_err(|e| invalid(&e.to_string()))?,
    })
}
