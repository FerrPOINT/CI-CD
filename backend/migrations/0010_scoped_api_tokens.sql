-- 0010_scoped_api_tokens: project-bound PAT scopes and soft revoke.

ALTER TABLE api_tokens
    ADD COLUMN IF NOT EXISTS project_id UUID REFERENCES projects(id) ON DELETE CASCADE;

ALTER TABLE api_tokens
    ADD COLUMN IF NOT EXISTS scopes TEXT[] NOT NULL DEFAULT ARRAY['api:read','api:write','git:read','git:write'];

ALTER TABLE api_tokens
    ADD COLUMN IF NOT EXISTS revoked_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_api_tokens_active_owner_project
    ON api_tokens(user_id, project_id)
    WHERE revoked_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_api_tokens_active_project
    ON api_tokens(project_id)
    WHERE revoked_at IS NULL;
