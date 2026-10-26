ALTER TABLE core_sessions ADD COLUMN tags_manual INTEGER NOT NULL DEFAULT 0;
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
