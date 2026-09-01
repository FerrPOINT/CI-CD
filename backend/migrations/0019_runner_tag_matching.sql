-- 0019_runner_tag_matching: persist job placement tags and match them during runner claim.

ALTER TABLE jobs ADD COLUMN IF NOT EXISTS required_tags TEXT[] NOT NULL DEFAULT '{}';

CREATE INDEX IF NOT EXISTS idx_jobs_required_tags_gin
    ON jobs USING GIN(required_tags);

CREATE INDEX IF NOT EXISTS idx_job_queue_required_tags_gin
    ON job_queue USING GIN(required_tags);

UPDATE job_queue q
SET required_tags = j.required_tags,
    updated_at = now()
FROM jobs j
WHERE q.job_id = j.id
  AND q.required_tags IS DISTINCT FROM j.required_tags;
