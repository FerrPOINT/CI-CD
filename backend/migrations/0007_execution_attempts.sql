-- 0007_execution_attempts: preserve retry evidence instead of rewriting job logs.

CREATE TABLE IF NOT EXISTS execution_attempts (
    id UUID PRIMARY KEY,
    job_id UUID NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    attempt_no INTEGER NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('queued','running','success','failed','canceled')),
    trigger TEXT NOT NULL DEFAULT 'initial',
    exit_code INTEGER,
    error_tail TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ,
    UNIQUE(job_id, attempt_no)
);

INSERT INTO execution_attempts (id, job_id, attempt_no, status, trigger, started_at, finished_at)
SELECT gen_random_uuid(), j.id, 1, j.status, 'initial', j.started_at, j.finished_at
FROM jobs j
WHERE NOT EXISTS (
    SELECT 1 FROM execution_attempts a WHERE a.job_id = j.id
);

ALTER TABLE job_logs ADD COLUMN IF NOT EXISTS attempt_id UUID;

UPDATE job_logs l
SET attempt_id = a.id
FROM execution_attempts a
WHERE a.job_id = l.job_id
  AND a.attempt_no = 1
  AND l.attempt_id IS NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'job_logs_attempt_id_fkey'
    ) THEN
        ALTER TABLE job_logs
            ADD CONSTRAINT job_logs_attempt_id_fkey
            FOREIGN KEY (attempt_id) REFERENCES execution_attempts(id) ON DELETE CASCADE;
    END IF;
END $$;

ALTER TABLE job_logs ALTER COLUMN attempt_id SET NOT NULL;
ALTER TABLE job_logs DROP CONSTRAINT IF EXISTS job_logs_job_id_sequence_key;

CREATE UNIQUE INDEX IF NOT EXISTS idx_job_logs_attempt_sequence
    ON job_logs(attempt_id, sequence);
CREATE INDEX IF NOT EXISTS idx_job_logs_job_id
    ON job_logs(job_id);
CREATE INDEX IF NOT EXISTS idx_stages_pipeline_id
    ON stages(pipeline_id);
CREATE INDEX IF NOT EXISTS idx_jobs_stage_id
    ON jobs(stage_id);
CREATE INDEX IF NOT EXISTS idx_execution_attempts_job
    ON execution_attempts(job_id, attempt_no DESC);
CREATE UNIQUE INDEX IF NOT EXISTS idx_execution_attempts_active_job
    ON execution_attempts(job_id)
    WHERE status IN ('queued','running');

ALTER TABLE artifacts ADD COLUMN IF NOT EXISTS attempt_id UUID;

UPDATE artifacts ar
SET attempt_id = a.id
FROM execution_attempts a
WHERE a.job_id = ar.job_id
  AND a.attempt_no = 1
  AND ar.attempt_id IS NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'artifacts_attempt_id_fkey'
    ) THEN
        ALTER TABLE artifacts
            ADD CONSTRAINT artifacts_attempt_id_fkey
            FOREIGN KEY (attempt_id) REFERENCES execution_attempts(id) ON DELETE SET NULL;
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS idx_artifacts_attempt
    ON artifacts(attempt_id);
