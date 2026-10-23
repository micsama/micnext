use mic_store::{Schema, SecretColumn};

pub(crate) const SCHEMAS: &[Schema] = &[Schema {
    module: "wechat",
    version: include_str!("schema/version"),
    sql: include_str!("schema.sql"),
    secret_columns: &[
        SecretColumn {
            table: "wechat_account",
            column: "bot_token",
        },
        SecretColumn {
            table: "wechat_state",
            column: "context_token",
        },
    ],
}];

#[cfg(test)]
mod tests;
