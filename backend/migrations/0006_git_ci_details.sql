-- 0006_git_ci_details: git-server + CI feature parity (Gitea/Forgejo/Jenkins reference).
-- repository visibility (public/private) for clone ACL,
-- releases (tags with notes + artifact refs),
-- pipeline run variables, junit test report summaries.

ALTER TABLE repositories ADD COLUMN IF NOT EXISTS visibility TEXT NOT NULL DEFAULT 'private'
    CHECK (visibility IN ('private','public'));

CREATE TABLE IF NOT EXISTS releases (
    id UUID PRIMARY KEY,
    repository_name TEXT NOT NULL,
    tag_name TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    prerelease BOOLEAN NOT NULL DEFAULT FALSE,
    created_by TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (repository_name, tag_name)
);

ALTER TABLE pipelines ADD COLUMN IF NOT EXISTS variables JSONB NOT NULL DEFAULT '{}'::jsonb;

CREATE TABLE IF NOT EXISTS test_reports (
    id UUID PRIMARY KEY,
    job_id UUID NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    suite_name TEXT NOT NULL DEFAULT '',
    tests_total INTEGER NOT NULL DEFAULT 0,
    tests_passed INTEGER NOT NULL DEFAULT 0,
    tests_failed INTEGER NOT NULL DEFAULT 0,
    tests_skipped INTEGER NOT NULL DEFAULT 0,
    duration_ms INTEGER,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS test_reports_job_idx ON test_reports (job_id);
