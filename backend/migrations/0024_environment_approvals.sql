-- Protected environment approvals and traceable rollback delivery.

ALTER TABLE environments
    ADD COLUMN IF NOT EXISTS protected BOOLEAN NOT NULL DEFAULT FALSE;

ALTER TABLE environments
    ADD COLUMN IF NOT EXISTS required_approvals INTEGER NOT NULL DEFAULT 0;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'environments_required_approvals_check'
    ) THEN
        ALTER TABLE environments
            ADD CONSTRAINT environments_required_approvals_check
            CHECK (
                required_approvals BETWEEN 0 AND 10
                AND (
                    (protected = FALSE AND required_approvals = 0)
                    OR (protected = TRUE AND required_approvals >= 1)
                )
            );
    END IF;
END $$;

ALTER TABLE deployments
    ADD COLUMN IF NOT EXISTS rollback_of_id UUID;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'deployments_rollback_of_id_fkey'
    ) THEN
        ALTER TABLE deployments
            ADD CONSTRAINT deployments_rollback_of_id_fkey
            FOREIGN KEY (rollback_of_id) REFERENCES deployments(id) ON DELETE SET NULL;
    END IF;
END $$;

CREATE TABLE IF NOT EXISTS deployment_approvals (
    id UUID PRIMARY KEY,
    deployment_id UUID NOT NULL REFERENCES deployments(id) ON DELETE CASCADE,
    decision TEXT NOT NULL CHECK (decision IN ('approved','rejected')),
    actor TEXT NOT NULL,
    comment TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (deployment_id, actor)
);

CREATE INDEX IF NOT EXISTS idx_deployment_approvals_deployment
    ON deployment_approvals(deployment_id, created_at, id);

CREATE INDEX IF NOT EXISTS idx_deployments_rollback_of
    ON deployments(rollback_of_id);
