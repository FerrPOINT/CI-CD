-- 0015_job_leases: durable owner for embedded runner execution attempts.

CREATE TABLE IF NOT EXISTS job_leases (
    id UUID PRIMARY KEY,
    job_id UUID NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    attempt_id UUID NOT NULL REFERENCES execution_attempts(id) ON DELETE CASCADE,
    runner_id UUID REFERENCES runners(id) ON DELETE SET NULL,
    runner_name TEXT NOT NULL,
    lease_status TEXT NOT NULL DEFAULT 'active'
        CHECK (lease_status IN ('active','completed','expired','canceled')),
    generation BIGINT NOT NULL,
    lease_expires_at TIMESTAMPTZ NOT NULL,
    acquired_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_renewed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ,
    terminal_status TEXT
        CHECK (terminal_status IS NULL OR terminal_status IN ('success','failed','canceled')),
    error_tail TEXT,
    UNIQUE(job_id, generation),
    CHECK (
        (lease_status = 'active' AND completed_at IS NULL AND terminal_status IS NULL)
        OR
        (lease_status <> 'active' AND completed_at IS NOT NULL)
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_job_leases_active_job
    ON job_leases(job_id)
    WHERE lease_status = 'active';

CREATE INDEX IF NOT EXISTS idx_job_leases_attempt
    ON job_leases(attempt_id);

CREATE INDEX IF NOT EXISTS idx_job_leases_active_expiry
    ON job_leases(lease_expires_at)
    WHERE lease_status = 'active';

CREATE INDEX IF NOT EXISTS idx_job_leases_runner_active
    ON job_leases(runner_id, lease_status)
    WHERE runner_id IS NOT NULL;
