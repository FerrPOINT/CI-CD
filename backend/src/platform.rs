use std::{path::PathBuf, sync::Arc};

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, State},
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

use crate::api::{ApiError, AppState, pool};

const MAX_ARTIFACT_BYTES: usize = 50 * 1024 * 1024;

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
            get(list_artifacts).post(upload_artifact),
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
            "/api/v1/projects/{project_id}/notifications",
            get(list_notifications).put(replace_notifications),
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

#[derive(Debug, Serialize, FromRow)]
struct Runner {
    id: Uuid,
    name: String,
    tags: Vec<String>,
    status: String,
    last_seen_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}
#[derive(Debug, Deserialize)]
struct RegisterRunner {
    name: String,
    #[serde(default)]
    tags: Vec<String>,
}
#[derive(Debug, Deserialize)]
struct RunnerHeartbeat {
    status: Option<String>,
}

async fn list_runners(State(state): State<Arc<AppState>>) -> ApiResult<Vec<Runner>> {
    Ok(Json(sqlx::query_as("SELECT id, name, tags, status, last_seen_at, created_at FROM runners ORDER BY created_at DESC")
        .fetch_all(pool(&state)?).await.map_err(ApiError::internal)?))
}
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

#[derive(Debug, Serialize, FromRow)]
struct SecretMetadata {
    id: Uuid,
    project_id: Uuid,
    key: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}
#[derive(Debug, Deserialize)]
struct CreateSecret {
    key: String,
    value: String,
}
async fn list_secrets(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
) -> ApiResult<Vec<SecretMetadata>> {
    Ok(Json(sqlx::query_as("SELECT id, project_id, key, created_at, updated_at FROM project_secrets WHERE project_id = $1 ORDER BY key")
        .bind(project_id).fetch_all(pool(&state)?).await.map_err(ApiError::internal)?))
}
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

#[derive(Debug, Serialize, FromRow)]
struct Artifact {
    id: Uuid,
    job_id: Uuid,
    name: String,
    content_type: String,
    size_bytes: i64,
    created_at: DateTime<Utc>,
}
async fn list_artifacts(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
) -> ApiResult<Vec<Artifact>> {
    Ok(Json(sqlx::query_as("SELECT id, job_id, name, content_type, size_bytes, created_at FROM artifacts WHERE job_id = $1 ORDER BY created_at DESC")
        .bind(job_id).fetch_all(pool(&state)?).await.map_err(ApiError::internal)?))
}
async fn upload_artifact(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Artifact> {
    if body.is_empty() || body.len() > MAX_ARTIFACT_BYTES {
        return Err(ApiError::bad_request(
            "artifact must be between 1 byte and 50 MiB",
        ));
    }
    let name = headers
        .get("x-artifact-name")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("artifact.bin")
        .trim();
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        return Err(ApiError::bad_request("invalid artifact name"));
    }
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream");
    let id = Uuid::new_v4();
    let path = artifact_path(id);
    std::fs::create_dir_all(path.parent().expect("artifact parent")).map_err(io_error)?;
    std::fs::write(&path, &body).map_err(io_error)?;
    let artifact = sqlx::query_as("INSERT INTO artifacts (id, job_id, name, storage_path, content_type, size_bytes) VALUES ($1, $2, $3, $4, $5, $6) RETURNING id, job_id, name, content_type, size_bytes, created_at")
        .bind(id).bind(job_id).bind(name).bind(path.to_string_lossy().as_ref()).bind(content_type).bind(body.len() as i64).fetch_one(pool(&state)?).await.map_err(ApiError::internal)?;
    audit(pool(&state)?, "artifact.uploaded", "artifact", id, None).await?;
    Ok(Json(artifact))
}
async fn download_artifact(
    State(state): State<Arc<AppState>>,
    Path(artifact_id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let row: (String, String, String) =
        sqlx::query_as("SELECT storage_path, name, content_type FROM artifacts WHERE id = $1")
            .bind(artifact_id)
            .fetch_optional(pool(&state)?)
            .await
            .map_err(ApiError::internal)?
            .ok_or_else(ApiError::not_found)?;
    let bytes = std::fs::read(&row.0).map_err(|_| ApiError::not_found())?;
    Ok((
        [
            ("content-type", row.2),
            (
                "content-disposition",
                format!("attachment; filename=\"{}\"", row.1),
            ),
        ],
        bytes,
    )
        .into_response())
}

#[derive(Debug, Serialize, FromRow)]
struct Environment {
    id: Uuid,
    project_id: Uuid,
    name: String,
    url: Option<String>,
    status: String,
    created_at: DateTime<Utc>,
}
#[derive(Debug, Deserialize)]
struct CreateEnvironment {
    name: String,
    url: Option<String>,
}
#[derive(Debug, Deserialize)]
struct UpdateEnvironment {
    name: Option<String>,
    url: Option<String>,
    status: Option<String>,
}
async fn list_environments(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
) -> ApiResult<Vec<Environment>> {
    Ok(Json(sqlx::query_as("SELECT id, project_id, name, url, status, created_at FROM environments WHERE project_id = $1 ORDER BY name").bind(project_id).fetch_all(pool(&state)?).await.map_err(ApiError::internal)?))
}
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

#[derive(Debug, Serialize, FromRow)]
struct Deployment {
    id: Uuid,
    environment_id: Uuid,
    pipeline_id: Option<Uuid>,
    git_ref: String,
    status: String,
    created_at: DateTime<Utc>,
}
#[derive(Debug, Deserialize)]
struct CreateDeployment {
    git_ref: String,
    pipeline_id: Option<Uuid>,
    status: Option<String>,
}
async fn list_deployments(
    State(state): State<Arc<AppState>>,
    Path(environment_id): Path<Uuid>,
) -> ApiResult<Vec<Deployment>> {
    Ok(Json(sqlx::query_as("SELECT id, environment_id, pipeline_id, git_ref, status, created_at FROM deployments WHERE environment_id = $1 ORDER BY created_at DESC LIMIT 50").bind(environment_id).fetch_all(pool(&state)?).await.map_err(ApiError::internal)?))
}
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

#[derive(Debug, Serialize, FromRow)]
struct Schedule {
    id: Uuid,
    project_id: Uuid,
    cron: String,
    git_ref: String,
    enabled: bool,
    created_at: DateTime<Utc>,
}
#[derive(Debug, Deserialize)]
struct ScheduleInput {
    cron: String,
    git_ref: String,
    enabled: Option<bool>,
}
async fn list_schedules(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
) -> ApiResult<Vec<Schedule>> {
    Ok(Json(sqlx::query_as("SELECT id, project_id, cron, git_ref, enabled, created_at FROM schedules WHERE project_id = $1 ORDER BY created_at DESC").bind(project_id).fetch_all(pool(&state)?).await.map_err(ApiError::internal)?))
}
async fn create_schedule(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<ScheduleInput>,
) -> ApiResult<Schedule> {
    if !valid_cron(&input.cron) || input.git_ref.trim().is_empty() {
        return Err(ApiError::bad_request(
            "cron must have five fields and git_ref is required",
        ));
    }
    Ok(Json(sqlx::query_as("INSERT INTO schedules (id, project_id, cron, git_ref, enabled) VALUES ($1, $2, $3, $4, $5) RETURNING id, project_id, cron, git_ref, enabled, created_at").bind(Uuid::new_v4()).bind(project_id).bind(input.cron.trim()).bind(input.git_ref.trim()).bind(input.enabled.unwrap_or(true)).fetch_one(pool(&state)?).await.map_err(ApiError::internal)?))
}
async fn update_schedule(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(input): Json<ScheduleInput>,
) -> ApiResult<Schedule> {
    if !valid_cron(&input.cron) || input.git_ref.trim().is_empty() {
        return Err(ApiError::bad_request(
            "cron must have five fields and git_ref is required",
        ));
    }
    Ok(Json(sqlx::query_as("UPDATE schedules SET cron = $2, git_ref = $3, enabled = $4 WHERE id = $1 RETURNING id, project_id, cron, git_ref, enabled, created_at").bind(id).bind(input.cron.trim()).bind(input.git_ref.trim()).bind(input.enabled.unwrap_or(true)).fetch_optional(pool(&state)?).await.map_err(ApiError::internal)?.ok_or_else(ApiError::not_found)?))
}
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

#[derive(Debug, Serialize, FromRow)]
struct Webhook {
    id: Uuid,
    project_id: Uuid,
    url: String,
    events: Vec<String>,
    enabled: bool,
    created_at: DateTime<Utc>,
}
#[derive(Debug, Deserialize)]
struct CreateWebhook {
    url: String,
    #[serde(default)]
    events: Vec<String>,
    enabled: Option<bool>,
}
async fn list_webhooks(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
) -> ApiResult<Vec<Webhook>> {
    Ok(Json(sqlx::query_as("SELECT id, project_id, url, events, enabled, created_at FROM webhooks WHERE project_id = $1 ORDER BY created_at DESC").bind(project_id).fetch_all(pool(&state)?).await.map_err(ApiError::internal)?))
}
async fn create_webhook(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<CreateWebhook>,
) -> ApiResult<Webhook> {
    if !input.url.starts_with("http://") && !input.url.starts_with("https://") {
        return Err(ApiError::bad_request("webhook url must be http(s)"));
    }
    Ok(Json(sqlx::query_as("INSERT INTO webhooks (id, project_id, url, events, enabled) VALUES ($1, $2, $3, $4, $5) RETURNING id, project_id, url, events, enabled, created_at").bind(Uuid::new_v4()).bind(project_id).bind(input.url.trim()).bind(input.events).bind(input.enabled.unwrap_or(true)).fetch_one(pool(&state)?).await.map_err(ApiError::internal)?))
}
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

#[derive(Debug, Serialize, FromRow)]
struct Notification {
    id: Uuid,
    project_id: Uuid,
    channel: String,
    target: String,
    enabled: bool,
    created_at: DateTime<Utc>,
}
#[derive(Debug, Deserialize)]
struct NotificationInput {
    channel: String,
    target: String,
    enabled: Option<bool>,
}
async fn list_notifications(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
) -> ApiResult<Vec<Notification>> {
    Ok(Json(sqlx::query_as("SELECT id, project_id, channel, target, enabled, created_at FROM notification_configs WHERE project_id = $1 ORDER BY channel").bind(project_id).fetch_all(pool(&state)?).await.map_err(ApiError::internal)?))
}
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

#[derive(Debug, Serialize, FromRow)]
struct Report {
    total_pipelines: i64,
    successful_pipelines: i64,
    failed_pipelines: i64,
    success_rate: f64,
    average_duration_seconds: f64,
}
async fn project_report(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
) -> ApiResult<Report> {
    Ok(Json(sqlx::query_as("SELECT count(*)::bigint AS total_pipelines, count(*) FILTER (WHERE status = 'success')::bigint AS successful_pipelines, count(*) FILTER (WHERE status = 'failed')::bigint AS failed_pipelines, COALESCE(count(*) FILTER (WHERE status = 'success')::float8 / NULLIF(count(*) FILTER (WHERE status IN ('success', 'failed', 'canceled')), 0), 0)::float8 AS success_rate, COALESCE(avg(EXTRACT(EPOCH FROM (finished_at - started_at))) FILTER (WHERE finished_at IS NOT NULL AND started_at IS NOT NULL), 0)::float8 AS average_duration_seconds FROM pipelines WHERE project_id = $1").bind(project_id).fetch_one(pool(&state)?).await.map_err(ApiError::internal)?))
}

#[derive(Debug, Serialize, FromRow)]
struct AuditEvent {
    id: i64,
    action: String,
    resource_type: String,
    resource_id: Option<Uuid>,
    actor: Option<String>,
    created_at: DateTime<Utc>,
}
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

#[derive(Debug, Serialize, FromRow)]
struct User {
    id: Uuid,
    username: String,
    role: String,
    enabled: bool,
    created_at: DateTime<Utc>,
}
#[derive(Debug, Deserialize)]
struct UserInput {
    username: String,
    role: String,
    enabled: Option<bool>,
    /// Optional argon2id password; enables interactive login (AUTHZ_CONTRACT).
    password: Option<String>,
}
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

#[derive(Debug, Serialize, FromRow)]
struct ApiToken {
    id: Uuid,
    name: String,
    token_hint: String,
    user_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    last_used_at: Option<DateTime<Utc>>,
}
#[derive(Debug, Serialize)]
struct CreatedToken {
    #[serde(flatten)]
    token: ApiToken,
    value: String,
}
#[derive(Debug, Deserialize)]
struct CreateToken {
    name: String,
    user_id: Option<Uuid>,
}
async fn list_tokens(State(state): State<Arc<AppState>>) -> ApiResult<Vec<ApiToken>> {
    Ok(Json(sqlx::query_as("SELECT id, name, token_hint, user_id, created_at, last_used_at FROM api_tokens ORDER BY created_at DESC").fetch_all(pool(&state)?).await.map_err(ApiError::internal)?))
}
async fn create_token(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::auth::AccessClaims>>,
    Json(input): Json<CreateToken>,
) -> ApiResult<CreatedToken> {
    let user_id = input.user_id.or(claims.map(|c| c.0.sub));
    if input.name.trim().is_empty() {
        return Err(ApiError::bad_request("token name is required"));
    }
    let value = format!(
        "cicd_{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    );
    let token_hash = sha256(&value);
    let hint = format!("{}...{}", &value[..9], &value[value.len() - 4..]);
    let token = sqlx::query_as::<_, ApiToken>("INSERT INTO api_tokens (id, name, token_hash, token_hint, user_id) VALUES ($1, $2, $3, $4, $5) RETURNING id, name, token_hint, user_id, created_at, last_used_at").bind(Uuid::new_v4()).bind(input.name.trim()).bind(token_hash).bind(hint).bind(user_id).fetch_one(pool(&state)?).await.map_err(ApiError::internal)?;
    audit(pool(&state)?, "token.created", "api_token", token.id, None).await?;
    Ok(Json(CreatedToken { token, value }))
}
async fn delete_token(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> ApiResult<serde_json::Value> {
    let id: Uuid = sqlx::query_scalar("DELETE FROM api_tokens WHERE id = $1 RETURNING id")
        .bind(id)
        .fetch_optional(pool(&state)?)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    audit(pool(&state)?, "token.revoked", "api_token", id, None).await?;
    Ok(Json(serde_json::json!({"deleted": id})))
}

fn artifact_path(id: Uuid) -> PathBuf {
    std::env::var("CICD_ARTIFACTS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/var/lib/forge/artifacts"))
        .join(format!("{id}.bin"))
}
fn io_error(error: std::io::Error) -> ApiError {
    ApiError::internal(sqlx::Error::Io(error))
}
fn sha256(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
fn valid_role(role: &str) -> bool {
    matches!(role, "admin" | "maintainer" | "developer" | "viewer")
}
fn valid_cron(value: &str) -> bool {
    value.split_whitespace().count() == 5
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
        assert!(valid_cron("0 4 * * 1"));
        assert!(!valid_cron("not cron"));
    }
    #[test]
    fn roles_are_allowlisted() {
        assert!(valid_role("admin"));
        assert!(!valid_role("owner"));
    }
}
