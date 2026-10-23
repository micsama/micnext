CREATE TEMP TABLE tool_occurrences (
  reply_id INTEGER NOT NULL,
  call_index INTEGER NOT NULL,
  wire_id TEXT NOT NULL,
  run_id INTEGER,
  created_at INTEGER NOT NULL,
  PRIMARY KEY(reply_id, call_index)
);
INSERT INTO tool_occurrences
SELECT m.id, b.key, json_extract(b.value, '$.ToolCall.id'), m.run_id, m.created_at
FROM core_messages m, json_each(m.payload, '$.blocks') b
WHERE m.kind = 'Reply' AND json_type(b.value, '$.ToolCall') = 'object';

-- 多个候选触发主键冲突；没有候选触发 NOT NULL，不能猜归属。
CREATE TEMP TABLE result_occurrences (
  message_id INTEGER PRIMARY KEY,
  reply_id INTEGER NOT NULL,
  call_index INTEGER NOT NULL
);
INSERT INTO result_occurrences
SELECT r.id, c.reply_id, c.call_index FROM core_messages r
LEFT JOIN tool_occurrences c ON c.wire_id = json_extract(r.payload, '$.tool_call_id')
  AND c.reply_id = (SELECT max(p.reply_id) FROM tool_occurrences p
                    WHERE p.run_id IS r.run_id AND p.reply_id < r.id
                      AND p.wire_id = json_extract(r.payload, '$.tool_call_id'))
WHERE r.kind = 'ToolResult';

CREATE TEMP TABLE job_occurrences (
  job_id INTEGER PRIMARY KEY,
  reply_id INTEGER NOT NULL,
  call_index INTEGER NOT NULL
);
INSERT INTO job_occurrences
SELECT j.id, r.reply_id, r.call_index FROM core_jobs j
JOIN core_messages m ON m.kind = 'ToolResult'
  AND json_extract(m.payload, '$.outcome.Dispatched.exec_id') = CAST(j.id AS TEXT)
JOIN result_occurrences r ON r.message_id = m.id;
INSERT INTO job_occurrences
SELECT j.id, c.reply_id, c.call_index FROM core_jobs j
JOIN tool_occurrences c ON c.wire_id = j.origin_tool_call_id
  AND c.reply_id = (SELECT max(p.reply_id) FROM tool_occurrences p
                    WHERE p.run_id = j.origin_run_id AND p.wire_id = j.origin_tool_call_id
                      AND p.created_at <= j.created_at)
WHERE NOT EXISTS (SELECT 1 FROM job_occurrences o WHERE o.job_id = j.id);

CREATE TABLE core_jobs_v23 (
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
INSERT INTO core_jobs_v23
SELECT j.id, j.kind, j.owner_session_id, j.person_id, j.origin_run_id,
       j.tool_name, j.description, j.created_at, j.timeout_ms, j.state, j.finished_at,
       j.child_session_id, o.reply_id, o.call_index
FROM core_jobs j LEFT JOIN job_occurrences o ON o.job_id = j.id;
DROP TABLE core_jobs;
ALTER TABLE core_jobs_v23 RENAME TO core_jobs;
CREATE INDEX idx_jobs_owner ON core_jobs(owner_session_id);
CREATE INDEX idx_jobs_running ON core_jobs(id) WHERE state = 'running';
UPDATE core_messages SET payload = json_set(payload, '$.call',
  json_object('reply', (SELECT reply_id FROM result_occurrences WHERE message_id = core_messages.id),
              'index', (SELECT call_index FROM result_occurrences WHERE message_id = core_messages.id)))
WHERE id IN (SELECT message_id FROM result_occurrences);
DROP TABLE job_occurrences;
DROP TABLE result_occurrences;
DROP TABLE tool_occurrences;
ALTER TABLE core_sessions DROP COLUMN parent_tool_call_id;
