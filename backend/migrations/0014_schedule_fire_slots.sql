ALTER TABLE schedules ADD COLUMN IF NOT EXISTS next_fire_at TIMESTAMPTZ;

CREATE TABLE IF NOT EXISTS schedule_fires (
    id UUID PRIMARY KEY,
    schedule_id UUID NOT NULL REFERENCES schedules(id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    scheduled_for TIMESTAMPTZ NOT NULL,
    pipeline_id UUID REFERENCES pipelines(id) ON DELETE SET NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','triggered','failed')),
    error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(schedule_id, scheduled_for)
);

CREATE INDEX IF NOT EXISTS idx_schedules_due
    ON schedules(next_fire_at)
    WHERE enabled AND last_fire_error IS NULL;

CREATE INDEX IF NOT EXISTS idx_schedule_fires_schedule_created
    ON schedule_fires(schedule_id, scheduled_for DESC);

CREATE INDEX IF NOT EXISTS idx_schedule_fires_pending
    ON schedule_fires(scheduled_for)
    WHERE status = 'pending';
