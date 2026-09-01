ALTER TABLE jobs
  ADD COLUMN IF NOT EXISTS required_secrets TEXT[] NOT NULL DEFAULT '{}';

CREATE INDEX IF NOT EXISTS idx_jobs_required_secrets_gin
  ON jobs USING GIN (required_secrets);
