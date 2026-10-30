use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};

use crate::{Schema, SecretColumn, StoreError};

const CORE: Schema = Schema {
    module: "core",
    version: include_str!("schema/version"),
    sql: include_str!("schema/core.sql"),
    secret_columns: &[],
};

/// 只创建当前结构；已有模块必须与当前版本完全一致。
pub(crate) fn init(conn: &mut Connection, modules: &[Schema]) -> Result<(), StoreError> {
    conn.pragma_update_and_check(None, "journal_mode", "WAL", |_| Ok(()))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    let tx = conn.transaction()?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
           module TEXT PRIMARY KEY,
           version INTEGER NOT NULL
         );",
    )?;
    for schema in std::iter::once(&CORE).chain(modules) {
        let expected = schema
            .version
            .trim()
            .parse::<u32>()
            .expect("schema 版本为整数");
        match applied_version(&tx, schema.module)? {
            Some(db) if db != expected => {
                return Err(StoreError::SchemaMismatch {
                    module: schema.module.to_owned(),
                    db,
                    expected,
                });
            }
            Some(_) => {}
            None => {
                tx.execute_batch(schema.sql)?;
                tx.execute(
                    "INSERT INTO schema_migrations (module, version) VALUES (?1, ?2)",
                    params![schema.module, expected],
                )?;
            }
        }
    }
    tx.commit()?;
    crate::personas::sync(conn, now_ms())?;
    Ok(())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时钟早于 1970")
        .as_millis() as i64
}

pub(crate) fn secret_columns(modules: &[Schema]) -> Vec<SecretColumn> {
    modules
        .iter()
        .flat_map(|schema| schema.secret_columns.iter().copied())
        .collect()
}

fn applied_version(conn: &Connection, module: &str) -> Result<Option<u32>, StoreError> {
    Ok(conn
        .query_row(
            "SELECT version FROM schema_migrations WHERE module = ?1",
            [module],
            |r| r.get(0),
        )
        .optional()?)
}

#[cfg(test)]
mod tests;

/// 诊断表目录：表名、分组、说明；仅展示当前库实际存在的表。
pub(crate) const TABLE_CATALOG: &[(&str, &str, &str)] = &[
    ("schema_migrations", "系统", "模块 schema 版本"),
    ("core_persons", "身份与设置", "人员"),
    ("core_person_identities", "身份与设置", "渠道身份绑定"),
    ("core_personas", "身份与设置", "人设"),
    ("core_endpoints", "身份与设置", "模型服务商"),
    ("core_models", "身份与设置", "模型与当前单价"),
    ("core_settings", "身份与设置", "用户设置"),
    ("core_env_vars", "身份与设置", "命令环境变量（值加密）"),
    ("core_sessions", "会话与执行", "会话"),
    ("core_runs", "会话与执行", "执行轮次"),
    ("core_jobs", "会话与执行", "后台任务"),
    ("core_messages", "会话与执行", "消息事实"),
    ("core_model_calls", "调用与上下文", "模型调用尝试"),
    ("core_model_call_inputs", "调用与上下文", "调用输入选择依据"),
    ("core_boundary_inputs", "调用与上下文", "压缩边界输入"),
    ("core_input_holds", "调用与上下文", "暂停调度的输入"),
    ("core_images", "会话与执行", "消息图片"),
    ("core_pushes", "推送", "发给人的推送"),
    ("core_push_deliveries", "推送", "推送的各 Channel 投递"),
    ("core_schedules", "定时任务", "定时任务"),
    ("core_schedule_fires", "定时任务", "每次到点的触发"),
    ("wechat_account", "微信", "账号与连接"),
    ("wechat_settings", "微信", "当前账号"),
    ("wechat_state", "微信", "轮询状态"),
    ("wechat_inbound_batch", "微信", "入站批次"),
    ("wechat_inbound_message", "微信", "入站消息映射"),
    ("wechat_delivery", "微信", "投递计划与结果"),
    ("wechat_delivery_attempt", "微信", "分块投递尝试"),
];
