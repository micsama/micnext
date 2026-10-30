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
, images INTEGER NOT NULL DEFAULT 1 CHECK(images IN (0, 1))
, starred INTEGER NOT NULL DEFAULT 0 CHECK(starred IN (0, 1)));

CREATE UNIQUE INDEX idx_models_name ON core_models(endpoint_id, name) WHERE deleted_at IS NULL;

CREATE TABLE core_settings (
  id                          INTEGER PRIMARY KEY CHECK (id = 1),
  default_persona_id          INTEGER NOT NULL REFERENCES core_personas(id),
  general_prompt              TEXT NOT NULL,
  default_workdir             TEXT NOT NULL,
  max_turns                   INTEGER NOT NULL CHECK (max_turns >= 1),
  default_model_id            INTEGER REFERENCES core_models(id),
  compaction_trigger_percent  INTEGER NOT NULL DEFAULT 80 CHECK(compaction_trigger_percent BETWEEN 1 AND 80),
  utility_model_id            INTEGER REFERENCES core_models(id),
  utility_reasoning_effort    TEXT
    CHECK(utility_reasoning_effort IS NULL OR (utility_model_id IS NOT NULL AND utility_reasoning_effort IN ('none','minimal','low','medium','high','xhigh','max')))
, compaction_keep_percent INTEGER NOT NULL DEFAULT 15
  CHECK(compaction_keep_percent BETWEEN 0 AND 50), push_channels TEXT NOT NULL DEFAULT '[]');

CREATE TABLE core_sessions (
  id                  INTEGER PRIMARY KEY,
  kind                TEXT NOT NULL,
  channel             TEXT,
  chat                TEXT,
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
, plain_after INTEGER, pinned_at INTEGER, seen_message_id INTEGER, tags_manual INTEGER NOT NULL DEFAULT 0);

CREATE UNIQUE INDEX idx_root_chat ON core_sessions(channel, chat) WHERE kind = 'root';

CREATE INDEX idx_sessions_delivery ON core_sessions(delivery_channel)
  WHERE delivery_channel IS NOT NULL;

CREATE TABLE core_runs (
  id           INTEGER PRIMARY KEY,
  session_id   INTEGER NOT NULL REFERENCES core_sessions(id),
  state        TEXT NOT NULL,
  created_at   INTEGER NOT NULL,
  finished_at  INTEGER
, report_message_id INTEGER REFERENCES core_messages(id));

CREATE INDEX idx_runs_executing ON core_runs(session_id) WHERE state = 'executing';

CREATE INDEX idx_runs_created ON core_runs(created_at);

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
, context_keep_from INTEGER REFERENCES core_messages(id));

CREATE INDEX idx_model_calls_run ON core_model_calls(run_id);

CREATE INDEX idx_model_calls_session ON core_model_calls(session_id);

CREATE INDEX idx_model_calls_started ON core_model_calls(started_at);

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
, delivery_failure TEXT
  CHECK(delivery_failure IS NULL OR delivered_at IS NULL));

CREATE INDEX idx_messages_session ON core_messages(session_id, id);

CREATE INDEX idx_messages_run ON core_messages(run_id);

CREATE INDEX idx_messages_call ON core_messages(model_call_id, kind) WHERE model_call_id IS NOT NULL;

CREATE INDEX idx_messages_boundary ON core_messages(session_id, id) WHERE kind = 'Boundary';

CREATE INDEX idx_messages_time ON core_messages(session_id, created_at, id);

CREATE INDEX idx_messages_tool_time ON core_messages(created_at) WHERE kind = 'ToolResult';

CREATE INDEX idx_messages_tool_failed ON core_messages(session_id) WHERE kind = 'ToolResult' AND (json_extract(payload, '$.outcome.Terminal.Failed') IS NOT NULL OR json_extract(payload, '$.outcome.Terminal.TimedOut') IS NOT NULL);

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

CREATE TABLE "core_model_call_inputs" (
  call_id INTEGER NOT NULL REFERENCES core_model_calls(id),
  message_id INTEGER NOT NULL REFERENCES core_messages(id),
  role TEXT NOT NULL CHECK(role IN ('excluded', 'instruction', 'elide')),
  PRIMARY KEY (call_id, message_id, role)
);

CREATE TABLE core_pushes (
  id                 INTEGER PRIMARY KEY,
  person_id          INTEGER NOT NULL REFERENCES core_persons(id),
  source_session_id  INTEGER NOT NULL REFERENCES core_sessions(id),
  source_run_id      INTEGER REFERENCES core_runs(id),
  text               TEXT NOT NULL,
  created_at         INTEGER NOT NULL,
  read_at            INTEGER
, files TEXT NOT NULL DEFAULT '[]');

CREATE INDEX idx_pushes_person ON core_pushes(person_id, id);

CREATE INDEX idx_pushes_run ON core_pushes(source_run_id) WHERE source_run_id IS NOT NULL;

CREATE TABLE "core_push_deliveries" (
  push_id     INTEGER NOT NULL REFERENCES core_pushes(id),
  channel     TEXT NOT NULL,
  message_id  INTEGER REFERENCES core_messages(id),
  failure     TEXT,
  PRIMARY KEY (push_id, channel),
  CHECK((message_id IS NULL) <> (failure IS NULL))
);

CREATE INDEX idx_messages_unclaimed ON core_messages(session_id, id)
  WHERE run_id IS NULL AND kind IN ('UserInput', 'Completion', 'Command', 'JobNotice', 'Trigger');

CREATE TABLE core_schedules (
  id             INTEGER PRIMARY KEY,
  person_id      INTEGER NOT NULL REFERENCES core_persons(id),
  name           TEXT NOT NULL,
  rule           TEXT NOT NULL,
  prompt         TEXT NOT NULL,
  next_run_at    INTEGER,
  paused_reason  TEXT CHECK(paused_reason IS NULL OR next_run_at IS NULL),
  enabled_at     INTEGER NOT NULL,
  session_id     INTEGER NOT NULL UNIQUE REFERENCES core_sessions(id),
  created_at     INTEGER NOT NULL,
  updated_at     INTEGER NOT NULL
);

CREATE TABLE core_schedule_fires (
  id            INTEGER PRIMARY KEY,
  schedule_id   INTEGER NOT NULL REFERENCES core_schedules(id) ON DELETE CASCADE,
  due_at        INTEGER NOT NULL,
  message_id    INTEGER REFERENCES core_messages(id),
  skipped       TEXT CHECK(skipped IN ('missed', 'busy')),
  missed_count  INTEGER,
  CHECK((message_id IS NULL) <> (skipped IS NULL)),
  CHECK((skipped = 'missed') = (missed_count IS NOT NULL))
);

CREATE INDEX idx_schedule_fires ON core_schedule_fires(schedule_id, id);

CREATE INDEX idx_messages_undelivered ON core_messages(session_id)
  WHERE delivered_at IS NULL AND delivery_failure IS NULL AND kind IN ('Reply', 'Notification', 'Attachment');

CREATE TABLE "core_jobs" (
  id INTEGER PRIMARY KEY,
  kind TEXT NOT NULL CHECK(kind IN ('tool', 'agent')),
  owner_session_id INTEGER NOT NULL REFERENCES core_sessions(id),
  person_id INTEGER NOT NULL REFERENCES core_persons(id),
  origin_run_id INTEGER NOT NULL REFERENCES core_runs(id),
  tool_name TEXT NOT NULL,
  description TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  timeout_ms INTEGER,
  state TEXT CHECK(state IN ('running', 'completed', 'failed', 'timed_out', 'cancelled', 'interrupted')),
  finished_at INTEGER,
  child_session_id INTEGER UNIQUE REFERENCES core_sessions(id),
  origin_reply_id INTEGER NOT NULL REFERENCES core_messages(id),
  origin_call_index INTEGER NOT NULL CHECK(origin_call_index >= 0),
  UNIQUE(origin_reply_id, origin_call_index),
  CHECK((kind = 'tool') = (state IS NOT NULL)),
  CHECK((kind = 'agent') = (child_session_id IS NOT NULL))
);

CREATE INDEX idx_jobs_owner ON core_jobs(owner_session_id);

CREATE INDEX idx_jobs_running ON core_jobs(id) WHERE state = 'running';

CREATE TABLE core_tags (
  id          INTEGER PRIMARY KEY,
  name        TEXT NOT NULL UNIQUE,
  created_at  INTEGER NOT NULL
);

CREATE TABLE core_session_tags (
  session_id  INTEGER NOT NULL REFERENCES core_sessions(id),
  tag_id      INTEGER NOT NULL REFERENCES core_tags(id) ON DELETE CASCADE,
  PRIMARY KEY (session_id, tag_id)
);

CREATE INDEX idx_session_tags_tag ON core_session_tags(tag_id);

CREATE TABLE core_env_vars (
  name        TEXT PRIMARY KEY,
  value_cipher BLOB NOT NULL
);

INSERT INTO core_personas (id, name, prompt, builtin, created_at, updated_at) VALUES (1, '默认', '', 1, 0, 0);
INSERT INTO core_settings (id, default_persona_id, general_prompt, default_workdir, max_turns) VALUES (1, 1, '', '~/workspace/mic', 50);
