use sqlx::{PgPool, Row};

pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::raw_sql(
        r#"
        CREATE TABLE IF NOT EXISTS projects (
            id UUID PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            repository_url TEXT NOT NULL,
            default_branch TEXT NOT NULL DEFAULT 'main',
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        );
        CREATE TABLE IF NOT EXISTS pipelines (
            id UUID PRIMARY KEY,
            project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            git_ref TEXT NOT NULL,
            status TEXT NOT NULL CHECK (status IN ('queued','running','success','failed','canceled')),
            created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            started_at TIMESTAMPTZ,
            finished_at TIMESTAMPTZ
        );
        CREATE TABLE IF NOT EXISTS stages (
            id UUID PRIMARY KEY,
            pipeline_id UUID NOT NULL REFERENCES pipelines(id) ON DELETE CASCADE,
            name TEXT NOT NULL,
            position INTEGER NOT NULL,
            status TEXT NOT NULL CHECK (status IN ('queued','running','success','failed','canceled')),
            UNIQUE(pipeline_id, position)
        );
        CREATE TABLE IF NOT EXISTS jobs (
            id UUID PRIMARY KEY,
            stage_id UUID NOT NULL REFERENCES stages(id) ON DELETE CASCADE,
            name TEXT NOT NULL,
            image TEXT NOT NULL,
            command TEXT NOT NULL,
            position INTEGER NOT NULL,
            status TEXT NOT NULL CHECK (status IN ('queued','running','success','failed','canceled')),
            started_at TIMESTAMPTZ,
            finished_at TIMESTAMPTZ,
            UNIQUE(stage_id, position)
        );
        CREATE TABLE IF NOT EXISTS repositories (
            id UUID PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        );
        CREATE TABLE IF NOT EXISTS pull_requests (
            id UUID PRIMARY KEY,
            repository_name TEXT NOT NULL,
            number INTEGER NOT NULL,
            title TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            source_branch TEXT NOT NULL,
            target_branch TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open','merged','closed')),
            created_by TEXT NOT NULL DEFAULT '',
            created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            merged_at TIMESTAMPTZ,
            merge_commit_sha TEXT,
            UNIQUE(repository_name, number)
        );
        CREATE TABLE IF NOT EXISTS job_logs (
            id BIGSERIAL PRIMARY KEY,
            job_id UUID NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
            sequence INTEGER NOT NULL,
            message TEXT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            UNIQUE(job_id, sequence)
        );
        CREATE TABLE IF NOT EXISTS runners (
            id UUID PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            tags TEXT[] NOT NULL DEFAULT '{}',
            status TEXT NOT NULL DEFAULT 'offline' CHECK (status IN ('online','offline','paused')),
            last_seen_at TIMESTAMPTZ,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        );
        CREATE TABLE IF NOT EXISTS project_secrets (
            id UUID PRIMARY KEY,
            project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            key TEXT NOT NULL,
            encrypted_value TEXT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            UNIQUE(project_id, key)
        );
        CREATE TABLE IF NOT EXISTS artifacts (
            id UUID PRIMARY KEY,
            job_id UUID NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
            name TEXT NOT NULL,
            storage_path TEXT NOT NULL,
            content_type TEXT NOT NULL DEFAULT 'application/octet-stream',
            size_bytes BIGINT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        );
        CREATE TABLE IF NOT EXISTS environments (
            id UUID PRIMARY KEY,
            project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            name TEXT NOT NULL,
            url TEXT,
            status TEXT NOT NULL DEFAULT 'available' CHECK (status IN ('available','stopped','degraded')),
            created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            UNIQUE(project_id, name)
        );
        CREATE TABLE IF NOT EXISTS deployments (
            id UUID PRIMARY KEY,
            environment_id UUID NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
            pipeline_id UUID REFERENCES pipelines(id) ON DELETE SET NULL,
            git_ref TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','running','success','failed')),
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        );
        CREATE TABLE IF NOT EXISTS schedules (
            id UUID PRIMARY KEY,
            project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            cron TEXT NOT NULL,
            git_ref TEXT NOT NULL,
            enabled BOOLEAN NOT NULL DEFAULT TRUE,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        );
        CREATE TABLE IF NOT EXISTS webhooks (
            id UUID PRIMARY KEY,
            project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            url TEXT NOT NULL,
            events TEXT[] NOT NULL DEFAULT '{}',
            enabled BOOLEAN NOT NULL DEFAULT TRUE,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        );
        CREATE TABLE IF NOT EXISTS notification_configs (
            id UUID PRIMARY KEY,
            project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            channel TEXT NOT NULL,
            target TEXT NOT NULL,
            enabled BOOLEAN NOT NULL DEFAULT TRUE,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        );
        CREATE TABLE IF NOT EXISTS audit_log (
            id BIGSERIAL PRIMARY KEY,
            action TEXT NOT NULL,
            resource_type TEXT NOT NULL,
            resource_id UUID,
            actor TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        );
        CREATE TABLE IF NOT EXISTS users (
            id UUID PRIMARY KEY,
            username TEXT NOT NULL UNIQUE,
            role TEXT NOT NULL CHECK (role IN ('admin','maintainer','developer','viewer')),
            enabled BOOLEAN NOT NULL DEFAULT TRUE,
            created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now()
        );
        CREATE TABLE IF NOT EXISTS api_tokens (
            id UUID PRIMARY KEY,
            name TEXT NOT NULL,
            token_hash TEXT NOT NULL UNIQUE,
            token_hint TEXT NOT NULL,
            user_id UUID REFERENCES users(id) ON DELETE SET NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            last_used_at TIMESTAMPTZ
        );
        CREATE INDEX IF NOT EXISTS idx_runners_status ON runners(status);
        CREATE INDEX IF NOT EXISTS idx_project_secrets_project ON project_secrets(project_id);
        CREATE INDEX IF NOT EXISTS idx_artifacts_job ON artifacts(job_id);
        CREATE INDEX IF NOT EXISTS idx_deployments_environment ON deployments(environment_id);
        CREATE INDEX IF NOT EXISTS idx_schedules_project ON schedules(project_id);
        CREATE INDEX IF NOT EXISTS idx_webhooks_project ON webhooks(project_id);
        CREATE INDEX IF NOT EXISTS idx_audit_log_created ON audit_log(created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_pipelines_project_id ON pipelines(project_id);
        "#,
    ).execute(pool).await?;
    Ok(())
}

pub async fn next_log_sequence(pool: &PgPool, job_id: uuid::Uuid) -> Result<i32, sqlx::Error> {
    let row = sqlx::query(
        "SELECT COALESCE(MAX(sequence), 0) + 1 AS next_sequence FROM job_logs WHERE job_id = $1",
    )
    .bind(job_id)
    .fetch_one(pool)
    .await?;
    Ok(row.get("next_sequence"))
}
