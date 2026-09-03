-- 0026_session_family_reuse: refresh token family tracking and reuse revocation.
--
-- A refresh token that has already been rotated must never be accepted again.
-- Reuse marks the whole family revoked and bumps user token_version so already
-- issued access JWTs fail server-side validation.

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS token_version BIGINT NOT NULL DEFAULT 0;

ALTER TABLE sessions
    ADD COLUMN IF NOT EXISTS family_id UUID;

ALTER TABLE sessions
    ADD COLUMN IF NOT EXISTS replaced_by UUID REFERENCES sessions(id) ON DELETE SET NULL;

ALTER TABLE sessions
    ADD COLUMN IF NOT EXISTS reuse_detected_at TIMESTAMPTZ;

UPDATE sessions
SET family_id = id
WHERE family_id IS NULL;

ALTER TABLE sessions
    ALTER COLUMN family_id SET NOT NULL;

CREATE INDEX IF NOT EXISTS idx_sessions_family ON sessions(family_id);
CREATE INDEX IF NOT EXISTS idx_sessions_replaced_by ON sessions(replaced_by);
