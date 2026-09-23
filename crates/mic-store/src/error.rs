#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("payload 序列化: {0}")]
    Payload(#[from] serde_json::Error),
    #[error("模块 {module} 库版本 v{db} 高于二进制已知 v{known}")]
    SchemaAhead { module: String, db: u32, known: u32 },
}
