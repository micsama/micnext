ALTER TABLE core_pushes ADD COLUMN files TEXT NOT NULL DEFAULT '[]';
UPDATE core_messages SET payload = json_set(payload, '$.files', json('[]')) WHERE kind = 'Notification';
DROP INDEX idx_messages_undelivered;
CREATE INDEX idx_messages_undelivered ON core_messages(session_id)
  WHERE delivered_at IS NULL AND delivery_failure IS NULL AND kind IN ('Reply', 'Notification', 'Attachment');
