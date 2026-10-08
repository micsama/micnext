#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("payload 序列化: {0}")]
    Payload(#[from] serde_json::Error),
    #[error("模块 {module} 库版本 v{db} 高于二进制已知 v{known}")]
    SchemaAhead { module: String, db: u32, known: u32 },
    #[error("主密钥文件读写失败: {0}")]
    SecretKeyIo(#[from] std::io::Error),
    #[error("主密钥文件不可用: {0}")]
    SecretKeyInvalid(String),
    #[error(
        "服务商 {endpoint} 的 API key 无法解密：主密钥与库不匹配或数据被改动，请在设置页重填该 key"
    )]
    SecretIntegrity { endpoint: i64 },
}

/// 设置与人设写入的业务错误；文案面向用户。
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("人设不存在")]
    PersonaNotFound,
    #[error("内置人设不能修改或删除，可以复制后编辑")]
    Builtin,
    #[error("这是默认人设：先在对话偏好里换一个默认，再删除它")]
    IsDefault,
    #[error("已有同名人设「{0}」")]
    NameTaken(String),
    #[error("人设已删除，请重新选择")]
    Deleted,
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for SettingsError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Store(e.into())
    }
}

/// 模型条目与选择的业务错误；文案面向用户。
#[derive(Debug, thiserror::Error)]
pub enum ModelSettingsError {
    #[error("模型不存在")]
    NotFound,
    #[error("模型已删除，请重新选择")]
    Deleted,
    #[error("服务商不存在")]
    EndpointNotFound,
    #[error("服务商已删除")]
    EndpointDeleted,
    #[error("已有同名项「{0}」")]
    NameTaken(String),
    #[error("默认模型在这里：先把默认换到别处，再删除")]
    DefaultInUse,
    #[error("会话正在执行，等本轮结束后再切换模型")]
    SessionExecuting,
    #[error("新建模型没有可沿用的 API key")]
    InvalidSecretEdit,
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for ModelSettingsError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Store(e.into())
    }
}
