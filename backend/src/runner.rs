#![allow(dead_code)]

use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};

use axum::body::Bytes;
use sqlx::PgPool;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::ChildStdout,
    sync::Mutex,
};
use uuid::Uuid;

use crate::api::ApiError;

const RUNNER_RECONCILE_INTERVAL_SECONDS: u64 = 2;
const RUNNER_STALE_OFFLINE_AFTER_SECONDS: i64 = 120;

/// Job processes currently executed by the embedded runner.
/// Maps job_id -> child process id so that cancel can kill it.
pub type RunningJobs = Arc<Mutex<HashMap<Uuid, u32>>>;

#[derive(Debug)]
struct EmbeddedJobLease {
    id: Uuid,
    attempt_id: Uuid,
    #[allow(dead_code)]
    generation: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RunnerMode {
    /// Execute jobs inside Docker containers using the declared image.
    Docker,
    /// Fallback: run on the host shell (local dev without Docker).
    HostShell,
}

fn runner_mode() -> RunnerMode {
    match std::env::var("CICD_RUNNER_MODE").ok().as_deref() {
        Some("host") => RunnerMode::HostShell,
        _ => RunnerMode::Docker,
    }
}

/// Executes a single job: marks running, streams stdout/stderr into job_logs,
/// sets success/failed from the exit code and refreshes stage/pipeline status.
pub async fn run_job(pool: PgPool, job_id: Uuid, running: RunningJobs) {
    if let Err(error) = run_job_inner(pool.clone(), job_id, running.clone()).await {
        tracing::error!(%job_id, error = ?error, "runner job failed");
        running.lock().await.remove(&job_id);
        finish_job_after_runner_error(&pool, job_id, &error).await;
    }
}

async fn finish_job_after_runner_error(pool: &PgPool, job_id: Uuid, error: &ApiError) {
    let message = truncate_error_tail(format!("runner: internal failure: {}", error.message));
    let status = match sqlx::query_scalar::<_, String>("SELECT status FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_optional(pool)
        .await
    {
        Ok(status) => status,
        Err(db_error) => {
            tracing::warn!(%job_id, error = ?db_error, "could not read job status after runner error");
            None
        }
    };

    if matches!(status.as_deref(), Some("success" | "canceled") | None) {
        return;
    }

    if let Err(db_error) = sqlx::query(
        "UPDATE jobs SET status = 'failed', finished_at = now() \
         WHERE id = $1 AND status IN ('queued','running')",
    )
    .bind(job_id)
    .execute(pool)
    .await
    {
        tracing::warn!(%job_id, error = ?db_error, "could not mark job failed after runner error");
    }

    match crate::store::active_or_latest_attempt_id(pool, job_id).await {
        Ok(attempt_id) => {
            if let Err(log_error) = append_attempt_log(pool, job_id, attempt_id, &message).await {
                tracing::warn!(%job_id, %attempt_id, error = ?log_error, "could not append runner error log");
            }
            if let Err(db_error) = mark_attempt_failed(pool, attempt_id, &message).await {
                tracing::warn!(%job_id, %attempt_id, error = ?db_error, "could not mark attempt failed after runner error");
            }
        }
        Err(db_error) => {
            tracing::warn!(%job_id, error = ?db_error, "could not resolve attempt after runner error");
        }
    }

    if let Err(db_error) =
        complete_active_lease_for_job(pool, job_id, "failed", Some(&message)).await
    {
        tracing::warn!(%job_id, error = ?db_error, "could not close lease after runner error");
    }

    if let Err(refresh_error) = refresh_stage(pool.clone(), job_id).await {
        tracing::warn!(%job_id, error = ?refresh_error, "could not refresh statuses after runner error");
    }
}

async fn claim_embedded_job_lease(
    pool: &PgPool,
    job_id: Uuid,
) -> Result<Option<EmbeddedJobLease>, ApiError> {
    crate::store::enqueue_missing_ready_jobs(pool)
        .await
        .map_err(ApiError::internal)?;
    let row = sqlx::query_as::<_, (Uuid, Uuid, i64)>(
        r#"
        WITH queue_row AS (
            SELECT
                q.id AS queue_id,
                j.id AS job_id,
                q.attempt_id,
                LEAST(GREATEST(COALESCE(j.timeout_seconds, 3600), 5), 86400)::bigint
                    AS lease_ttl_seconds
            FROM job_queue q
            JOIN jobs j ON j.id = q.job_id
            JOIN execution_attempts a ON a.id = q.attempt_id
            JOIN stages s ON s.id = j.stage_id
            JOIN pipelines p ON p.id = s.pipeline_id
            WHERE j.id = $1
              AND q.state = 'queued'
              AND q.not_before <= now()
              AND cardinality(q.required_tags) = 0
              AND j.status = 'queued'
              AND a.status = 'queued'
              AND NOT j.manual
              AND p.status IN ('queued','running')
              AND NOT EXISTS (
                  SELECT 1
                  FROM job_leases l
                  WHERE l.job_id = j.id AND l.lease_status = 'active'
              )
            FOR UPDATE OF q SKIP LOCKED
        ),
        claimed_job AS (
            UPDATE jobs j
            SET status = 'running', started_at = now()
            FROM queue_row q
            WHERE j.id = q.job_id
              AND j.status = 'queued'
            RETURNING j.id
        ),
        claimed_attempt AS (
            UPDATE execution_attempts a
            SET status = 'running',
                trigger = 'runner',
                started_at = COALESCE(started_at, now())
            FROM queue_row q
            WHERE a.id = q.attempt_id
              AND a.status = 'queued'
              AND EXISTS (SELECT 1 FROM claimed_job)
            RETURNING a.id
        ),
        next_generation AS (
            SELECT COALESCE(MAX(generation), 0) + 1 AS generation
            FROM job_leases
            WHERE job_id = $1
        ),
        created_lease AS (
            INSERT INTO job_leases (
                id,
                job_id,
                attempt_id,
                runner_name,
                lease_status,
                generation,
                lease_expires_at
            )
            SELECT
                $2,
                q.job_id,
                claimed_attempt.id,
                'embedded',
                'active',
                next_generation.generation,
                now() + ((q.lease_ttl_seconds + 60) * interval '1 second')
            FROM queue_row q
            CROSS JOIN claimed_attempt
            CROSS JOIN next_generation
            RETURNING id, attempt_id, generation
        ),
        claimed_queue AS (
            UPDATE job_queue q
            SET state = 'leased',
                lease_id = created_lease.id,
                leased_at = now(),
                updated_at = now()
            FROM queue_row qr
            CROSS JOIN created_lease
            WHERE q.id = qr.queue_id
              AND q.state = 'queued'
            RETURNING created_lease.id, created_lease.attempt_id, created_lease.generation
        )
        SELECT id, attempt_id, generation FROM claimed_queue
        "#,
    )
    .bind(job_id)
    .bind(Uuid::new_v4())
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?;

    Ok(row.map(|(id, attempt_id, generation)| EmbeddedJobLease {
        id,
        attempt_id,
        generation,
    }))
}

async fn active_embedded_job_lease(
    pool: &PgPool,
    job_id: Uuid,
) -> Result<Option<EmbeddedJobLease>, ApiError> {
    let row = sqlx::query_as::<_, (Uuid, Uuid, i64)>(
        "SELECT id, attempt_id, generation \
         FROM job_leases \
         WHERE job_id = $1 AND lease_status = 'active' \
         ORDER BY generation DESC \
         LIMIT 1",
    )
    .bind(job_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(row.map(|(id, attempt_id, generation)| EmbeddedJobLease {
        id,
        attempt_id,
        generation,
    }))
}

async fn complete_embedded_job_lease(
    pool: &PgPool,
    lease_id: Uuid,
    terminal_status: &str,
    error_tail: Option<&str>,
) -> Result<(), ApiError> {
    complete_lease_by_id(pool, lease_id, terminal_status, error_tail)
        .await
        .map_err(ApiError::internal)?;
    Ok(())
}

async fn complete_lease_by_id(
    pool: &PgPool,
    lease_id: Uuid,
    terminal_status: &str,
    error_tail: Option<&str>,
) -> Result<u64, sqlx::Error> {
    let lease_status = lease_status_for_terminal(terminal_status);
    let bounded_tail = error_tail.map(|value| truncate_error_tail(value.to_owned()));
    let result = sqlx::query(
        "UPDATE job_leases \
         SET lease_status = $2, \
             completed_at = COALESCE(completed_at, now()), \
             terminal_status = $3, \
             error_tail = COALESCE(error_tail, $4) \
         WHERE id = $1 AND lease_status = 'active'",
    )
    .bind(lease_id)
    .bind(lease_status)
    .bind(terminal_status)
    .bind(bounded_tail.as_deref())
    .execute(pool)
    .await?;
    if result.rows_affected() > 0 {
        crate::store::close_job_queue_for_lease(pool, lease_id, terminal_status).await?;
    }
    Ok(result.rows_affected())
}

pub async fn complete_active_lease_for_job(
    pool: &PgPool,
    job_id: Uuid,
    terminal_status: &str,
    error_tail: Option<&str>,
) -> Result<u64, sqlx::Error> {
    let lease_status = lease_status_for_terminal(terminal_status);
    let bounded_tail = error_tail.map(|value| truncate_error_tail(value.to_owned()));
    let result = sqlx::query(
        "UPDATE job_leases \
         SET lease_status = $2, \
             completed_at = COALESCE(completed_at, now()), \
             terminal_status = $3, \
             error_tail = COALESCE(error_tail, $4) \
         WHERE job_id = $1 AND lease_status = 'active'",
    )
    .bind(job_id)
    .bind(lease_status)
    .bind(terminal_status)
    .bind(bounded_tail.as_deref())
    .execute(pool)
    .await?;
    crate::store::close_job_queue_for_job(pool, job_id, terminal_status).await?;
    Ok(result.rows_affected())
}

pub async fn cancel_active_leases_for_pipeline(
    pool: &PgPool,
    pipeline_id: Uuid,
    reason: &str,
) -> Result<u64, sqlx::Error> {
    let bounded_reason = truncate_error_tail(reason.to_owned());
    let signaled_external = sqlx::query(
        "UPDATE job_leases \
         SET cancel_requested_at = COALESCE(cancel_requested_at, now()), \
             error_tail = COALESCE(error_tail, $2) \
         WHERE lease_status = 'active' \
           AND cancel_requested_at IS NULL \
           AND runner_id IS NOT NULL \
           AND lease_token_hash IS NOT NULL \
           AND job_id IN ( \
             SELECT j.id \
             FROM jobs j \
             JOIN stages s ON s.id = j.stage_id \
             WHERE s.pipeline_id = $1 \
           )",
    )
    .bind(pipeline_id)
    .bind(&bounded_reason)
    .execute(pool)
    .await?;
    let closed_embedded = sqlx::query(
        "UPDATE job_leases \
         SET lease_status = 'canceled', \
             completed_at = COALESCE(completed_at, now()), \
             terminal_status = 'canceled', \
             error_tail = COALESCE(error_tail, $2) \
         WHERE lease_status = 'active' \
           AND NOT (runner_id IS NOT NULL AND lease_token_hash IS NOT NULL) \
           AND job_id IN ( \
             SELECT j.id \
             FROM jobs j \
             JOIN stages s ON s.id = j.stage_id \
             WHERE s.pipeline_id = $1 \
           )",
    )
    .bind(pipeline_id)
    .bind(bounded_reason)
    .execute(pool)
    .await?;
    crate::store::close_job_queue_for_pipeline(pool, pipeline_id, "canceled").await?;
    Ok(signaled_external.rows_affected() + closed_embedded.rows_affected())
}

pub async fn force_cancel_active_leases_for_pipeline(
    pool: &PgPool,
    pipeline_id: Uuid,
    reason: &str,
) -> Result<u64, sqlx::Error> {
    let bounded_reason = truncate_error_tail(reason.to_owned());
    let result = sqlx::query(
        "UPDATE job_leases \
         SET lease_status = 'canceled', \
             completed_at = COALESCE(completed_at, now()), \
             terminal_status = 'canceled', \
             error_tail = COALESCE(error_tail, $2) \
         WHERE lease_status = 'active' \
           AND job_id IN ( \
             SELECT j.id \
             FROM jobs j \
             JOIN stages s ON s.id = j.stage_id \
             WHERE s.pipeline_id = $1 \
           )",
    )
    .bind(pipeline_id)
    .bind(bounded_reason)
    .execute(pool)
    .await?;
    crate::store::close_job_queue_for_pipeline(pool, pipeline_id, "canceled").await?;
    Ok(result.rows_affected())
}

async fn cancel_active_leases_for_canceled_pipelines(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let signaled_external = sqlx::query(
        "UPDATE job_leases \
         SET cancel_requested_at = COALESCE(cancel_requested_at, now()), \
             error_tail = COALESCE(error_tail, 'pipeline canceled') \
         WHERE lease_status = 'active' \
           AND cancel_requested_at IS NULL \
           AND runner_id IS NOT NULL \
           AND lease_token_hash IS NOT NULL \
           AND job_id IN ( \
             SELECT j.id \
             FROM jobs j \
             JOIN stages s ON s.id = j.stage_id \
             JOIN pipelines p ON p.id = s.pipeline_id \
             WHERE p.status = 'canceled' \
           )",
    )
    .execute(pool)
    .await?;
    let closed_embedded = sqlx::query(
        "UPDATE job_leases \
         SET lease_status = 'canceled', \
             completed_at = COALESCE(completed_at, now()), \
             terminal_status = 'canceled', \
             error_tail = COALESCE(error_tail, 'pipeline canceled') \
         WHERE lease_status = 'active' \
           AND NOT (runner_id IS NOT NULL AND lease_token_hash IS NOT NULL) \
           AND job_id IN ( \
             SELECT j.id \
             FROM jobs j \
             JOIN stages s ON s.id = j.stage_id \
             JOIN pipelines p ON p.id = s.pipeline_id \
             WHERE p.status = 'canceled' \
           )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "UPDATE job_queue \
         SET state = 'canceled', completed_at = COALESCE(completed_at, now()), updated_at = now() \
         WHERE state IN ('queued','leased') \
           AND pipeline_id IN (SELECT id FROM pipelines WHERE status = 'canceled')",
    )
    .execute(pool)
    .await?;
    Ok(signaled_external.rows_affected() + closed_embedded.rows_affected())
}

pub async fn reconcile_expired_leases(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let expired_jobs = sqlx::query_as::<_, (Uuid, Uuid)>(
        "WITH expired AS ( \
             UPDATE job_leases \
             SET lease_status = CASE WHEN cancel_requested_at IS NOT NULL THEN 'canceled' ELSE 'expired' END, \
                 completed_at = COALESCE(completed_at, now()), \
                 terminal_status = CASE WHEN cancel_requested_at IS NOT NULL THEN 'canceled' ELSE 'failed' END, \
                 error_tail = COALESCE(error_tail, CASE WHEN cancel_requested_at IS NOT NULL THEN 'pipeline canceled' ELSE 'runner lease expired' END) \
             WHERE lease_status = 'active' AND lease_expires_at <= now() \
             RETURNING id AS lease_id, job_id, attempt_id, terminal_status \
         ), updated_attempts AS ( \
             UPDATE execution_attempts a \
             SET status = e.terminal_status, \
                 finished_at = COALESCE(a.finished_at, now()), \
                 error_tail = COALESCE(a.error_tail, CASE WHEN e.terminal_status = 'canceled' THEN 'pipeline canceled' ELSE 'runner lease expired' END) \
             FROM expired e \
             WHERE a.id = e.attempt_id AND a.status IN ('queued','running') \
             RETURNING a.job_id \
         ), updated_jobs AS ( \
             UPDATE jobs j \
             SET status = e.terminal_status, finished_at = COALESCE(j.finished_at, now()) \
             FROM expired e \
             WHERE j.id = e.job_id AND j.status IN ('queued','running') \
             RETURNING j.id, j.stage_id \
         ), updated_queue AS ( \
             UPDATE job_queue q \
             SET state = CASE WHEN e.terminal_status = 'canceled' THEN 'canceled' ELSE 'completed' END, \
                 completed_at = COALESCE(q.completed_at, now()), \
                 updated_at = now() \
             FROM expired e \
             WHERE q.state IN ('queued','leased') \
               AND (q.lease_id = e.lease_id OR q.attempt_id = e.attempt_id) \
             RETURNING q.id \
         ) \
         SELECT id, stage_id FROM updated_jobs",
    )
    .fetch_all(pool)
    .await?;

    for (job_id, stage_id) in &expired_jobs {
        if let Err(error) = crate::api::refresh_statuses(pool, *stage_id).await {
            tracing::warn!(%job_id, %stage_id, error = ?error, "could not refresh statuses after lease expiry");
        }
    }

    Ok(expired_jobs.len() as u64)
}

pub async fn reconcile_stale_runners(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE runners r \
         SET status = 'offline', draining = false, capacity_busy_slots = 0 \
         WHERE r.disabled_at IS NULL \
           AND r.status = 'online' \
           AND r.last_seen_at IS NOT NULL \
           AND r.last_seen_at <= now() - ($1::bigint * interval '1 second') \
           AND NOT EXISTS ( \
             SELECT 1 \
             FROM job_leases l \
             WHERE l.runner_id = r.id \
               AND l.lease_status = 'active' \
               AND l.lease_expires_at > now() \
           )",
    )
    .bind(RUNNER_STALE_OFFLINE_AFTER_SECONDS)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

async fn reconcile_unleased_running_jobs(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let stale_jobs = sqlx::query_as::<_, (Uuid, Uuid)>(
        "WITH stale AS ( \
             SELECT j.id, j.stage_id \
             FROM jobs j \
             WHERE j.status = 'running' \
               AND (j.started_at IS NULL OR j.started_at <= now() - interval '5 minutes') \
               AND NOT EXISTS ( \
                 SELECT 1 FROM job_leases l \
                 WHERE l.job_id = j.id AND l.lease_status = 'active' \
               ) \
         ), updated_attempts AS ( \
             UPDATE execution_attempts a \
             SET status = 'failed', \
                 finished_at = COALESCE(a.finished_at, now()), \
                 error_tail = COALESCE(a.error_tail, 'runner lease missing') \
             FROM stale s \
             WHERE a.job_id = s.id AND a.status IN ('queued','running') \
             RETURNING a.job_id \
         ), updated_jobs AS ( \
             UPDATE jobs j \
             SET status = 'failed', finished_at = COALESCE(j.finished_at, now()) \
             FROM stale s \
             WHERE j.id = s.id AND j.status = 'running' \
             RETURNING j.id, j.stage_id \
         ), updated_queue AS ( \
             UPDATE job_queue q \
             SET state = 'completed', \
                 completed_at = COALESCE(q.completed_at, now()), \
                 updated_at = now() \
             FROM stale s \
             WHERE q.job_id = s.id AND q.state IN ('queued','leased') \
             RETURNING q.id \
         ) \
         SELECT id, stage_id FROM updated_jobs",
    )
    .fetch_all(pool)
    .await?;

    for (job_id, stage_id) in &stale_jobs {
        if let Err(error) = crate::api::refresh_statuses(pool, *stage_id).await {
            tracing::warn!(%job_id, %stage_id, error = ?error, "could not refresh statuses after missing lease reconciliation");
        }
    }

    Ok(stale_jobs.len() as u64)
}

fn lease_status_for_terminal(terminal_status: &str) -> &'static str {
    match terminal_status {
        "canceled" => "canceled",
        _ => "completed",
    }
}

async fn run_job_inner(pool: PgPool, job_id: Uuid, running: RunningJobs) -> Result<(), ApiError> {
    let job = sqlx::query_as::<_, JobRow>(
        "SELECT j.id, j.stage_id, j.name, j.image, j.command, j.required_secrets, j.artifact_paths, j.status, \
                s.pipeline_id, p.project_id, s.name AS stage_name, \
                p.git_ref, p.commit_sha, pr.name AS project_name \
         FROM jobs j JOIN stages s ON s.id = j.stage_id \
         JOIN pipelines p ON p.id = s.pipeline_id \
         JOIN projects pr ON pr.id = p.project_id \
         WHERE j.id = $1",
    )
    .bind(job_id)
    .fetch_optional(&pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::not_found)?;

    if job.status != "queued" && job.status != "running" {
        return Ok(()); // terminal already
    }

    let lease = if job.status == "queued" {
        let Some(lease) = claim_embedded_job_lease(&pool, job_id).await? else {
            return Ok(());
        };
        lease
    } else {
        let Some(lease) = active_embedded_job_lease(&pool, job_id).await? else {
            tracing::warn!(%job_id, "running job has no active lease; skipping embedded execution");
            return Ok(());
        };
        lease
    };
    let attempt_id = lease.attempt_id;

    let workspace = prepare_workspace(&pool, &job).await?;

    // REQ-SEC-002: inject only job-declared project secrets as env vars.
    let secrets = crate::platform::project_secret_pairs_for_names(
        &pool,
        job.project_id,
        &job.required_secrets,
    )
    .await?;
    let masks: Vec<String> = secrets
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(_, v)| v.clone())
        .collect();

    // P0-2/P0-3: per-job timeout + shared pipeline artifacts directory.
    let timeout_secs =
        sqlx::query_scalar::<_, Option<i32>>("SELECT timeout_seconds FROM jobs WHERE id = $1")
            .bind(job_id)
            .fetch_one(&pool)
            .await
            .map_err(ApiError::internal)?
            .unwrap_or(3600)
            .clamp(5, 24 * 3600);
    let artifacts_root =
        std::env::var("CICD_ARTIFACTS_DIR").unwrap_or_else(|_| "/var/lib/forge/artifacts".into());
    let pipeline_artifacts = std::path::Path::new(&artifacts_root)
        .join("pipelines")
        .join(job.pipeline_id.to_string());
    let job_artifacts = pipeline_artifacts.join("jobs").join(job_id.to_string());
    tokio::fs::create_dir_all(&job_artifacts)
        .await
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;

    append_attempt_log(
        &pool,
        job_id,
        attempt_id,
        &format!("runner: starting job {}", job.name),
    )
    .await?;
    refresh_stage(pool.clone(), job.id).await?;

    let command_shell = shell_capture_command(&job.command);

    // P1-8: pipeline run variables -> CICD_VAR_<KEY>.
    let run_vars: serde_json::Map<String, serde_json::Value> =
        sqlx::query_scalar::<_, Option<serde_json::Value>>(
            "SELECT variables FROM pipelines WHERE id = $1",
        )
        .bind(job.pipeline_id)
        .fetch_one(&pool)
        .await
        .ok()
        .flatten()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();

    // P0-1: GitLab/Jenkins-style CI variables, CICD_-prefixed.
    let mut envs: Vec<(String, String)> = vec![
        ("CICD_PIPELINE_ID".into(), job.pipeline_id.to_string()),
        ("CICD_JOB_ID".into(), job_id.to_string()),
        ("CICD_JOB_NAME".into(), job.name.clone()),
        ("CICD_STAGE_NAME".into(), job.stage_name.clone()),
        ("CICD_PROJECT_ID".into(), job.project_id.to_string()),
        ("CICD_PROJECT_NAME".into(), job.project_name.clone()),
        ("CICD_COMMIT_REF".into(), job.git_ref.clone()),
        (
            "CICD_COMMIT_SHA".into(),
            job.commit_sha.clone().unwrap_or_default(),
        ),
        (
            "CICD_ARTIFACTS_DIR".into(),
            job_artifacts.display().to_string(),
        ),
        (
            "CICD_PIPELINE_ARTIFACTS_DIR".into(),
            pipeline_artifacts.display().to_string(),
        ),
    ];
    for (key, value) in &run_vars {
        if let Ok(s) = serde_json::to_string(value) {
            let trimmed = s.trim_matches('"').to_string();
            envs.push((
                format!("CICD_VAR_{}", key.to_uppercase().replace('-', "_")),
                trimmed,
            ));
        }
    }
    envs.extend(secrets.iter().cloned());

    let mut child = if runner_mode() == RunnerMode::Docker {
        let workspace_volume = workspace
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| workspace.clone());
        let mut cmd = tokio::process::Command::new("docker");
        cmd.args(docker_run_args(
            &format!("forge-job-{job_id}"),
            &job.image,
            &command_shell,
            "forge_runner_workspaces",
            &workspace_volume.display().to_string(),
        ));
        for (k, v) in &envs {
            cmd.env(k, v);
        }
        cmd.current_dir(&workspace)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        cmd
    } else {
        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c")
            .arg(&command_shell)
            .current_dir(&workspace)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        for (k, v) in &envs {
            cmd.env(k, v);
        }
        cmd
    };

    let mut child = child
        .spawn()
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;

    if let Some(pid) = child.id() {
        running.lock().await.insert(job_id, pid);
    }

    let mut stdout_task = child.stdout.take().map(|stdout| {
        let pool = pool.clone();
        let masks = masks.clone();
        tokio::spawn(async move {
            stream_stdout_to_attempt(pool, job_id, attempt_id, stdout, masks).await
        })
    });
    let exit_status = tokio::select! {
        status = child.wait() => status,
        _ = tokio::time::sleep(std::time::Duration::from_secs(timeout_secs as u64)) => {
            let message = format!("runner: job timed out after {timeout_secs}s, killing");
            append_attempt_log(&pool, job_id, attempt_id, &message).await?;
            let _ = child.start_kill();
            let _ = child.wait().await;
            if let Some(task) = stdout_task.take() {
                await_stdout_task(task).await?;
            }
            running.lock().await.remove(&job_id);
            let updated = sqlx::query(
                "UPDATE jobs SET status = 'failed', finished_at = now() \
                 WHERE id = $1 AND status NOT IN ('canceled')",
            )
                .bind(job_id)
                .execute(&pool)
                .await
                .map_err(ApiError::internal)?;
            if updated.rows_affected() == 0 {
                mark_attempt_canceled(
                    &pool,
                    attempt_id,
                    "runner: job canceled before timeout result",
                )
                .await?;
                complete_embedded_job_lease(
                    &pool,
                    lease.id,
                    "canceled",
                    Some("runner: job canceled before timeout result"),
                )
                .await?;
                refresh_stage(pool.clone(), job.id).await?;
                let _ = tokio::fs::remove_dir_all(&workspace).await;
                return Ok(());
            }
            mark_attempt_failed(&pool, attempt_id, &message).await?;
            complete_embedded_job_lease(&pool, lease.id, "failed", Some(&message)).await?;
            refresh_stage(pool.clone(), job.id).await?;
            let _ = tokio::fs::remove_dir_all(&workspace).await;
            return Ok(());
        }
    };

    running.lock().await.remove(&job_id);

    if let Some(task) = stdout_task {
        await_stdout_task(task).await?;
    }

    let (final_status, exit_code, error_tail) = match exit_status {
        Ok(status) if status.success() => ("success", status.code(), None),
        Ok(status) => (
            "failed",
            status.code(),
            Some(format!("runner: process exited with status {status}")),
        ),
        Err(error) => (
            "failed",
            None,
            Some(format!("runner: failed to wait for process: {error}")),
        ),
    };
    let mut final_status = final_status;
    let mut exit_code = exit_code;
    let mut error_tail = error_tail;
    let artifact_error =
        collect_declared_artifacts(&pool, job_id, attempt_id, &workspace, &job.artifact_paths)
            .await
            .err();
    if final_status == "success" {
        if let Some(error) = artifact_error {
            final_status = "failed";
            exit_code = None;
            error_tail = Some(truncate_error_tail(format!(
                "runner: artifact upload failed: {}",
                error.message
            )));
        }
    }

    // Cleanup workspace unless CICD_RUNNER_KEEP_WORKSPACE=1.
    if std::env::var("CICD_RUNNER_KEEP_WORKSPACE").ok().as_deref() != Some("1") {
        let _ = tokio::fs::remove_dir_all(&workspace).await;
    }

    let updated = sqlx::query(
        "UPDATE jobs SET status = $2, finished_at = now() \
         WHERE id = $1 AND status NOT IN ('canceled')",
    )
    .bind(job_id)
    .bind(final_status)
    .execute(&pool)
    .await
    .map_err(ApiError::internal)?;
    if updated.rows_affected() == 0 {
        mark_attempt_canceled(
            &pool,
            attempt_id,
            "runner: job canceled before process result",
        )
        .await?;
        complete_embedded_job_lease(
            &pool,
            lease.id,
            "canceled",
            Some("runner: job canceled before process result"),
        )
        .await?;
        refresh_stage(pool, job.id).await?;
        return Ok(());
    }
    sqlx::query(
        "UPDATE execution_attempts \
         SET status = $2, finished_at = now(), exit_code = $3, error_tail = $4 \
         WHERE id = $1 AND status NOT IN ('success','failed','canceled')",
    )
    .bind(attempt_id)
    .bind(final_status)
    .bind(exit_code)
    .bind(error_tail.as_deref())
    .execute(&pool)
    .await
    .map_err(ApiError::internal)?;
    complete_embedded_job_lease(&pool, lease.id, final_status, error_tail.as_deref()).await?;
    refresh_stage(pool, job.id).await?;
    Ok(())
}

async fn mark_attempt_failed(
    pool: &PgPool,
    attempt_id: Uuid,
    error_tail: &str,
) -> Result<(), ApiError> {
    sqlx::query(
        "UPDATE execution_attempts \
         SET status = 'failed', finished_at = COALESCE(finished_at, now()), error_tail = $2 \
         WHERE id = $1 AND status NOT IN ('success','failed','canceled')",
    )
    .bind(attempt_id)
    .bind(error_tail)
    .execute(pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(())
}

async fn mark_attempt_canceled(
    pool: &PgPool,
    attempt_id: Uuid,
    error_tail: &str,
) -> Result<(), ApiError> {
    sqlx::query(
        "UPDATE execution_attempts \
         SET status = 'canceled', \
             finished_at = COALESCE(finished_at, now()), \
             error_tail = COALESCE(error_tail, $2) \
         WHERE id = $1 AND status NOT IN ('success','failed','canceled')",
    )
    .bind(attempt_id)
    .bind(error_tail)
    .execute(pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(())
}

fn truncate_error_tail(value: String) -> String {
    const MAX_ERROR_TAIL_CHARS: usize = 1000;
    value.chars().take(MAX_ERROR_TAIL_CHARS).collect()
}

async fn await_stdout_task(
    task: tokio::task::JoinHandle<Result<(), ApiError>>,
) -> Result<(), ApiError> {
    task.await.map_err(|error| {
        ApiError::internal(sqlx::Error::Protocol(format!(
            "stdout reader task failed: {error}"
        )))
    })?
}

async fn stream_stdout_to_attempt(
    pool: PgPool,
    job_id: Uuid,
    attempt_id: Uuid,
    stdout: ChildStdout,
    masks: Vec<String>,
) -> Result<(), ApiError> {
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await.unwrap_or(0);
        if n == 0 {
            break;
        }
        append_attempt_log(
            &pool,
            job_id,
            attempt_id,
            &mask_secrets(line.trim_end(), &masks),
        )
        .await?;
    }
    Ok(())
}

async fn collect_declared_artifacts(
    pool: &PgPool,
    job_id: Uuid,
    attempt_id: Uuid,
    workspace: &Path,
    artifact_paths: &[String],
) -> Result<(), ApiError> {
    if artifact_paths.is_empty() {
        return Ok(());
    }
    let workspace = tokio::fs::canonicalize(workspace)
        .await
        .map_err(|error| ApiError::internal(sqlx::Error::Io(error)))?;
    for artifact_path in artifact_paths {
        let file = resolve_workspace_artifact(&workspace, artifact_path).await?;
        let metadata = tokio::fs::metadata(&file)
            .await
            .map_err(|error| ApiError::internal(sqlx::Error::Io(error)))?;
        if metadata.len() == 0 || metadata.len() > crate::body_limits::ARTIFACT_UPLOAD_BYTES as u64
        {
            return Err(ApiError::bad_request(
                "artifact must be between 1 byte and 50 MiB",
            ));
        }
        let bytes = tokio::fs::read(&file)
            .await
            .map_err(|error| ApiError::internal(sqlx::Error::Io(error)))?;
        let name = artifact_name_from_declared_path(artifact_path);
        crate::platform::store_job_artifact(
            pool,
            job_id,
            Some(attempt_id),
            &name,
            "application/octet-stream",
            Bytes::from(bytes),
        )
        .await?;
        append_attempt_log(
            pool,
            job_id,
            attempt_id,
            &format!("runner: uploaded artifact {artifact_path}"),
        )
        .await?;
    }
    Ok(())
}

async fn resolve_workspace_artifact(
    workspace: &Path,
    artifact_path: &str,
) -> Result<PathBuf, ApiError> {
    let requested = Path::new(artifact_path);
    if requested.components().any(|component| {
        matches!(
            component,
            Component::Prefix(_) | Component::RootDir | Component::ParentDir
        )
    }) {
        return Err(ApiError::bad_request(
            "artifact path must stay within workspace",
        ));
    }
    let file = tokio::fs::canonicalize(workspace.join(requested))
        .await
        .map_err(|_| ApiError::bad_request("declared artifact file does not exist"))?;
    if !file.starts_with(workspace) {
        return Err(ApiError::bad_request(
            "artifact path must stay within workspace",
        ));
    }
    let metadata = tokio::fs::metadata(&file)
        .await
        .map_err(|error| ApiError::internal(sqlx::Error::Io(error)))?;
    if !metadata.is_file() {
        return Err(ApiError::bad_request(
            "declared artifact path must be a file",
        ));
    }
    Ok(file)
}

fn artifact_name_from_declared_path(path: &str) -> String {
    path.replace(['/', '\\'], "__")
}

#[derive(Debug, sqlx::FromRow)]
#[allow(dead_code)]
struct JobRow {
    id: Uuid,
    #[allow(dead_code)]
    stage_id: Uuid,
    name: String,
    #[allow(dead_code)]
    image: String,
    command: String,
    required_secrets: Vec<String>,
    artifact_paths: Vec<String>,
    status: String,
    #[allow(dead_code)]
    pipeline_id: Uuid,
    #[allow(dead_code)]
    project_id: Uuid,
    #[allow(dead_code)]
    stage_name: String,
    #[allow(dead_code)]
    git_ref: String,
    commit_sha: Option<String>,
    #[allow(dead_code)]
    project_name: String,
}

/// Clones the project repository (bare) into /tmp/forge-runner/<job> at git_ref.
async fn prepare_workspace(pool: &PgPool, job: &JobRow) -> Result<std::path::PathBuf, ApiError> {
    let repo_url: String = sqlx::query_scalar("SELECT repository_url FROM projects WHERE id = $1")
        .bind(job.project_id)
        .fetch_one(pool)
        .await
        .map_err(ApiError::internal)?;

    let git_ref: String = sqlx::query_scalar("SELECT git_ref FROM pipelines WHERE id = $1")
        .bind(job.pipeline_id)
        .fetch_one(pool)
        .await
        .map_err(ApiError::internal)?;

    let workspace = std::env::temp_dir().join(format!("forge-runner-{}", job.id));
    let _ = tokio::fs::remove_dir_all(&workspace).await;
    tokio::fs::create_dir_all(&workspace)
        .await
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;

    append_log(
        pool,
        job.id,
        &format!("runner: cloning {} at {}", repo_url, git_ref),
    )
    .await?;

    // Prefer local bare repo: avoid HTTP round-trips that can deadlock if the
    // repository_url points back at the same backend serving this runner.
    let cloned = clone_from_local_bare(&repo_url, &git_ref, &workspace).await;
    if !cloned {
        if let Err(error) = clone_via_http(&repo_url, &git_ref, &workspace, pool, job.id).await {
            let _ = tokio::fs::remove_dir_all(&workspace).await;
            return Err(error);
        }
    }
    Ok(workspace.join("workspace"))
}

/// Attempts `git clone` from a local bare repo under `CICD_GIT_ROOT`.
async fn clone_from_local_bare(repo_url: &str, git_ref: &str, workspace: &std::path::Path) -> bool {
    let Some(name) = extract_repo_name_from_url(repo_url) else {
        return false;
    };
    let git_root = std::env::var("CICD_GIT_ROOT").unwrap_or_else(|_| "/var/lib/forge/git".into());
    let bare_path = std::path::Path::new(&git_root).join(format!("{name}.git"));
    if !bare_path.is_dir() {
        return false;
    }
    let output = tokio::process::Command::new("git")
        .arg("clone")
        .arg("--quiet")
        .arg("--depth")
        .arg("50")
        .arg("--branch")
        .arg(git_ref)
        .arg(&bare_path)
        .arg("workspace")
        .current_dir(workspace)
        .output()
        .await;
    matches!(output, Ok(out) if out.status.success())
}

async fn clone_via_http(
    repo_url: &str,
    git_ref: &str,
    workspace: &std::path::Path,
    pool: &PgPool,
    job_id: Uuid,
) -> Result<(), ApiError> {
    let clone = tokio::process::Command::new("git")
        .arg("clone")
        .arg("--quiet")
        .arg("--depth")
        .arg("50")
        .arg("--branch")
        .arg(git_ref)
        .arg(repo_url)
        .arg("workspace")
        .current_dir(workspace)
        .output()
        .await
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
    if !clone.status.success() {
        let stderr = String::from_utf8_lossy(&clone.stderr);
        append_log(
            pool,
            job_id,
            &format!("runner: clone failed: {}", stderr.trim()),
        )
        .await?;
        return Err(ApiError::bad_request(stderr.to_string()));
    }
    Ok(())
}

fn extract_repo_name_from_url(url: &str) -> Option<String> {
    let path = url.split('/').next_back()?;
    let name = path.strip_suffix(".git").unwrap_or(path);
    Some(name.to_string())
}

async fn refresh_stage(pool: PgPool, job_id: Uuid) -> Result<(), ApiError> {
    let stage_id: Option<Uuid> = sqlx::query_scalar("SELECT stage_id FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_optional(&pool)
        .await
        .map_err(ApiError::internal)?;
    if let Some(stage_id) = stage_id {
        crate::api::refresh_statuses(&pool, stage_id).await?;
    }
    Ok(())
}

fn mask_secrets(line: &str, masks: &[String]) -> String {
    let mut out = line.to_string();
    for secret in masks {
        if !out.contains(secret.as_str()) {
            continue;
        }
        out = out.replace(secret.as_str(), "***");
    }
    out
}

async fn append_log(pool: &PgPool, job_id: Uuid, message: &str) -> Result<(), ApiError> {
    let attempt_id = crate::store::active_or_latest_attempt_id(pool, job_id)
        .await
        .map_err(ApiError::internal)?;
    append_attempt_log(pool, job_id, attempt_id, message).await
}

async fn append_attempt_log(
    pool: &PgPool,
    job_id: Uuid,
    attempt_id: Uuid,
    message: &str,
) -> Result<(), ApiError> {
    crate::store::append_job_log(pool, job_id, attempt_id, message)
        .await
        .map_err(ApiError::internal)?;
    Ok(())
}

/// Wakes up on newly queued pipelines and executes stage-by-stage.
pub async fn supervisor_loop(pool: PgPool, running: RunningJobs) {
    loop {
        match poll_and_dispatch(&pool, running.clone()).await {
            Ok(()) => {}
            Err(error) => tracing::error!(%error, "runner poll failed"),
        }
        tokio::time::sleep(Duration::from_secs(RUNNER_RECONCILE_INTERVAL_SECONDS)).await;
    }
}

/// Reconciles leases and runner health when the embedded executor is disabled.
pub async fn maintenance_loop(pool: PgPool) {
    loop {
        match reconcile_runtime_state(&pool).await {
            Ok(()) => {}
            Err(error) => tracing::error!(%error, "runner maintenance failed"),
        }
        tokio::time::sleep(Duration::from_secs(RUNNER_RECONCILE_INTERVAL_SECONDS)).await;
    }
}

/// Picks the first queued job of every non-terminal pipeline whose previous
/// stages all finished successfully, and spawns it.
async fn poll_and_dispatch(pool: &PgPool, running: RunningJobs) -> Result<(), sqlx::Error> {
    reconcile_runtime_state(pool).await?;

    let enqueued = crate::store::enqueue_missing_ready_jobs(pool).await?;
    if enqueued > 0 {
        tracing::debug!(enqueued, "runner materialized missing job queue rows");
    }

    let candidates = sqlx::query_as::<_, Candidate>(
        "SELECT j.id, j.stage_id \
         FROM job_queue q \
         JOIN jobs j ON j.id = q.job_id \
         JOIN execution_attempts a ON a.id = q.attempt_id \
         JOIN stages s ON s.id = j.stage_id \
         JOIN pipelines p ON p.id = s.pipeline_id \
         WHERE q.state = 'queued' \
           AND q.not_before <= now() \
           AND cardinality(q.required_tags) = 0 \
           AND j.status = 'queued' \
           AND a.status = 'queued' \
           AND NOT j.manual \
           AND p.status IN ('queued','running') \
           AND NOT EXISTS ( \
             SELECT 1 FROM job_leases l \
             WHERE l.job_id = j.id AND l.lease_status = 'active' \
           ) \
           AND NOT EXISTS ( \
             SELECT 1 FROM jobs x JOIN stages xs ON xs.id = x.stage_id \
             WHERE xs.pipeline_id = p.id AND xs.position < s.position \
               AND x.status NOT IN ('success') \
               AND NOT (x.status = 'failed' AND x.allow_failure) \
           ) \
           AND NOT EXISTS ( \
             SELECT 1 FROM jobs y JOIN stages ys ON ys.id = y.stage_id \
             WHERE ys.pipeline_id = p.id AND ys.position = s.position \
               AND y.status = 'failed' AND NOT y.allow_failure \
           ) \
         ORDER BY q.priority DESC, q.not_before, q.queued_at, p.created_at, s.position, j.position, q.id \
         LIMIT 16",
    )
    .fetch_all(pool)
    .await?;

    for candidate in candidates {
        let pool2 = pool.clone();
        let running2 = running.clone();
        tokio::spawn(async move {
            run_job(pool2, candidate.id, running2).await;
        });
    }
    Ok(())
}

async fn reconcile_runtime_state(pool: &PgPool) -> Result<(), sqlx::Error> {
    let expired = reconcile_expired_leases(pool).await?;
    if expired > 0 {
        tracing::warn!(expired, "runner reconciled expired leases");
    }

    let stale_runners = reconcile_stale_runners(pool).await?;
    if stale_runners > 0 {
        tracing::warn!(stale_runners, "runner marked stale runners offline");
    }

    let unleased = reconcile_unleased_running_jobs(pool).await?;
    if unleased > 0 {
        tracing::warn!(
            unleased,
            "runner reconciled running jobs without active leases"
        );
    }

    let canceled_leases = cancel_active_leases_for_canceled_pipelines(pool).await?;
    if canceled_leases > 0 {
        tracing::info!(
            canceled_leases,
            "runner signaled or closed active leases for canceled pipelines"
        );
    }

    cancel_jobs_for_canceled_pipelines(pool).await?;
    Ok(())
}

async fn cancel_jobs_for_canceled_pipelines(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE jobs SET status = 'canceled', finished_at = now() \
         WHERE status IN ('queued','running') AND stage_id IN \
         (SELECT id FROM stages WHERE pipeline_id IN \
          (SELECT id FROM pipelines WHERE status = 'canceled'))",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "UPDATE execution_attempts \
         SET status = 'canceled', \
             finished_at = COALESCE(finished_at, now()), \
             error_tail = COALESCE(error_tail, 'pipeline canceled') \
         WHERE status IN ('queued','running') \
           AND job_id IN ( \
             SELECT j.id FROM jobs j \
             JOIN stages s ON s.id = j.stage_id \
             JOIN pipelines p ON p.id = s.pipeline_id \
             WHERE p.status = 'canceled' \
           )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "UPDATE stages SET status = 'canceled' \
         WHERE status IN ('queued','running') \
           AND pipeline_id IN (SELECT id FROM pipelines WHERE status = 'canceled')",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "UPDATE job_queue \
         SET state = 'canceled', completed_at = COALESCE(completed_at, now()), updated_at = now() \
         WHERE state IN ('queued','leased') \
           AND pipeline_id IN (SELECT id FROM pipelines WHERE status = 'canceled')",
    )
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, sqlx::FromRow)]
struct Candidate {
    id: Uuid,
    #[allow(dead_code)]
    stage_id: Uuid,
}

/// Builds the `docker run` argument vector for executing a job in an
/// isolated container. Exported for unit tests.
fn docker_run_args(
    name: &str,
    image: &str,
    command: &str,
    volume_name: &str,
    workspace_mount: &str,
) -> Vec<String> {
    vec![
        "run".into(),
        "--rm".into(),
        "--name".into(),
        name.into(),
        "--network".into(),
        "none".into(),
        "--cap-drop".into(),
        "ALL".into(),
        "--read-only".into(),
        "--tmpfs".into(),
        "/tmp:rw,size=64m".into(),
        "--memory".into(),
        "512m".into(),
        "--pids-limit".into(),
        "256".into(),
        "--workdir".into(),
        "/workspace".into(),
        "--volume".into(),
        format!("{volume_name}:/workspaces"),
        "--mount".into(),
        format!("type=volume,src={volume_name},dst=/workspaces"),
        "--mount".into(),
        format!("type=bind,src={workspace_mount},dst=/workspace,readonly=false"),
        image.into(),
        "sh".into(),
        "-lc".into(),
        command.into(),
    ]
}

fn shell_capture_command(command: &str) -> String {
    format!("{{\n{command}\n}} 2>&1")
}

/// Mirrors the SQL `CASE WHEN` aggregation used in `refresh_statuses` so it
/// can be unit-tested without a database. Priority: failed > running >
/// canceled > queued; success only when everything succeeded.
fn aggregate_statuses<'a, I, S>(statuses: I) -> &'static str
where
    I: IntoIterator<Item = S>,
    S: AsRef<str> + 'a,
{
    let mut has_failed = false;
    let mut has_running = false;
    let mut has_canceled = false;
    let mut all_success = true;
    let mut any = false;
    for status in statuses {
        any = true;
        match status.as_ref() {
            "failed" => {
                has_failed = true;
                all_success = false;
            }
            "running" => {
                has_running = true;
                all_success = false;
            }
            "canceled" => {
                has_canceled = true;
                all_success = false;
            }
            "success" => {}
            _ => {
                all_success = false;
            }
        }
    }
    if !any {
        return "queued";
    }
    if has_failed {
        "failed"
    } else if has_running {
        "running"
    } else if has_canceled {
        "canceled"
    } else if all_success {
        "success"
    } else {
        "queued"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docker_execution_uses_the_declared_image_and_isolated_workspace_volume() {
        let args = docker_run_args(
            "forge-job-123",
            "rust:1.86",
            "cargo test",
            "forge_runner_workspaces",
            "/workspace/123/workspace",
        );

        assert!(
            args.windows(2)
                .any(|pair| pair == ["--name", "forge-job-123"])
        );
        assert!(args.windows(2).any(|pair| pair == ["--network", "none"]));
        assert!(args.windows(2).any(|pair| pair == ["--cap-drop", "ALL"]));
        assert!(args.iter().any(|arg| arg == "rust:1.86"));
        assert!(
            args.windows(3)
                .any(|pair| pair == ["sh", "-lc", "cargo test"])
        );
        assert!(!args.iter().any(|arg| arg == "cargo"));
    }

    #[test]
    fn shell_capture_wraps_multiline_commands_as_one_redirected_group() {
        assert_eq!(
            shell_capture_command("set -e\ncargo test\ncargo clippy"),
            "{\nset -e\ncargo test\ncargo clippy\n} 2>&1"
        );
    }

    #[test]
    fn aggregate_orders_priority_failed_running_canceled_then_queued() {
        assert_eq!(aggregate_statuses(["success", "queued"]), "queued");
        assert_eq!(aggregate_statuses(["success", "running"]), "running");
        assert_eq!(aggregate_statuses(["success", "canceled"]), "canceled");
        assert_eq!(aggregate_statuses(["success", "failed"]), "failed");
    }
}
