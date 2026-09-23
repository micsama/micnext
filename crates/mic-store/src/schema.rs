use std::collections::BTreeMap;

use rusqlite::{params, Connection, OptionalExtension};

use crate::{Migration, StoreError};

const CORE: &str = "core";

const CORE_MIGRATIONS: &[Migration] = &[Migration {
    module: CORE,
    version: 1,
    sql: CORE_V1,
}];

const CORE_V1: &str = "
CREATE TABLE persons (
  id          INTEGER PRIMARY KEY,
  name        TEXT NOT NULL UNIQUE,
  created_at  INTEGER NOT NULL
);

CREATE TABLE person_identities (
  channel       TEXT NOT NULL,
  external_id   TEXT NOT NULL,
  person_id     INTEGER NOT NULL REFERENCES persons(id),
  display_name  TEXT NOT NULL,
  created_at    INTEGER NOT NULL,
  PRIMARY KEY (channel, external_id)
);

CREATE TABLE sessions (
  id                  INTEGER PRIMARY KEY,
  kind                TEXT NOT NULL,
  channel             TEXT,
  chat                TEXT,
  parent_tool_call_id TEXT,
  trigger_module      TEXT,
  trigger_ref         TEXT,
  parent_session_id   INTEGER REFERENCES sessions(id),
  delivery_channel    TEXT,
  delivery_version    INTEGER,
  delivery_payload    TEXT,
  pwd                 TEXT NOT NULL,
  tool_scope          TEXT NOT NULL,
  created_at          INTEGER NOT NULL
);
CREATE UNIQUE INDEX idx_root_chat ON sessions(channel, chat) WHERE kind = 'root';
CREATE INDEX idx_sessions_delivery ON sessions(delivery_channel)
  WHERE delivery_channel IS NOT NULL;

CREATE TABLE session_entries (
  id            INTEGER PRIMARY KEY,
  session_id    INTEGER NOT NULL REFERENCES sessions(id),
  entry_kind    TEXT NOT NULL,
  author_kind   TEXT,
  author_ident  TEXT,
  content_kind  TEXT,
  person_id     INTEGER REFERENCES persons(id),
  payload       TEXT NOT NULL,
  created_at    INTEGER NOT NULL,
  delivered_at  INTEGER
);
CREATE INDEX idx_entries_session ON session_entries(session_id, id);
CREATE INDEX idx_entries_undelivered ON session_entries(session_id)
  WHERE delivered_at IS NULL AND author_kind IN ('assistant', 'notification');

CREATE TABLE queries (
  id               INTEGER PRIMARY KEY,
  session_id       INTEGER NOT NULL REFERENCES sessions(id),
  person_id        INTEGER NOT NULL REFERENCES persons(id),
  claimed_start_id INTEGER NOT NULL,
  claimed_end_id   INTEGER NOT NULL,
  state            TEXT NOT NULL,
  reason           TEXT,
  created_at       INTEGER NOT NULL,
  finished_at      INTEGER
);
CREATE INDEX idx_queries_session ON queries(session_id, id);

CREATE TABLE model_calls (
  id                 INTEGER PRIMARY KEY,
  session_id         INTEGER NOT NULL REFERENCES sessions(id),
  query_id           INTEGER REFERENCES queries(id),
  model              TEXT NOT NULL,
  input_tokens       INTEGER NOT NULL,
  output_tokens      INTEGER NOT NULL,
  cache_read_tokens  INTEGER NOT NULL,
  cache_write_tokens INTEGER NOT NULL,
  reasoning_tokens   INTEGER NOT NULL,
  outcome            TEXT NOT NULL,
  started_at         INTEGER NOT NULL,
  finished_at        INTEGER NOT NULL
);
CREATE INDEX idx_model_calls_session ON model_calls(session_id);
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
    Ok(())
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
