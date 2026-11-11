use mic_store::{Schema, SecretColumn};

pub(crate) const SCHEMAS: &[Schema] = &[Schema {
    module: "web",
    version: include_str!("schema/version"),
    sql: include_str!("schema.sql"),
    secret_columns: &[
        SecretColumn {
            table: "web_admin",
            column: "password_hash",
        },
        SecretColumn {
            table: "web_admin",
            column: "session_key",
        },
        SecretColumn {
            table: "web_push_keys",
            column: "private_key",
        },
    ],
}];
