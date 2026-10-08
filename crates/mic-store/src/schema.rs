use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};

use crate::{Migration, StoreError};

const CORE: &str = "core";

const CORE_MIGRATIONS: &[Migration] = &[
    Migration {
        module: CORE,
        version: 1,
        sql: CORE_V1,
    },
    Migration {
        module: CORE,
        version: 2,
        sql: CORE_V2,
    },
];

const CORE_V1: &str = "
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
  delivery_version    INTEGER,
  delivery_payload    TEXT,
  pwd                 TEXT NOT NULL,
  tool_scope          TEXT NOT NULL,
  created_at          INTEGER NOT NULL
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
  id                 INTEGER PRIMARY KEY,
  session_id         INTEGER NOT NULL REFERENCES core_sessions(id),
  run_id             INTEGER REFERENCES core_runs(id),
  model              TEXT NOT NULL,
  error              TEXT,
  input_tokens       INTEGER,
  output_tokens      INTEGER,
  cache_read_tokens  INTEGER,
  cache_write_tokens INTEGER,
  reasoning_tokens   INTEGER,
  started_at         INTEGER NOT NULL,
  finished_at        INTEGER NOT NULL
);
CREATE INDEX idx_model_calls_session ON core_model_calls(session_id);

CREATE TABLE core_messages (
  id             INTEGER PRIMARY KEY,
  session_id     INTEGER NOT NULL REFERENCES core_sessions(id),
  run_id         INTEGER REFERENCES core_runs(id),
  model_call_id  INTEGER REFERENCES core_model_calls(id),
  payload        TEXT NOT NULL,
  created_at     INTEGER NOT NULL,
  delivered_at   INTEGER,
  kind           TEXT NOT NULL GENERATED ALWAYS AS (json_extract(payload, '$.kind')) VIRTUAL,
  person_id      INTEGER GENERATED ALWAYS AS (json_extract(payload, '$.person')) VIRTUAL
                 REFERENCES core_persons(id)
);
CREATE INDEX idx_messages_session ON core_messages(session_id, id);
CREATE INDEX idx_messages_run ON core_messages(run_id);
CREATE INDEX idx_messages_unclaimed ON core_messages(session_id, id)
  WHERE run_id IS NULL AND kind IN ('UserInput', 'Completion');
CREATE INDEX idx_messages_undelivered ON core_messages(session_id)
  WHERE delivered_at IS NULL AND kind IN ('Reply', 'Notification');
";

/// 人设、对话偏好与 run 设置快照。
const CORE_V2: &str = "
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

CREATE TABLE core_settings (
  id                  INTEGER PRIMARY KEY CHECK (id = 1),
  default_persona_id  INTEGER NOT NULL REFERENCES core_personas(id),
  general_prompt      TEXT NOT NULL,
  default_workdir     TEXT NOT NULL,
  max_turns           INTEGER NOT NULL CHECK (max_turns >= 1)
);
INSERT INTO core_settings VALUES (1, 1, '', '~/workspace/mic', 50);

-- NOTE: 开着外键时 ADD COLUMN 不能带非 NULL 默认值的 REFERENCES，引用由 store 写入处校验。
ALTER TABLE core_sessions ADD COLUMN persona_id INTEGER NOT NULL DEFAULT 1;
ALTER TABLE core_runs ADD COLUMN persona_id INTEGER;
ALTER TABLE core_runs ADD COLUMN persona_name TEXT;
ALTER TABLE core_runs ADD COLUMN persona_prompt TEXT;
ALTER TABLE core_runs ADD COLUMN general_prompt TEXT;
ALTER TABLE core_runs ADD COLUMN max_turns INTEGER;
";

/// 设置连接级 pragma，先应用内核迁移，再按传入顺序应用各模块迁移。
/// 版本检查先于任何写入：任一模块库版本超前即整体拒绝打开。
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
        let db = applied_version(conn, module)?;
        let known = migrations.iter().map(|m| m.version).max().unwrap_or(0);
        if db > known {
            return Err(StoreError::SchemaAhead {
                module: module.to_owned(),
                db,
                known,
            });
        }
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
        .map_or(0, |d| d.as_millis() as i64)
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

fn applied_version(conn: &Connection, module: &str) -> Result<u32, StoreError> {
    let version = conn
        .query_row(
            "SELECT version FROM schema_migrations WHERE module = ?1",
            [module],
            |r| r.get(0),
        )
        .optional()?;
    Ok(version.unwrap_or(0))
}
