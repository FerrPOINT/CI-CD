-- 0004_outbox_delivery: transactional outbox + scheduled triggers
-- (ADR-0006, EVENT_CONTRACT; canonical names per ADR-0009).

CREATE TABLE IF NOT EXISTS domain_events (
    id UUID PRIMARY KEY,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    event_type TEXT NOT NULL,
    aggregate_type TEXT NOT NULL,
    aggregate_id UUID NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    correlation_id UUID,
    causation_id UUID
);
CREATE INDEX IF NOT EXISTS idx_domain_events_aggregate ON domain_events(aggregate_type, aggregate_id, occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_domain_events_type ON domain_events(event_type, occurred_at DESC);

CREATE TABLE IF NOT EXISTS outbox_messages (
    id UUID PRIMARY KEY,
    event_id UUID NOT NULL REFERENCES domain_events(id) ON DELETE CASCADE,
    subscription_id TEXT NOT NULL,
    channel TEXT NOT NULL CHECK (channel IN ('webhook','notification','sse')),
    destination TEXT NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    delivered_at TIMESTAMPTZ,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- Delivery worker polls this partial index (undelivered, due).
CREATE INDEX IF NOT EXISTS idx_outbox_pending ON outbox_messages(next_attempt_at)
    WHERE delivered_at IS NULL;

-- Scheduler lease bookkeeping: a fire claim prevents double-triggering.
ALTER TABLE schedules ADD COLUMN IF NOT EXISTS last_fired_at TIMESTAMPTZ;
ALTER TABLE schedules ADD COLUMN IF NOT EXISTS last_fire_error TEXT;
