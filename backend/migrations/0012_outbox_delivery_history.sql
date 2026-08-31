-- 0012_outbox_delivery_history: observable outbox delivery attempts and replay.

ALTER TABLE outbox_messages ADD COLUMN IF NOT EXISTS project_id UUID;
ALTER TABLE outbox_messages ADD COLUMN IF NOT EXISTS generation INTEGER NOT NULL DEFAULT 0;
ALTER TABLE outbox_messages ADD COLUMN IF NOT EXISTS replay_of_id UUID REFERENCES outbox_messages(id) ON DELETE SET NULL;
ALTER TABLE outbox_messages ADD COLUMN IF NOT EXISTS failed_at TIMESTAMPTZ;

UPDATE outbox_messages
SET project_id = (payload->>'project_id')::uuid
WHERE project_id IS NULL
  AND payload->>'project_id' ~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$';

UPDATE outbox_messages m
SET project_id = (e.payload->>'project_id')::uuid
FROM domain_events e
WHERE m.event_id = e.id
  AND m.project_id IS NULL
  AND e.payload->>'project_id' ~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$';

UPDATE outbox_messages m
SET project_id = p.project_id
FROM domain_events e
JOIN pipelines p ON p.id = e.aggregate_id
WHERE m.event_id = e.id
  AND m.project_id IS NULL
  AND e.aggregate_type = 'pipeline';

UPDATE outbox_messages
SET failed_at = COALESCE(next_attempt_at, created_at)
WHERE delivered_at IS NULL
  AND failed_at IS NULL
  AND attempts >= 8
  AND last_error IS NOT NULL;

CREATE TABLE IF NOT EXISTS outbox_delivery_attempts (
    id BIGSERIAL PRIMARY KEY,
    message_id UUID NOT NULL REFERENCES outbox_messages(id) ON DELETE CASCADE,
    attempt_number INTEGER NOT NULL,
    started_at TIMESTAMPTZ NOT NULL,
    finished_at TIMESTAMPTZ NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN ('delivered', 'retry_scheduled', 'failed')),
    http_status INTEGER,
    error_message TEXT,
    duration_ms INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(message_id, attempt_number)
);

CREATE INDEX IF NOT EXISTS idx_outbox_project_created
    ON outbox_messages(project_id, created_at DESC, id DESC)
    WHERE project_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_outbox_project_dead
    ON outbox_messages(project_id, created_at DESC, id DESC)
    WHERE project_id IS NOT NULL AND delivered_at IS NULL AND failed_at IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_outbox_delivery_attempts_message
    ON outbox_delivery_attempts(message_id, attempt_number DESC);
