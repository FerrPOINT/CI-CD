use std::{
    path::{Path as FsPath, PathBuf},
    sync::Arc,
    time::Duration as StdDuration,
};

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::{
    api::{ApiError, AppState, pool},
    body_limits,
};

pub(crate) const MAX_ARTIFACT_BYTES: usize = body_limits::ARTIFACT_UPLOAD_BYTES;
const DEFAULT_ARTIFACT_RETENTION_DAYS: i64 = 30;
const MAX_ARTIFACT_RETENTION_DAYS: i64 = 3650;
const ARTIFACT_RETENTION_BATCH_LIMIT: i64 = 100;
const ARTIFACT_RETENTION_INTERVAL_SECONDS: u64 = 60;
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
            "/api/v1/deployments/{deployment_id}/approvals",
            get(list_deployment_approvals).post(record_deployment_approval),
        )
        .route(
            "/api/v1/deployments/{deployment_id}/rollback",
            post(rollback_deployment),
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
    expires_at: DateTime<Utc>,
    purged_at: Option<DateTime<Utc>>,
}
#[utoipa::path(get, path = "/api/v1/jobs/{job_id}/artifacts", tag = "artifacts", params(("job_id" = Uuid, Path)), responses((status = 200, body = [Artifact])))]
async fn list_artifacts(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
) -> ApiResult<Vec<Artifact>> {
    Ok(Json(sqlx::query_as("SELECT id, job_id, attempt_id, name, content_type, sha256, size_bytes, created_at, expires_at, purged_at FROM artifacts WHERE job_id = $1 ORDER BY created_at DESC")
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
    let expires_at = artifact_expires_at()?;
    let path = new_artifact_path(id)?;
    std::fs::write(&path, &body).map_err(io_error)?;
    let artifact = sqlx::query_as("INSERT INTO artifacts (id, job_id, attempt_id, name, storage_path, content_type, sha256, size_bytes, expires_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING id, job_id, attempt_id, name, content_type, sha256, size_bytes, created_at, expires_at, purged_at")
        .bind(id).bind(job_id).bind(attempt_id).bind(name).bind(path.to_string_lossy().as_ref()).bind(content_type).bind(&artifact_sha256).bind(body.len() as i64).bind(expires_at).fetch_one(db).await.map_err(ApiError::internal)?;
    audit(db, "artifact.uploaded", "artifact", id, None).await?;
    Ok(artifact)
}
#[utoipa::path(get, path = "/api/v1/artifacts/{artifact_id}/download", tag = "artifacts", params(("artifact_id" = Uuid, Path)), responses((status = 200, description = "Artifact download"), (status = 404)))]
async fn download_artifact(
    State(state): State<Arc<AppState>>,
    Path(artifact_id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let row: (String, String, String, Option<String>) = sqlx::query_as(
        "SELECT storage_path, name, content_type, sha256 FROM artifacts WHERE id = $1 AND purged_at IS NULL AND expires_at > now()",
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

pub fn artifact_retention_days_from_env(raw: Option<String>) -> Result<i64, std::io::Error> {
    let Some(raw) = raw else {
        return Ok(DEFAULT_ARTIFACT_RETENTION_DAYS);
    };
    let value = raw.trim();
    if value.is_empty() {
        return Ok(DEFAULT_ARTIFACT_RETENTION_DAYS);
    }
    let days: i64 = value.parse().map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "CICD_ARTIFACT_RETENTION_DAYS must be an integer number of days",
        )
    })?;
    if !(1..=MAX_ARTIFACT_RETENTION_DAYS).contains(&days) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("CICD_ARTIFACT_RETENTION_DAYS must be 1..={MAX_ARTIFACT_RETENTION_DAYS}"),
        ));
    }
    Ok(days)
}

fn artifact_expires_at() -> Result<DateTime<Utc>, ApiError> {
    let retention_days =
        artifact_retention_days_from_env(std::env::var("CICD_ARTIFACT_RETENTION_DAYS").ok())
            .map_err(|error| config_error(error.to_string()))?;
    Ok(Utc::now() + ChronoDuration::days(retention_days))
}

#[derive(Debug, FromRow)]
struct ExpiredArtifactCandidate {
    id: Uuid,
    storage_path: String,
}

pub async fn purge_expired_artifacts(db: &PgPool, batch_limit: i64) -> Result<u64, ApiError> {
    let batch_limit = batch_limit.clamp(1, 1000);
    let mut tx = db.begin().await.map_err(ApiError::internal)?;
    let candidates = sqlx::query_as::<_, ExpiredArtifactCandidate>(
        "SELECT id, storage_path \
         FROM artifacts \
         WHERE purged_at IS NULL AND expires_at <= now() \
         ORDER BY expires_at ASC, id ASC \
         LIMIT $1 \
         FOR UPDATE SKIP LOCKED",
    )
    .bind(batch_limit)
    .fetch_all(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    let mut purged = 0;
    for candidate in candidates {
        let path = match artifact_path_for_delete(&candidate.storage_path) {
            Ok(path) => path,
            Err(error) => {
                tracing::warn!(
                    artifact_id = %candidate.id,
                    storage_path = %candidate.storage_path,
                    reason = %error.message,
                    "artifact retention skipped unsafe path"
                );
                continue;
            }
        };
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::warn!(
                    artifact_id = %candidate.id,
                    path = %path.display(),
                    %error,
                    "artifact retention failed to remove file"
                );
                continue;
            }
        }
        let result = sqlx::query(
            "UPDATE artifacts \
             SET purged_at = now() \
             WHERE id = $1 AND purged_at IS NULL AND expires_at <= now()",
        )
        .bind(candidate.id)
        .execute(&mut *tx)
        .await
        .map_err(ApiError::internal)?;
        if result.rows_affected() > 0 {
            sqlx::query(
                "INSERT INTO audit_log (action, resource_type, resource_id, actor) \
                 VALUES ('artifact.purged', 'artifact', $1, NULL)",
            )
            .bind(candidate.id)
            .execute(&mut *tx)
            .await
            .map_err(ApiError::internal)?;
            purged += 1;
        }
    }
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(purged)
}

pub async fn artifact_retention_loop(pool: PgPool) {
    loop {
        tokio::time::sleep(StdDuration::from_secs(ARTIFACT_RETENTION_INTERVAL_SECONDS)).await;
        match purge_expired_artifacts(&pool, ARTIFACT_RETENTION_BATCH_LIMIT).await {
            Ok(0) => {}
            Ok(purged) => tracing::info!(purged, "artifact retention purged expired files"),
            Err(error) => tracing::error!(message = %error.message, "artifact retention failed"),
        }
    }
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct Environment {
    id: Uuid,
    project_id: Uuid,
    name: String,
    url: Option<String>,
    status: String,
    protected: bool,
    required_approvals: i32,
    created_at: DateTime<Utc>,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct CreateEnvironment {
    name: String,
    url: Option<String>,
    #[serde(default)]
    protected: Option<bool>,
    #[serde(default)]
    required_approvals: Option<i32>,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct UpdateEnvironment {
    name: Option<String>,
    url: Option<String>,
    status: Option<String>,
    protected: Option<bool>,
    required_approvals: Option<i32>,
}
#[utoipa::path(get, path = "/api/v1/projects/{project_id}/environments", tag = "environments", params(("project_id" = Uuid, Path)), responses((status = 200, body = [Environment])))]
async fn list_environments(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
) -> ApiResult<Vec<Environment>> {
    Ok(Json(sqlx::query_as("SELECT id, project_id, name, url, status, protected, required_approvals, created_at FROM environments WHERE project_id = $1 ORDER BY name").bind(project_id).fetch_all(pool(&state)?).await.map_err(ApiError::internal)?))
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
    let protected = input.protected.unwrap_or(false);
    let required_approvals =
        normalize_required_approvals(protected, input.required_approvals, None)?;
    let value = sqlx::query_as("INSERT INTO environments (id, project_id, name, url, protected, required_approvals) VALUES ($1, $2, $3, $4, $5, $6) RETURNING id, project_id, name, url, status, protected, required_approvals, created_at")
        .bind(Uuid::new_v4())
        .bind(project_id)
        .bind(input.name.trim())
        .bind(trim_optional(input.url))
        .bind(protected)
        .bind(required_approvals)
        .fetch_one(pool(&state)?)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(value))
}
#[utoipa::path(patch, path = "/api/v1/environments/{environment_id}", tag = "environments", request_body = UpdateEnvironment, params(("environment_id" = Uuid, Path)), responses((status = 200, body = Environment), (status = 404)))]
async fn update_environment(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateEnvironment>,
) -> ApiResult<Environment> {
    let name = input.name.as_deref().map(str::trim);
    if name == Some("") {
        return Err(ApiError::bad_request("environment name is required"));
    }
    if let Some(status) = &input.status {
        if !matches!(status.as_str(), "available" | "stopped" | "degraded") {
            return Err(ApiError::bad_request("invalid environment status"));
        }
    }
    let db = pool(&state)?;
    let current = fetch_environment_policy(db, id).await?;
    let protected = input.protected.unwrap_or(current.protected);
    let required_approvals =
        normalize_required_approvals(protected, input.required_approvals, Some(&current))?;
    let value = sqlx::query_as("UPDATE environments SET name = COALESCE($2, name), url = COALESCE($3, url), status = COALESCE($4, status), protected = $5, required_approvals = $6 WHERE id = $1 RETURNING id, project_id, name, url, status, protected, required_approvals, created_at")
        .bind(id)
        .bind(name)
        .bind(trim_optional(input.url))
        .bind(input.status)
        .bind(protected)
        .bind(required_approvals)
        .fetch_optional(db)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
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
    rollback_of_id: Option<Uuid>,
    git_ref: String,
    status: String,
    approval_required: bool,
    approval_state: String,
    approval_count: i64,
    required_approvals: i32,
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
    let sql = deployment_select(
        "d.environment_id = $1",
        "ORDER BY d.created_at DESC, d.id DESC LIMIT 50",
    );
    Ok(Json(
        sqlx::query_as(sql.as_str())
            .bind(environment_id)
            .fetch_all(pool(&state)?)
            .await
            .map_err(ApiError::internal)?,
    ))
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
    let db = pool(&state)?;
    let policy = fetch_environment_policy(db, environment_id).await?;
    validate_pipeline_project(db, input.pipeline_id, policy.project_id).await?;
    let approval_required = policy.requires_approval();
    if approval_required && input.pipeline_id.is_some() {
        return Err(ApiError::bad_request(
            "protected environment deployment must be approved before pipeline execution",
        ));
    }
    let status = input.status.unwrap_or_else(|| {
        if approval_required {
            "pending".into()
        } else {
            "success".into()
        }
    });
    validate_deployment_status(&status)?;
    if approval_required && status != "pending" {
        return Err(ApiError::bad_request(
            "protected environment deployment must start as pending approval",
        ));
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO deployments (id, environment_id, pipeline_id, git_ref, status) VALUES ($1, $2, $3, $4, $5)")
        .bind(id)
        .bind(environment_id)
        .bind(input.pipeline_id)
        .bind(input.git_ref.trim())
        .bind(status)
        .execute(db)
        .await
        .map_err(ApiError::internal)?;
    audit(db, "deployment.created", "deployment", id, None).await?;
    let value = fetch_deployment(db, id).await?;
    Ok(Json(value))
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct DeploymentApproval {
    id: Uuid,
    deployment_id: Uuid,
    decision: String,
    actor: String,
    comment: Option<String>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct RecordDeploymentApproval {
    decision: String,
    actor: Option<String>,
    comment: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct RollbackDeployment {
    git_ref: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/deployments/{deployment_id}/approvals", tag = "environments", params(("deployment_id" = Uuid, Path)), responses((status = 200, body = [DeploymentApproval]), (status = 404)))]
async fn list_deployment_approvals(
    State(state): State<Arc<AppState>>,
    Path(deployment_id): Path<Uuid>,
) -> ApiResult<Vec<DeploymentApproval>> {
    let db = pool(&state)?;
    ensure_deployment_exists(db, deployment_id).await?;
    Ok(Json(sqlx::query_as("SELECT id, deployment_id, decision, actor, comment, created_at FROM deployment_approvals WHERE deployment_id = $1 ORDER BY created_at ASC, id ASC")
        .bind(deployment_id)
        .fetch_all(db)
        .await
        .map_err(ApiError::internal)?))
}

#[utoipa::path(post, path = "/api/v1/deployments/{deployment_id}/approvals", tag = "environments", request_body = RecordDeploymentApproval, params(("deployment_id" = Uuid, Path)), responses((status = 200, body = Deployment), (status = 400), (status = 404), (status = 409)))]
async fn record_deployment_approval(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::auth::AccessClaims>>,
    Path(deployment_id): Path<Uuid>,
    Json(input): Json<RecordDeploymentApproval>,
) -> ApiResult<Deployment> {
    let db = pool(&state)?;
    let decision = normalize_approval_decision(&input.decision)?;
    let actor = approval_actor(input.actor.as_deref(), claims.as_ref().map(|c| c.0.sub))?;
    let comment = trim_optional(input.comment);
    if comment.as_ref().is_some_and(|value| value.len() > 1000) {
        return Err(ApiError::bad_request("approval comment is too long"));
    }

    let target = record_approval_decision(db, deployment_id, &decision, &actor, comment).await?;
    if target.should_start_pipeline {
        start_deployment_pipeline(
            db,
            deployment_id,
            target.project_id,
            target.environment_id,
            target.git_ref,
            target.rollback_of_id,
            "deployment-approval",
        )
        .await?;
    }
    let value = fetch_deployment(db, deployment_id).await?;
    Ok(Json(value))
}

#[utoipa::path(post, path = "/api/v1/deployments/{deployment_id}/rollback", tag = "environments", request_body = RollbackDeployment, params(("deployment_id" = Uuid, Path)), responses((status = 200, body = Deployment), (status = 400), (status = 404), (status = 409)))]
async fn rollback_deployment(
    State(state): State<Arc<AppState>>,
    Path(deployment_id): Path<Uuid>,
    Json(input): Json<RollbackDeployment>,
) -> ApiResult<Deployment> {
    let db = pool(&state)?;
    let source = fetch_rollback_source(db, deployment_id).await?;
    if source.status != "success" {
        return Err(ApiError::conflict(
            "only successful deployments can be rollback targets",
        ));
    }
    let git_ref = input
        .git_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(source.git_ref.as_str())
        .to_owned();
    let new_id = Uuid::new_v4();
    sqlx::query("INSERT INTO deployments (id, environment_id, pipeline_id, rollback_of_id, git_ref, status) VALUES ($1, $2, $3, $4, $5, $6)")
        .bind(new_id)
        .bind(source.environment_id)
        .bind(None::<Uuid>)
        .bind(deployment_id)
        .bind(&git_ref)
        .bind("pending")
        .execute(db)
        .await
        .map_err(ApiError::internal)?;
    if !source.requires_approval() {
        start_deployment_pipeline(
            db,
            new_id,
            source.project_id,
            source.environment_id,
            git_ref,
            Some(deployment_id),
            "deployment-rollback",
        )
        .await?;
    }
    audit(
        db,
        "deployment.rollback.created",
        "deployment",
        new_id,
        None,
    )
    .await?;
    Ok(Json(fetch_deployment(db, new_id).await?))
}

#[derive(Debug, FromRow)]
struct EnvironmentPolicy {
    project_id: Uuid,
    protected: bool,
    required_approvals: i32,
}

impl EnvironmentPolicy {
    fn requires_approval(&self) -> bool {
        self.protected && self.required_approvals > 0
    }
}

#[derive(Debug, FromRow)]
struct ApprovalTarget {
    environment_id: Uuid,
    project_id: Uuid,
    git_ref: String,
    status: String,
    pipeline_id: Option<Uuid>,
    rollback_of_id: Option<Uuid>,
    protected: bool,
    required_approvals: i32,
}

impl ApprovalTarget {
    fn requires_approval(&self) -> bool {
        self.protected && self.required_approvals > 0
    }
}

#[derive(Debug)]
struct RecordedApprovalTarget {
    environment_id: Uuid,
    project_id: Uuid,
    git_ref: String,
    rollback_of_id: Option<Uuid>,
    should_start_pipeline: bool,
}

#[derive(Debug, FromRow)]
struct RollbackSource {
    environment_id: Uuid,
    project_id: Uuid,
    git_ref: String,
    status: String,
    protected: bool,
    required_approvals: i32,
}

impl RollbackSource {
    fn requires_approval(&self) -> bool {
        self.protected && self.required_approvals > 0
    }
}

async fn fetch_environment_policy(
    db: &PgPool,
    environment_id: Uuid,
) -> Result<EnvironmentPolicy, ApiError> {
    sqlx::query_as(
        "SELECT project_id, protected, required_approvals FROM environments WHERE id = $1",
    )
    .bind(environment_id)
    .fetch_optional(db)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::not_found)
}

async fn validate_pipeline_project(
    db: &PgPool,
    pipeline_id: Option<Uuid>,
    project_id: Uuid,
) -> Result<(), ApiError> {
    let Some(pipeline_id) = pipeline_id else {
        return Ok(());
    };
    let pipeline_project_id: Uuid =
        sqlx::query_scalar("SELECT project_id FROM pipelines WHERE id = $1")
            .bind(pipeline_id)
            .fetch_optional(db)
            .await
            .map_err(ApiError::internal)?
            .ok_or_else(ApiError::not_found)?;
    if pipeline_project_id != project_id {
        return Err(ApiError::bad_request(
            "pipeline belongs to a different project than environment",
        ));
    }
    Ok(())
}

async fn ensure_deployment_exists(db: &PgPool, deployment_id: Uuid) -> Result<(), ApiError> {
    let exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM deployments WHERE id = $1)")
            .bind(deployment_id)
            .fetch_one(db)
            .await
            .map_err(ApiError::internal)?;
    if exists {
        Ok(())
    } else {
        Err(ApiError::not_found())
    }
}

fn deployment_select(where_clause: &str, tail_clause: &str) -> String {
    format!(
        "SELECT d.id, d.environment_id, d.pipeline_id, d.rollback_of_id, d.git_ref, d.status, d.created_at, \
                (e.protected AND e.required_approvals > 0) AS approval_required, \
                e.required_approvals, \
                COALESCE(COUNT(a.id) FILTER (WHERE a.decision = 'approved'), 0)::bigint AS approval_count, \
                CASE \
                    WHEN NOT (e.protected AND e.required_approvals > 0) THEN 'not_required' \
                    WHEN COALESCE(COUNT(a.id) FILTER (WHERE a.decision = 'rejected'), 0) > 0 THEN 'rejected' \
                    WHEN COALESCE(COUNT(a.id) FILTER (WHERE a.decision = 'approved'), 0) >= e.required_approvals THEN 'approved' \
                    ELSE 'pending' \
                END AS approval_state \
         FROM deployments d \
         JOIN environments e ON e.id = d.environment_id \
         LEFT JOIN deployment_approvals a ON a.deployment_id = d.id \
         WHERE {where_clause} \
         GROUP BY d.id, e.protected, e.required_approvals \
         {tail_clause}"
    )
}

async fn fetch_deployment(db: &PgPool, deployment_id: Uuid) -> Result<Deployment, ApiError> {
    let sql = deployment_select("d.id = $1", "");
    sqlx::query_as(sql.as_str())
        .bind(deployment_id)
        .fetch_optional(db)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)
}

async fn record_approval_decision(
    db: &PgPool,
    deployment_id: Uuid,
    decision: &str,
    actor: &str,
    comment: Option<String>,
) -> Result<RecordedApprovalTarget, ApiError> {
    let mut tx = db.begin().await.map_err(ApiError::internal)?;
    let target: ApprovalTarget = sqlx::query_as(
        "SELECT d.environment_id, e.project_id, d.git_ref, d.status, d.pipeline_id, d.rollback_of_id, e.protected, e.required_approvals \
         FROM deployments d \
         JOIN environments e ON e.id = d.environment_id \
         WHERE d.id = $1 \
         FOR UPDATE OF d",
    )
    .bind(deployment_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::not_found)?;
    if !target.requires_approval() {
        return Err(ApiError::bad_request(
            "deployment does not require environment approval",
        ));
    }
    if target.status != "pending" || target.pipeline_id.is_some() {
        return Err(ApiError::conflict("deployment is not pending approval"));
    }
    let existing_by_actor = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM deployment_approvals WHERE deployment_id = $1 AND actor = $2)",
    )
    .bind(deployment_id)
    .bind(actor)
    .fetch_one(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    if existing_by_actor {
        return Err(ApiError::conflict(
            "actor already recorded a deployment approval decision",
        ));
    }
    let (approved_count, rejected_count): (i64, i64) = sqlx::query_as(
        "SELECT \
            COALESCE(COUNT(*) FILTER (WHERE decision = 'approved'), 0)::bigint, \
            COALESCE(COUNT(*) FILTER (WHERE decision = 'rejected'), 0)::bigint \
         FROM deployment_approvals WHERE deployment_id = $1",
    )
    .bind(deployment_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    if rejected_count > 0 || approved_count >= i64::from(target.required_approvals) {
        return Err(ApiError::conflict("deployment approval is already decided"));
    }
    sqlx::query("INSERT INTO deployment_approvals (id, deployment_id, decision, actor, comment) VALUES ($1, $2, $3, $4, $5)")
        .bind(Uuid::new_v4())
        .bind(deployment_id)
        .bind(decision)
        .bind(actor)
        .bind(comment)
        .execute(&mut *tx)
        .await
        .map_err(ApiError::internal)?;
    let should_start_pipeline =
        decision == "approved" && approved_count + 1 >= i64::from(target.required_approvals);
    if decision == "rejected" {
        sqlx::query(
            "UPDATE deployments SET status = 'failed' WHERE id = $1 AND status = 'pending'",
        )
        .bind(deployment_id)
        .execute(&mut *tx)
        .await
        .map_err(ApiError::internal)?;
    }
    tx.commit().await.map_err(ApiError::internal)?;
    audit(
        db,
        "deployment.approval.recorded",
        "deployment",
        deployment_id,
        None,
    )
    .await?;
    Ok(RecordedApprovalTarget {
        environment_id: target.environment_id,
        project_id: target.project_id,
        git_ref: target.git_ref,
        rollback_of_id: target.rollback_of_id,
        should_start_pipeline,
    })
}

async fn start_deployment_pipeline(
    db: &PgPool,
    deployment_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    git_ref: String,
    rollback_of_id: Option<Uuid>,
    source: &str,
) -> Result<Uuid, ApiError> {
    let outcome = crate::api::create_pipeline_with_vars_idempotent(
        db,
        project_id,
        git_ref,
        serde_json::json!({
            "deployment_id": deployment_id.to_string(),
            "environment_id": environment_id.to_string(),
            "rollback_of_deployment_id": rollback_of_id.map(|id| id.to_string()),
        }),
        source,
        Some(&format!("{source}:{deployment_id}")),
    )
    .await?;
    let deployment_status = deployment_status_from_pipeline(outcome.pipeline.status.as_str());
    sqlx::query(
        "UPDATE deployments SET pipeline_id = $2, status = $3 \
         WHERE id = $1 AND pipeline_id IS NULL AND status = 'pending' \
           AND NOT EXISTS (SELECT 1 FROM deployment_approvals WHERE deployment_id = $1 AND decision = 'rejected')",
    )
    .bind(deployment_id)
    .bind(outcome.pipeline.id)
    .bind(deployment_status)
    .execute(db)
    .await
    .map_err(ApiError::internal)?;
    Ok(outcome.pipeline.id)
}

async fn fetch_rollback_source(
    db: &PgPool,
    deployment_id: Uuid,
) -> Result<RollbackSource, ApiError> {
    sqlx::query_as(
        "SELECT d.environment_id, e.project_id, d.git_ref, d.status, e.protected, e.required_approvals \
         FROM deployments d \
         JOIN environments e ON e.id = d.environment_id \
         WHERE d.id = $1",
    )
    .bind(deployment_id)
    .fetch_optional(db)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::not_found)
}

fn normalize_required_approvals(
    protected: bool,
    requested: Option<i32>,
    current: Option<&EnvironmentPolicy>,
) -> Result<i32, ApiError> {
    if !protected {
        if requested.is_some_and(|value| value != 0) {
            return Err(ApiError::bad_request(
                "required_approvals must be 0 for unprotected environments",
            ));
        }
        return Ok(0);
    }
    let value = requested
        .or_else(|| {
            current
                .filter(|policy| policy.protected)
                .map(|policy| policy.required_approvals)
        })
        .unwrap_or(1);
    if !(1..=10).contains(&value) {
        return Err(ApiError::bad_request(
            "required_approvals must be between 1 and 10",
        ));
    }
    Ok(value)
}

fn validate_deployment_status(status: &str) -> Result<(), ApiError> {
    if matches!(status, "pending" | "running" | "success" | "failed") {
        Ok(())
    } else {
        Err(ApiError::bad_request("invalid deployment status"))
    }
}

fn deployment_status_from_pipeline(status: &str) -> &'static str {
    match status {
        "running" => "running",
        "success" => "success",
        "failed" | "canceled" => "failed",
        _ => "pending",
    }
}

fn normalize_approval_decision(raw: &str) -> Result<String, ApiError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "approved" | "approve" => Ok("approved".to_owned()),
        "rejected" | "reject" => Ok("rejected".to_owned()),
        _ => Err(ApiError::bad_request(
            "approval decision must be approved or rejected",
        )),
    }
}

fn approval_actor(raw: Option<&str>, claims_user_id: Option<Uuid>) -> Result<String, ApiError> {
    let actor = claims_user_id
        .map(|id| id.to_string())
        .or_else(|| {
            raw.map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "trusted-network".to_owned());
    if actor.len() > 128 {
        return Err(ApiError::bad_request("approval actor is too long"));
    }
    Ok(actor)
}

fn trim_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
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

fn artifact_path_for_delete(raw_path: &str) -> Result<PathBuf, ApiError> {
    let root = canonical_artifacts_root()?;
    let path = FsPath::new(raw_path);
    let parent = path.parent().ok_or_else(ApiError::not_found)?;
    let parent = parent.canonicalize().map_err(|_| ApiError::not_found())?;
    if !parent.starts_with(&root) {
        return Err(ApiError::not_found());
    }
    Ok(path.to_path_buf())
}

fn canonical_artifacts_root() -> Result<PathBuf, ApiError> {
    artifacts_root()
        .canonicalize()
        .map_err(|_| ApiError::not_found())
}
fn io_error(error: std::io::Error) -> ApiError {
    ApiError::internal(sqlx::Error::Io(error))
}
fn config_error(message: impl Into<String>) -> ApiError {
    ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: message.into(),
    }
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
    fn approval_actor_uses_authenticated_subject_over_body_actor() {
        let user_id = Uuid::parse_str("018f3c59-38f6-7c2a-bc55-081eb78cbf17").unwrap();
        assert_eq!(
            approval_actor(Some("release-manager"), Some(user_id)).unwrap(),
            user_id.to_string()
        );
        assert_eq!(
            approval_actor(Some("release-manager"), None).unwrap(),
            "release-manager"
        );
    }
    #[test]
    fn deployment_status_maps_pipeline_domain_to_deployment_domain() {
        assert_eq!(deployment_status_from_pipeline("queued"), "pending");
        assert_eq!(deployment_status_from_pipeline("running"), "running");
        assert_eq!(deployment_status_from_pipeline("success"), "success");
        assert_eq!(deployment_status_from_pipeline("failed"), "failed");
        assert_eq!(deployment_status_from_pipeline("canceled"), "failed");
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
    #[test]
    fn artifact_retention_env_defaults_and_bounds() {
        assert_eq!(artifact_retention_days_from_env(None).unwrap(), 30);
        assert_eq!(
            artifact_retention_days_from_env(Some(" 45 ".to_string())).unwrap(),
            45
        );
        assert!(artifact_retention_days_from_env(Some("0".to_string())).is_err());
        assert!(artifact_retention_days_from_env(Some("3651".to_string())).is_err());
        assert!(artifact_retention_days_from_env(Some("soon".to_string())).is_err());
    }
}
