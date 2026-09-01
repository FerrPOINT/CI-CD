-- 0017_runner_protocol_mvp: external runner protocol credential and lease fields.

ALTER TABLE runners ADD COLUMN IF NOT EXISTS credential_hash TEXT;
ALTER TABLE runners ADD COLUMN IF NOT EXISTS token_hint TEXT;
ALTER TABLE runners ADD COLUMN IF NOT EXISTS credential_expires_at TIMESTAMPTZ;
ALTER TABLE runners ADD COLUMN IF NOT EXISTS disabled_at TIMESTAMPTZ;
ALTER TABLE runners ADD COLUMN IF NOT EXISTS draining BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE runners ADD COLUMN IF NOT EXISTS capacity_total_slots INTEGER;
ALTER TABLE runners ADD COLUMN IF NOT EXISTS capacity_busy_slots INTEGER;
ALTER TABLE runners ADD COLUMN IF NOT EXISTS capabilities JSONB NOT NULL DEFAULT '{}'::jsonb;
ALTER TABLE runners ADD COLUMN IF NOT EXISTS heartbeat_payload JSONB NOT NULL DEFAULT '{}'::jsonb;

CREATE UNIQUE INDEX IF NOT EXISTS idx_runners_active_credential_hash
    ON runners(credential_hash)
    WHERE credential_hash IS NOT NULL AND disabled_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_runners_last_seen
    ON runners(last_seen_at DESC)
    WHERE disabled_at IS NULL;

ALTER TABLE job_leases ADD COLUMN IF NOT EXISTS lease_token_hash TEXT;
ALTER TABLE job_leases ADD COLUMN IF NOT EXISTS ack_deadline TIMESTAMPTZ;
ALTER TABLE job_leases ADD COLUMN IF NOT EXISTS acknowledged_at TIMESTAMPTZ;
ALTER TABLE job_leases ADD COLUMN IF NOT EXISTS cancel_requested_at TIMESTAMPTZ;
ALTER TABLE job_leases ADD COLUMN IF NOT EXISTS runner_protocol_version INTEGER;

CREATE UNIQUE INDEX IF NOT EXISTS idx_job_leases_active_token_hash
    ON job_leases(lease_token_hash)
    WHERE lease_status = 'active' AND lease_token_hash IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_job_leases_ack_deadline
    ON job_leases(ack_deadline)
    WHERE lease_status = 'active' AND acknowledged_at IS NULL;
