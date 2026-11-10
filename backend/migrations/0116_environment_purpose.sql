ALTER TABLE environments ADD COLUMN purpose text NOT NULL DEFAULT 'stand'
    CHECK (purpose IN ('stand', 'workspace_target'));

UPDATE environments e SET purpose = 'workspace_target'
WHERE EXISTS (SELECT 1 FROM development_workspaces w WHERE w.environment_id = e.id);

ALTER TABLE development_workspaces ADD COLUMN retired_at timestamptz;
ALTER TABLE deployments ADD COLUMN source_pipeline_id uuid;
ALTER TABLE deployments ADD COLUMN publication_target jsonb;
