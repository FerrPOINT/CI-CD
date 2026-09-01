-- 0021_runner_artifact_paths: persist declared job artifact paths for runner upload.

ALTER TABLE jobs
  ADD COLUMN IF NOT EXISTS artifact_paths TEXT[] NOT NULL DEFAULT '{}';
