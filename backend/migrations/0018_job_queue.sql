-- 0018_job_queue: durable dispatch queue for runner attempts.

CREATE TABLE IF NOT EXISTS job_queue (
    id UUID PRIMARY KEY,
    job_id UUID NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    attempt_id UUID NOT NULL REFERENCES execution_attempts(id) ON DELETE CASCADE,
    pipeline_id UUID NOT NULL REFERENCES pipelines(id) ON DELETE CASCADE,
    stage_id UUID NOT NULL REFERENCES stages(id) ON DELETE CASCADE,
    state TEXT NOT NULL DEFAULT 'queued'
        CHECK (state IN ('queued','leased','completed','canceled')),
    priority INTEGER NOT NULL DEFAULT 0,
    not_before TIMESTAMPTZ NOT NULL DEFAULT now(),
    required_tags TEXT[] NOT NULL DEFAULT '{}',
    queued_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    leased_at TIMESTAMPTZ,
    lease_id UUID REFERENCES job_leases(id) ON DELETE SET NULL,
    completed_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (
        (state = 'queued' AND leased_at IS NULL AND lease_id IS NULL AND completed_at IS NULL)
        OR
        (state = 'leased' AND leased_at IS NOT NULL AND completed_at IS NULL)
        OR
        (state IN ('completed','canceled') AND completed_at IS NOT NULL)
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_job_queue_attempt
    ON job_queue(attempt_id);

CREATE UNIQUE INDEX IF NOT EXISTS idx_job_queue_open_job
    ON job_queue(job_id)
    WHERE state IN ('queued','leased');

CREATE UNIQUE INDEX IF NOT EXISTS idx_job_queue_lease
    ON job_queue(lease_id)
    WHERE lease_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_job_queue_ready
    ON job_queue(priority DESC, not_before, queued_at, id)
    WHERE state = 'queued';

CREATE INDEX IF NOT EXISTS idx_job_queue_pipeline_state
    ON job_queue(pipeline_id, state);

CREATE INDEX IF NOT EXISTS idx_job_queue_stage_state
    ON job_queue(stage_id, state);

INSERT INTO job_queue (
    id,
    job_id,
    attempt_id,
    pipeline_id,
    stage_id,
    state,
    queued_at
)
SELECT
    gen_random_uuid(),
    j.id,
    a.id,
    s.pipeline_id,
    s.id,
    'queued',
    a.created_at
FROM jobs j
JOIN stages s ON s.id = j.stage_id
JOIN pipelines p ON p.id = s.pipeline_id
JOIN LATERAL (
    SELECT id, created_at
    FROM execution_attempts
    WHERE job_id = j.id AND status = 'queued'
    ORDER BY attempt_no DESC
    LIMIT 1
) a ON TRUE
WHERE j.status = 'queued'
  AND NOT j.manual
  AND p.status IN ('queued','running')
  AND NOT EXISTS (
      SELECT 1
      FROM job_leases l
      WHERE l.job_id = j.id AND l.lease_status = 'active'
  )
ON CONFLICT (attempt_id) DO NOTHING;

INSERT INTO job_queue (
    id,
    job_id,
    attempt_id,
    pipeline_id,
    stage_id,
    state,
    queued_at,
    leased_at,
    lease_id
)
SELECT
    gen_random_uuid(),
    j.id,
    a.id,
    s.pipeline_id,
    s.id,
    'leased',
    a.created_at,
    l.acquired_at,
    l.id
FROM job_leases l
JOIN jobs j ON j.id = l.job_id
JOIN stages s ON s.id = j.stage_id
JOIN execution_attempts a ON a.id = l.attempt_id
WHERE l.lease_status = 'active'
  AND j.status = 'running'
  AND a.status = 'running'
ON CONFLICT (attempt_id) DO NOTHING;
