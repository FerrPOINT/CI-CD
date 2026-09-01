-- 0023_artifact_retention: add default TTL and purge marker for local artifacts.

ALTER TABLE artifacts
  ADD COLUMN IF NOT EXISTS expires_at TIMESTAMPTZ;

UPDATE artifacts
   SET expires_at = created_at + interval '30 days'
 WHERE expires_at IS NULL;

ALTER TABLE artifacts
  ALTER COLUMN expires_at SET NOT NULL,
  ALTER COLUMN expires_at SET DEFAULT (now() + interval '30 days');

ALTER TABLE artifacts
  ADD COLUMN IF NOT EXISTS purged_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_artifacts_expired_unpurged
  ON artifacts(expires_at, id)
  WHERE purged_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_artifacts_job_created
  ON artifacts(job_id, created_at DESC, id DESC);
