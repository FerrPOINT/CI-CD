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
