//! Jobs/attempts/logs pipeline-execution vertical (ADR-0012).

use super::dto::Job;
use super::{ApiError, ApiResult, AppState, pool};
use crate::domain::JobStatus;
use crate::store::{active_or_latest_attempt_id, append_job_log, open_attempt_id};
use axum::Json;
use axum::extract::{Path, State};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

fn attempt_lookup_error(error: sqlx::Error) -> ApiError {
    match error {
        sqlx::Error::RowNotFound => ApiError::not_found(),
        other => ApiError::internal(other),
    }
}

#[utoipa::path(post, path="/api/v1/jobs/{job_id}/status", tag="jobs", params(("job_id"=Uuid, Path)), responses((status=200, body=Job), (status=404)))]
pub(crate) async fn change_job_status(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
    Json(input): Json<ChangeStatus>,
) -> ApiResult<Job> {
    let pool = pool(&state)?;
    let job = sqlx::query_as::<_, Job>("SELECT id, stage_id, name, image, command, required_tags, required_secrets, artifact_paths, position, status, started_at, finished_at FROM jobs WHERE id = $1").bind(job_id).fetch_optional(pool).await.map_err(ApiError::internal)?.ok_or_else(ApiError::not_found)?;
    let current = JobStatus::try_from(job.status.as_str()).map_err(ApiError::bad_request)?;
    current
        .transition_to(input.status)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    transition_open_attempt(pool, job_id, input.status.as_str(), "manual_status").await?;
    let updated = sqlx::query_as::<_, Job>("UPDATE jobs SET status = $2, started_at = CASE WHEN $2 = 'running' THEN now() ELSE started_at END, finished_at = CASE WHEN $2 IN ('success','failed','canceled') THEN now() ELSE finished_at END WHERE id = $1 RETURNING id, stage_id, name, image, command, required_tags, required_secrets, artifact_paths, position, status, started_at, finished_at").bind(job_id).bind(input.status.as_str()).fetch_one(pool).await.map_err(ApiError::internal)?;
    if matches!(
        input.status,
        JobStatus::Success | JobStatus::Failed | JobStatus::Canceled
    ) {
        crate::runner::complete_active_lease_for_job(
            pool,
            job_id,
            input.status.as_str(),
            Some("manual status transition"),
        )
        .await
        .map_err(ApiError::internal)?;
    }
    refresh_statuses(pool, updated.stage_id).await?;
    Ok(Json(updated))
}

pub(crate) async fn transition_open_attempt(
    pool: &PgPool,
    job_id: Uuid,
    status: &str,
    trigger: &str,
) -> Result<Uuid, ApiError> {
    let attempt_id = open_attempt_id(pool, job_id, trigger)
        .await
        .map_err(attempt_lookup_error)?;
    let finished_status = matches!(status, "success" | "failed" | "canceled");
    sqlx::query(
        "UPDATE execution_attempts \
         SET status = $2, \
             started_at = CASE WHEN $2 = 'running' THEN COALESCE(started_at, now()) ELSE started_at END, \
             finished_at = CASE WHEN $3 THEN now() ELSE finished_at END \
         WHERE id = $1",
    )
    .bind(attempt_id)
    .bind(status)
    .bind(finished_status)
    .execute(pool)
    .await
    .map_err(ApiError::internal)?;
    if finished_status {
        crate::store::close_job_queue_for_attempt(pool, attempt_id, status)
            .await
            .map_err(ApiError::internal)?;
    }
    Ok(attempt_id)
}

#[utoipa::path(post, path="/api/v1/pipelines/{pipeline_id}/cancel", tag="pipelines", params(("pipeline_id"=Uuid, Path)), responses((status=200, body=CanceledPipelineResult), (status=404), (status=409)))]
pub(crate) async fn cancel_pipeline(
    State(state): State<Arc<AppState>>,
    Path(pipeline_id): Path<Uuid>,
) -> ApiResult<CanceledPipelineResult> {
    let pool = pool(&state)?;
    let pipeline = sqlx::query_scalar::<_, String>("SELECT status FROM pipelines WHERE id = $1")
        .bind(pipeline_id)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    if pipeline != "queued" && pipeline != "running" {
        return Err(ApiError::conflict("pipeline is not active"));
    }
    if let Some(running) = state.running_jobs.as_ref() {
        let job_ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT j.id FROM jobs j JOIN stages s ON s.id = j.stage_id WHERE s.pipeline_id = $1",
        )
        .bind(pipeline_id)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)?;
        let mut guard = running.lock().await;
        for job_id in job_ids {
            if let Some(pid) = guard.remove(&job_id) {
                kill_running_job(job_id, pid).await;
            }
        }
    }
    sqlx::query("UPDATE pipelines SET status = 'canceled', finished_at = now() WHERE id = $1")
        .bind(pipeline_id)
        .execute(pool)
        .await
        .map_err(ApiError::internal)?;
    sqlx::query(
        "UPDATE jobs SET status = 'canceled', finished_at = now() \
         WHERE status IN ('queued','running') AND stage_id IN \
         (SELECT id FROM stages WHERE pipeline_id = $1)",
    )
    .bind(pipeline_id)
    .execute(pool)
    .await
    .map_err(ApiError::internal)?;
    sqlx::query(
        "UPDATE execution_attempts \
         SET status = 'canceled', \
             finished_at = COALESCE(finished_at, now()), \
             error_tail = COALESCE(error_tail, 'pipeline canceled') \
         WHERE status IN ('queued','running') \
           AND job_id IN ( \
             SELECT j.id FROM jobs j \
             JOIN stages s ON s.id = j.stage_id \
             WHERE s.pipeline_id = $1 \
           )",
    )
    .bind(pipeline_id)
    .execute(pool)
    .await
    .map_err(ApiError::internal)?;
    crate::runner::cancel_active_leases_for_pipeline(pool, pipeline_id, "pipeline canceled")
        .await
        .map_err(ApiError::internal)?;
    sqlx::query(
        "UPDATE stages SET status = 'canceled' \
         WHERE status IN ('queued','running') AND pipeline_id = $1",
    )
    .bind(pipeline_id)
    .execute(pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(CanceledPipelineResult {
        canceled: pipeline_id,
    }))
}

/// Kill a running job process: try Docker container stop by name, then
/// SIGTERM and SIGKILL the child PID as fallback.
pub(crate) async fn kill_running_job(job_id: Uuid, pid: u32) {
    let container_name = format!("forge-job-{job_id}");
    let _ = tokio::process::Command::new("docker")
        .args(["stop", "-t", "2", &container_name])
        .status()
        .await;
    let _ = tokio::process::Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status()
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(2), async {}).await;
    let _ = tokio::process::Command::new("kill")
        .arg("-KILL")
        .arg(pid.to_string())
        .status()
        .await;
}

#[utoipa::path(post, path="/api/v1/pipelines/{pipeline_id}/retry", tag="pipelines", params(("pipeline_id"=Uuid, Path)), responses((status=200, body=RetriedPipelineResult), (status=404), (status=409)))]
pub(crate) async fn retry_pipeline(
    State(state): State<Arc<AppState>>,
    Path(pipeline_id): Path<Uuid>,
) -> ApiResult<RetriedPipelineResult> {
    let pool = pool(&state)?;
    let status = sqlx::query_scalar::<_, String>("SELECT status FROM pipelines WHERE id = $1")
        .bind(pipeline_id)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    if status != "failed" && status != "canceled" {
        return Err(ApiError::conflict(
            "only failed or canceled pipelines can be retried",
        ));
    }
    crate::runner::force_cancel_active_leases_for_pipeline(
        pool,
        pipeline_id,
        "pipeline retry superseded canceled lease",
    )
    .await
    .map_err(ApiError::internal)?;
    sqlx::query(
        "WITH retry_jobs AS ( \
             SELECT j.id \
             FROM jobs j JOIN stages s ON s.id = j.stage_id \
             WHERE s.pipeline_id = $1 AND j.status IN ('failed','canceled') \
         ), nexts AS ( \
             SELECT r.id AS job_id, COALESCE(MAX(a.attempt_no), 0) + 1 AS attempt_no \
             FROM retry_jobs r LEFT JOIN execution_attempts a ON a.job_id = r.id \
             GROUP BY r.id \
         ) \
         INSERT INTO execution_attempts (id, job_id, attempt_no, status, trigger) \
         SELECT gen_random_uuid(), job_id, attempt_no, 'queued', 'pipeline_retry' FROM nexts",
    )
    .bind(pipeline_id)
    .execute(pool)
    .await
    .map_err(ApiError::internal)?;
    sqlx::query(
        "UPDATE jobs SET status = 'queued', started_at = NULL, finished_at = NULL WHERE status IN ('failed','canceled') AND stage_id IN (SELECT id FROM stages WHERE pipeline_id = $1)",
    )
    .bind(pipeline_id)
    .execute(pool)
    .await
    .map_err(ApiError::internal)?;
    sqlx::query(
        "UPDATE stages SET status = 'queued' WHERE status IN ('failed','canceled') AND pipeline_id = $1",
    )
    .bind(pipeline_id)
    .execute(pool)
    .await
    .map_err(ApiError::internal)?;
    sqlx::query(
        "UPDATE pipelines SET status = 'queued', started_at = NULL, finished_at = NULL WHERE id = $1",
    )
    .bind(pipeline_id)
    .execute(pool)
    .await
    .map_err(ApiError::internal)?;
    crate::store::enqueue_missing_ready_jobs(pool)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(RetriedPipelineResult {
        retried: pipeline_id,
    }))
}

#[utoipa::path(post, path="/api/v1/jobs/{job_id}/retry", tag="jobs", params(("job_id"=Uuid, Path)), responses((status=200, body=Job), (status=404)))]
pub(crate) async fn retry_job(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
) -> ApiResult<Job> {
    let pool = pool(&state)?;
    let job = sqlx::query_as::<_, Job>("SELECT id, stage_id, name, image, command, required_tags, required_secrets, artifact_paths, position, status, started_at, finished_at FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    if job.status != "failed" && job.status != "canceled" {
        return Err(ApiError::conflict(
            "only failed or canceled jobs can be retried",
        ));
    }
    crate::runner::complete_active_lease_for_job(
        pool,
        job_id,
        "canceled",
        Some("job retry superseded canceled lease"),
    )
    .await
    .map_err(ApiError::internal)?;
    sqlx::query(
        "INSERT INTO execution_attempts (id, job_id, attempt_no, status, trigger) \
         SELECT $2, $1, COALESCE(MAX(attempt_no), 0) + 1, 'queued', 'job_retry' \
         FROM execution_attempts WHERE job_id = $1",
    )
    .bind(job_id)
    .bind(Uuid::new_v4())
    .execute(pool)
    .await
    .map_err(ApiError::internal)?;
    let updated = sqlx::query_as::<_, Job>("UPDATE jobs SET status = 'queued', started_at = NULL, finished_at = NULL WHERE id = $1 RETURNING id, stage_id, name, image, command, required_tags, required_secrets, artifact_paths, position, status, started_at, finished_at")
        .bind(job_id)
        .fetch_one(pool)
        .await
        .map_err(ApiError::internal)?;
    sqlx::query(
        "UPDATE stages SET status = 'queued' WHERE id = $1 AND status IN ('failed','canceled')",
    )
    .bind(job.stage_id)
    .execute(pool)
    .await
    .map_err(ApiError::internal)?;
    sqlx::query("UPDATE pipelines SET status = 'running', finished_at = NULL WHERE id = (SELECT pipeline_id FROM stages WHERE id = $1) AND status IN ('failed','canceled')")
        .bind(job.stage_id)
        .execute(pool)
        .await
        .map_err(ApiError::internal)?;
    crate::store::enqueue_current_job_attempt(pool, job_id)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(updated))
}

pub(crate) async fn refresh_statuses(pool: &PgPool, stage_id: Uuid) -> Result<(), ApiError> {
    let stage_status: String = sqlx::query_scalar("SELECT CASE WHEN bool_or(status = 'failed' AND NOT allow_failure) THEN 'failed' WHEN bool_and(status = 'success' OR (status = 'failed' AND allow_failure)) THEN 'success' WHEN bool_or(status = 'running') THEN 'running' WHEN bool_or(status = 'canceled') THEN 'canceled' ELSE 'queued' END FROM jobs WHERE stage_id = $1").bind(stage_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    let pipeline_id: Uuid =
        sqlx::query_scalar("UPDATE stages SET status = $2 WHERE id = $1 RETURNING pipeline_id")
            .bind(stage_id)
            .bind(stage_status)
            .fetch_one(pool)
            .await
            .map_err(ApiError::internal)?;
    let pipeline_status: String = sqlx::query_scalar("SELECT CASE WHEN bool_or(status = 'failed') THEN 'failed' WHEN bool_and(status = 'success') THEN 'success' WHEN bool_or(status = 'running') THEN 'running' WHEN bool_or(status = 'canceled') THEN 'canceled' ELSE 'queued' END FROM stages WHERE pipeline_id = $1").bind(pipeline_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    let previous: Option<String> = sqlx::query_scalar("SELECT status FROM pipelines WHERE id = $1")
        .bind(pipeline_id)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal)?;
    sqlx::query("UPDATE pipelines SET status = $2, started_at = CASE WHEN $2 = 'running' THEN COALESCE(started_at, now()) ELSE started_at END, finished_at = CASE WHEN $2 IN ('success','failed','canceled') THEN now() ELSE finished_at END WHERE id = $1").bind(pipeline_id).bind(&pipeline_status).execute(pool).await.map_err(ApiError::internal)?;
    if matches!(pipeline_status.as_str(), "queued" | "running") {
        crate::dispatch_signal::notify_runner_work_available();
    }
    // Emit a domain event exactly once, on the terminal transition, so
    // outbox webhook fan-out fires (ADR-0006).
    if matches!(pipeline_status.as_str(), "success" | "failed" | "canceled")
        && previous.as_deref() != Some(pipeline_status.as_str())
    {
        let project_id: Option<Uuid> =
            sqlx::query_scalar("SELECT project_id FROM pipelines WHERE id = $1")
                .bind(pipeline_id)
                .fetch_optional(pool)
                .await
                .map_err(ApiError::internal)?;
        if let Some(project_id) = project_id {
            crate::outbox::emit_pipeline_event(
                pool,
                project_id,
                pipeline_id,
                &format!("pipeline.{pipeline_status}"),
                &pipeline_status,
            )
            .await
            .map_err(ApiError::internal)?;
        }
    }
    Ok(())
}

#[utoipa::path(post, path="/api/v1/jobs/{job_id}/start", tag="jobs", params(("job_id"=Uuid, Path)), responses((status=200, body=ManualJobStartResult), (status=404), (status=409, description="job is not a waiting manual job")))]
/// Starts a manual (`when: manual`) job — approval gate (GitLab parity).
pub(crate) async fn start_manual_job(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
) -> ApiResult<ManualJobStartResult> {
    let pool = pool(&state)?;
    let manual_job: Option<(bool, String)> =
        sqlx::query_as("SELECT manual, status FROM jobs WHERE id = $1")
            .bind(job_id)
            .fetch_optional(pool)
            .await
            .map_err(ApiError::internal)?;
    match manual_job {
        Some((true, status)) if status == "queued" => {}
        Some((true, _)) => return Err(ApiError::conflict("manual job is not waiting")),
        Some((false, _)) => return Err(ApiError::conflict("job is not manual")),
        None => return Err(ApiError::not_found()),
    }
    let updated = sqlx::query_scalar::<_, bool>(
        "UPDATE jobs SET manual = false WHERE id = $1 AND manual AND status = 'queued' RETURNING TRUE",
    )
        .bind(job_id)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal)?;
    if !updated.unwrap_or(false) {
        return Err(ApiError::conflict("job already started"));
    }
    crate::store::enqueue_current_job_attempt(pool, job_id)
        .await
        .map_err(ApiError::internal)?;
    crate::metrics::PIPELINES_CREATED_TOTAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(Json(ManualJobStartResult { started: true }))
}

#[utoipa::path(get, path="/api/v1/jobs/{job_id}/logs/stream", tag="jobs", params(("job_id"=Uuid, Path), ("after"=Option<i32>, Query)), responses((status=200, description="text/event-stream of job log lines")))]
/// SSE live log stream: emits existing lines, then polls for new ones.
pub(crate) async fn job_log_stream(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
    axum::extract::Query(params): axum::extract::Query<StreamParams>,
) -> Result<
    axum::response::Sse<
        tokio_stream::wrappers::UnboundedReceiverStream<
            Result<axum::response::sse::Event, std::convert::Infallible>,
        >,
    >,
    ApiError,
> {
    let pool = pool(&state)?;
    let attempt_id = active_or_latest_attempt_id(pool, job_id)
        .await
        .map_err(attempt_lookup_error)?;
    let mut after = params.after.unwrap_or(-1);
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let p = pool.clone();
    let jid = job_id;
    let aid = attempt_id;
    tokio::spawn(async move {
        loop {
            let rows = sqlx::query_as::<_, (i32, String)>(
                "SELECT sequence, message FROM job_logs WHERE job_id = $1 AND attempt_id = $2 AND sequence > $3 ORDER BY sequence",
            )
            .bind(jid)
            .bind(aid)
            .bind(after)
            .fetch_all(&p)
            .await
            .unwrap_or_default();
            for (seq, message) in rows {
                after = seq;
                let _ = sender.send(Ok(axum::response::sse::Event::default()
                    .id(seq.to_string())
                    .data(message)));
            }
            let done: Option<String> = sqlx::query_scalar(
                "SELECT status FROM jobs WHERE id = $1 AND status IN ('success','failed','canceled')",
            )
            .bind(jid)
            .fetch_optional(&p)
            .await
            .unwrap_or_default();
            if let Some(status) = done {
                let _ = sender.send(Ok(axum::response::sse::Event::default()
                    .event("done")
                    .data(status)));
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        }
    });
    let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(receiver);
    Ok(axum::response::sse::Sse::new(stream))
}

#[utoipa::path(get, path="/api/v1/jobs/{job_id}/attempts", tag="jobs", params(("job_id"=Uuid, Path)), responses((status=200, body=[JobAttempt]), (status=404)))]
pub(crate) async fn list_job_attempts(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
) -> ApiResult<Vec<JobAttempt>> {
    let pool = pool(&state)?;
    let job_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM jobs WHERE id = $1)")
        .bind(job_id)
        .fetch_one(pool)
        .await
        .map_err(ApiError::internal)?;
    if !job_exists {
        return Err(ApiError::not_found());
    }
    let _ = active_or_latest_attempt_id(pool, job_id)
        .await
        .map_err(attempt_lookup_error)?;
    let attempts = sqlx::query_as::<_, JobAttempt>(
        "SELECT id, job_id, attempt_no, status, trigger, exit_code, error_tail, created_at, started_at, finished_at \
         FROM execution_attempts WHERE job_id = $1 ORDER BY attempt_no DESC",
    )
    .bind(job_id)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(attempts))
}

#[utoipa::path(get, path="/api/v1/jobs/{job_id}/attempts/{attempt_id}/logs", tag="jobs", params(("job_id"=Uuid, Path), ("attempt_id"=Uuid, Path)), responses((status=200, body=[JobLog]), (status=404)))]
pub(crate) async fn list_attempt_logs(
    State(state): State<Arc<AppState>>,
    Path((job_id, attempt_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Vec<JobLog>> {
    ensure_attempt_belongs_to_job(pool(&state)?, job_id, attempt_id).await?;
    let logs = sqlx::query_as::<_, JobLog>(
        "SELECT id, job_id, attempt_id, sequence, message, created_at \
         FROM job_logs WHERE job_id = $1 AND attempt_id = $2 ORDER BY sequence",
    )
    .bind(job_id)
    .bind(attempt_id)
    .fetch_all(pool(&state)?)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(logs))
}

#[utoipa::path(get, path="/api/v1/jobs/{job_id}/attempts/{attempt_id}/logs/page", tag="jobs", params(("job_id"=Uuid, Path), ("attempt_id"=Uuid, Path), LogPageParams), responses((status=200, body=JobLogPage), (status=400), (status=404)))]
/// Bounded page of logs for a concrete attempt. Preserves the legacy array endpoint.
pub(crate) async fn list_attempt_logs_page(
    State(state): State<Arc<AppState>>,
    Path((job_id, attempt_id)): Path<(Uuid, Uuid)>,
    axum::extract::Query(params): axum::extract::Query<LogPageParams>,
) -> ApiResult<JobLogPage> {
    let db = pool(&state)?;
    ensure_attempt_belongs_to_job(db, job_id, attempt_id).await?;
    Ok(Json(log_page(db, job_id, attempt_id, params).await?))
}

#[utoipa::path(get, path="/api/v1/jobs/{job_id}/logs", tag="jobs", params(("job_id"=Uuid, Path)), responses((status=200, body=[JobLog])))]
pub(crate) async fn list_logs(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
) -> ApiResult<Vec<JobLog>> {
    let attempt_id = active_or_latest_attempt_id(pool(&state)?, job_id)
        .await
        .map_err(attempt_lookup_error)?;
    let logs = sqlx::query_as::<_, JobLog>("SELECT id, job_id, attempt_id, sequence, message, created_at FROM job_logs WHERE job_id = $1 AND attempt_id = $2 ORDER BY sequence").bind(job_id).bind(attempt_id).fetch_all(pool(&state)?).await.map_err(ApiError::internal)?;
    Ok(Json(logs))
}

#[utoipa::path(get, path="/api/v1/jobs/{job_id}/logs/page", tag="jobs", params(("job_id"=Uuid, Path), LogPageParams), responses((status=200, body=JobLogPage), (status=400), (status=404)))]
/// Bounded page of logs for the active or latest attempt.
pub(crate) async fn list_logs_page(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
    axum::extract::Query(params): axum::extract::Query<LogPageParams>,
) -> ApiResult<JobLogPage> {
    let db = pool(&state)?;
    let attempt_id = active_or_latest_attempt_id(db, job_id)
        .await
        .map_err(attempt_lookup_error)?;
    Ok(Json(log_page(db, job_id, attempt_id, params).await?))
}

#[utoipa::path(post, path="/api/v1/jobs/{job_id}/logs", tag="jobs", request_body=AppendLog, params(("job_id"=Uuid, Path)), responses((status=200, body=JobLog), (status=413, description="log append body exceeds 1 MiB")))]
pub(crate) async fn append_log(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
    Json(input): Json<AppendLog>,
) -> ApiResult<JobLog> {
    if input.message.trim().is_empty() {
        return Err(ApiError::bad_request("message is required"));
    }
    let pool = pool(&state)?;
    let attempt_id = active_or_latest_attempt_id(pool, job_id)
        .await
        .map_err(attempt_lookup_error)?;
    let record = append_job_log(pool, job_id, attempt_id, input.message.trim())
        .await
        .map_err(ApiError::internal)?;
    let log = JobLog {
        id: record.id,
        job_id: record.job_id,
        attempt_id: record.attempt_id,
        sequence: record.sequence,
        message: record.message,
        created_at: record.created_at,
    };
    Ok(Json(log))
}

async fn ensure_attempt_belongs_to_job(
    pool: &PgPool,
    job_id: Uuid,
    attempt_id: Uuid,
) -> Result<(), ApiError> {
    let attempt_belongs_to_job: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM execution_attempts WHERE id = $1 AND job_id = $2)",
    )
    .bind(attempt_id)
    .bind(job_id)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)?;
    if attempt_belongs_to_job {
        Ok(())
    } else {
        Err(ApiError::not_found())
    }
}

async fn log_page(
    pool: &PgPool,
    job_id: Uuid,
    attempt_id: Uuid,
    params: LogPageParams,
) -> Result<JobLogPage, ApiError> {
    let limit = params.bounded_limit()?;
    let fetch_limit = limit + 1;
    let search_pattern = params.search_pattern()?;
    let pattern_ref = search_pattern.as_deref();

    let (items, total, has_more_before): (Vec<JobLog>, i64, bool) =
        if let Some(before) = params.before_sequence()? {
            // Tail window: newest rows strictly below `before`, ascending.
            let rows = sqlx::query_as::<_, JobLog>(
                "SELECT id, job_id, attempt_id, sequence, message, created_at \
                 FROM job_logs \
                 WHERE job_id = $1 AND attempt_id = $2 AND sequence < $3 \
                   AND ($4::TEXT IS NULL OR message ILIKE $4 ESCAPE '\\') \
                 ORDER BY sequence DESC LIMIT $5",
            )
            .bind(job_id)
            .bind(attempt_id)
            .bind(before)
            .bind(pattern_ref)
            .bind(fetch_limit)
            .fetch_all(pool)
            .await
            .map_err(ApiError::internal)?;
            let (total,): (i64,) = sqlx::query_as(
                "SELECT count(*) FROM job_logs \
                 WHERE job_id = $1 AND attempt_id = $2 AND sequence < $3 \
                   AND ($4::TEXT IS NULL OR message ILIKE $4 ESCAPE '\\')",
            )
            .bind(job_id)
            .bind(attempt_id)
            .bind(before)
            .bind(pattern_ref)
            .fetch_one(pool)
            .await
            .map_err(ApiError::internal)?;
            let has_more = rows.len() as i64 > limit;
            let mut rows = rows;
            rows.truncate(limit as usize);
            rows.reverse();
            (rows, total, has_more)
        } else {
            let after = params.after_sequence()?;
            let rows = sqlx::query_as::<_, JobLog>(
                "SELECT id, job_id, attempt_id, sequence, message, created_at \
                 FROM job_logs \
                 WHERE job_id = $1 AND attempt_id = $2 AND sequence > $3 \
                   AND ($4::TEXT IS NULL OR message ILIKE $4 ESCAPE '\\') \
                 ORDER BY sequence LIMIT $5",
            )
            .bind(job_id)
            .bind(attempt_id)
            .bind(after)
            .bind(pattern_ref)
            .bind(fetch_limit)
            .fetch_all(pool)
            .await
            .map_err(ApiError::internal)?;
            let (total,): (i64,) = sqlx::query_as(
                "SELECT count(*) FROM job_logs \
                 WHERE job_id = $1 AND attempt_id = $2 AND sequence > $3 \
                   AND ($4::TEXT IS NULL OR message ILIKE $4 ESCAPE '\\')",
            )
            .bind(job_id)
            .bind(attempt_id)
            .bind(after)
            .bind(pattern_ref)
            .fetch_one(pool)
            .await
            .map_err(ApiError::internal)?;
            let mut rows = rows;
            rows.truncate(limit as usize);
            (rows, total, false)
        };

    let next_after = if has_more_before {
        items.first().map(|log| log.sequence)
    } else {
        items
            .last()
            .map(|log| log.sequence)
            .filter(|_| total > items.len() as i64)
    };
    Ok(JobLogPage {
        items,
        next_after,
        total,
        has_more_before,
    })
}

fn like_contains_pattern(value: &str) -> String {
    let mut pattern = String::with_capacity(value.len() + 2);
    pattern.push('%');
    for ch in value.chars() {
        if matches!(ch, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(ch);
    }
    pattern.push('%');
    pattern
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct CanceledPipelineResult {
    #[schema(value_type = String, format = Uuid)]
    canceled: Uuid,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct RetriedPipelineResult {
    #[schema(value_type = String, format = Uuid)]
    retried: Uuid,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct ManualJobStartResult {
    started: bool,
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct JobAttempt {
    id: Uuid,
    job_id: Uuid,
    attempt_no: i32,
    status: String,
    trigger: String,
    exit_code: Option<i32>,
    error_tail: Option<String>,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct JobLog {
    id: i64,
    job_id: Uuid,
    attempt_id: Uuid,
    sequence: i32,
    message: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct JobLogPage {
    items: Vec<JobLog>,
    next_after: Option<i32>,
    /// Total matching rows for the current filter (K4.2 windowing).
    pub(crate) total: i64,
    /// True when a `before` window has older rows beyond this page.
    pub(crate) has_more_before: bool,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct ChangeStatus {
    status: JobStatus,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct AppendLog {
    message: String,
}

#[derive(serde::Deserialize, utoipa::IntoParams)]
pub(crate) struct StreamParams {
    pub(crate) after: Option<i32>,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub(crate) struct LogPageParams {
    /// Return log rows with sequence greater than this value.
    pub(crate) after: Option<i32>,
    /// Tail-window cursor: return the newest rows with sequence less than
    /// this value (descending fetch, returned in ascending order).
    pub(crate) before: Option<i32>,
    /// Page size. Default and maximum are 200 rows.
    pub(crate) limit: Option<i64>,
    /// Optional case-insensitive substring filter for message text.
    pub(crate) q: Option<String>,
}

impl LogPageParams {
    fn before_sequence(&self) -> Result<Option<i32>, ApiError> {
        match self.before {
            None => Ok(None),
            Some(before) => {
                if before <= 0 {
                    return Err(ApiError::bad_request("before must be greater than 0"));
                }
                Ok(Some(before))
            }
        }
    }

    fn after_sequence(&self) -> Result<i32, ApiError> {
        let after = self.after.unwrap_or(0);
        if after < 0 {
            return Err(ApiError::bad_request(
                "after must be greater than or equal to 0",
            ));
        }
        Ok(after)
    }

    fn bounded_limit(&self) -> Result<i64, ApiError> {
        let limit = self.limit.unwrap_or(200);
        if !(1..=200).contains(&limit) {
            return Err(ApiError::bad_request("limit must be between 1 and 200"));
        }
        Ok(limit)
    }

    fn search_pattern(&self) -> Result<Option<String>, ApiError> {
        let Some(raw) = self.q.as_deref().map(str::trim).filter(|q| !q.is_empty()) else {
            return Ok(None);
        };
        if raw.chars().count() > 128 {
            return Err(ApiError::bad_request("q must be at most 128 characters"));
        }
        Ok(Some(like_contains_pattern(raw)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_page_params_are_bounded_and_search_is_escaped() {
        let params = LogPageParams {
            after: Some(0),
            before: None,
            limit: Some(200),
            q: Some("100%_ok\\done".to_string()),
        };
        assert_eq!(params.after_sequence().unwrap(), 0);
        assert_eq!(params.bounded_limit().unwrap(), 200);
        assert_eq!(
            params.search_pattern().unwrap(),
            Some("%100\\%\\_ok\\\\done%".to_string())
        );

        assert!(
            LogPageParams {
                after: Some(-1),
                before: None,
                limit: Some(50),
                q: None,
            }
            .after_sequence()
            .is_err()
        );
        assert!(
            LogPageParams {
                after: None,
                before: None,
                limit: Some(201),
                q: None,
            }
            .bounded_limit()
            .is_err()
        );
        assert!(
            LogPageParams {
                after: None,
                before: None,
                limit: Some(50),
                q: Some("x".repeat(129)),
            }
            .search_pattern()
            .is_err()
        );
    }
}
