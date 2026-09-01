use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

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
            sha256 TEXT CHECK (sha256 IS NULL OR sha256 ~ '^[0-9a-f]{64}$'),
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
            next_fire_at TIMESTAMPTZ,
            last_fired_at TIMESTAMPTZ,
            last_fire_error TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        );
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
            project_id UUID REFERENCES projects(id) ON DELETE CASCADE,
            scopes TEXT[] NOT NULL DEFAULT ARRAY['api:read','api:write','git:read','git:write'],
            created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            last_used_at TIMESTAMPTZ,
            expires_at TIMESTAMPTZ,
            revoked_at TIMESTAMPTZ
        );
        CREATE INDEX IF NOT EXISTS idx_runners_status ON runners(status);
        CREATE INDEX IF NOT EXISTS idx_project_secrets_project ON project_secrets(project_id);
        CREATE INDEX IF NOT EXISTS idx_artifacts_job ON artifacts(job_id);
        CREATE INDEX IF NOT EXISTS idx_deployments_environment ON deployments(environment_id);
        CREATE INDEX IF NOT EXISTS idx_schedules_project ON schedules(project_id);
        CREATE INDEX IF NOT EXISTS idx_schedules_due ON schedules(next_fire_at) WHERE enabled AND last_fire_error IS NULL;
        CREATE INDEX IF NOT EXISTS idx_schedule_fires_schedule_created ON schedule_fires(schedule_id, scheduled_for DESC);
        CREATE INDEX IF NOT EXISTS idx_schedule_fires_pending ON schedule_fires(scheduled_for) WHERE status = 'pending';
        CREATE INDEX IF NOT EXISTS idx_webhooks_project ON webhooks(project_id);
        CREATE INDEX IF NOT EXISTS idx_audit_log_created ON audit_log(created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_pipelines_project_id ON pipelines(project_id);
        CREATE INDEX IF NOT EXISTS idx_api_tokens_active_owner_project ON api_tokens(user_id, project_id) WHERE revoked_at IS NULL;
        CREATE INDEX IF NOT EXISTS idx_api_tokens_active_project ON api_tokens(project_id) WHERE revoked_at IS NULL;
        "#,
    ).execute(pool).await?;
    Ok(())
}

pub async fn active_or_latest_attempt_id(pool: &PgPool, job_id: Uuid) -> Result<Uuid, sqlx::Error> {
    let attempt_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM execution_attempts \
         WHERE job_id = $1 \
         ORDER BY CASE WHEN status IN ('queued','running') THEN 0 ELSE 1 END, attempt_no DESC \
         LIMIT 1",
    )
    .bind(job_id)
    .fetch_optional(pool)
    .await?;

    if let Some(attempt_id) = attempt_id {
        return Ok(attempt_id);
    }

    sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO execution_attempts (id, job_id, attempt_no, status, trigger) \
         SELECT $2, j.id, 1, j.status, 'compat' \
         FROM jobs j WHERE j.id = $1 \
         RETURNING id",
    )
    .bind(job_id)
    .bind(Uuid::new_v4())
    .fetch_one(pool)
    .await
}

pub async fn open_attempt_id(
    pool: &PgPool,
    job_id: Uuid,
    trigger: &str,
) -> Result<Uuid, sqlx::Error> {
    let attempt_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM execution_attempts \
         WHERE job_id = $1 AND status IN ('queued','running') \
         ORDER BY attempt_no DESC \
         LIMIT 1",
    )
    .bind(job_id)
    .fetch_optional(pool)
    .await?;

    if let Some(attempt_id) = attempt_id {
        return Ok(attempt_id);
    }

    sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO execution_attempts (id, job_id, attempt_no, status, trigger) \
         SELECT $2, j.id, COALESCE(MAX(a.attempt_no), 0) + 1, 'queued', $3 \
         FROM jobs j LEFT JOIN execution_attempts a ON a.job_id = j.id \
         WHERE j.id = $1 \
         GROUP BY j.id \
         RETURNING id",
    )
    .bind(job_id)
    .bind(Uuid::new_v4())
    .bind(trigger)
    .fetch_one(pool)
    .await
}

pub async fn enqueue_job_attempt_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    job_id: Uuid,
    attempt_id: Uuid,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO job_queue (id, job_id, attempt_id, pipeline_id, stage_id, state, required_tags) \
         SELECT $3, j.id, a.id, s.pipeline_id, s.id, 'queued', j.required_tags \
         FROM jobs j \
         JOIN execution_attempts a ON a.job_id = j.id \
         JOIN stages s ON s.id = j.stage_id \
         JOIN pipelines p ON p.id = s.pipeline_id \
         WHERE j.id = $1 \
           AND a.id = $2 \
           AND j.status = 'queued' \
           AND a.status = 'queued' \
           AND NOT j.manual \
           AND p.status IN ('queued','running') \
         ON CONFLICT (attempt_id) DO UPDATE \
         SET state = 'queued', \
             lease_id = NULL, \
             leased_at = NULL, \
             completed_at = NULL, \
             not_before = now(), \
             required_tags = EXCLUDED.required_tags, \
             updated_at = now() \
         WHERE job_queue.state IN ('queued','completed','canceled')",
    )
    .bind(job_id)
    .bind(attempt_id)
    .bind(Uuid::new_v4())
    .execute(&mut **tx)
    .await?;
    Ok(result.rows_affected())
}

pub async fn enqueue_job_attempt(
    pool: &PgPool,
    job_id: Uuid,
    attempt_id: Uuid,
) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let affected = enqueue_job_attempt_tx(&mut tx, job_id, attempt_id).await?;
    tx.commit().await?;
    if affected > 0 {
        crate::dispatch_signal::notify_runner_work_available();
    }
    Ok(affected)
}

pub async fn enqueue_current_job_attempt(pool: &PgPool, job_id: Uuid) -> Result<u64, sqlx::Error> {
    let attempt_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT id \
         FROM execution_attempts \
         WHERE job_id = $1 AND status = 'queued' \
         ORDER BY attempt_no DESC \
         LIMIT 1",
    )
    .bind(job_id)
    .fetch_optional(pool)
    .await?;

    match attempt_id {
        Some(attempt_id) => enqueue_job_attempt(pool, job_id, attempt_id).await,
        None => Ok(0),
    }
}

pub async fn enqueue_missing_ready_jobs(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO job_queue (id, job_id, attempt_id, pipeline_id, stage_id, state, required_tags, queued_at) \
         SELECT gen_random_uuid(), j.id, a.id, s.pipeline_id, s.id, 'queued', j.required_tags, a.created_at \
         FROM jobs j \
         JOIN stages s ON s.id = j.stage_id \
         JOIN pipelines p ON p.id = s.pipeline_id \
         JOIN LATERAL ( \
             SELECT id, created_at \
             FROM execution_attempts \
             WHERE job_id = j.id AND status = 'queued' \
             ORDER BY attempt_no DESC \
             LIMIT 1 \
         ) a ON TRUE \
         WHERE j.status = 'queued' \
           AND NOT j.manual \
           AND p.status IN ('queued','running') \
           AND NOT EXISTS ( \
               SELECT 1 \
               FROM job_leases l \
               WHERE l.job_id = j.id AND l.lease_status = 'active' \
           ) \
         ON CONFLICT (attempt_id) DO UPDATE \
         SET state = 'queued', \
             lease_id = NULL, \
             leased_at = NULL, \
             completed_at = NULL, \
             required_tags = EXCLUDED.required_tags, \
             updated_at = now() \
         WHERE job_queue.state IN ('queued','completed','canceled')",
    )
    .execute(pool)
    .await?;
    if result.rows_affected() > 0 {
        crate::dispatch_signal::notify_runner_work_available();
    }
    Ok(result.rows_affected())
}

pub async fn close_job_queue_for_attempt_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    attempt_id: Uuid,
    terminal_status: &str,
) -> Result<u64, sqlx::Error> {
    let queue_state = queue_state_for_terminal(terminal_status);
    let result = sqlx::query(
        "UPDATE job_queue \
         SET state = $2, completed_at = COALESCE(completed_at, now()), updated_at = now() \
         WHERE attempt_id = $1 AND state IN ('queued','leased')",
    )
    .bind(attempt_id)
    .bind(queue_state)
    .execute(&mut **tx)
    .await?;
    Ok(result.rows_affected())
}

pub async fn close_job_queue_for_attempt(
    pool: &PgPool,
    attempt_id: Uuid,
    terminal_status: &str,
) -> Result<u64, sqlx::Error> {
    let queue_state = queue_state_for_terminal(terminal_status);
    let result = sqlx::query(
        "UPDATE job_queue \
         SET state = $2, completed_at = COALESCE(completed_at, now()), updated_at = now() \
         WHERE attempt_id = $1 AND state IN ('queued','leased')",
    )
    .bind(attempt_id)
    .bind(queue_state)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn close_job_queue_for_job(
    pool: &PgPool,
    job_id: Uuid,
    terminal_status: &str,
) -> Result<u64, sqlx::Error> {
    let queue_state = queue_state_for_terminal(terminal_status);
    let result = sqlx::query(
        "UPDATE job_queue \
         SET state = $2, completed_at = COALESCE(completed_at, now()), updated_at = now() \
         WHERE job_id = $1 AND state IN ('queued','leased')",
    )
    .bind(job_id)
    .bind(queue_state)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn close_job_queue_for_lease(
    pool: &PgPool,
    lease_id: Uuid,
    terminal_status: &str,
) -> Result<u64, sqlx::Error> {
    let queue_state = queue_state_for_terminal(terminal_status);
    let result = sqlx::query(
        "UPDATE job_queue \
         SET state = $2, completed_at = COALESCE(completed_at, now()), updated_at = now() \
         WHERE lease_id = $1 AND state IN ('queued','leased')",
    )
    .bind(lease_id)
    .bind(queue_state)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn close_job_queue_for_pipeline(
    pool: &PgPool,
    pipeline_id: Uuid,
    terminal_status: &str,
) -> Result<u64, sqlx::Error> {
    let queue_state = queue_state_for_terminal(terminal_status);
    let result = sqlx::query(
        "UPDATE job_queue \
         SET state = $2, completed_at = COALESCE(completed_at, now()), updated_at = now() \
         WHERE pipeline_id = $1 AND state IN ('queued','leased')",
    )
    .bind(pipeline_id)
    .bind(queue_state)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

fn queue_state_for_terminal(terminal_status: &str) -> &'static str {
    match terminal_status {
        "canceled" => "canceled",
        _ => "completed",
    }
}

pub async fn next_attempt_log_sequence(
    pool: &PgPool,
    attempt_id: Uuid,
) -> Result<i32, sqlx::Error> {
    let row = sqlx::query(
        "SELECT COALESCE(MAX(sequence), 0) + 1 AS next_sequence FROM job_logs WHERE attempt_id = $1",
    )
    .bind(attempt_id)
    .fetch_one(pool)
    .await?;
    Ok(row.get("next_sequence"))
}

pub async fn next_log_sequence(pool: &PgPool, job_id: Uuid) -> Result<i32, sqlx::Error> {
    let attempt_id = active_or_latest_attempt_id(pool, job_id).await?;
    next_attempt_log_sequence(pool, attempt_id).await
}

#[derive(Debug, sqlx::FromRow)]
pub struct StoredJobLog {
    pub id: i64,
    pub job_id: Uuid,
    pub attempt_id: Uuid,
    pub sequence: i32,
    pub message: String,
    pub created_at: DateTime<Utc>,
}

pub async fn append_job_log(
    pool: &PgPool,
    job_id: Uuid,
    attempt_id: Uuid,
    message: &str,
) -> Result<StoredJobLog, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 0))")
        .bind(attempt_id)
        .execute(&mut *tx)
        .await?;
    let record = sqlx::query_as::<_, StoredJobLog>(
        "WITH target_attempt AS ( \
             SELECT id FROM execution_attempts WHERE id = $2 AND job_id = $1 \
         ), next_log AS ( \
             SELECT COALESCE(MAX(sequence), 0) + 1 AS sequence \
             FROM job_logs WHERE attempt_id = $2 \
         ) \
         INSERT INTO job_logs (job_id, attempt_id, sequence, message) \
         SELECT $1, target_attempt.id, next_log.sequence, $3 FROM target_attempt, next_log \
         RETURNING id, job_id, attempt_id, sequence, message, created_at",
    )
    .bind(job_id)
    .bind(attempt_id)
    .bind(message)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(record)
}
