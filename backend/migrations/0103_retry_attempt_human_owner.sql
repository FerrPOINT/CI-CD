-- Retrying is a new human operation; preserve the original pipeline history.
CREATE TABLE execution_attempt_human_owners (
    attempt_id uuid PRIMARY KEY REFERENCES execution_attempts(id) ON DELETE CASCADE,
    owner_subject uuid NOT NULL CHECK(owner_subject <> '00000000-0000-0000-0000-000000000000'),
    requires_admin boolean NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
