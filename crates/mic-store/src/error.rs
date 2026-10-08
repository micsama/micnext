#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("payload 序列化: {0}")]
    Payload(#[from] serde_json::Error),
    #[error("模块 {module} 库版本 v{db} 高于二进制已知 v{known}")]
    SchemaAhead { module: String, db: u32, known: u32 },
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
