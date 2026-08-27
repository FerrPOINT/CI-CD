-- 0005_execution_gaps: P0/P1 CI fundamentals vs GitLab/Jenkins reference.
-- job timeout, protected branches + merge gate data, PAT expiry,
-- DSL execution controls (allow_failure, manual), webhook signing secrets.

ALTER TABLE jobs ADD COLUMN IF NOT EXISTS timeout_seconds INTEGER;
ALTER TABLE jobs ADD COLUMN IF NOT EXISTS allow_failure BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE jobs ADD COLUMN IF NOT EXISTS manual BOOLEAN NOT NULL DEFAULT FALSE;

-- Protected branches: merge gate requires a success pipeline on the PR head.
ALTER TABLE projects ADD COLUMN IF NOT EXISTS protected_branches TEXT[] NOT NULL DEFAULT '{}';

-- PAT lifetime (NULL = no expiry, current behaviour preserved).
ALTER TABLE api_tokens ADD COLUMN IF NOT EXISTS expires_at TIMESTAMPTZ;

-- Resolved commit for a pipeline (merge gate + CI env).
ALTER TABLE pipelines ADD COLUMN IF NOT EXISTS commit_sha TEXT;

-- Webhook HMAC signing secret (NULL = unsigned delivery, current behaviour).
ALTER TABLE webhooks ADD COLUMN IF NOT EXISTS secret TEXT;
