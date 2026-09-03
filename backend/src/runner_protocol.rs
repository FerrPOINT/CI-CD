use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration as StdDuration,
};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::{
    api::{ApiError, AppState, pool},
    body_limits,
};

const PROTOCOL_VERSION: i32 = 1;
const HEARTBEAT_INTERVAL_SECONDS: i32 = 15;
const POLL_WAIT_MAX_SECONDS: i32 = 30;
const ACK_DEADLINE_SECONDS: i64 = 30;
const LEASE_TTL_SECONDS: i64 = 120;
const RENEW_AFTER_SECONDS: i64 = 40;
const CREDENTIAL_TTL_DAYS: i64 = 30;
const CURRENT_EXECUTOR_KIND: &str = "shell";
const MAX_TAGS: usize = 64;
const MAX_NAME_LEN: usize = 128;
const MAX_DIAGNOSTIC_LEN: usize = 4096;
const MAX_LOG_LINES: usize = 100;
const MAX_LOG_MESSAGE_LEN: usize = 8192;
const MAX_SECRET_NAMES: usize = 64;
const SECRET_BUNDLE_TTL_SECONDS: i64 = 300;

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
            "/api/v1/runner/leases/{lease_id}/control",
            get(poll_runner_lease_control),
        )
        .route(
            "/api/v1/runner/leases/{lease_id}/secrets:resolve",
            post(resolve_runner_lease_secrets),
        )
        .route(
            "/api/v1/runner/leases/{lease_id}/artifacts",
            post(upload_runner_lease_artifact)
                .layer(DefaultBodyLimit::max(body_limits::ARTIFACT_UPLOAD_BYTES)),
        )
        .route(
            "/api/v1/runner/leases/{lease_id}/logs",
            post(append_runner_lease_logs)
                .layer(DefaultBodyLimit::max(body_limits::RUNNER_LOG_APPEND_BYTES)),
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
    #[serde(default = "default_capabilities")]
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
    tags: Option<Vec<String>>,
    #[serde(default = "default_capabilities")]
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
    #[schema(minimum = 0, maximum = 30)]
    wait_seconds: i32,
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
    secrets: Vec<String>,
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
pub(crate) struct RunnerSecretResolveRequest {
    protocol_version: i32,
    lease_token: String,
    fencing_token: i64,
    secret_names: Vec<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunnerSecretResolveResponse {
    protocol_version: i32,
    expires_at: DateTime<Utc>,
    items: Vec<RunnerSecretItem>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunnerSecretItem {
    name: String,
    injection: String,
    value: String,
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
    tags: Vec<String>,
    status: String,
    draining: bool,
    capabilities: serde_json::Value,
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
    required_secrets: Vec<String>,
    artifact_paths: Vec<String>,
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

#[derive(Debug, FromRow)]
struct RunnerSecretLeaseRow {
    project_id: Uuid,
    required_secrets: Vec<String>,
}

#[derive(Debug, FromRow)]
struct RunnerArtifactLeaseRow {
    job_id: Uuid,
    attempt_id: Uuid,
    artifact_paths: Vec<String>,
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
    validate_registration_token(
        &input.registration_token,
        state.config.runner.registration_token.as_deref(),
    )?;
    let name = input.name.trim();
    validate_name(name)?;
    let tags = normalize_tags(&input.tags)?;
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
    .bind(&tags)
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
    validate_capabilities(&input.capabilities)?;
    let runner = authenticate_runner(pool(&state)?, &headers).await?;
    let tags = input
        .tags
        .as_ref()
        .map(|tags| normalize_tags(tags))
        .transpose()?
        .unwrap_or_else(|| runner.tags.clone());
    if !matches!(input.status.as_str(), "online" | "draining") {
        return Err(ApiError::bad_request(
            "runner status must be online or draining",
        ));
    }
    let status = input.status;
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
    let runner_tags = poll_runner_tags(&runner, &input.tags)?;
    if runner.status != "online" || runner.draining || input.capacity.free_slots == 0 {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    sqlx::query("UPDATE runners SET last_seen_at = now() WHERE id = $1 AND disabled_at IS NULL")
        .bind(runner.id)
        .execute(db)
        .await
        .map_err(ApiError::internal)?;

    crate::runner::reconcile_unacknowledged_leases(db)
        .await
        .map_err(ApiError::internal)?;
    crate::runner::reconcile_expired_leases(db)
        .await
        .map_err(ApiError::internal)?;

    if !runner_supports_executor(&runner.capabilities, CURRENT_EXECUTOR_KIND) {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let poll_started = tokio::time::Instant::now();
    let wait_budget = StdDuration::from_secs(input.wait_seconds as u64);
    loop {
        let notified = crate::dispatch_signal::runner_work_notifier().notified();
        match claim_next_work(db, &runner, &runner_tags).await? {
            Some(offer) => return Ok(Json(offer).into_response()),
            None if input.wait_seconds == 0 => {
                return Ok(StatusCode::NO_CONTENT.into_response());
            }
            None => {}
        }
        let Some(remaining) = wait_budget.checked_sub(poll_started.elapsed()) else {
            return Ok(StatusCode::NO_CONTENT.into_response());
        };
        if remaining.is_zero() {
            return Ok(StatusCode::NO_CONTENT.into_response());
        }
        if tokio::time::timeout(remaining, notified).await.is_err() {
            return Ok(StatusCode::NO_CONTENT.into_response());
        }
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
    get,
    path = "/api/v1/runner/leases/{lease_id}/control",
    tag = "runner-protocol",
    params(
        ("lease_id" = Uuid, Path),
        ("X-Runner-Protocol-Version" = i32, Header),
        ("X-Lease-Token" = String, Header),
        ("X-Fencing-Token" = i64, Header)
    ),
    responses((status = 200, body = RunnerLeaseControlResponse), (status = 400), (status = 401), (status = 409), (status = 410))
)]
pub(crate) async fn poll_runner_lease_control(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(lease_id): Path<Uuid>,
) -> Result<Json<RunnerLeaseControlResponse>, ApiError> {
    let protocol_version = required_i32_header(
        &headers,
        "x-runner-protocol-version",
        "runner protocol version header is required",
    )?;
    validate_protocol_version(protocol_version)?;
    let lease_token =
        required_text_header(&headers, "x-lease-token", "lease token header is required")?;
    let fencing_token = required_i64_header(
        &headers,
        "x-fencing-token",
        "fencing token header is required",
    )?;
    if fencing_token < 1 {
        return Err(ApiError::bad_request(
            "lease token and fencing token are required",
        ));
    }

    let db = pool(&state)?;
    let runner = authenticate_runner(db, &headers).await?;
    let token_hash = crate::auth::hash_token(&lease_token);
    let row = sqlx::query_as::<_, LeaseControlRow>(
        "SELECT lease_expires_at, now() + ($5::bigint * interval '1 second') AS renew_after, \
                cancel_requested_at IS NOT NULL AS cancel_requested \
         FROM job_leases \
         WHERE id = $1 \
           AND runner_id = $2 \
           AND lease_status = 'active' \
           AND lease_token_hash = $3 \
           AND generation = $4 \
           AND lease_expires_at > now() \
           AND acknowledged_at IS NOT NULL",
    )
    .bind(lease_id)
    .bind(runner.id)
    .bind(token_hash)
    .bind(fencing_token)
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
    path = "/api/v1/runner/leases/{lease_id}/secrets:resolve",
    tag = "runner-protocol",
    request_body = RunnerSecretResolveRequest,
    params(("lease_id" = Uuid, Path)),
    responses((status = 200, body = RunnerSecretResolveResponse), (status = 400), (status = 401), (status = 403), (status = 409), (status = 410))
)]
pub(crate) async fn resolve_runner_lease_secrets(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(lease_id): Path<Uuid>,
    Json(input): Json<RunnerSecretResolveRequest>,
) -> Result<Response, ApiError> {
    validate_protocol_version(input.protocol_version)?;
    if input.fencing_token < 1 || input.lease_token.trim().is_empty() {
        return Err(ApiError::bad_request(
            "lease token and fencing token are required",
        ));
    }
    let requested = normalize_secret_names(&input.secret_names)?;

    let db = pool(&state)?;
    let runner = authenticate_runner(db, &headers).await?;
    let token_hash = crate::auth::hash_token(input.lease_token.trim());
    let row = sqlx::query_as::<_, RunnerSecretLeaseRow>(
        "SELECT p.project_id, j.required_secrets \
         FROM job_leases l \
         JOIN jobs j ON j.id = l.job_id \
         JOIN execution_attempts a ON a.id = l.attempt_id \
         JOIN stages s ON s.id = j.stage_id \
         JOIN pipelines p ON p.id = s.pipeline_id \
         WHERE l.id = $1 \
           AND l.runner_id = $2 \
           AND l.lease_status = 'active' \
           AND l.lease_token_hash = $3 \
           AND l.generation = $4 \
           AND l.lease_expires_at > now() \
           AND l.acknowledged_at IS NOT NULL \
           AND j.status = 'running' \
           AND a.status = 'running'",
    )
    .bind(lease_id)
    .bind(runner.id)
    .bind(token_hash)
    .bind(input.fencing_token)
    .fetch_optional(db)
    .await
    .map_err(ApiError::internal)?;

    let Some(row) = row else {
        return Err(lease_mutation_error(db, lease_id).await);
    };

    let allowed: BTreeSet<&str> = row.required_secrets.iter().map(String::as_str).collect();
    if requested
        .iter()
        .any(|secret_name| !allowed.contains(secret_name.as_str()))
    {
        return Err(ApiError::forbidden());
    }

    let pairs = crate::platform::project_secret_pairs_for_names_with_config(
        db,
        row.project_id,
        &requested,
        &state.config.secrets,
    )
    .await?;
    let response_body = RunnerSecretResolveResponse {
        protocol_version: PROTOCOL_VERSION,
        expires_at: Utc::now() + Duration::seconds(SECRET_BUNDLE_TTL_SECONDS),
        items: pairs
            .into_iter()
            .map(|(name, value)| RunnerSecretItem {
                name,
                injection: "env".to_string(),
                value,
            })
            .collect(),
    };
    let mut response = Json(response_body).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

#[utoipa::path(
    post,
    path = "/api/v1/runner/leases/{lease_id}/artifacts",
    tag = "runner-protocol",
    request_body = Vec<u8>,
    params(
        ("lease_id" = Uuid, Path),
        ("X-Runner-Protocol-Version" = i32, Header),
        ("X-Lease-Token" = String, Header),
        ("X-Fencing-Token" = i64, Header),
        ("X-Attempt-Id" = Uuid, Header),
        ("X-Artifact-Path" = String, Header),
        ("X-Artifact-Name" = String, Header)
    ),
    responses((status = 200, body = crate::platform::Artifact), (status = 400), (status = 401), (status = 403), (status = 409), (status = 410), (status = 413, description = "artifact body exceeds 50 MiB"))
)]
pub(crate) async fn upload_runner_lease_artifact(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(lease_id): Path<Uuid>,
    body: Bytes,
) -> Result<Json<crate::platform::Artifact>, ApiError> {
    let protocol_version = required_i32_header(
        &headers,
        "x-runner-protocol-version",
        "runner protocol version header is required",
    )?;
    validate_protocol_version(protocol_version)?;
    let lease_token =
        required_text_header(&headers, "x-lease-token", "lease token header is required")?;
    let fencing_token = required_i64_header(
        &headers,
        "x-fencing-token",
        "fencing token header is required",
    )?;
    let attempt_id =
        required_uuid_header(&headers, "x-attempt-id", "attempt id header is required")?;
    let artifact_path = required_text_header(
        &headers,
        "x-artifact-path",
        "artifact path header is required",
    )?;
    let artifact_name = required_text_header(
        &headers,
        "x-artifact-name",
        "artifact name header is required",
    )?;
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream");

    if fencing_token < 1 {
        return Err(ApiError::bad_request(
            "lease token and fencing token are required",
        ));
    }

    let db = pool(&state)?;
    let runner = authenticate_runner(db, &headers).await?;
    let token_hash = crate::auth::hash_token(&lease_token);
    let row = sqlx::query_as::<_, RunnerArtifactLeaseRow>(
        "SELECT l.job_id, l.attempt_id, j.artifact_paths \
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
    .bind(fencing_token)
    .bind(attempt_id)
    .fetch_optional(db)
    .await
    .map_err(ApiError::internal)?;

    let Some(row) = row else {
        return Err(lease_mutation_error(db, lease_id).await);
    };
    if !row.artifact_paths.iter().any(|path| path == &artifact_path) {
        return Err(ApiError::forbidden());
    }

    let artifact = crate::platform::store_job_artifact_with_config(
        db,
        &state.config.artifacts,
        row.job_id,
        Some(row.attempt_id),
        &artifact_name,
        content_type,
        body,
    )
    .await?;
    Ok(Json(artifact))
}

#[utoipa::path(
    post,
    path = "/api/v1/runner/leases/{lease_id}/logs",
    tag = "runner-protocol",
    request_body = RunnerLogAppendRequest,
    params(("lease_id" = Uuid, Path)),
    responses((status = 200, body = RunnerLogAppendResponse), (status = 400), (status = 401), (status = 409), (status = 410), (status = 413, description = "log append body exceeds 1 MiB"))
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
           AND ( \
             (j.status IN ('queued','running') AND a.status IN ('queued','running')) \
             OR ($6 = 'canceled' AND j.status = 'canceled' AND a.status = 'canceled') \
           ) \
         FOR UPDATE OF l, j, a",
    )
    .bind(lease_id)
    .bind(runner.id)
    .bind(token_hash)
    .bind(input.fencing_token)
    .bind(input.attempt_id)
    .bind(terminal_status)
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

    crate::store::close_job_queue_for_attempt_tx(&mut tx, row.attempt_id, terminal_status)
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
    runner_tags: &[String],
) -> Result<Option<RunnerLeaseOffer>, ApiError> {
    crate::store::enqueue_missing_ready_jobs(db)
        .await
        .map_err(ApiError::internal)?;
    let lease_id = Uuid::new_v4();
    let lease_token = new_opaque_token("cicd_lease");
    let lease_token_hash = crate::auth::hash_token(&lease_token);
    let row = sqlx::query_as::<_, ClaimedWork>(
        "WITH candidate AS ( \
             SELECT q.id AS queue_id, q.attempt_id, \
                    j.id AS job_id, j.stage_id, j.name AS job_name, j.image, j.command, \
                    LEAST(GREATEST(COALESCE(j.timeout_seconds, 3600), 5), 86400)::integer AS timeout_seconds, \
                    s.pipeline_id, p.git_ref, p.commit_sha, pr.repository_url, j.required_secrets, j.artifact_paths, \
                    pp.plan_sha256 \
             FROM job_queue q \
             JOIN jobs j ON j.id = q.job_id \
             JOIN execution_attempts a ON a.id = q.attempt_id \
             JOIN stages s ON s.id = j.stage_id \
             JOIN pipelines p ON p.id = s.pipeline_id \
             JOIN projects pr ON pr.id = p.project_id \
             LEFT JOIN pipeline_plans pp ON pp.pipeline_id = p.id \
             WHERE q.state = 'queued' \
               AND q.not_before <= now() \
               AND q.required_tags <@ $8::text[] \
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
             LIMIT 1 \
             FOR UPDATE OF q SKIP LOCKED \
         ), claimed_job AS ( \
             UPDATE jobs j \
             SET status = 'running', started_at = COALESCE(started_at, now()) \
             FROM candidate c \
             WHERE j.id = c.job_id \
               AND j.status = 'queued' \
             RETURNING j.id \
         ), claimed_attempt AS ( \
             UPDATE execution_attempts a \
             SET status = 'running', trigger = 'external_runner', started_at = COALESCE(started_at, now()) \
             FROM candidate c \
             WHERE a.id = c.attempt_id \
               AND a.status = 'queued' \
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
         ), claimed_queue AS ( \
             UPDATE job_queue q \
             SET state = 'leased', lease_id = cl.lease_id, leased_at = now(), updated_at = now() \
             FROM candidate c \
             CROSS JOIN created_lease cl \
             WHERE q.id = c.queue_id \
               AND q.state = 'queued' \
             RETURNING cl.lease_id, cl.generation, cl.lease_expires_at, cl.ack_deadline \
         ) \
         SELECT cq.lease_id, c.job_id, c.stage_id, ca.id AS attempt_id, ca.attempt_no, \
                c.pipeline_id, c.job_name, c.image, c.command, c.timeout_seconds, \
                c.git_ref, c.commit_sha, c.repository_url, c.required_secrets, c.artifact_paths, cq.generation, \
                cq.lease_expires_at, cq.ack_deadline, c.plan_sha256 \
         FROM candidate c \
         CROSS JOIN claimed_attempt ca \
         CROSS JOIN claimed_queue cq",
    )
    .bind(lease_id)
    .bind(runner.id)
    .bind(&runner.name)
    .bind(LEASE_TTL_SECONDS)
    .bind(&lease_token_hash)
    .bind(ACK_DEADLINE_SECONDS)
    .bind(PROTOCOL_VERSION)
    .bind(runner_tags)
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
            secrets: row.required_secrets,
            timeout_seconds: row.timeout_seconds,
            workspace: RunnerWorkspace {
                checkout: true,
                checkout_url: Some(row.repository_url),
            },
            artifacts: row.artifact_paths,
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
        "SELECT id, name, tags, status, draining, capabilities \
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

fn required_text_header(
    headers: &HeaderMap,
    name: &'static str,
    message: &'static str,
) -> Result<String, ApiError> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| ApiError::bad_request(message))
}

fn required_i32_header(
    headers: &HeaderMap,
    name: &'static str,
    message: &'static str,
) -> Result<i32, ApiError> {
    required_text_header(headers, name, message)?
        .parse()
        .map_err(|_| ApiError::bad_request(message))
}

fn required_i64_header(
    headers: &HeaderMap,
    name: &'static str,
    message: &'static str,
) -> Result<i64, ApiError> {
    required_text_header(headers, name, message)?
        .parse()
        .map_err(|_| ApiError::bad_request(message))
}

fn required_uuid_header(
    headers: &HeaderMap,
    name: &'static str,
    message: &'static str,
) -> Result<Uuid, ApiError> {
    required_text_header(headers, name, message)?
        .parse()
        .map_err(|_| ApiError::bad_request(message))
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

fn validate_registration_token(value: &str, configured: Option<&str>) -> Result<(), ApiError> {
    let configured = configured
        .map(str::trim)
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
    normalize_tags(tags).map(|_| ())
}

fn normalize_tags(tags: &[String]) -> Result<Vec<String>, ApiError> {
    if tags.len() > MAX_TAGS {
        return Err(ApiError::bad_request(
            "runner tags must match ^[a-z0-9][a-z0-9._-]{0,62}$ and max 64 items",
        ));
    }
    let mut normalized = BTreeSet::new();
    for tag in tags {
        let tag = tag.trim();
        if !valid_tag(tag) {
            return Err(ApiError::bad_request(
                "runner tags must match ^[a-z0-9][a-z0-9._-]{0,62}$ and max 64 items",
            ));
        }
        normalized.insert(tag.to_string());
    }
    Ok(normalized.into_iter().collect())
}

fn poll_runner_tags(
    runner: &AuthenticatedRunner,
    requested_tags: &[String],
) -> Result<Vec<String>, ApiError> {
    let requested = normalize_tags(requested_tags)?;
    if requested.is_empty() {
        return Ok(runner.tags.clone());
    }
    let stored: BTreeSet<&str> = runner.tags.iter().map(String::as_str).collect();
    if requested
        .iter()
        .any(|requested| !stored.contains(requested.as_str()))
    {
        return Err(ApiError::bad_request(
            "poll tags must be a subset of current runner tags",
        ));
    }
    Ok(requested)
}

fn normalize_secret_names(names: &[String]) -> Result<Vec<String>, ApiError> {
    if names.is_empty() || names.len() > MAX_SECRET_NAMES {
        return Err(ApiError::bad_request(
            "secret names must match ^[A-Z][A-Z0-9_]{0,127}$ and require 1..64 items",
        ));
    }
    let mut normalized = BTreeSet::new();
    for name in names {
        let name = name.trim();
        if !valid_secret_name(name) {
            return Err(ApiError::bad_request(
                "secret names must match ^[A-Z][A-Z0-9_]{0,127}$ and require 1..64 items",
            ));
        }
        normalized.insert(name.to_string());
    }
    Ok(normalized.into_iter().collect())
}

fn valid_secret_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 128 {
        return false;
    }
    let first = bytes[0];
    if !first.is_ascii_uppercase() {
        return false;
    }
    bytes
        .iter()
        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || *b == b'_')
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
    let Some(object) = value.as_object() else {
        return Err(ApiError::bad_request("capabilities must be an object"));
    };

    if let Some(executor_kinds) = object.get("executorKinds") {
        let Some(executor_kinds) = executor_kinds.as_array() else {
            return Err(ApiError::bad_request(
                "capabilities.executorKinds must be an array",
            ));
        };
        if executor_kinds.len() > 16 {
            return Err(ApiError::bad_request(
                "capabilities.executorKinds must contain at most 16 items",
            ));
        }
        for kind in executor_kinds {
            let Some(kind) = kind.as_str().map(str::trim) else {
                return Err(ApiError::bad_request(
                    "capabilities.executorKinds must contain executor names",
                ));
            };
            if !valid_tag(kind) {
                return Err(ApiError::bad_request(
                    "capabilities.executorKinds must match ^[a-z0-9][a-z0-9._-]{0,62}$",
                ));
            }
        }
    }
    Ok(())
}

fn runner_supports_executor(capabilities: &serde_json::Value, executor: &str) -> bool {
    match capabilities.get("executorKinds") {
        None => true,
        Some(serde_json::Value::Array(kinds)) => kinds
            .iter()
            .filter_map(serde_json::Value::as_str)
            .any(|kind| kind.trim() == executor),
        Some(_) => false,
    }
}

fn default_capabilities() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
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
    if !(0..=POLL_WAIT_MAX_SECONDS).contains(&input.wait_seconds) {
        return Err(ApiError::bad_request(
            "wait_seconds must be between 0 and 30",
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
    fn runner_poll_tags_are_normalized_and_scoped() {
        let runner = AuthenticatedRunner {
            id: Uuid::nil(),
            name: "runner-1".to_string(),
            tags: vec!["docker".to_string(), "linux".to_string()],
            status: "online".to_string(),
            draining: false,
            capabilities: serde_json::json!({}),
        };

        assert_eq!(
            poll_runner_tags(&runner, &[]).unwrap(),
            vec!["docker".to_string(), "linux".to_string()]
        );
        assert_eq!(
            poll_runner_tags(&runner, &[" linux ".to_string()]).unwrap(),
            vec!["linux".to_string()]
        );
        assert!(poll_runner_tags(&runner, &["prod".to_string()]).is_err());
    }

    #[test]
    fn runner_poll_wait_defaults_and_is_bounded() {
        let default_wait: RunnerPollRequest = serde_json::from_value(serde_json::json!({
            "protocolVersion": 1,
            "capacity": {"freeSlots": 1}
        }))
        .unwrap();
        assert_eq!(default_wait.wait_seconds, 0);
        assert!(validate_poll_request(&default_wait).is_ok());

        let max_wait: RunnerPollRequest = serde_json::from_value(serde_json::json!({
            "protocolVersion": 1,
            "capacity": {"freeSlots": 1},
            "waitSeconds": 30
        }))
        .unwrap();
        assert!(validate_poll_request(&max_wait).is_ok());

        let too_long: RunnerPollRequest = serde_json::from_value(serde_json::json!({
            "protocolVersion": 1,
            "capacity": {"freeSlots": 1},
            "waitSeconds": 31
        }))
        .unwrap();
        assert!(validate_poll_request(&too_long).is_err());
    }

    #[test]
    fn runner_executor_capabilities_are_validated_and_matched() {
        assert!(runner_supports_executor(&serde_json::json!({}), "shell"));
        assert!(runner_supports_executor(
            &serde_json::json!({"executorKinds": ["shell", "docker"]}),
            "shell"
        ));
        assert!(!runner_supports_executor(
            &serde_json::json!({"executorKinds": ["docker"]}),
            "shell"
        ));
        assert!(
            validate_capabilities(&serde_json::json!({
                "executorKinds": ["shell", "docker-24"],
                "os": "linux"
            }))
            .is_ok()
        );
        assert!(validate_capabilities(&serde_json::json!([])).is_err());
        assert!(
            validate_capabilities(&serde_json::json!({
                "executorKinds": "shell"
            }))
            .is_err()
        );
        assert!(
            validate_capabilities(&serde_json::json!({
                "executorKinds": ["Shell"]
            }))
            .is_err()
        );
    }

    #[test]
    fn runner_secret_names_are_normalized_and_required() {
        assert_eq!(
            normalize_secret_names(&[
                " DEPLOY_TOKEN ".to_string(),
                "DEPLOY_TOKEN".to_string(),
                "AWS_ACCESS_KEY_ID".to_string(),
            ])
            .unwrap(),
            vec!["AWS_ACCESS_KEY_ID".to_string(), "DEPLOY_TOKEN".to_string()]
        );
        assert!(normalize_secret_names(&[]).is_err());
        assert!(normalize_secret_names(&["deploy_token".to_string()]).is_err());
        assert!(normalize_secret_names(&vec!["A".to_string(); 65]).is_err());
    }

    #[test]
    fn runner_capabilities_default_to_empty_object() {
        let register: RunnerRegisterRequest = serde_json::from_value(serde_json::json!({
            "protocolVersion": 1,
            "registrationToken": "registration-token",
            "name": "runner-1",
            "tags": ["linux"]
        }))
        .unwrap();
        assert_eq!(register.capabilities, serde_json::json!({}));
        assert!(validate_capabilities(&register.capabilities).is_ok());

        let heartbeat: RunnerHeartbeatRequest = serde_json::from_value(serde_json::json!({
            "protocolVersion": 1,
            "status": "online",
            "capacity": {"totalSlots": 1, "busySlots": 0}
        }))
        .unwrap();
        assert!(heartbeat.tags.is_none());
        assert_eq!(heartbeat.capabilities, serde_json::json!({}));
        assert!(validate_capabilities(&heartbeat.capabilities).is_ok());
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
