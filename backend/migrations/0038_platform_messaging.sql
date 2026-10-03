-- Optional default outbox for new products. Existing outboxes use OutboxStore adapters.
ALTER TABLE pipelines ADD COLUMN terminal_event_seq BIGINT NOT NULL DEFAULT 0 CHECK (terminal_event_seq >= 0);
CREATE TABLE messaging_outbox (
    id UUID PRIMARY KEY,
    source TEXT NOT NULL,
    event_id UUID NOT NULL,
    subject TEXT NOT NULL,
    payload BYTEA NOT NULL CHECK (octet_length(payload) <= 65536),
    fingerprint BYTEA NOT NULL CHECK (octet_length(fingerprint) = 32),
    infrastructure_attempts BIGINT NOT NULL DEFAULT 0 CHECK (infrastructure_attempts >= 0),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    lease_token UUID,
    lease_until TIMESTAMPTZ,
    published_at TIMESTAMPTZ,
    stream TEXT,
    stream_sequence BIGINT,
    failed_at TIMESTAMPTZ,
    last_error_code TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (source, event_id)
);
CREATE INDEX messaging_outbox_due ON messaging_outbox (subject, next_attempt_at, created_at)
    WHERE published_at IS NULL AND failed_at IS NULL;
