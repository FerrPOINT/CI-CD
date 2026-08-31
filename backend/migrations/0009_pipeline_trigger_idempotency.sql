-- 0009_pipeline_trigger_idempotency: durable replay protection for pipeline triggers.
-- Manual API runs and internal git push events can now reuse the same key
-- without creating duplicate pipelines.

CREATE TABLE IF NOT EXISTS pipeline_triggers (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    source TEXT NOT NULL CHECK (char_length(source) BETWEEN 1 AND 64),
    idempotency_key TEXT NOT NULL CHECK (char_length(idempotency_key) BETWEEN 1 AND 512),
    request_fingerprint TEXT NOT NULL CHECK (request_fingerprint ~ '^[0-9a-f]{64}$'),
    pipeline_id UUID NOT NULL REFERENCES pipelines(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, source, idempotency_key)
);

CREATE INDEX IF NOT EXISTS idx_pipeline_triggers_pipeline
    ON pipeline_triggers(pipeline_id);

CREATE INDEX IF NOT EXISTS idx_pipeline_triggers_project_created
    ON pipeline_triggers(project_id, created_at DESC);
