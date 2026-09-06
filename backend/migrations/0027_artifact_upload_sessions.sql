-- K4.1: resumable artifact upload sessions for runner protocol v2.
-- A session pins (lease, attempt, artifact path/name/size); chunks arrive
-- sequentially, deduplicated by (session, chunk_index), so a runner that
-- reconnects after a network failure resumes from the persisted high-water
-- mark instead of re-uploading the whole artifact body.

CREATE TABLE artifact_upload_sessions (
    id              UUID PRIMARY KEY,
    lease_id        UUID NOT NULL REFERENCES job_leases(id) ON DELETE CASCADE,
    attempt_id      UUID NOT NULL REFERENCES execution_attempts(id) ON DELETE CASCADE,
    job_id          UUID NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    runner_id       UUID NOT NULL REFERENCES runners(id) ON DELETE CASCADE,
    artifact_path   TEXT NOT NULL,
    artifact_name   TEXT NOT NULL,
    content_type    TEXT NOT NULL DEFAULT 'application/octet-stream',
    declared_size   BIGINT NOT NULL,
    received_bytes  BIGINT NOT NULL DEFAULT 0,
    highest_chunk   INTEGER NOT NULL DEFAULT -1,
    status          TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'completed', 'aborted')),
    artifact_id     UUID REFERENCES artifacts(id) ON DELETE SET NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (attempt_id, artifact_path)
);

CREATE TABLE artifact_upload_chunks (
    session_id      UUID NOT NULL REFERENCES artifact_upload_sessions(id) ON DELETE CASCADE,
    chunk_index     INTEGER NOT NULL CHECK (chunk_index >= 0),
    byte_offset     BIGINT NOT NULL,
    byte_length     BIGINT NOT NULL,
    sha256          TEXT NOT NULL,
    received_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (session_id, chunk_index)
);

CREATE INDEX idx_artifact_upload_sessions_lease
    ON artifact_upload_sessions(lease_id)
    WHERE status = 'open';

-- Resume diagnostics: how far a session got before its last disconnect.
CREATE INDEX idx_artifact_upload_chunks_session_offset
    ON artifact_upload_chunks(session_id, byte_offset);

-- K4.1: chunk payloads live in a side table so metadata queries (resume
-- high-water marks) stay narrow; the pair (session_id, chunk_index) mirrors
-- artifact_upload_chunks.
CREATE TABLE artifact_chunk_blobs (
    session_id  UUID NOT NULL,
    chunk_index INTEGER NOT NULL,
    data        BYTEA NOT NULL,
    PRIMARY KEY (session_id, chunk_index)
);
