use mic_store::{Migration, SecretColumn};

pub(crate) const MIGRATIONS: &[Migration] = &[Migration {
    module: "wechat",
    version: 3,
    sql: "
CREATE TABLE wechat_account (
  user_id TEXT PRIMARY KEY,
  bot_id TEXT NOT NULL,
  bot_token TEXT NOT NULL,
  base_url TEXT NOT NULL,
  person_id INTEGER NOT NULL REFERENCES core_persons(id),
  session_id INTEGER NOT NULL REFERENCES core_sessions(id),
  connection TEXT NOT NULL CHECK(connection IN ('connected', 'needs_login', 'faulted')),
  updated_at INTEGER NOT NULL
);
CREATE TABLE wechat_settings (
  id INTEGER PRIMARY KEY CHECK(id = 1),
  active_user_id TEXT REFERENCES wechat_account(user_id)
);
INSERT INTO wechat_settings(id) VALUES (1);
CREATE TABLE wechat_state (
  user_id TEXT PRIMARY KEY REFERENCES wechat_account(user_id),
  get_updates_buf TEXT NOT NULL,
  context_token TEXT,
  longpoll_timeout_ms INTEGER NOT NULL
);
CREATE TABLE wechat_inbound_batch (
  id INTEGER PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES wechat_account(user_id),
  snapshot TEXT NOT NULL,
  phase TEXT NOT NULL CHECK(phase IN ('importing', 'completed', 'interrupted')),
  created_at INTEGER NOT NULL,
  finished_at INTEGER
);
CREATE TABLE wechat_delivery (
  message_id INTEGER PRIMARY KEY REFERENCES core_messages(id),
  user_id TEXT NOT NULL REFERENCES wechat_account(user_id),
  plan TEXT NOT NULL,
  outcome TEXT NOT NULL CHECK(outcome IN ('sending', 'sent', 'skipped', 'interrupted')),
  finished_at INTEGER
);
CREATE TABLE wechat_delivery_attempt (
  message_id INTEGER NOT NULL REFERENCES wechat_delivery(message_id),
  chunk_index INTEGER NOT NULL CHECK(chunk_index >= 0),
  attempt_no INTEGER NOT NULL CHECK(attempt_no IN (1, 2)),
  client_id TEXT NOT NULL,
  started_at INTEGER NOT NULL,
  finished_at INTEGER,
  external_message_id TEXT,
  failure TEXT,
  PRIMARY KEY (message_id, chunk_index, attempt_no)
);
CREATE TABLE wechat_inbound_message (
  message_id INTEGER PRIMARY KEY REFERENCES core_messages(id),
  user_id TEXT NOT NULL REFERENCES wechat_account(user_id),
  external_message_id TEXT NOT NULL,
  source_start INTEGER NOT NULL CHECK(source_start >= 0)
);
CREATE INDEX wechat_inbound_message_external
  ON wechat_inbound_message(user_id, external_message_id, message_id);
",
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
