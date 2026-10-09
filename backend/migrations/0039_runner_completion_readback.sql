-- Only an authenticated accepted runner completion sets this discriminator.
-- Do not backfill historical terminal/expiry/cancellation records as receipts.
ALTER TABLE job_leases ADD COLUMN completion_received_at TIMESTAMPTZ;
ALTER TABLE job_leases ADD CONSTRAINT job_leases_completion_received_terminal
    CHECK (completion_received_at IS NULL OR (
        lease_status IN ('completed', 'canceled')
        AND completed_at IS NOT NULL
        AND acknowledged_at IS NOT NULL
        AND terminal_status IS NOT NULL
        AND terminal_status IN ('success', 'failed', 'canceled')
    ));
