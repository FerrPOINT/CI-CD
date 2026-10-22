CREATE TABLE pull_request_merge_attempts (
    id UUID PRIMARY KEY,
    pull_request_id UUID NOT NULL REFERENCES pull_requests(id) ON DELETE RESTRICT,
    repository_id UUID NOT NULL REFERENCES repository_catalog(id) ON DELETE RESTRICT,
    source_ref TEXT NOT NULL CHECK (source_ref LIKE 'refs/heads/%'),
    source_oid TEXT NOT NULL CHECK (source_oid ~ '^([0-9a-f]{40}|[0-9a-f]{64})$'),
    target_ref TEXT NOT NULL CHECK (target_ref LIKE 'refs/heads/%' AND target_ref <> source_ref),
    target_oid TEXT NOT NULL CHECK (target_oid ~ '^([0-9a-f]{40}|[0-9a-f]{64})$'),
    merge_oid TEXT NOT NULL CHECK (merge_oid ~ '^([0-9a-f]{40}|[0-9a-f]{64})$'),
    state TEXT NOT NULL DEFAULT 'prepared' CHECK (state IN ('prepared', 'confirmed', 'rejected')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ,
    CHECK ((state = 'prepared') = (completed_at IS NULL))
);
CREATE UNIQUE INDEX pull_request_merge_one_active
    ON pull_request_merge_attempts(pull_request_id) WHERE state <> 'rejected';
