use std::{
    path::{Path as FsPath, PathBuf},
    sync::Arc,
};

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::{get, patch, post},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::{
    api::{ApiError, AppState, pool},
    body_limits,
};

pub(crate) const MAX_ARTIFACT_BYTES: usize = body_limits::ARTIFACT_UPLOAD_BYTES;
const DEFAULT_TOKEN_LIFETIME_DAYS: i32 = 30;
const MAX_TOKEN_LIFETIME_DAYS: i32 = 365;
const DEFAULT_TOKEN_SCOPES: &[&str] = &["api:read", "api:write", "git:read", "git:write"];

type ApiResult<T> = Result<Json<T>, ApiError>;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/runners", get(list_runners).post(register_runner))
        .route(
            "/api/v1/runners/{runner_id}",
            axum::routing::delete(delete_runner),
        )
        .route(
            "/api/v1/runners/{runner_id}/heartbeat",
            post(runner_heartbeat),
        )
        .route(
            "/api/v1/projects/{project_id}/secrets",
            get(list_secrets).post(create_secret),
        )
        .route(
            "/api/v1/secrets/{secret_id}",
            axum::routing::delete(delete_secret),
        )
        .route(
            "/api/v1/jobs/{job_id}/artifacts",
            get(list_artifacts)
                .post(upload_artifact)
                .layer(DefaultBodyLimit::max(body_limits::ARTIFACT_UPLOAD_BYTES)),
        )
        .route(
            "/api/v1/artifacts/{artifact_id}/download",
            get(download_artifact),
        )
        .route(
            "/api/v1/projects/{project_id}/environments",
            get(list_environments).post(create_environment),
        )
        .route(
            "/api/v1/environments/{environment_id}",
            patch(update_environment).delete(delete_environment),
        )
        .route(
            "/api/v1/environments/{environment_id}/deployments",
            get(list_deployments).post(create_deployment),
        )
        .route(
            "/api/v1/projects/{project_id}/schedules",
            get(list_schedules).post(create_schedule),
        )
        .route(
            "/api/v1/schedules/{schedule_id}",
            patch(update_schedule).delete(delete_schedule),
        )
        .route(
            "/api/v1/projects/{project_id}/webhooks",
            get(list_webhooks).post(create_webhook),
        )
        .route(
            "/api/v1/webhooks/{webhook_id}",
            axum::routing::delete(delete_webhook),
        )
        .route(
            "/api/v1/projects/{project_id}/outbox-deliveries",
            get(list_outbox_deliveries),
        )
        .route(
            "/api/v1/outbox-deliveries/{delivery_id}",
            get(get_outbox_delivery),
        )
        .route(
            "/api/v1/outbox-deliveries/{delivery_id}/requeue",
            post(requeue_outbox_delivery),
        )
        .route(
            "/api/v1/projects/{project_id}/notifications",
            get(list_notifications).put(replace_notifications),
        )
        .route(
            "/api/v1/projects/{project_id}/notification-events",
            get(list_notification_events),
        )
        .route(
            "/api/v1/projects/{project_id}/notifications/stream",
            get(notification_stream),
        )
        .route(
            "/api/v1/projects/{project_id}/reports/summary",
            get(project_report),
        )
        .route("/api/v1/audit-log", get(list_audit_log))
        .route("/api/v1/users", get(list_users).post(create_user))
        .route("/api/v1/users/{user_id}", patch(update_user))
        .route("/api/v1/api-tokens", get(list_tokens).post(create_token))
        .route(
            "/api/v1/api-tokens/{token_id}",
            axum::routing::delete(delete_token),
        )
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct Runner {
    id: Uuid,
    name: String,
    tags: Vec<String>,
    status: String,
    last_seen_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct RegisterRunner {
    name: String,
    #[serde(default)]
    tags: Vec<String>,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct RunnerHeartbeat {
    status: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/runners", tag = "runners", responses((status = 200, body = [Runner])))]
async fn list_runners(State(state): State<Arc<AppState>>) -> ApiResult<Vec<Runner>> {
    Ok(Json(sqlx::query_as("SELECT id, name, tags, status, last_seen_at, created_at FROM runners ORDER BY created_at DESC")
        .fetch_all(pool(&state)?).await.map_err(ApiError::internal)?))
}
#[utoipa::path(post, path = "/api/v1/runners", tag = "runners", request_body = RegisterRunner, responses((status = 200, body = Runner), (status = 400)))]
async fn register_runner(
    State(state): State<Arc<AppState>>,
    Json(input): Json<RegisterRunner>,
) -> ApiResult<Runner> {
    if input.name.trim().is_empty() {
        return Err(ApiError::bad_request("runner name is required"));
    }
    let runner = sqlx::query_as::<_, Runner>("INSERT INTO runners (id, name, tags, status, last_seen_at) VALUES ($1, $2, $3, 'online', now()) RETURNING id, name, tags, status, last_seen_at, created_at")
        .bind(Uuid::new_v4()).bind(input.name.trim()).bind(input.tags).fetch_one(pool(&state)?).await.map_err(ApiError::internal)?;
    audit(
        pool(&state)?,
        "runner.registered",
        "runner",
        runner.id,
        None,
    )
    .await?;
    Ok(Json(runner))
}
#[utoipa::path(post, path = "/api/v1/runners/{runner_id}/heartbeat", tag = "runners", request_body = RunnerHeartbeat, params(("runner_id" = Uuid, Path)), responses((status = 200, body = Runner), (status = 404)))]
async fn runner_heartbeat(
    State(state): State<Arc<AppState>>,
    Path(runner_id): Path<Uuid>,
    Json(input): Json<RunnerHeartbeat>,
) -> ApiResult<Runner> {
    let status = input.status.unwrap_or_else(|| "online".into());
    if !matches!(status.as_str(), "online" | "offline" | "paused") {
        return Err(ApiError::bad_request("invalid runner status"));
    }
    let runner = sqlx::query_as("UPDATE runners SET status = $2, last_seen_at = now() WHERE id = $1 RETURNING id, name, tags, status, last_seen_at, created_at")
        .bind(runner_id).bind(status).fetch_optional(pool(&state)?).await.map_err(ApiError::internal)?.ok_or_else(ApiError::not_found)?;
    audit(pool(&state)?, "runner.heartbeat", "runner", runner_id, None).await?;
    Ok(Json(runner))
}
#[utoipa::path(delete, path = "/api/v1/runners/{runner_id}", tag = "runners", params(("runner_id" = Uuid, Path)), responses((status = 200), (status = 404)))]
async fn delete_runner(
    State(state): State<Arc<AppState>>,
    Path(runner_id): Path<Uuid>,
) -> ApiResult<serde_json::Value> {
    let id: Uuid = sqlx::query_scalar("DELETE FROM runners WHERE id = $1 RETURNING id")
        .bind(runner_id)
        .fetch_optional(pool(&state)?)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    audit(pool(&state)?, "runner.deleted", "runner", id, None).await?;
    Ok(Json(serde_json::json!({"deleted": id})))
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct SecretMetadata {
    id: Uuid,
    project_id: Uuid,
    key: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct CreateSecret {
    key: String,
    value: String,
}
#[utoipa::path(get, path = "/api/v1/projects/{project_id}/secrets", tag = "secrets", params(("project_id" = Uuid, Path)), responses((status = 200, body = [SecretMetadata])))]
async fn list_secrets(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
) -> ApiResult<Vec<SecretMetadata>> {
    Ok(Json(sqlx::query_as("SELECT id, project_id, key, created_at, updated_at FROM project_secrets WHERE project_id = $1 ORDER BY key")
        .bind(project_id).fetch_all(pool(&state)?).await.map_err(ApiError::internal)?))
}
#[utoipa::path(post, path = "/api/v1/projects/{project_id}/secrets", tag = "secrets", request_body = CreateSecret, params(("project_id" = Uuid, Path)), responses((status = 200, body = SecretMetadata), (status = 400)))]
async fn create_secret(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<CreateSecret>,
) -> ApiResult<SecretMetadata> {
    if input.key.trim().is_empty() || input.value.is_empty() {
        return Err(ApiError::bad_request("secret key and value are required"));
    }
    let encrypted_value = encrypt_secret(&input.value).map_err(ApiError::bad_request)?;
    let secret = sqlx::query_as::<_, SecretMetadata>("INSERT INTO project_secrets (id, project_id, key, encrypted_value) VALUES ($1, $2, $3, $4) ON CONFLICT (project_id, key) DO UPDATE SET encrypted_value = EXCLUDED.encrypted_value, updated_at = now() RETURNING id, project_id, key, created_at, updated_at")
        .bind(Uuid::new_v4()).bind(project_id).bind(input.key.trim()).bind(encrypted_value).fetch_one(pool(&state)?).await.map_err(ApiError::internal)?;
    audit(
        pool(&state)?,
        "secret.upserted",
        "project_secret",
        secret.id,
        None,
    )
    .await?;
    Ok(Json(secret))
}
#[utoipa::path(delete, path = "/api/v1/secrets/{secret_id}", tag = "secrets", params(("secret_id" = Uuid, Path)), responses((status = 200), (status = 404)))]
async fn delete_secret(
    State(state): State<Arc<AppState>>,
    Path(secret_id): Path<Uuid>,
) -> ApiResult<serde_json::Value> {
    let id: Uuid = sqlx::query_scalar("DELETE FROM project_secrets WHERE id = $1 RETURNING id")
        .bind(secret_id)
        .fetch_optional(pool(&state)?)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    audit(pool(&state)?, "secret.deleted", "project_secret", id, None).await?;
    Ok(Json(serde_json::json!({"deleted": id})))
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct Artifact {
    id: Uuid,
    job_id: Uuid,
    attempt_id: Option<Uuid>,
    name: String,
    content_type: String,
    sha256: Option<String>,
    size_bytes: i64,
    created_at: DateTime<Utc>,
}
#[utoipa::path(get, path = "/api/v1/jobs/{job_id}/artifacts", tag = "artifacts", params(("job_id" = Uuid, Path)), responses((status = 200, body = [Artifact])))]
async fn list_artifacts(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
) -> ApiResult<Vec<Artifact>> {
    Ok(Json(sqlx::query_as("SELECT id, job_id, attempt_id, name, content_type, sha256, size_bytes, created_at FROM artifacts WHERE job_id = $1 ORDER BY created_at DESC")
        .bind(job_id).fetch_all(pool(&state)?).await.map_err(ApiError::internal)?))
}
#[utoipa::path(post, path = "/api/v1/jobs/{job_id}/artifacts", tag = "artifacts", params(("job_id" = Uuid, Path), ("X-Artifact-Name" = String, Header)), request_body = Vec<u8>, responses((status = 200, body = Artifact), (status = 400), (status = 413, description = "artifact body exceeds 50 MiB")))]
#[allow(clippy::too_many_arguments)]
async fn upload_artifact(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Artifact> {
    let name = headers
        .get("x-artifact-name")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("artifact.bin")
        .trim();
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream");
    let artifact =
        store_job_artifact(pool(&state)?, job_id, None, name, content_type, body).await?;
    Ok(Json(artifact))
}

pub(crate) async fn store_job_artifact(
    db: &PgPool,
    job_id: Uuid,
    attempt_id: Option<Uuid>,
    name: &str,
    content_type: &str,
    body: Bytes,
) -> Result<Artifact, ApiError> {
    if body.is_empty() || body.len() > MAX_ARTIFACT_BYTES {
        return Err(ApiError::bad_request(
            "artifact must be between 1 byte and 50 MiB",
        ));
    }
    let name = name.trim();
    if !valid_artifact_name(name) {
        return Err(ApiError::bad_request("invalid artifact name"));
    }
    let content_type = content_type.trim();
    if content_type.is_empty() {
        return Err(ApiError::bad_request("artifact content-type is required"));
    }
    let attempt_id = match attempt_id {
        Some(attempt_id) => attempt_id,
        None => crate::store::active_or_latest_attempt_id(db, job_id)
            .await
            .map_err(|error| match error {
                sqlx::Error::RowNotFound => ApiError::not_found(),
                other => ApiError::internal(other),
            })?,
    };
    let id = Uuid::new_v4();
    let artifact_sha256 = sha256_bytes(body.as_ref());
    let path = new_artifact_path(id)?;
    std::fs::write(&path, &body).map_err(io_error)?;
    let artifact = sqlx::query_as("INSERT INTO artifacts (id, job_id, attempt_id, name, storage_path, content_type, sha256, size_bytes) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id, job_id, attempt_id, name, content_type, sha256, size_bytes, created_at")
        .bind(id).bind(job_id).bind(attempt_id).bind(name).bind(path.to_string_lossy().as_ref()).bind(content_type).bind(&artifact_sha256).bind(body.len() as i64).fetch_one(db).await.map_err(ApiError::internal)?;
    audit(db, "artifact.uploaded", "artifact", id, None).await?;
    Ok(artifact)
}
#[utoipa::path(get, path = "/api/v1/artifacts/{artifact_id}/download", tag = "artifacts", params(("artifact_id" = Uuid, Path)), responses((status = 200, description = "Artifact download"), (status = 404)))]
async fn download_artifact(
    State(state): State<Arc<AppState>>,
    Path(artifact_id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let row: (String, String, String, Option<String>) = sqlx::query_as(
        "SELECT storage_path, name, content_type, sha256 FROM artifacts WHERE id = $1",
    )
    .bind(artifact_id)
    .fetch_optional(pool(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::not_found)?;
    let path = contained_artifact_path(&row.0)?;
    let bytes = std::fs::read(path).map_err(|_| ApiError::not_found())?;
    if let Some(expected) = row.3.as_deref() {
        let actual = sha256_bytes(&bytes);
        if actual != expected {
            return Err(ApiError::conflict("artifact checksum mismatch"));
        }
    }
    Ok((
        [
            ("content-type", row.2),
            ("content-disposition", artifact_content_disposition(&row.1)),
        ],
        bytes,
    )
        .into_response())
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct Environment {
    id: Uuid,
    project_id: Uuid,
    name: String,
    url: Option<String>,
    status: String,
    created_at: DateTime<Utc>,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct CreateEnvironment {
    name: String,
    url: Option<String>,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct UpdateEnvironment {
    name: Option<String>,
    url: Option<String>,
    status: Option<String>,
}
#[utoipa::path(get, path = "/api/v1/projects/{project_id}/environments", tag = "environments", params(("project_id" = Uuid, Path)), responses((status = 200, body = [Environment])))]
async fn list_environments(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
) -> ApiResult<Vec<Environment>> {
    Ok(Json(sqlx::query_as("SELECT id, project_id, name, url, status, created_at FROM environments WHERE project_id = $1 ORDER BY name").bind(project_id).fetch_all(pool(&state)?).await.map_err(ApiError::internal)?))
}
#[utoipa::path(post, path = "/api/v1/projects/{project_id}/environments", tag = "environments", request_body = CreateEnvironment, params(("project_id" = Uuid, Path)), responses((status = 200, body = Environment), (status = 400)))]
async fn create_environment(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<CreateEnvironment>,
) -> ApiResult<Environment> {
    if input.name.trim().is_empty() {
        return Err(ApiError::bad_request("environment name is required"));
    }
    let value = sqlx::query_as("INSERT INTO environments (id, project_id, name, url) VALUES ($1, $2, $3, $4) RETURNING id, project_id, name, url, status, created_at").bind(Uuid::new_v4()).bind(project_id).bind(input.name.trim()).bind(input.url).fetch_one(pool(&state)?).await.map_err(ApiError::internal)?;
    Ok(Json(value))
}
#[utoipa::path(patch, path = "/api/v1/environments/{environment_id}", tag = "environments", request_body = UpdateEnvironment, params(("environment_id" = Uuid, Path)), responses((status = 200, body = Environment), (status = 404)))]
async fn update_environment(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateEnvironment>,
) -> ApiResult<Environment> {
    if let Some(status) = &input.status {
        if !matches!(status.as_str(), "available" | "stopped" | "degraded") {
            return Err(ApiError::bad_request("invalid environment status"));
        }
    }
    let value = sqlx::query_as("UPDATE environments SET name = COALESCE($2, name), url = COALESCE($3, url), status = COALESCE($4, status) WHERE id = $1 RETURNING id, project_id, name, url, status, created_at").bind(id).bind(input.name.as_deref().map(str::trim)).bind(input.url).bind(input.status).fetch_optional(pool(&state)?).await.map_err(ApiError::internal)?.ok_or_else(ApiError::not_found)?;
    Ok(Json(value))
}
#[utoipa::path(delete, path = "/api/v1/environments/{environment_id}", tag = "environments", params(("environment_id" = Uuid, Path)), responses((status = 200), (status = 404)))]
async fn delete_environment(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> ApiResult<serde_json::Value> {
    let id: Uuid = sqlx::query_scalar("DELETE FROM environments WHERE id = $1 RETURNING id")
        .bind(id)
        .fetch_optional(pool(&state)?)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(serde_json::json!({"deleted": id})))
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct Deployment {
    id: Uuid,
    environment_id: Uuid,
    pipeline_id: Option<Uuid>,
    git_ref: String,
    status: String,
    created_at: DateTime<Utc>,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct CreateDeployment {
    git_ref: String,
    pipeline_id: Option<Uuid>,
    status: Option<String>,
}
#[utoipa::path(get, path = "/api/v1/environments/{environment_id}/deployments", tag = "environments", params(("environment_id" = Uuid, Path)), responses((status = 200, body = [Deployment])))]
async fn list_deployments(
    State(state): State<Arc<AppState>>,
    Path(environment_id): Path<Uuid>,
) -> ApiResult<Vec<Deployment>> {
    Ok(Json(sqlx::query_as("SELECT id, environment_id, pipeline_id, git_ref, status, created_at FROM deployments WHERE environment_id = $1 ORDER BY created_at DESC LIMIT 50").bind(environment_id).fetch_all(pool(&state)?).await.map_err(ApiError::internal)?))
}
#[utoipa::path(post, path = "/api/v1/environments/{environment_id}/deployments", tag = "environments", request_body = CreateDeployment, params(("environment_id" = Uuid, Path)), responses((status = 200, body = Deployment), (status = 400)))]
async fn create_deployment(
    State(state): State<Arc<AppState>>,
    Path(environment_id): Path<Uuid>,
    Json(input): Json<CreateDeployment>,
) -> ApiResult<Deployment> {
    if input.git_ref.trim().is_empty() {
        return Err(ApiError::bad_request("git_ref is required"));
    }
    let status = input.status.unwrap_or_else(|| "success".into());
    if !matches!(
        status.as_str(),
        "pending" | "running" | "success" | "failed"
    ) {
        return Err(ApiError::bad_request("invalid deployment status"));
    }
    let value = sqlx::query_as("INSERT INTO deployments (id, environment_id, pipeline_id, git_ref, status) VALUES ($1, $2, $3, $4, $5) RETURNING id, environment_id, pipeline_id, git_ref, status, created_at").bind(Uuid::new_v4()).bind(environment_id).bind(input.pipeline_id).bind(input.git_ref.trim()).bind(status).fetch_one(pool(&state)?).await.map_err(ApiError::internal)?;
    Ok(Json(value))
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct Schedule {
    id: Uuid,
    project_id: Uuid,
    cron: String,
    git_ref: String,
    enabled: bool,
    next_fire_at: Option<DateTime<Utc>>,
    last_fired_at: Option<DateTime<Utc>>,
    last_fire_error: Option<String>,
    created_at: DateTime<Utc>,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct ScheduleInput {
    cron: String,
    git_ref: String,
    enabled: Option<bool>,
}
#[utoipa::path(get, path = "/api/v1/projects/{project_id}/schedules", tag = "schedules", params(("project_id" = Uuid, Path)), responses((status = 200, body = [Schedule])))]
async fn list_schedules(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
) -> ApiResult<Vec<Schedule>> {
    Ok(Json(sqlx::query_as("SELECT id, project_id, cron, git_ref, enabled, next_fire_at, last_fired_at, last_fire_error, created_at FROM schedules WHERE project_id = $1 ORDER BY created_at DESC").bind(project_id).fetch_all(pool(&state)?).await.map_err(ApiError::internal)?))
}
#[utoipa::path(post, path = "/api/v1/projects/{project_id}/schedules", tag = "schedules", request_body = ScheduleInput, params(("project_id" = Uuid, Path)), responses((status = 200, body = Schedule), (status = 400)))]
async fn create_schedule(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<ScheduleInput>,
) -> ApiResult<Schedule> {
    let cron = input.cron.trim();
    let git_ref = input.git_ref.trim();
    let enabled = input.enabled.unwrap_or(true);
    if git_ref.is_empty() {
        return Err(ApiError::bad_request("git_ref is required"));
    }
    let next_fire_at = schedule_next_fire_at(cron, enabled)?;
    Ok(Json(sqlx::query_as("INSERT INTO schedules (id, project_id, cron, git_ref, enabled, next_fire_at) VALUES ($1, $2, $3, $4, $5, $6) RETURNING id, project_id, cron, git_ref, enabled, next_fire_at, last_fired_at, last_fire_error, created_at").bind(Uuid::new_v4()).bind(project_id).bind(cron).bind(git_ref).bind(enabled).bind(next_fire_at).fetch_one(pool(&state)?).await.map_err(ApiError::internal)?))
}
#[utoipa::path(patch, path = "/api/v1/schedules/{schedule_id}", tag = "schedules", request_body = ScheduleInput, params(("schedule_id" = Uuid, Path)), responses((status = 200, body = Schedule), (status = 400), (status = 404)))]
async fn update_schedule(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(input): Json<ScheduleInput>,
) -> ApiResult<Schedule> {
    let cron = input.cron.trim();
    let git_ref = input.git_ref.trim();
    let enabled = input.enabled.unwrap_or(true);
    if git_ref.is_empty() {
        return Err(ApiError::bad_request("git_ref is required"));
    }
    let next_fire_at = schedule_next_fire_at(cron, enabled)?;
    Ok(Json(sqlx::query_as("UPDATE schedules SET cron = $2, git_ref = $3, enabled = $4, next_fire_at = $5, last_fire_error = NULL WHERE id = $1 RETURNING id, project_id, cron, git_ref, enabled, next_fire_at, last_fired_at, last_fire_error, created_at").bind(id).bind(cron).bind(git_ref).bind(enabled).bind(next_fire_at).fetch_optional(pool(&state)?).await.map_err(ApiError::internal)?.ok_or_else(ApiError::not_found)?))
}
#[utoipa::path(delete, path = "/api/v1/schedules/{schedule_id}", tag = "schedules", params(("schedule_id" = Uuid, Path)), responses((status = 200), (status = 404)))]
async fn delete_schedule(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> ApiResult<serde_json::Value> {
    let id: Uuid = sqlx::query_scalar("DELETE FROM schedules WHERE id = $1 RETURNING id")
        .bind(id)
        .fetch_optional(pool(&state)?)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(serde_json::json!({"deleted": id})))
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct Webhook {
    id: Uuid,
    project_id: Uuid,
    url: String,
    events: Vec<String>,
    enabled: bool,
    created_at: DateTime<Utc>,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct CreateWebhook {
    url: String,
    #[serde(default)]
    events: Vec<String>,
    enabled: Option<bool>,
    /// Optional HMAC-SHA256 signing secret; deliveries carry
    /// `X-Forge-Signature: sha256=<hex>`.
    #[serde(default)]
    secret: Option<String>,
}
#[utoipa::path(get, path = "/api/v1/projects/{project_id}/webhooks", tag = "webhooks", params(("project_id" = Uuid, Path)), responses((status = 200, body = [Webhook])))]
async fn list_webhooks(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
) -> ApiResult<Vec<Webhook>> {
    Ok(Json(sqlx::query_as("SELECT id, project_id, url, events, enabled, created_at FROM webhooks WHERE project_id = $1 ORDER BY created_at DESC").bind(project_id).fetch_all(pool(&state)?).await.map_err(ApiError::internal)?))
}
#[utoipa::path(post, path = "/api/v1/projects/{project_id}/webhooks", tag = "webhooks", request_body = CreateWebhook, params(("project_id" = Uuid, Path)), responses((status = 200, body = Webhook), (status = 400)))]
async fn create_webhook(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<CreateWebhook>,
) -> ApiResult<Webhook> {
    if !input.url.starts_with("http://") && !input.url.starts_with("https://") {
        return Err(ApiError::bad_request("webhook url must be http(s)"));
    }
    Ok(Json(sqlx::query_as("INSERT INTO webhooks (id, project_id, url, events, enabled, secret) VALUES ($1, $2, $3, $4, $5, $6) RETURNING id, project_id, url, events, enabled, created_at").bind(Uuid::new_v4()).bind(project_id).bind(input.url.trim()).bind(input.events).bind(input.enabled.unwrap_or(true)).bind(input.secret.as_deref().map(str::trim).filter(|s| !s.is_empty())).fetch_one(pool(&state)?).await.map_err(ApiError::internal)?))
}
#[utoipa::path(delete, path = "/api/v1/webhooks/{webhook_id}", tag = "webhooks", params(("webhook_id" = Uuid, Path)), responses((status = 200), (status = 404)))]
async fn delete_webhook(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> ApiResult<serde_json::Value> {
    let id: Uuid = sqlx::query_scalar("DELETE FROM webhooks WHERE id = $1 RETURNING id")
        .bind(id)
        .fetch_optional(pool(&state)?)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(serde_json::json!({"deleted": id})))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub(crate) struct OutboxDeliveriesParams {
    /// Maximum number of recent deliveries to return.
    limit: Option<i64>,
    /// Optional status filter: pending, retry_scheduled, delivered or failed.
    status: Option<String>,
    /// Optional channel filter: webhook, notification or sse.
    channel: Option<String>,
}
impl OutboxDeliveriesParams {
    fn bounded_limit(&self) -> Result<i64, ApiError> {
        let limit = self.limit.unwrap_or(50);
        if !(1..=200).contains(&limit) {
            return Err(ApiError::bad_request("limit must be between 1 and 200"));
        }
        Ok(limit)
    }

    fn status_filter(&self) -> Result<Option<String>, ApiError> {
        optional_allowlisted(
            &self.status,
            &["pending", "retry_scheduled", "delivered", "failed"],
        )
    }

    fn channel_filter(&self) -> Result<Option<String>, ApiError> {
        optional_allowlisted(&self.channel, &["webhook", "notification", "sse"])
    }
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct OutboxDelivery {
    id: Uuid,
    project_id: Option<Uuid>,
    event_id: Uuid,
    replay_of_id: Option<Uuid>,
    generation: i32,
    subscription_id: String,
    channel: String,
    destination: String,
    event_type: String,
    aggregate_type: String,
    aggregate_id: Uuid,
    status: String,
    attempts: i32,
    next_attempt_at: DateTime<Utc>,
    delivered_at: Option<DateTime<Utc>>,
    failed_at: Option<DateTime<Utc>>,
    last_error: Option<String>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct OutboxDeliveryAttempt {
    id: i64,
    message_id: Uuid,
    attempt_number: i32,
    started_at: DateTime<Utc>,
    finished_at: DateTime<Utc>,
    outcome: String,
    http_status: Option<i32>,
    error_message: Option<String>,
    duration_ms: i32,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct OutboxDeliveryDetail {
    delivery: OutboxDelivery,
    attempts: Vec<OutboxDeliveryAttempt>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct RequeuedOutboxDelivery {
    id: Uuid,
    replay_of_id: Uuid,
}

#[utoipa::path(get, path = "/api/v1/projects/{project_id}/outbox-deliveries", tag = "outbox", params(("project_id" = Uuid, Path), OutboxDeliveriesParams), responses((status = 200, body = [OutboxDelivery]), (status = 400)))]
pub(crate) async fn list_outbox_deliveries(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    Query(params): Query<OutboxDeliveriesParams>,
) -> ApiResult<Vec<OutboxDelivery>> {
    Ok(Json(
        project_outbox_deliveries(
            pool(&state)?,
            project_id,
            params.bounded_limit()?,
            params.status_filter()?,
            params.channel_filter()?,
        )
        .await
        .map_err(ApiError::internal)?,
    ))
}

#[utoipa::path(get, path = "/api/v1/outbox-deliveries/{delivery_id}", tag = "outbox", params(("delivery_id" = Uuid, Path)), responses((status = 200, body = OutboxDeliveryDetail), (status = 404)))]
pub(crate) async fn get_outbox_delivery(
    State(state): State<Arc<AppState>>,
    Path(delivery_id): Path<Uuid>,
) -> ApiResult<OutboxDeliveryDetail> {
    let db = pool(&state)?;
    let delivery = outbox_delivery_by_id(db, delivery_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    let attempts = sqlx::query_as::<_, OutboxDeliveryAttempt>(
        "SELECT id, message_id, attempt_number, started_at, finished_at, outcome, \
            http_status, error_message, duration_ms, created_at \
         FROM outbox_delivery_attempts \
         WHERE message_id = $1 \
         ORDER BY attempt_number DESC",
    )
    .bind(delivery_id)
    .fetch_all(db)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(OutboxDeliveryDetail { delivery, attempts }))
}

#[utoipa::path(post, path = "/api/v1/outbox-deliveries/{delivery_id}/requeue", tag = "outbox", params(("delivery_id" = Uuid, Path)), responses((status = 200, body = RequeuedOutboxDelivery), (status = 400), (status = 404)))]
pub(crate) async fn requeue_outbox_delivery(
    State(state): State<Arc<AppState>>,
    Path(delivery_id): Path<Uuid>,
) -> ApiResult<RequeuedOutboxDelivery> {
    let db = pool(&state)?;
    match crate::outbox::requeue_failed_delivery(db, delivery_id)
        .await
        .map_err(ApiError::internal)?
    {
        Ok(id) => {
            audit(db, "outbox.requeue", "outbox_delivery", id, None).await?;
            Ok(Json(RequeuedOutboxDelivery {
                id,
                replay_of_id: delivery_id,
            }))
        }
        Err(crate::outbox::RequeueDeliveryError::NotFound) => Err(ApiError::not_found()),
        Err(crate::outbox::RequeueDeliveryError::NotFailed) => Err(ApiError::bad_request(
            "only failed deliveries can be requeued",
        )),
    }
}

fn optional_allowlisted(
    value: &Option<String>,
    allowed: &[&str],
) -> Result<Option<String>, ApiError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim().to_ascii_lowercase();
    if allowed.contains(&value.as_str()) {
        Ok(Some(value))
    } else {
        Err(ApiError::bad_request("unsupported filter value"))
    }
}

async fn project_outbox_deliveries(
    pool: &PgPool,
    project_id: Uuid,
    limit: i64,
    status: Option<String>,
    channel: Option<String>,
) -> Result<Vec<OutboxDelivery>, sqlx::Error> {
    sqlx::query_as::<_, OutboxDelivery>(
        "SELECT * FROM ( \
            SELECT m.id, m.project_id, m.event_id, m.replay_of_id, m.generation, \
                m.subscription_id, m.channel, m.destination, e.event_type, e.aggregate_type, e.aggregate_id, \
                CASE \
                    WHEN m.delivered_at IS NOT NULL THEN 'delivered' \
                    WHEN m.failed_at IS NOT NULL THEN 'failed' \
                    WHEN m.attempts > 0 THEN 'retry_scheduled' \
                    ELSE 'pending' \
                END AS status, \
                m.attempts, m.next_attempt_at, m.delivered_at, m.failed_at, m.last_error, m.created_at \
             FROM outbox_messages m \
             JOIN domain_events e ON e.id = m.event_id \
             WHERE m.project_id = $1 AND ($2::text IS NULL OR m.channel = $2) \
         ) deliveries \
         WHERE $3::text IS NULL OR status = $3 \
         ORDER BY created_at DESC, id DESC LIMIT $4",
    )
    .bind(project_id)
    .bind(channel)
    .bind(status)
    .bind(limit)
    .fetch_all(pool)
    .await
}

async fn outbox_delivery_by_id(
    pool: &PgPool,
    delivery_id: Uuid,
) -> Result<Option<OutboxDelivery>, sqlx::Error> {
    sqlx::query_as::<_, OutboxDelivery>(
        "SELECT m.id, m.project_id, m.event_id, m.replay_of_id, m.generation, \
            m.subscription_id, m.channel, m.destination, e.event_type, e.aggregate_type, e.aggregate_id, \
            CASE \
                WHEN m.delivered_at IS NOT NULL THEN 'delivered' \
                WHEN m.failed_at IS NOT NULL THEN 'failed' \
                WHEN m.attempts > 0 THEN 'retry_scheduled' \
                ELSE 'pending' \
            END AS status, \
            m.attempts, m.next_attempt_at, m.delivered_at, m.failed_at, m.last_error, m.created_at \
         FROM outbox_messages m \
         JOIN domain_events e ON e.id = m.event_id \
         WHERE m.id = $1",
    )
    .bind(delivery_id)
    .fetch_optional(pool)
    .await
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct Notification {
    id: Uuid,
    project_id: Uuid,
    channel: String,
    target: String,
    enabled: bool,
    created_at: DateTime<Utc>,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct NotificationInput {
    channel: String,
    target: String,
    enabled: Option<bool>,
}
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub(crate) struct NotificationEventsParams {
    /// Maximum number of recent notification events to return.
    limit: Option<i64>,
}
impl NotificationEventsParams {
    fn bounded_limit(&self) -> Result<i64, ApiError> {
        let limit = self.limit.unwrap_or(50);
        if !(1..=200).contains(&limit) {
            return Err(ApiError::bad_request("limit must be between 1 and 200"));
        }
        Ok(limit)
    }
}
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct NotificationEvent {
    id: Uuid,
    event_id: Uuid,
    subscription_id: String,
    channel: String,
    target: String,
    event_type: String,
    pipeline_id: Uuid,
    status: String,
    message: String,
    attempts: i32,
    delivered_at: Option<DateTime<Utc>>,
    last_error: Option<String>,
    created_at: DateTime<Utc>,
}
#[utoipa::path(get, path = "/api/v1/projects/{project_id}/notifications", tag = "notifications", params(("project_id" = Uuid, Path)), responses((status = 200, body = [Notification])))]
async fn list_notifications(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
) -> ApiResult<Vec<Notification>> {
    Ok(Json(sqlx::query_as("SELECT id, project_id, channel, target, enabled, created_at FROM notification_configs WHERE project_id = $1 ORDER BY channel").bind(project_id).fetch_all(pool(&state)?).await.map_err(ApiError::internal)?))
}
#[utoipa::path(put, path = "/api/v1/projects/{project_id}/notifications", tag = "notifications", request_body = [NotificationInput], params(("project_id" = Uuid, Path)), responses((status = 200, body = [Notification]), (status = 400)))]
async fn replace_notifications(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    Json(inputs): Json<Vec<NotificationInput>>,
) -> ApiResult<Vec<Notification>> {
    let db = pool(&state)?;
    sqlx::query("DELETE FROM notification_configs WHERE project_id = $1")
        .bind(project_id)
        .execute(db)
        .await
        .map_err(ApiError::internal)?;
    for input in inputs {
        if input.channel.trim().is_empty() || input.target.trim().is_empty() {
            return Err(ApiError::bad_request(
                "notification channel and target are required",
            ));
        }
        sqlx::query("INSERT INTO notification_configs (id, project_id, channel, target, enabled) VALUES ($1, $2, $3, $4, $5)").bind(Uuid::new_v4()).bind(project_id).bind(input.channel.trim()).bind(input.target.trim()).bind(input.enabled.unwrap_or(true)).execute(db).await.map_err(ApiError::internal)?;
    }
    list_notifications(State(state), Path(project_id)).await
}

#[utoipa::path(get, path = "/api/v1/projects/{project_id}/notification-events", tag = "notifications", params(("project_id" = Uuid, Path), NotificationEventsParams), responses((status = 200, body = [NotificationEvent]), (status = 400)))]
pub(crate) async fn list_notification_events(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    Query(params): Query<NotificationEventsParams>,
) -> ApiResult<Vec<NotificationEvent>> {
    let limit = params.bounded_limit()?;
    Ok(Json(
        recent_notification_events(pool(&state)?, project_id, limit)
            .await
            .map_err(ApiError::internal)?,
    ))
}

#[utoipa::path(get, path = "/api/v1/projects/{project_id}/notifications/stream", tag = "notifications", params(("project_id" = Uuid, Path)), responses((status = 200, description = "text/event-stream of project notification events")))]
pub(crate) async fn notification_stream(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
) -> Result<
    axum::response::Sse<
        tokio_stream::wrappers::UnboundedReceiverStream<
            Result<axum::response::sse::Event, std::convert::Infallible>,
        >,
    >,
    ApiError,
> {
    let pool = pool(&state)?.clone();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut after_created_at = Utc::now();
        let mut after_id = Uuid::nil();
        loop {
            if sender.is_closed() {
                return;
            }
            let rows = notification_events_after(&pool, project_id, after_created_at, after_id, 50)
                .await
                .unwrap_or_default();
            for event in rows {
                after_created_at = event.created_at;
                after_id = event.id;
                let data = serde_json::to_string(&event).unwrap_or_default();
                if sender
                    .send(Ok(axum::response::sse::Event::default()
                        .event("notification")
                        .id(event.id.to_string())
                        .data(data)))
                    .is_err()
                {
                    return;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        }
    });
    Ok(axum::response::Sse::new(
        tokio_stream::wrappers::UnboundedReceiverStream::new(receiver),
    ))
}

async fn recent_notification_events(
    pool: &PgPool,
    project_id: Uuid,
    limit: i64,
) -> Result<Vec<NotificationEvent>, sqlx::Error> {
    sqlx::query_as::<_, NotificationEvent>(
        "SELECT id, event_id, subscription_id, \
            COALESCE(payload->>'channel', '') AS channel, \
            COALESCE(payload->>'target', '') AS target, \
            COALESCE(payload->>'event', '') AS event_type, \
            (payload->>'pipeline_id')::uuid AS pipeline_id, \
            COALESCE(payload->>'status', '') AS status, \
            COALESCE(payload->>'message', '') AS message, \
            attempts, delivered_at, last_error, created_at \
         FROM outbox_messages \
         WHERE channel = 'notification' AND destination = $1 \
         ORDER BY created_at DESC, id DESC LIMIT $2",
    )
    .bind(crate::outbox::notification_destination(project_id))
    .bind(limit)
    .fetch_all(pool)
    .await
}

async fn notification_events_after(
    pool: &PgPool,
    project_id: Uuid,
    after_created_at: DateTime<Utc>,
    after_id: Uuid,
    limit: i64,
) -> Result<Vec<NotificationEvent>, sqlx::Error> {
    sqlx::query_as::<_, NotificationEvent>(
        "SELECT id, event_id, subscription_id, \
            COALESCE(payload->>'channel', '') AS channel, \
            COALESCE(payload->>'target', '') AS target, \
            COALESCE(payload->>'event', '') AS event_type, \
            (payload->>'pipeline_id')::uuid AS pipeline_id, \
            COALESCE(payload->>'status', '') AS status, \
            COALESCE(payload->>'message', '') AS message, \
            attempts, delivered_at, last_error, created_at \
         FROM outbox_messages \
         WHERE channel = 'notification' AND destination = $1 \
           AND (created_at, id) > ($2, $3) \
         ORDER BY created_at ASC, id ASC LIMIT $4",
    )
    .bind(crate::outbox::notification_destination(project_id))
    .bind(after_created_at)
    .bind(after_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct Report {
    total_pipelines: i64,
    successful_pipelines: i64,
    failed_pipelines: i64,
    success_rate: f64,
    average_duration_seconds: f64,
}
#[utoipa::path(get, path = "/api/v1/projects/{project_id}/reports/summary", tag = "reports", params(("project_id" = Uuid, Path)), responses((status = 200, body = Report)))]
async fn project_report(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
) -> ApiResult<Report> {
    Ok(Json(sqlx::query_as("SELECT count(*)::bigint AS total_pipelines, count(*) FILTER (WHERE status = 'success')::bigint AS successful_pipelines, count(*) FILTER (WHERE status = 'failed')::bigint AS failed_pipelines, COALESCE(count(*) FILTER (WHERE status = 'success')::float8 / NULLIF(count(*) FILTER (WHERE status IN ('success', 'failed', 'canceled')), 0), 0)::float8 AS success_rate, COALESCE(avg(EXTRACT(EPOCH FROM (finished_at - started_at))) FILTER (WHERE finished_at IS NOT NULL AND started_at IS NOT NULL), 0)::float8 AS average_duration_seconds FROM pipelines WHERE project_id = $1").bind(project_id).fetch_one(pool(&state)?).await.map_err(ApiError::internal)?))
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct AuditEvent {
    id: i64,
    action: String,
    resource_type: String,
    resource_id: Option<Uuid>,
    actor: Option<String>,
    created_at: DateTime<Utc>,
}
#[utoipa::path(get, path = "/api/v1/audit-log", tag = "audit", responses((status = 200, body = [AuditEvent])))]
async fn list_audit_log(State(state): State<Arc<AppState>>) -> ApiResult<Vec<AuditEvent>> {
    Ok(Json(sqlx::query_as("SELECT id, action, resource_type, resource_id, actor, created_at FROM audit_log ORDER BY created_at DESC LIMIT 200").fetch_all(pool(&state)?).await.map_err(ApiError::internal)?))
}
pub(crate) async fn audit(
    db: &PgPool,
    action: &str,
    resource_type: &str,
    resource_id: Uuid,
    actor: Option<&str>,
) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO audit_log (action, resource_type, resource_id, actor) VALUES ($1, $2, $3, $4)",
    )
    .bind(action)
    .bind(resource_type)
    .bind(resource_id)
    .bind(actor)
    .execute(db)
    .await
    .map_err(ApiError::internal)?;
    Ok(())
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct User {
    id: Uuid,
    username: String,
    role: String,
    enabled: bool,
    created_at: DateTime<Utc>,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct UserInput {
    username: String,
    role: String,
    enabled: Option<bool>,
    /// Optional argon2id password; enables interactive login (AUTHZ_CONTRACT).
    password: Option<String>,
}
#[utoipa::path(get, path = "/api/v1/users", tag = "users", responses((status = 200, body = [User])))]
async fn list_users(State(state): State<Arc<AppState>>) -> ApiResult<Vec<User>> {
    Ok(Json(
        sqlx::query_as(
            "SELECT id, username, role, enabled, created_at FROM users ORDER BY username",
        )
        .fetch_all(pool(&state)?)
        .await
        .map_err(ApiError::internal)?,
    ))
}
#[utoipa::path(post, path = "/api/v1/users", tag = "users", request_body = UserInput, responses((status = 200, body = User), (status = 400)))]
async fn create_user(
    State(state): State<Arc<AppState>>,
    Json(input): Json<UserInput>,
) -> ApiResult<User> {
    if input.username.trim().is_empty() || !valid_role(&input.role) {
        return Err(ApiError::bad_request(
            "username and role (admin, maintainer, developer, viewer) are required",
        ));
    }
    let pool = pool(&state)?;
    let user: User = sqlx::query_as("INSERT INTO users (id, username, role, enabled) VALUES ($1, $2, $3, $4) RETURNING id, username, role, enabled, created_at").bind(Uuid::new_v4()).bind(input.username.trim()).bind(input.role).bind(input.enabled.unwrap_or(true)).fetch_one(pool).await.map_err(ApiError::internal)?;
    if let Some(password) = input.password.as_deref().filter(|p| !p.is_empty()) {
        let hash = crate::auth::hash_password(password)
            .map_err(|_| ApiError::bad_request("password hashing failed"))?;
        sqlx::query("INSERT INTO user_credentials (user_id, password_hash) VALUES ($1, $2)")
            .bind(user.id)
            .bind(hash)
            .execute(pool)
            .await
            .map_err(ApiError::internal)?;
    }
    Ok(Json(user))
}
#[utoipa::path(patch, path = "/api/v1/users/{user_id}", tag = "users", request_body = UserInput, params(("user_id" = Uuid, Path)), responses((status = 200, body = User), (status = 400), (status = 404)))]
async fn update_user(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(input): Json<UserInput>,
) -> ApiResult<User> {
    if input.username.trim().is_empty() || !valid_role(&input.role) {
        return Err(ApiError::bad_request("username and role are required"));
    }
    let pool = pool(&state)?;
    if let Some(password) = input.password.as_deref().filter(|p| !p.is_empty()) {
        let hash = crate::auth::hash_password(password)
            .map_err(|_| ApiError::bad_request("password hashing failed"))?;
        sqlx::query("INSERT INTO user_credentials (user_id, password_hash) VALUES ($1, $2) ON CONFLICT (user_id) DO UPDATE SET password_hash = EXCLUDED.password_hash, updated_at = now()")
            .bind(id)
            .bind(hash)
            .execute(pool)
            .await
            .map_err(ApiError::internal)?;
    }
    Ok(Json(sqlx::query_as("UPDATE users SET username = $2, role = $3, enabled = $4 WHERE id = $1 RETURNING id, username, role, enabled, created_at").bind(id).bind(input.username.trim()).bind(input.role).bind(input.enabled.unwrap_or(true)).fetch_optional(pool).await.map_err(ApiError::internal)?.ok_or_else(ApiError::not_found)?))
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct ApiToken {
    id: Uuid,
    name: String,
    token_hint: String,
    user_id: Option<Uuid>,
    project_id: Option<Uuid>,
    scopes: Vec<String>,
    expires_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    last_used_at: Option<DateTime<Utc>>,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct CreatedToken {
    #[serde(flatten)]
    token: ApiToken,
    value: String,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct CreateToken {
    name: String,
    user_id: Option<Uuid>,
    /// Required in auth mode; omitted only for trusted-network legacy tokens.
    #[serde(default)]
    project_id: Option<Uuid>,
    /// Explicit token scopes. Defaults to project API/Git read-write scopes.
    #[serde(default = "default_token_scope_strings")]
    scopes: Vec<String>,
    /// Optional lifetime in days; defaults to 30 days in auth mode.
    #[serde(default)]
    expires_in_days: Option<i32>,
}
#[utoipa::path(get, path = "/api/v1/api-tokens", tag = "tokens", responses((status = 200, body = [ApiToken])))]
async fn list_tokens(State(state): State<Arc<AppState>>) -> ApiResult<Vec<ApiToken>> {
    Ok(Json(sqlx::query_as("SELECT id, name, token_hint, user_id, project_id, scopes, expires_at, revoked_at, created_at, last_used_at FROM api_tokens WHERE revoked_at IS NULL ORDER BY created_at DESC").fetch_all(pool(&state)?).await.map_err(ApiError::internal)?))
}
#[utoipa::path(post, path = "/api/v1/api-tokens", tag = "tokens", request_body = CreateToken, responses((status = 200, body = CreatedToken), (status = 400)))]
async fn create_token(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::auth::AccessClaims>>,
    Json(input): Json<CreateToken>,
) -> ApiResult<CreatedToken> {
    let auth_enabled = state.auth_secret.is_some();
    let requester = claims.map(|c| c.0);
    let user_id = input
        .user_id
        .or_else(|| requester.as_ref().map(|claims| claims.sub));
    if input.name.trim().is_empty() {
        return Err(ApiError::bad_request("token name is required"));
    }
    if auth_enabled && input.project_id.is_none() {
        return Err(ApiError::bad_request(
            "project_id is required for scoped tokens",
        ));
    }
    let scopes = normalize_token_scopes(input.scopes)?;
    let db = pool(&state)?;
    validate_token_owner_and_project(db, user_id, input.project_id).await?;
    let value = format!(
        "cicd_{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    );
    let token_hash = sha256(&value);
    let hint = format!("{}...{}", &value[..9], &value[value.len() - 4..]);
    let expires_at = token_expires_at(input.expires_in_days, auth_enabled)?;
    let token = sqlx::query_as::<_, ApiToken>("INSERT INTO api_tokens (id, name, token_hash, token_hint, user_id, project_id, scopes, expires_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id, name, token_hint, user_id, project_id, scopes, expires_at, revoked_at, created_at, last_used_at")
        .bind(Uuid::new_v4())
        .bind(input.name.trim())
        .bind(token_hash)
        .bind(hint)
        .bind(user_id)
        .bind(input.project_id)
        .bind(scopes)
        .bind(expires_at)
        .fetch_one(db)
        .await
        .map_err(ApiError::internal)?;
    audit(db, "token.created", "api_token", token.id, None).await?;
    Ok(Json(CreatedToken { token, value }))
}
#[utoipa::path(delete, path = "/api/v1/api-tokens/{token_id}", tag = "tokens", params(("token_id" = Uuid, Path)), responses((status = 200), (status = 404)))]
async fn delete_token(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> ApiResult<serde_json::Value> {
    let id: Uuid = sqlx::query_scalar(
        "UPDATE api_tokens SET revoked_at = now() \
         WHERE id = $1 AND revoked_at IS NULL RETURNING id",
    )
    .bind(id)
    .fetch_optional(pool(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::not_found)?;
    audit(pool(&state)?, "token.revoked", "api_token", id, None).await?;
    Ok(Json(serde_json::json!({"deleted": id})))
}

fn default_token_scope_strings() -> Vec<String> {
    DEFAULT_TOKEN_SCOPES
        .iter()
        .map(|scope| (*scope).to_string())
        .collect()
}

fn normalize_token_scopes(scopes: Vec<String>) -> Result<Vec<String>, ApiError> {
    let mut normalized = Vec::new();
    for scope in scopes {
        let scope = scope.trim();
        if scope.is_empty() {
            continue;
        }
        if !DEFAULT_TOKEN_SCOPES.contains(&scope) {
            return Err(ApiError::bad_request("unsupported token scope"));
        }
        if !normalized.iter().any(|existing| existing == scope) {
            normalized.push(scope.to_string());
        }
    }
    if normalized.is_empty() {
        return Err(ApiError::bad_request(
            "at least one token scope is required",
        ));
    }
    Ok(normalized)
}

fn token_expires_at(
    expires_in_days: Option<i32>,
    auth_enabled: bool,
) -> Result<Option<DateTime<Utc>>, ApiError> {
    let Some(days) = expires_in_days.or(auth_enabled.then_some(DEFAULT_TOKEN_LIFETIME_DAYS)) else {
        return Ok(None);
    };
    if !(1..=MAX_TOKEN_LIFETIME_DAYS).contains(&days) {
        return Err(ApiError::bad_request(
            "expires_in_days must be between 1 and 365",
        ));
    }
    Ok(Some(
        chrono::Utc::now() + chrono::Duration::days(days as i64),
    ))
}

async fn validate_token_owner_and_project(
    pool: &PgPool,
    user_id: Option<Uuid>,
    project_id: Option<Uuid>,
) -> Result<(), ApiError> {
    let Some(project_id) = project_id else {
        return Ok(());
    };
    let exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM projects WHERE id = $1)")
            .bind(project_id)
            .fetch_one(pool)
            .await
            .map_err(ApiError::internal)?;
    if !exists {
        return Err(ApiError::not_found());
    }
    let Some(user_id) = user_id else {
        return Err(ApiError::bad_request(
            "user_id is required for scoped tokens",
        ));
    };
    let role = sqlx::query_scalar::<_, String>("SELECT role FROM users WHERE id = $1 AND enabled")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    if role == crate::authz::Role::Admin.as_str() {
        return Ok(());
    }
    if crate::api::project_membership_role(pool, user_id, project_id)
        .await?
        .is_some()
    {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "token owner must be a member of the scoped project",
        ))
    }
}

fn artifacts_root() -> PathBuf {
    std::env::var("CICD_ARTIFACTS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/var/lib/forge/artifacts"))
}

fn new_artifact_path(id: Uuid) -> Result<PathBuf, ApiError> {
    let root = artifacts_root();
    std::fs::create_dir_all(&root).map_err(io_error)?;
    let root = root.canonicalize().map_err(io_error)?;
    Ok(root.join(format!("{id}.bin")))
}

fn contained_artifact_path(raw_path: &str) -> Result<PathBuf, ApiError> {
    let root = canonical_artifacts_root()?;
    let path = FsPath::new(raw_path)
        .canonicalize()
        .map_err(|_| ApiError::not_found())?;
    if !path.starts_with(&root) {
        return Err(ApiError::not_found());
    }
    Ok(path)
}

fn canonical_artifacts_root() -> Result<PathBuf, ApiError> {
    artifacts_root()
        .canonicalize()
        .map_err(|_| ApiError::not_found())
}
fn io_error(error: std::io::Error) -> ApiError {
    ApiError::internal(sqlx::Error::Io(error))
}
fn valid_artifact_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains('"')
        && !name.chars().any(char::is_control)
}
fn artifact_content_disposition(name: &str) -> String {
    let fallback = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!(
        "attachment; filename=\"{}\"; filename*=UTF-8''{}",
        fallback,
        percent_encode_utf8(name)
    )
}
fn percent_encode_utf8(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-') {
                (*byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}
fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn sha256(value: &str) -> String {
    sha256_bytes(value.as_bytes())
}

fn decrypt_secret(stored: &str) -> Result<String, String> {
    let key = secret_key()?;
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|_| "invalid CICD_SECRETS_KEY".to_owned())?;
    let parts: Vec<&str> = stored.splitn(3, ':').collect();
    if parts.len() != 3 || parts[0] != "v1" {
        return Err("unsupported secret format".to_owned());
    }
    let nonce_bytes = BASE64
        .decode(parts[1])
        .map_err(|_| "corrupt secret nonce".to_owned())?;
    let ciphertext = BASE64
        .decode(parts[2])
        .map_err(|_| "corrupt secret payload".to_owned())?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let plain = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|_| "unable to decrypt secret".to_owned())?;
    String::from_utf8(plain).map_err(|_| "secret is not valid utf-8".to_owned())
}

/// Project secrets resolved for a job environment (runner injection).
pub(crate) async fn project_secret_pairs_for_names(
    pool: &PgPool,
    project_id: Uuid,
    names: &[String],
) -> Result<Vec<(String, String)>, ApiError> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let requested: std::collections::BTreeSet<&str> = names.iter().map(String::as_str).collect();
    let requested_vec: Vec<String> = requested.iter().map(|name| (*name).to_string()).collect();
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT key, encrypted_value \
         FROM project_secrets \
         WHERE project_id = $1 AND key = ANY($2::text[]) \
         ORDER BY key",
    )
    .bind(project_id)
    .bind(&requested_vec)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    let found: std::collections::BTreeSet<&str> =
        rows.iter().map(|(name, _)| name.as_str()).collect();
    if found.len() != requested.len() || requested.iter().any(|name| !found.contains(*name)) {
        return Err(ApiError::not_found_named(
            "declared project secret is missing",
        ));
    }
    let mut pairs = Vec::with_capacity(rows.len());
    for (name, stored) in rows {
        let value = decrypt_secret(&stored).map_err(|msg| ApiError {
            status: axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            message: msg,
        })?;
        pairs.push((name, value));
    }
    Ok(pairs)
}

fn valid_role(role: &str) -> bool {
    matches!(role, "admin" | "maintainer" | "developer" | "viewer")
}
fn schedule_next_fire_at(cron: &str, enabled: bool) -> Result<Option<DateTime<Utc>>, ApiError> {
    if !enabled {
        return Ok(None);
    }
    crate::schedule::next_fire_after_expr(cron, Utc::now())
        .map(Some)
        .map_err(ApiError::bad_request)
}
fn encrypt_secret(value: &str) -> Result<String, String> {
    let key = secret_key()?;
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|_| "invalid CICD_SECRETS_KEY".to_owned())?;
    let source = Uuid::new_v4();
    let nonce = Nonce::from_slice(&source.as_bytes()[..12]);
    let ciphertext = cipher
        .encrypt(nonce, value.as_bytes())
        .map_err(|_| "unable to encrypt secret".to_owned())?;
    Ok(format!(
        "v1:{}:{}",
        BASE64.encode(nonce),
        BASE64.encode(ciphertext)
    ))
}
fn secret_key() -> Result<[u8; 32], String> {
    let configured = std::env::var("CICD_SECRETS_KEY")
        .map_err(|_| "CICD_SECRETS_KEY must be configured before storing secrets".to_owned())?;
    let decoded = BASE64
        .decode(configured.trim())
        .map_err(|_| "CICD_SECRETS_KEY must be base64-encoded 32 bytes".to_owned())?;
    decoded
        .try_into()
        .map_err(|_| "CICD_SECRETS_KEY must be base64-encoded 32 bytes".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cron_requires_five_fields() {
        assert!(crate::schedule::parse_cron("0 4 * * 1").is_ok());
        assert!(crate::schedule::parse_cron("not cron").is_err());
    }
    #[test]
    fn roles_are_allowlisted() {
        assert!(valid_role("admin"));
        assert!(!valid_role("owner"));
    }
    #[test]
    fn artifact_names_reject_path_and_header_breakers() {
        assert!(valid_artifact_name("report.txt"));
        assert!(valid_artifact_name("отчёт.txt"));
        assert!(!valid_artifact_name("../report.txt"));
        assert!(!valid_artifact_name("bad\"name.txt"));
        assert!(!valid_artifact_name("bad\nname.txt"));
    }
    #[test]
    fn artifact_content_disposition_is_header_safe() {
        let header = artifact_content_disposition("отчёт final.txt");
        assert!(header.is_ascii());
        assert!(header.contains("filename=\""));
        assert!(header.contains("filename*=UTF-8''"));
        assert!(header.contains("%D0%BE%D1%82%D1%87"));
    }
}
