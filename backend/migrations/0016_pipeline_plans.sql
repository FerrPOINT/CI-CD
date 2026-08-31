-- 0016_pipeline_plans: immutable execution plan snapshot for each pipeline.

CREATE TABLE IF NOT EXISTS pipeline_plans (
    pipeline_id UUID PRIMARY KEY REFERENCES pipelines(id) ON DELETE CASCADE,
    config_source TEXT NOT NULL
        CHECK (config_source IN ('repository','legacy_template')),
    parser_version TEXT NOT NULL
        CHECK (char_length(parser_version) BETWEEN 1 AND 64),
    git_ref TEXT NOT NULL,
    resolved_commit_sha TEXT,
    config_sha256 TEXT NOT NULL
        CHECK (config_sha256 ~ '^[0-9a-f]{64}$'),
    plan_sha256 TEXT NOT NULL
        CHECK (plan_sha256 ~ '^[0-9a-f]{64}$'),
    raw_config TEXT NOT NULL,
    plan JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_pipeline_plans_plan_sha256
    ON pipeline_plans(plan_sha256);

CREATE OR REPLACE FUNCTION forbid_pipeline_plan_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'pipeline_plans rows are immutable';
END;
$$;

DROP TRIGGER IF EXISTS trg_pipeline_plans_no_update ON pipeline_plans;
CREATE TRIGGER trg_pipeline_plans_no_update
    BEFORE UPDATE ON pipeline_plans
    FOR EACH ROW
    EXECUTE FUNCTION forbid_pipeline_plan_update();
