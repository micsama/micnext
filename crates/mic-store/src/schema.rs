use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};

use crate::{Migration, SecretColumn, StoreError};

const CORE: &str = "core";

const CORE_MIGRATIONS: &[Migration] = &[
    Migration {
        module: CORE,
        version: 10,
        sql: CORE_BASELINE,
        secret_columns: &[],
    },
    Migration {
        module: CORE,
        version: 11,
        sql: "ALTER TABLE core_models ADD COLUMN images INTEGER NOT NULL DEFAULT 1 CHECK(images IN (0, 1));",
        secret_columns: &[],
    },
    Migration {
        module: CORE,
        version: 12,
        sql: CORE_JOBS,
        secret_columns: &[],
    },
    Migration {
        module: CORE,
        version: 13,
        sql: CORE_RETENTION,
        secret_columns: &[],
    },
    Migration {
        module: CORE,
        version: 14,
        sql: CORE_TIMELINE,
        secret_columns: &[],
    },
    Migration {
        module: CORE,
        version: 15,
        sql: CORE_JOB_NOTICE,
        secret_columns: &[],
    },
];

/// 后台任务中途通知 `JobNotice` 也是 run 输入。
const CORE_JOB_NOTICE: &str = "
DROP INDEX idx_messages_unclaimed;
CREATE INDEX idx_messages_unclaimed ON core_messages(session_id, id)
  WHERE run_id IS NULL AND kind IN ('UserInput', 'Completion', 'Command', 'JobNotice');
";

const CORE_TIMELINE: &str = "
CREATE TABLE core_model_call_inputs_v14 (
  call_id INTEGER NOT NULL REFERENCES core_model_calls(id),
  message_id INTEGER NOT NULL REFERENCES core_messages(id),
  role TEXT NOT NULL CHECK(role IN ('excluded', 'instruction', 'elide')),
  PRIMARY KEY (call_id, message_id, role)
);
INSERT INTO core_model_call_inputs_v14 SELECT call_id, message_id, role FROM core_model_call_inputs WHERE role <> 'tail';
DROP TABLE core_model_call_inputs;
ALTER TABLE core_model_call_inputs_v14 RENAME TO core_model_call_inputs;
";

/// 压缩保留按窗口比例；调用记录补压缩指令的重建事实。
const CORE_RETENTION: &str = "
ALTER TABLE core_settings DROP COLUMN compaction_keep_steps;
ALTER TABLE core_settings ADD COLUMN compaction_keep_percent INTEGER NOT NULL DEFAULT 15
  CHECK(compaction_keep_percent BETWEEN 0 AND 50);
ALTER TABLE core_model_calls ADD COLUMN context_keep_from INTEGER REFERENCES core_messages(id);
CREATE TABLE core_model_call_inputs_v13 (
  call_id     INTEGER NOT NULL REFERENCES core_model_calls(id),
  message_id  INTEGER NOT NULL REFERENCES core_messages(id),
  role        TEXT NOT NULL CHECK(role IN ('excluded', 'tail', 'instruction', 'elide')),
  PRIMARY KEY (call_id, message_id, role)
);
INSERT INTO core_model_call_inputs_v13 SELECT call_id, message_id, role FROM core_model_call_inputs;
DROP TABLE core_model_call_inputs;
ALTER TABLE core_model_call_inputs_v13 RENAME TO core_model_call_inputs;
";

/// 后台任务；输出在数据目录 `jobs/<id>/`，不进库。
const CORE_JOBS: &str = "
CREATE TABLE core_jobs (
  id                  INTEGER PRIMARY KEY,
  kind                TEXT NOT NULL CHECK(kind IN ('tool', 'agent')),
  owner_session_id    INTEGER NOT NULL REFERENCES core_sessions(id),
  person_id           INTEGER NOT NULL REFERENCES core_persons(id),
  origin_run_id       INTEGER NOT NULL REFERENCES core_runs(id),
  origin_tool_call_id TEXT NOT NULL,
  tool_name           TEXT NOT NULL,
  description         TEXT NOT NULL,
  created_at          INTEGER NOT NULL,
  timeout_ms          INTEGER,
  state               TEXT CHECK(state IN
                        ('running', 'completed', 'failed', 'timed_out', 'cancelled', 'interrupted')),
  finished_at         INTEGER,
  report_message_id   INTEGER REFERENCES core_messages(id),
  child_session_id    INTEGER UNIQUE REFERENCES core_sessions(id),
  UNIQUE(origin_run_id, origin_tool_call_id),
  CHECK((kind = 'tool') = (state IS NOT NULL)),
  CHECK((kind = 'agent') = (child_session_id IS NOT NULL))
);
CREATE INDEX idx_jobs_owner ON core_jobs(owner_session_id);
CREATE INDEX idx_jobs_running ON core_jobs(id) WHERE state = 'running';
ALTER TABLE core_runs ADD COLUMN report_message_id INTEGER REFERENCES core_messages(id);
";

/// 当前形态的完整 schema；更早的迁移链已删除。
const CORE_BASELINE: &str = "
CREATE TABLE core_persons (
  id          INTEGER PRIMARY KEY,
  name        TEXT NOT NULL UNIQUE,
  created_at  INTEGER NOT NULL
);

CREATE TABLE core_person_identities (
  channel       TEXT NOT NULL,
  external_id   TEXT NOT NULL,
  person_id     INTEGER NOT NULL REFERENCES core_persons(id),
  display_name  TEXT NOT NULL,
  created_at    INTEGER NOT NULL,
  PRIMARY KEY (channel, external_id)
);

CREATE TABLE core_personas (
  id          INTEGER PRIMARY KEY,
  name        TEXT NOT NULL,
  prompt      TEXT NOT NULL,
  builtin     INTEGER NOT NULL,
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL,
  deleted_at  INTEGER
);
CREATE UNIQUE INDEX idx_personas_name ON core_personas(name) WHERE deleted_at IS NULL;
-- NOTE: 内置人设内容由启动同步写入（personas.rs），此处只占位 id 1 以满足 core_settings 外键。
INSERT INTO core_personas (id, name, prompt, builtin, created_at, updated_at) VALUES (1, '默认', '', 1, 0, 0);

CREATE TABLE core_endpoints (
  id          INTEGER PRIMARY KEY,
  name        TEXT NOT NULL,
  kind        TEXT NOT NULL,
  config_json TEXT NOT NULL,
  key_cipher  BLOB,
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL,
  deleted_at  INTEGER
);
CREATE UNIQUE INDEX idx_endpoints_name ON core_endpoints(name) WHERE deleted_at IS NULL;

CREATE TABLE core_models (
  id                 INTEGER PRIMARY KEY,
  endpoint_id        INTEGER NOT NULL REFERENCES core_endpoints(id),
  name               TEXT NOT NULL,
  config_json        TEXT NOT NULL,
  context_window     INTEGER CHECK(context_window > 0),
  price_input        REAL CHECK(price_input >= 0),
  price_cache_read   REAL CHECK(price_cache_read >= 0),
  price_cache_write  REAL CHECK(price_cache_write >= 0),
  price_output       REAL CHECK(price_output >= 0),
  created_at         INTEGER NOT NULL,
  updated_at         INTEGER NOT NULL,
  deleted_at         INTEGER
);
CREATE UNIQUE INDEX idx_models_name ON core_models(endpoint_id, name) WHERE deleted_at IS NULL;

CREATE TABLE core_settings (
  id                          INTEGER PRIMARY KEY CHECK (id = 1),
  default_persona_id          INTEGER NOT NULL REFERENCES core_personas(id),
  general_prompt              TEXT NOT NULL,
  default_workdir             TEXT NOT NULL,
  max_turns                   INTEGER NOT NULL CHECK (max_turns >= 1),
  default_model_id            INTEGER REFERENCES core_models(id),
  compaction_trigger_percent  INTEGER NOT NULL DEFAULT 80 CHECK(compaction_trigger_percent BETWEEN 1 AND 80),
  compaction_keep_steps       INTEGER NOT NULL DEFAULT 15 CHECK(compaction_keep_steps BETWEEN 0 AND 100),
  title_model_id              INTEGER REFERENCES core_models(id),
  title_reasoning_effort      TEXT
    CHECK(title_reasoning_effort IS NULL OR (title_model_id IS NOT NULL AND title_reasoning_effort IN ('none','minimal','low','medium','high','xhigh','max')))
);
INSERT INTO core_settings (id, default_persona_id, general_prompt, default_workdir, max_turns)
  VALUES (1, 1, '', '~/workspace/mic', 50);

CREATE TABLE core_sessions (
  id                  INTEGER PRIMARY KEY,
  kind                TEXT NOT NULL,
  channel             TEXT,
  chat                TEXT,
  parent_tool_call_id TEXT,
  trigger_module      TEXT,
  trigger_ref         TEXT,
  parent_session_id   INTEGER REFERENCES core_sessions(id),
  delivery_channel    TEXT,
  delivery_payload    TEXT,
  pwd                 TEXT NOT NULL,
  tool_scope          TEXT NOT NULL,
  created_at          INTEGER NOT NULL,
  persona_id          INTEGER NOT NULL REFERENCES core_personas(id),
  model_id            INTEGER REFERENCES core_models(id),
  reasoning_effort    TEXT
    CHECK(reasoning_effort IS NULL OR (model_id IS NOT NULL AND reasoning_effort IN ('none','minimal','low','medium','high','xhigh','max'))),
  archived_at         INTEGER,
  title               TEXT,
  title_error         TEXT
);
CREATE UNIQUE INDEX idx_root_chat ON core_sessions(channel, chat) WHERE kind = 'root';
CREATE INDEX idx_sessions_delivery ON core_sessions(delivery_channel)
  WHERE delivery_channel IS NOT NULL;

CREATE TABLE core_runs (
  id           INTEGER PRIMARY KEY,
  session_id   INTEGER NOT NULL REFERENCES core_sessions(id),
  state        TEXT NOT NULL,
  created_at   INTEGER NOT NULL,
  finished_at  INTEGER
);
CREATE INDEX idx_runs_executing ON core_runs(session_id) WHERE state = 'executing';

CREATE TABLE core_model_calls (
  id                  INTEGER PRIMARY KEY,
  session_id          INTEGER NOT NULL REFERENCES core_sessions(id),
  run_id              INTEGER REFERENCES core_runs(id),
  purpose             TEXT NOT NULL,
  model               TEXT NOT NULL,
  model_id            INTEGER REFERENCES core_models(id),
  reasoning_effort    TEXT,
  error               TEXT,
  stop                TEXT,
  input_tokens        INTEGER,
  output_tokens       INTEGER,
  cache_read_tokens   INTEGER,
  cache_write_tokens  INTEGER,
  reasoning_tokens    INTEGER,
  started_at          INTEGER NOT NULL,
  first_output_at     INTEGER,
  finished_at         INTEGER NOT NULL,
  context_boundary_id INTEGER REFERENCES core_messages(id),
  context_through_id  INTEGER REFERENCES core_messages(id),
  context_skip        INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_model_calls_run ON core_model_calls(run_id);
CREATE INDEX idx_model_calls_session ON core_model_calls(session_id);

CREATE TABLE core_messages (
  id                    INTEGER PRIMARY KEY,
  session_id            INTEGER NOT NULL REFERENCES core_sessions(id),
  run_id                INTEGER REFERENCES core_runs(id),
  model_call_id         INTEGER REFERENCES core_model_calls(id),
  payload               TEXT NOT NULL,
  created_at            INTEGER NOT NULL,
  delivered_at          INTEGER,
  compaction_revoked_at INTEGER,
  kind                  TEXT NOT NULL GENERATED ALWAYS AS (json_extract(payload, '$.kind')) VIRTUAL,
  person_id             INTEGER GENERATED ALWAYS AS (json_extract(payload, '$.person')) VIRTUAL
                        REFERENCES core_persons(id)
);
CREATE INDEX idx_messages_session ON core_messages(session_id, id);
CREATE INDEX idx_messages_run ON core_messages(run_id);
CREATE INDEX idx_messages_unclaimed ON core_messages(session_id, id)
  WHERE run_id IS NULL AND kind IN ('UserInput', 'Completion', 'Command');
CREATE INDEX idx_messages_undelivered ON core_messages(session_id)
  WHERE delivered_at IS NULL AND kind IN ('Reply', 'Notification');

CREATE TABLE core_model_call_inputs (
  call_id     INTEGER NOT NULL REFERENCES core_model_calls(id),
  message_id  INTEGER NOT NULL REFERENCES core_messages(id),
  role        TEXT NOT NULL CHECK(role IN ('excluded', 'tail', 'instruction')),
  PRIMARY KEY (call_id, message_id, role)
);

CREATE TABLE core_boundary_inputs (
  boundary_id INTEGER NOT NULL REFERENCES core_messages(id),
  message_id INTEGER NOT NULL REFERENCES core_messages(id),
  PRIMARY KEY(boundary_id, message_id)
);

CREATE TABLE core_input_holds (
  message_id INTEGER PRIMARY KEY REFERENCES core_messages(id),
  held_at    INTEGER NOT NULL
);

CREATE TABLE core_images (
  id         INTEGER PRIMARY KEY,
  session_id INTEGER NOT NULL REFERENCES core_sessions(id),
  format     TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  bytes      BLOB NOT NULL
);
CREATE INDEX core_images_session ON core_images(session_id);
";

/// 设置连接级 pragma，先应用内核迁移，再按传入顺序应用各模块迁移。
/// 版本检查先于任何写入：模块没有版本记录即直接建基线；库版本低于基线（旧格式）或超前即整体拒绝打开。
pub(crate) fn init(conn: &mut Connection, modules: &[Migration]) -> Result<(), StoreError> {
    // NOTE: journal_mode/foreign_keys 在事务内无效，只能在迁移事务外设置。
    conn.pragma_update_and_check(None, "journal_mode", "WAL", |_| Ok(()))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
           module   TEXT PRIMARY KEY,
           version  INTEGER NOT NULL
         );",
    )?;

    let plan = group(CORE_MIGRATIONS.iter().chain(modules));
    let mut pending: Vec<&Migration> = Vec::new();
    for (module, migrations) in plan {
        let baseline = migrations.first().map_or(0, |m| m.version);
        let known = migrations.last().map_or(0, |m| m.version);
        let db = applied_version(conn, module)?;
        match db {
            Some(version) if version < baseline => {
                return Err(StoreError::SchemaRebuild {
                    module: module.to_owned(),
                    version,
                    baseline,
                })
            }
            Some(db) if db > known => {
                return Err(StoreError::SchemaAhead {
                    module: module.to_owned(),
                    db,
                    known,
                })
            }
            _ => {}
        }
        let db = db.unwrap_or(0);
        pending.extend(migrations.into_iter().filter(|m| m.version > db));
    }

    for m in pending {
        let tx = conn.transaction()?;
        tx.execute_batch(m.sql)?;
        tx.execute(
            "INSERT INTO schema_migrations (module, version) VALUES (?1, ?2)
             ON CONFLICT(module) DO UPDATE SET version = excluded.version",
            params![m.module, m.version],
        )?;
        tx.commit()?;
    }
    crate::personas::sync(conn, now_ms())?;
    Ok(())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时钟早于 1970")
        .as_millis() as i64
}

/// 内核与各模块迁移声明的全部凭据列。
pub(crate) fn secret_columns(modules: &[Migration]) -> Vec<SecretColumn> {
    CORE_MIGRATIONS
        .iter()
        .chain(modules)
        .flat_map(|m| m.secret_columns.iter().copied())
        .collect()
}

/// 按模块首次出现的顺序分组，组内按版本升序。
fn group<'a>(
    migrations: impl Iterator<Item = &'a Migration>,
) -> Vec<(&'a str, Vec<&'a Migration>)> {
    let mut order: Vec<&str> = Vec::new();
    let mut by_module: BTreeMap<&str, Vec<&Migration>> = BTreeMap::new();
    for m in migrations {
        by_module
            .entry(m.module)
            .or_insert_with(|| {
                order.push(m.module);
                Vec::new()
            })
            .push(m);
    }
    order
        .into_iter()
        .map(|module| {
            let mut ms = by_module.remove(module).unwrap_or_default();
            ms.sort_by_key(|m| m.version);
            (module, ms)
        })
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
    ("core_sessions", "会话与执行", "会话"),
    ("core_runs", "会话与执行", "执行轮次"),
    ("core_jobs", "会话与执行", "后台任务"),
    ("core_messages", "会话与执行", "消息事实"),
    ("core_model_calls", "调用与上下文", "模型调用尝试"),
    ("core_model_call_inputs", "调用与上下文", "调用输入选择依据"),
    ("core_boundary_inputs", "调用与上下文", "压缩边界输入"),
    ("core_input_holds", "调用与上下文", "暂停调度的输入"),
    ("core_images", "会话与执行", "消息图片"),
    ("wechat_account", "微信", "账号与连接"),
    ("wechat_settings", "微信", "当前账号"),
    ("wechat_state", "微信", "轮询状态"),
    ("wechat_inbound_batch", "微信", "入站批次"),
    ("wechat_inbound_message", "微信", "入站消息映射"),
    ("wechat_delivery", "微信", "投递计划与结果"),
    ("wechat_delivery_attempt", "微信", "分块投递尝试"),
];
