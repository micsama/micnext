ALTER TABLE core_settings DROP COLUMN redact_rules;
              ALTER TABLE core_sessions ADD COLUMN plain_after INTEGER;
