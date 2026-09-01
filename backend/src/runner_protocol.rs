use std::{collections::BTreeMap, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::api::{ApiError, AppState, pool};

const PROTOCOL_VERSION: i32 = 1;
const HEARTBEAT_INTERVAL_SECONDS: i32 = 15;
const POLL_WAIT_MAX_SECONDS: i32 = 0;
const ACK_DEADLINE_SECONDS: i64 = 30;
const LEASE_TTL_SECONDS: i64 = 120;
const RENEW_AFTER_SECONDS: i64 = 40;
const CREDENTIAL_TTL_DAYS: i64 = 30;
const MAX_TAGS: usize = 64;
const MAX_NAME_LEN: usize = 128;
const MAX_DIAGNOSTIC_LEN: usize = 4096;
const MAX_LOG_LINES: usize = 100;
const MAX_LOG_MESSAGE_LEN: usize = 8192;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/runner/register", post(register_runner_protocol))
        .route("/api/v1/runner/heartbeat", post(runner_protocol_heartbeat))
        .route("/api/v1/runner/work:poll", post(poll_runner_work))
        .route(
            "/api/v1/runner/leases/{lease_id}/ack",
            post(ack_runner_lease),
        )
        .route(
            "/api/v1/runner/leases/{lease_id}/renew",
            post(renew_runner_lease),
        )
        .route(
            "/api/v1/runner/leases/{lease_id}/logs",
            post(append_runner_lease_logs),
        )
        .route(
            "/api/v1/runner/leases/{lease_id}/complete",
            post(complete_runner_lease),
        )
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunnerRegisterRequest {
    protocol_version: i32,
    registration_token: String,
    name: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    capabilities: serde_json::Value,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunnerRegisterResponse {
    protocol_version: i32,
    runner_id: Uuid,
    credential: String,
    credential_expires_at: DateTime<Utc>,
    heartbeat_interval_seconds: i32,
    poll_wait_max_seconds: i32,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunnerHeartbeatRequest {
    protocol_version: i32,
    status: String,
    #[serde(default)]
    draining: bool,
    capacity: RunnerCapacity,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    capabilities: serde_json::Value,
    #[serde(default)]
    active_lease_ids: Vec<Uuid>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunnerCapacity {
    total_slots: i32,
    busy_slots: i32,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunnerPollRequest {
    protocol_version: i32,
    capacity: RunnerPollCapacity,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    capability_digest: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunnerPollCapacity {
    free_slots: i32,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunnerLeaseOffer {
    protocol_version: i32,
    lease_id: Uuid,
    lease_token: String,
    fencing_token: i64,
    ack_deadline: DateTime<Utc>,
    lease_expires_at: DateTime<Utc>,
    attempt: RunnerAttemptSpec,
    #[serde(skip_serializing_if = "Option::is_none")]
    plan_sha256: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunnerAttemptSpec {
    id: Uuid,
    number: i32,
    pipeline_id: Uuid,
    job_id: Uuid,
    job_key: String,
    git_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    commit_sha: Option<String>,
    executor: String,
    image: String,
    commands: Vec<String>,
    environment: BTreeMap<String, String>,
    timeout_seconds: i32,
    workspace: RunnerWorkspace,
    artifacts: Vec<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunnerWorkspace {
    checkout: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    checkout_url: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunnerLeaseControlRequest {
    protocol_version: i32,
    lease_token: String,
    fencing_token: i64,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunnerLeaseControlResponse {
    protocol_version: i32,
    lease_expires_at: DateTime<Utc>,
    renew_after: DateTime<Utc>,
    cancel_requested: bool,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunnerLogAppendRequest {
    protocol_version: i32,
    lease_token: String,
    fencing_token: i64,
    attempt_id: Uuid,
    lines: Vec<RunnerLogLine>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunnerLogLine {
    #[serde(default = "default_log_stream")]
    stream: String,
    message: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunnerLogAppendResponse {
    protocol_version: i32,
    accepted: usize,
    next_after: Option<i32>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunnerCompleteRequest {
    protocol_version: i32,
    lease_token: String,
    fencing_token: i64,
    attempt_id: Uuid,
    outcome: String,
    #[serde(default)]
    exit_code: Option<i32>,
    finished_at: DateTime<Utc>,
    #[serde(default)]
    diagnostic: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunnerCompleteResponse {
    protocol_version: i32,
    accepted: bool,
    terminal_status: String,
}

#[derive(Debug, FromRow)]
struct AuthenticatedRunner {
    id: Uuid,
    name: String,
    status: String,
    draining: bool,
}

#[derive(Debug, FromRow)]
struct ClaimedWork {
    lease_id: Uuid,
    job_id: Uuid,
    stage_id: Uuid,
    attempt_id: Uuid,
    attempt_no: i32,
    pipeline_id: Uuid,
    job_name: String,
    image: String,
    command: String,
    timeout_seconds: i32,
    git_ref: String,
    commit_sha: Option<String>,
    repository_url: String,
    generation: i64,
    lease_expires_at: DateTime<Utc>,
    ack_deadline: DateTime<Utc>,
    plan_sha256: Option<String>,
}

#[derive(Debug, FromRow)]
struct LeaseControlRow {
    lease_expires_at: DateTime<Utc>,
    renew_after: DateTime<Utc>,
    cancel_requested: bool,
}

#[derive(Debug, FromRow)]
struct LeaseStateRow {
    lease_status: String,
    expired: bool,
    ack_expired: bool,
}

#[derive(Debug, FromRow)]
struct CompleteLeaseRow {
    job_id: Uuid,
    stage_id: Uuid,
    attempt_id: Uuid,
}

#[derive(Debug, FromRow)]
struct RunnerLogLeaseRow {
    job_id: Uuid,
    attempt_id: Uuid,
}

#[utoipa::path(
    post,
    path = "/api/v1/runner/register",
    tag = "runner-protocol",
    request_body = RunnerRegisterRequest,
    responses((status = 201, body = RunnerRegisterResponse), (status = 400), (status = 401), (status = 503))
)]
pub(crate) async fn register_runner_protocol(
    State(state): State<Arc<AppState>>,
    Json(input): Json<RunnerRegisterRequest>,
) -> Result<(StatusCode, Json<RunnerRegisterResponse>), ApiError> {
    validate_protocol_version(input.protocol_version)?;
    validate_registration_token(&input.registration_token)?;
    let name = input.name.trim();
    validate_name(name)?;
    validate_tags(&input.tags)?;
    validate_capabilities(&input.capabilities)?;

    let credential = new_opaque_token("cicd_runner");
    let credential_hash = crate::auth::hash_token(&credential);
    let token_hint = token_hint(&credential);
    let credential_expires_at = Utc::now() + Duration::days(CREDENTIAL_TTL_DAYS);
    let db = pool(&state)?;
    let runner_id = Uuid::new_v4();

    sqlx::query(
        "INSERT INTO runners \
         (id, name, tags, status, last_seen_at, credential_hash, token_hint, \
          credential_expires_at, capabilities, heartbeat_payload) \
         VALUES ($1, $2, $3, 'offline', NULL, $4, $5, $6, $7, '{}'::jsonb)",
    )
    .bind(runner_id)
    .bind(name)
    .bind(&input.tags)
    .bind(&credential_hash)
    .bind(&token_hint)
    .bind(credential_expires_at)
    .bind(&input.capabilities)
    .execute(db)
    .await
    .map_err(map_runner_insert_error)?;

    crate::platform::audit(db, "runner.registered", "runner", runner_id, None).await?;

    Ok((
        StatusCode::CREATED,
        Json(RunnerRegisterResponse {
            protocol_version: PROTOCOL_VERSION,
            runner_id,
            credential,
            credential_expires_at,
            heartbeat_interval_seconds: HEARTBEAT_INTERVAL_SECONDS,
            poll_wait_max_seconds: POLL_WAIT_MAX_SECONDS,
        }),
    ))
}

#[utoipa::path(
    post,
    path = "/api/v1/runner/heartbeat",
    tag = "runner-protocol",
    request_body = RunnerHeartbeatRequest,
    responses((status = 204), (status = 400), (status = 401))
)]
pub(crate) async fn runner_protocol_heartbeat(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(input): Json<RunnerHeartbeatRequest>,
) -> Result<StatusCode, ApiError> {
    validate_protocol_version(input.protocol_version)?;
    validate_capacity(input.capacity.total_slots, input.capacity.busy_slots)?;
    validate_tags(&input.tags)?;
    validate_capabilities(&input.capabilities)?;
    let runner = authenticate_runner(pool(&state)?, &headers).await?;
    if !matches!(input.status.as_str(), "online" | "draining") {
        return Err(ApiError::bad_request(
            "runner status must be online or draining",
        ));
    }
    let status = input.status;
    let tags = input.tags;
    let capabilities = input.capabilities;
    let active_lease_ids = input.active_lease_ids;
    let draining = input.draining || status == "draining";
    let stored_status = "online";
    let payload = serde_json::json!({
        "protocolVersion": PROTOCOL_VERSION,
        "status": status,
        "draining": draining,
        "capacity": {
            "totalSlots": input.capacity.total_slots,
            "busySlots": input.capacity.busy_slots,
        },
        "tags": tags.clone(),
        "capabilities": capabilities.clone(),
        "activeLeaseIds": active_lease_ids,
    });

    sqlx::query(
        "UPDATE runners \
         SET status = $2, draining = $3, last_seen_at = now(), tags = $4, \
             capabilities = $5, capacity_total_slots = $6, capacity_busy_slots = $7, \
             heartbeat_payload = $8 \
         WHERE id = $1 AND disabled_at IS NULL",
    )
    .bind(runner.id)
    .bind(stored_status)
    .bind(draining)
    .bind(&tags)
    .bind(&capabilities)
    .bind(input.capacity.total_slots)
    .bind(input.capacity.busy_slots)
    .bind(payload)
    .execute(pool(&state)?)
    .await
    .map_err(ApiError::internal)?;

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/v1/runner/work:poll",
    tag = "runner-protocol",
    request_body = RunnerPollRequest,
    responses((status = 200, body = RunnerLeaseOffer), (status = 204), (status = 400), (status = 401), (status = 410))
)]
pub(crate) async fn poll_runner_work(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(input): Json<RunnerPollRequest>,
) -> Result<Response, ApiError> {
    validate_protocol_version(input.protocol_version)?;
    validate_poll_request(&input)?;
    let db = pool(&state)?;
    let runner = authenticate_runner(db, &headers).await?;
    if runner.status != "online" || runner.draining || input.capacity.free_slots == 0 {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    sqlx::query("UPDATE runners SET last_seen_at = now() WHERE id = $1 AND disabled_at IS NULL")
        .bind(runner.id)
        .execute(db)
        .await
        .map_err(ApiError::internal)?;

    crate::runner::reconcile_expired_leases(db)
        .await
        .map_err(ApiError::internal)?;

    match claim_next_work(db, &runner).await? {
        Some(offer) => Ok(Json(offer).into_response()),
        None => Ok(StatusCode::NO_CONTENT.into_response()),
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/runner/leases/{lease_id}/ack",
    tag = "runner-protocol",
    request_body = RunnerLeaseControlRequest,
    params(("lease_id" = Uuid, Path)),
    responses((status = 200, body = RunnerLeaseControlResponse), (status = 401), (status = 409), (status = 410))
)]
pub(crate) async fn ack_runner_lease(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(lease_id): Path<Uuid>,
    Json(input): Json<RunnerLeaseControlRequest>,
) -> Result<Json<RunnerLeaseControlResponse>, ApiError> {
    validate_protocol_version(input.protocol_version)?;
    if input.fencing_token < 1 || input.lease_token.trim().is_empty() {
        return Err(ApiError::bad_request(
            "lease token and fencing token are required",
        ));
    }
    let db = pool(&state)?;
    let runner = authenticate_runner(db, &headers).await?;
    let token_hash = crate::auth::hash_token(input.lease_token.trim());
    let row = sqlx::query_as::<_, LeaseControlRow>(
        "UPDATE job_leases \
         SET acknowledged_at = COALESCE(acknowledged_at, now()), last_renewed_at = now() \
         WHERE id = $1 \
           AND runner_id = $2 \
           AND lease_status = 'active' \
           AND lease_token_hash = $3 \
           AND generation = $4 \
           AND lease_expires_at > now() \
           AND (acknowledged_at IS NOT NULL OR ack_deadline IS NULL OR ack_deadline > now()) \
         RETURNING lease_expires_at, now() + ($5::bigint * interval '1 second') AS renew_after, \
                   cancel_requested_at IS NOT NULL AS cancel_requested",
    )
    .bind(lease_id)
    .bind(runner.id)
    .bind(token_hash)
    .bind(input.fencing_token)
    .bind(RENEW_AFTER_SECONDS)
    .fetch_optional(db)
    .await
    .map_err(ApiError::internal)?;

    match row {
        Some(row) => Ok(Json(control_response(row))),
        None => Err(lease_mutation_error(db, lease_id).await),
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/runner/leases/{lease_id}/renew",
    tag = "runner-protocol",
    request_body = RunnerLeaseControlRequest,
    params(("lease_id" = Uuid, Path)),
    responses((status = 200, body = RunnerLeaseControlResponse), (status = 401), (status = 409), (status = 410))
)]
pub(crate) async fn renew_runner_lease(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(lease_id): Path<Uuid>,
    Json(input): Json<RunnerLeaseControlRequest>,
) -> Result<Json<RunnerLeaseControlResponse>, ApiError> {
    validate_protocol_version(input.protocol_version)?;
    if input.fencing_token < 1 || input.lease_token.trim().is_empty() {
        return Err(ApiError::bad_request(
            "lease token and fencing token are required",
        ));
    }
    let db = pool(&state)?;
    let runner = authenticate_runner(db, &headers).await?;
    let token_hash = crate::auth::hash_token(input.lease_token.trim());
    let row = sqlx::query_as::<_, LeaseControlRow>(
        "UPDATE job_leases \
         SET lease_expires_at = now() + ($5::bigint * interval '1 second'), \
             last_renewed_at = now() \
         WHERE id = $1 \
           AND runner_id = $2 \
           AND lease_status = 'active' \
           AND lease_token_hash = $3 \
           AND generation = $4 \
           AND lease_expires_at > now() \
           AND acknowledged_at IS NOT NULL \
         RETURNING lease_expires_at, now() + ($6::bigint * interval '1 second') AS renew_after, \
                   cancel_requested_at IS NOT NULL AS cancel_requested",
    )
    .bind(lease_id)
    .bind(runner.id)
    .bind(token_hash)
    .bind(input.fencing_token)
    .bind(LEASE_TTL_SECONDS)
    .bind(RENEW_AFTER_SECONDS)
    .fetch_optional(db)
    .await
    .map_err(ApiError::internal)?;

    match row {
        Some(row) => Ok(Json(control_response(row))),
        None => Err(lease_mutation_error(db, lease_id).await),
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/runner/leases/{lease_id}/logs",
    tag = "runner-protocol",
    request_body = RunnerLogAppendRequest,
    params(("lease_id" = Uuid, Path)),
    responses((status = 200, body = RunnerLogAppendResponse), (status = 400), (status = 401), (status = 409), (status = 410))
)]
pub(crate) async fn append_runner_lease_logs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(lease_id): Path<Uuid>,
    Json(input): Json<RunnerLogAppendRequest>,
) -> Result<Json<RunnerLogAppendResponse>, ApiError> {
    validate_protocol_version(input.protocol_version)?;
    if input.fencing_token < 1 || input.lease_token.trim().is_empty() {
        return Err(ApiError::bad_request(
            "lease token and fencing token are required",
        ));
    }
    validate_log_lines(&input.lines)?;

    let db = pool(&state)?;
    let runner = authenticate_runner(db, &headers).await?;
    let token_hash = crate::auth::hash_token(input.lease_token.trim());
    let row = sqlx::query_as::<_, RunnerLogLeaseRow>(
        "SELECT l.job_id, l.attempt_id \
         FROM job_leases l \
         JOIN jobs j ON j.id = l.job_id \
         JOIN execution_attempts a ON a.id = l.attempt_id \
         WHERE l.id = $1 \
           AND l.runner_id = $2 \
           AND l.lease_status = 'active' \
           AND l.lease_token_hash = $3 \
           AND l.generation = $4 \
           AND l.attempt_id = $5 \
           AND l.lease_expires_at > now() \
           AND l.acknowledged_at IS NOT NULL \
           AND j.status = 'running' \
           AND a.status = 'running'",
    )
    .bind(lease_id)
    .bind(runner.id)
    .bind(token_hash)
    .bind(input.fencing_token)
    .bind(input.attempt_id)
    .fetch_optional(db)
    .await
    .map_err(ApiError::internal)?;

    let Some(row) = row else {
        return Err(lease_mutation_error(db, lease_id).await);
    };

    let mut accepted = 0;
    let mut next_after = None;
    for line in &input.lines {
        let record = crate::store::append_job_log(
            db,
            row.job_id,
            row.attempt_id,
            &format_runner_log_line(line),
        )
        .await
        .map_err(ApiError::internal)?;
        accepted += 1;
        next_after = Some(record.sequence);
    }

    Ok(Json(RunnerLogAppendResponse {
        protocol_version: PROTOCOL_VERSION,
        accepted,
        next_after,
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/runner/leases/{lease_id}/complete",
    tag = "runner-protocol",
    request_body = RunnerCompleteRequest,
    params(("lease_id" = Uuid, Path)),
    responses((status = 200, body = RunnerCompleteResponse), (status = 400), (status = 401), (status = 409), (status = 410))
)]
pub(crate) async fn complete_runner_lease(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(lease_id): Path<Uuid>,
    Json(input): Json<RunnerCompleteRequest>,
) -> Result<Json<RunnerCompleteResponse>, ApiError> {
    validate_protocol_version(input.protocol_version)?;
    if input.fencing_token < 1 || input.lease_token.trim().is_empty() {
        return Err(ApiError::bad_request(
            "lease token and fencing token are required",
        ));
    }
    if input
        .exit_code
        .is_some_and(|code| !(0..=255).contains(&code))
    {
        return Err(ApiError::bad_request("exit_code must be between 0 and 255"));
    }
    let terminal_status = terminal_status_for_outcome(&input.outcome)?;
    let lease_status = lease_status_for_terminal(terminal_status);
    let diagnostic = input
        .diagnostic
        .as_deref()
        .map(|value| truncate_diagnostic(value.trim()))
        .filter(|value| !value.is_empty());

    let db = pool(&state)?;
    let runner = authenticate_runner(db, &headers).await?;
    let token_hash = crate::auth::hash_token(input.lease_token.trim());
    let mut tx = db.begin().await.map_err(ApiError::internal)?;
    let row = sqlx::query_as::<_, CompleteLeaseRow>(
        "SELECT l.job_id, j.stage_id, l.attempt_id \
         FROM job_leases l \
         JOIN jobs j ON j.id = l.job_id \
         JOIN execution_attempts a ON a.id = l.attempt_id \
         WHERE l.id = $1 \
           AND l.runner_id = $2 \
           AND l.lease_status = 'active' \
           AND l.lease_token_hash = $3 \
           AND l.generation = $4 \
           AND l.attempt_id = $5 \
           AND l.lease_expires_at > now() \
           AND l.acknowledged_at IS NOT NULL \
           AND j.status IN ('queued','running') \
           AND a.status IN ('queued','running') \
         FOR UPDATE OF l, j, a",
    )
    .bind(lease_id)
    .bind(runner.id)
    .bind(token_hash)
    .bind(input.fencing_token)
    .bind(input.attempt_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    let Some(row) = row else {
        drop(tx);
        return Err(lease_mutation_error(db, lease_id).await);
    };

    sqlx::query(
        "UPDATE execution_attempts \
         SET status = $2, \
             exit_code = $3, \
             error_tail = CASE WHEN $4::text IS NULL THEN error_tail ELSE $4 END, \
             finished_at = COALESCE(finished_at, $5) \
         WHERE id = $1 AND status IN ('queued','running')",
    )
    .bind(row.attempt_id)
    .bind(terminal_status)
    .bind(input.exit_code)
    .bind(diagnostic.as_deref())
    .bind(input.finished_at)
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    sqlx::query(
        "UPDATE jobs \
         SET status = $2, \
             started_at = COALESCE(started_at, $3), \
             finished_at = COALESCE(finished_at, $3) \
         WHERE id = $1 AND status IN ('queued','running')",
    )
    .bind(row.job_id)
    .bind(terminal_status)
    .bind(input.finished_at)
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    sqlx::query(
        "UPDATE job_leases \
         SET lease_status = $2, completed_at = COALESCE(completed_at, $3), \
             terminal_status = $4, error_tail = COALESCE(error_tail, $5) \
         WHERE id = $1 AND lease_status = 'active'",
    )
    .bind(lease_id)
    .bind(lease_status)
    .bind(input.finished_at)
    .bind(terminal_status)
    .bind(diagnostic.as_deref())
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    tx.commit().await.map_err(ApiError::internal)?;
    crate::api::refresh_statuses(db, row.stage_id).await?;

    Ok(Json(RunnerCompleteResponse {
        protocol_version: PROTOCOL_VERSION,
        accepted: true,
        terminal_status: terminal_status.to_string(),
    }))
}

async fn claim_next_work(
    db: &PgPool,
    runner: &AuthenticatedRunner,
) -> Result<Option<RunnerLeaseOffer>, ApiError> {
    let lease_id = Uuid::new_v4();
    let lease_token = new_opaque_token("cicd_lease");
    let lease_token_hash = crate::auth::hash_token(&lease_token);
    let row = sqlx::query_as::<_, ClaimedWork>(
        "WITH candidate AS ( \
             SELECT j.id AS job_id, j.stage_id, j.name AS job_name, j.image, j.command, \
                    LEAST(GREATEST(COALESCE(j.timeout_seconds, 3600), 5), 86400)::integer AS timeout_seconds, \
                    s.pipeline_id, p.git_ref, p.commit_sha, pr.repository_url, pp.plan_sha256 \
             FROM jobs j \
             JOIN stages s ON s.id = j.stage_id \
             JOIN pipelines p ON p.id = s.pipeline_id \
             JOIN projects pr ON pr.id = p.project_id \
             LEFT JOIN pipeline_plans pp ON pp.pipeline_id = p.id \
             WHERE j.status = 'queued' \
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
             ORDER BY p.created_at, s.position, j.position \
             LIMIT 1 \
             FOR UPDATE OF j SKIP LOCKED \
         ), current_attempt AS ( \
             SELECT a.id, a.attempt_no \
             FROM execution_attempts a \
             JOIN candidate c ON c.job_id = a.job_id \
             WHERE a.status = 'queued' \
             ORDER BY a.attempt_no DESC \
             LIMIT 1 \
             FOR UPDATE OF a \
         ), claimed_job AS ( \
             UPDATE jobs j \
             SET status = 'running', started_at = COALESCE(started_at, now()) \
             FROM candidate c \
             WHERE j.id = c.job_id \
               AND EXISTS (SELECT 1 FROM current_attempt) \
             RETURNING j.id \
         ), claimed_attempt AS ( \
             UPDATE execution_attempts a \
             SET status = 'running', trigger = 'external_runner', started_at = COALESCE(started_at, now()) \
             FROM current_attempt ca \
             WHERE a.id = ca.id \
               AND EXISTS (SELECT 1 FROM claimed_job) \
             RETURNING a.id, a.attempt_no \
         ), next_generation AS ( \
             SELECT COALESCE(MAX(l.generation), 0) + 1 AS generation \
             FROM job_leases l \
             JOIN candidate c ON c.job_id = l.job_id \
         ), created_lease AS ( \
             INSERT INTO job_leases \
                 (id, job_id, attempt_id, runner_id, runner_name, lease_status, generation, \
                  lease_expires_at, lease_token_hash, ack_deadline, runner_protocol_version) \
             SELECT $1, c.job_id, ca.id, $2, $3, 'active', ng.generation, \
                    now() + ($4::bigint * interval '1 second'), $5, \
                    now() + ($6::bigint * interval '1 second'), $7 \
             FROM candidate c \
             CROSS JOIN claimed_attempt ca \
             CROSS JOIN next_generation ng \
             RETURNING id AS lease_id, generation, lease_expires_at, ack_deadline \
         ) \
         SELECT cl.lease_id, c.job_id, c.stage_id, ca.id AS attempt_id, ca.attempt_no, \
                c.pipeline_id, c.job_name, c.image, c.command, c.timeout_seconds, \
                c.git_ref, c.commit_sha, c.repository_url, cl.generation, cl.lease_expires_at, \
                cl.ack_deadline, c.plan_sha256 \
         FROM candidate c \
         CROSS JOIN claimed_attempt ca \
         CROSS JOIN created_lease cl",
    )
    .bind(lease_id)
    .bind(runner.id)
    .bind(&runner.name)
    .bind(LEASE_TTL_SECONDS)
    .bind(&lease_token_hash)
    .bind(ACK_DEADLINE_SECONDS)
    .bind(PROTOCOL_VERSION)
    .fetch_optional(db)
    .await
    .map_err(ApiError::internal)?;

    let Some(row) = row else {
        return Ok(None);
    };
    crate::api::refresh_statuses(db, row.stage_id).await?;

    Ok(Some(RunnerLeaseOffer {
        protocol_version: PROTOCOL_VERSION,
        lease_id: row.lease_id,
        lease_token,
        fencing_token: row.generation,
        ack_deadline: row.ack_deadline,
        lease_expires_at: row.lease_expires_at,
        plan_sha256: row.plan_sha256,
        attempt: RunnerAttemptSpec {
            id: row.attempt_id,
            number: row.attempt_no,
            pipeline_id: row.pipeline_id,
            job_id: row.job_id,
            job_key: row.job_name,
            git_ref: row.git_ref,
            commit_sha: row.commit_sha,
            executor: "shell".to_string(),
            image: row.image,
            commands: vec![row.command],
            environment: BTreeMap::new(),
            timeout_seconds: row.timeout_seconds,
            workspace: RunnerWorkspace {
                checkout: true,
                checkout_url: Some(row.repository_url),
            },
            artifacts: Vec::new(),
        },
    }))
}

async fn authenticate_runner(
    db: &PgPool,
    headers: &HeaderMap,
) -> Result<AuthenticatedRunner, ApiError> {
    let token = bearer_token(headers)?;
    let token_hash = crate::auth::hash_token(token);
    let runner = sqlx::query_as::<_, AuthenticatedRunner>(
        "SELECT id, name, status, draining \
         FROM runners \
         WHERE credential_hash = $1 \
           AND disabled_at IS NULL \
           AND (credential_expires_at IS NULL OR credential_expires_at > now())",
    )
    .bind(token_hash)
    .fetch_optional(db)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::unauthorized)?;
    Ok(runner)
}

fn bearer_token(headers: &HeaderMap) -> Result<&str, ApiError> {
    let value = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(ApiError::unauthorized)?;
    value
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(ApiError::unauthorized)
}

fn control_response(row: LeaseControlRow) -> RunnerLeaseControlResponse {
    RunnerLeaseControlResponse {
        protocol_version: PROTOCOL_VERSION,
        lease_expires_at: row.lease_expires_at,
        renew_after: row.renew_after,
        cancel_requested: row.cancel_requested,
    }
}

async fn lease_mutation_error(db: &PgPool, lease_id: Uuid) -> ApiError {
    match sqlx::query_as::<_, LeaseStateRow>(
        "SELECT lease_status, lease_expires_at <= now() AS expired, \
                (ack_deadline IS NOT NULL AND acknowledged_at IS NULL AND ack_deadline <= now()) AS ack_expired \
         FROM job_leases WHERE id = $1",
    )
    .bind(lease_id)
    .fetch_optional(db)
    .await
    {
        Ok(Some(row)) if row.expired || row.ack_expired => {
            ApiError::gone("runner lease expired")
        }
        Ok(Some(row)) if row.lease_status != "active" => ApiError::conflict("lease is not active"),
        Ok(Some(_)) => ApiError::conflict("lease_fenced"),
        Ok(None) => ApiError::not_found(),
        Err(error) => ApiError::internal(error),
    }
}

fn validate_registration_token(value: &str) -> Result<(), ApiError> {
    let configured = std::env::var("CICD_RUNNER_REGISTRATION_TOKEN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApiError::service_unavailable("runner registration token is not configured")
        })?;
    if configured.as_bytes().ct_eq(value.trim().as_bytes()).into() {
        Ok(())
    } else {
        Err(ApiError::unauthorized())
    }
}

fn validate_protocol_version(version: i32) -> Result<(), ApiError> {
    if version == PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(ApiError::bad_request("unsupported_protocol_version"))
    }
}

fn validate_name(name: &str) -> Result<(), ApiError> {
    if name.is_empty() || name.len() > MAX_NAME_LEN {
        return Err(ApiError::bad_request(
            "runner name must be 1..128 characters",
        ));
    }
    Ok(())
}

fn validate_tags(tags: &[String]) -> Result<(), ApiError> {
    if tags.len() > MAX_TAGS || tags.iter().any(|tag| !valid_tag(tag.trim())) {
        return Err(ApiError::bad_request(
            "runner tags must match ^[a-z0-9][a-z0-9._-]{0,62}$ and max 64 items",
        ));
    }
    Ok(())
}

fn valid_tag(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 {
        return false;
    }
    let first = bytes[0];
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }
    bytes
        .iter()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-'))
}

fn validate_capabilities(value: &serde_json::Value) -> Result<(), ApiError> {
    if value.is_object() {
        Ok(())
    } else {
        Err(ApiError::bad_request("capabilities must be an object"))
    }
}

fn validate_log_lines(lines: &[RunnerLogLine]) -> Result<(), ApiError> {
    if lines.is_empty() || lines.len() > MAX_LOG_LINES {
        return Err(ApiError::bad_request(
            "log append requires 1..100 lines per request",
        ));
    }
    for line in lines {
        let stream = normalize_log_stream(&line.stream);
        if !matches!(stream, "stdout" | "stderr" | "system") {
            return Err(ApiError::bad_request(
                "log stream must be stdout, stderr or system",
            ));
        }
        if line.message.chars().count() > MAX_LOG_MESSAGE_LEN {
            return Err(ApiError::bad_request(
                "log message must be at most 8192 characters",
            ));
        }
        let trimmed = line.message.trim_end_matches(&['\r', '\n'][..]);
        if trimmed.contains(['\r', '\n']) {
            return Err(ApiError::bad_request(
                "log message must contain a single line",
            ));
        }
    }
    Ok(())
}

fn validate_capacity(total_slots: i32, busy_slots: i32) -> Result<(), ApiError> {
    if !(1..=1024).contains(&total_slots) || busy_slots < 0 || busy_slots > total_slots {
        return Err(ApiError::bad_request(
            "capacity total_slots must be 1..1024 and busy_slots must be within total_slots",
        ));
    }
    Ok(())
}

fn validate_poll_request(input: &RunnerPollRequest) -> Result<(), ApiError> {
    validate_tags(&input.tags)?;
    if !(0..=1024).contains(&input.capacity.free_slots) {
        return Err(ApiError::bad_request(
            "free_slots must be between 0 and 1024",
        ));
    }
    if input
        .capability_digest
        .as_deref()
        .is_some_and(|value| value.len() > 128)
    {
        return Err(ApiError::bad_request(
            "capability_digest must be at most 128 characters",
        ));
    }
    Ok(())
}

fn terminal_status_for_outcome(outcome: &str) -> Result<&'static str, ApiError> {
    match outcome {
        "success" => Ok("success"),
        "failed" | "timed_out" | "lost" => Ok("failed"),
        "canceled" => Ok("canceled"),
        _ => Err(ApiError::bad_request("unsupported runner outcome")),
    }
}

fn lease_status_for_terminal(terminal_status: &str) -> &'static str {
    match terminal_status {
        "canceled" => "canceled",
        _ => "completed",
    }
}

fn truncate_diagnostic(value: &str) -> String {
    value.chars().take(MAX_DIAGNOSTIC_LEN).collect()
}

fn default_log_stream() -> String {
    "stdout".to_string()
}

fn normalize_log_stream(value: &str) -> &str {
    let stream = value.trim();
    if stream.is_empty() { "stdout" } else { stream }
}

fn format_runner_log_line(line: &RunnerLogLine) -> String {
    let stream = normalize_log_stream(&line.stream);
    let message = line.message.trim_end_matches(&['\r', '\n'][..]);
    format!("[{stream}] {message}")
}

fn new_opaque_token(prefix: &str) -> String {
    format!(
        "{}_{}{}",
        prefix,
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
}

fn token_hint(value: &str) -> String {
    let prefix = value.chars().take(18).collect::<String>();
    let suffix = value
        .chars()
        .rev()
        .take(6)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    format!("{prefix}...{suffix}")
}

fn map_runner_insert_error(error: sqlx::Error) -> ApiError {
    match error {
        sqlx::Error::Database(db_error) if db_error.constraint() == Some("runners_name_key") => {
            ApiError::conflict("runner name already exists")
        }
        other => ApiError::internal(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runner_tag_validation_matches_protocol_subset() {
        assert!(valid_tag("linux"));
        assert!(valid_tag("docker-1.amd64"));
        assert!(!valid_tag(""));
        assert!(!valid_tag("Docker"));
        assert!(!valid_tag("-docker"));
        assert!(!valid_tag("docker/linux"));
    }

    #[test]
    fn runner_outcomes_map_to_current_job_status_domain() {
        assert_eq!(terminal_status_for_outcome("success").unwrap(), "success");
        assert_eq!(terminal_status_for_outcome("failed").unwrap(), "failed");
        assert_eq!(terminal_status_for_outcome("timed_out").unwrap(), "failed");
        assert_eq!(terminal_status_for_outcome("lost").unwrap(), "failed");
        assert_eq!(terminal_status_for_outcome("canceled").unwrap(), "canceled");
        assert!(terminal_status_for_outcome("skipped").is_err());
    }

    #[test]
    fn runner_log_lines_are_prefixed_and_validated() {
        let line = RunnerLogLine {
            stream: " stderr ".to_string(),
            message: "warning\n".to_string(),
        };

        assert_eq!(format_runner_log_line(&line), "[stderr] warning");
        assert!(validate_log_lines(&[line]).is_ok());
        assert!(validate_log_lines(&[]).is_err());
        assert!(
            validate_log_lines(&[RunnerLogLine {
                stream: "debug".to_string(),
                message: "x".to_string(),
            }])
            .is_err()
        );
        assert!(
            validate_log_lines(&[RunnerLogLine {
                stream: "stdout".to_string(),
                message: "first\nsecond".to_string(),
            }])
            .is_err()
        );
    }
}
