ALTER TABLE core_sessions ADD COLUMN pinned_at INTEGER;
              ALTER TABLE core_sessions ADD COLUMN seen_message_id INTEGER;
              UPDATE core_sessions SET seen_message_id =
                (SELECT MAX(id) FROM core_messages m WHERE m.session_id = core_sessions.id);
