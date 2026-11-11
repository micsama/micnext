ALTER TABLE core_pushes ADD COLUMN title TEXT NOT NULL DEFAULT '';
UPDATE core_pushes SET title = coalesce(
  (SELECT nullif(s.title, '') FROM core_sessions s WHERE s.id = core_pushes.source_session_id), '通知');
