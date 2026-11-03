-- Cancellation belongs to the existing deployment operation and survives lost HTTP replies.
-- A terminal pipeline alone does not confirm physical execution termination.
ALTER TABLE deployments ADD COLUMN cancel_requested_at TIMESTAMPTZ;

-- The environment/request_key identity already exists in 0036. Bind new
-- requests to their verified actor, operation and input; unknown legacy keys
-- remain unproven and are rejected rather than silently reused.
ALTER TABLE deployments ADD COLUMN request_fingerprint TEXT
    CHECK(request_fingerprint ~ '^[0-9a-f]{64}$');
