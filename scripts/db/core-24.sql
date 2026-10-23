ALTER TABLE core_settings ADD COLUMN redact_rules INTEGER NOT NULL DEFAULT 1 CHECK(redact_rules IN (0, 1));
