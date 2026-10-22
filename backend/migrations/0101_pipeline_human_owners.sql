-- Existing task machine journals remain independent. Historical pipelines are
-- deliberately unresolved until owner provenance is confirmed by migration.
CREATE TABLE pipeline_human_owners (
    pipeline_id uuid PRIMARY KEY REFERENCES pipelines(id) ON DELETE CASCADE,
    owner_subject uuid NOT NULL CHECK(owner_subject <> '00000000-0000-0000-0000-000000000000'),
    requires_admin boolean NOT NULL DEFAULT false,
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE INDEX pipeline_human_owners_subject ON pipeline_human_owners(owner_subject);
