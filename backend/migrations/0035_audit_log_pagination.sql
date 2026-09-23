CREATE INDEX IF NOT EXISTS idx_audit_log_created_id
    ON audit_log (created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_audit_log_action_created_id
    ON audit_log (action, created_at DESC, id DESC);
