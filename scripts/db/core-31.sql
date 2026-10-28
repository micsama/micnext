CREATE INDEX idx_messages_call ON core_messages(model_call_id, kind) WHERE model_call_id IS NOT NULL;
CREATE INDEX idx_messages_boundary ON core_messages(session_id, id) WHERE kind = 'Boundary';
CREATE INDEX idx_messages_time ON core_messages(session_id, created_at, id);
CREATE INDEX idx_messages_tool_time ON core_messages(created_at) WHERE kind = 'ToolResult';
CREATE INDEX idx_messages_tool_failed ON core_messages(session_id) WHERE kind = 'ToolResult' AND (json_extract(payload, '$.outcome.Terminal.Failed') IS NOT NULL OR json_extract(payload, '$.outcome.Terminal.TimedOut') IS NOT NULL);
CREATE INDEX idx_model_calls_started ON core_model_calls(started_at);
CREATE INDEX idx_runs_created ON core_runs(created_at);
