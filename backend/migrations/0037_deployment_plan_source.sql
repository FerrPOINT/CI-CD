ALTER TABLE pipeline_plans DROP CONSTRAINT pipeline_plans_config_source_check;
ALTER TABLE pipeline_plans ADD CONSTRAINT pipeline_plans_config_source_check
    CHECK (config_source IN ('repository', 'legacy_template', 'deployment'));
