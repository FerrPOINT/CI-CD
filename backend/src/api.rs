use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use uuid::Uuid;

use crate::{
    domain::JobStatus,
    git_host::{
        create_repository, delete_repository, git_info_refs, git_service_endpoint,
        internal_git_push, list_repositories,
    },
    platform::audit,
    pulls::{
        compare_refs, create_pull_request, list_commits, list_pull_requests, list_refs, pr_action,
    },
    store::next_log_sequence,
};

static STATE_POOL: std::sync::OnceLock<sqlx::PgPool> = std::sync::OnceLock::new();
tokio::task_local! {
    static REQUEST_ID: uuid::Uuid;
}

pub struct AppState {
    pub pool: Option<PgPool>,
    pub git: crate::git_host::GitConfig,
    pub running_jobs: Option<crate::runner::RunningJobs>,
}

type ApiResult<T> = Result<Json<T>, ApiError>;

/// OpenAPI 3 document for the current API surface (API_CONTRACT, utoipa).
#[derive(utoipa::OpenApi)]
#[openapi(
    info(title = "Forge CI/CD API", version = "0.1.0", description = "Self-hosted CI/CD control plane. Array responses and error envelope follow docs/contracts/API_CONTRACT.md current compatibility mode."),
    paths(
        health, auth_login, auth_refresh,
        list_projects, create_project, get_project, update_project, delete_project,
        trigger_pipeline, list_pipelines, get_pipeline, cancel_pipeline, retry_pipeline,
        change_job_status, retry_job, list_logs, append_log,
        crate::platform::list_runners, crate::platform::register_runner,
        crate::platform::runner_heartbeat, crate::platform::delete_runner,
        crate::platform::list_secrets, crate::platform::create_secret, crate::platform::delete_secret,
        crate::platform::list_artifacts, crate::platform::upload_artifact, crate::platform::download_artifact,
        crate::platform::list_environments, crate::platform::create_environment,
        crate::platform::update_environment, crate::platform::delete_environment,
        crate::platform::list_deployments, crate::platform::create_deployment,
        crate::platform::list_schedules, crate::platform::create_schedule,
        crate::platform::update_schedule, crate::platform::delete_schedule,
        crate::platform::list_webhooks, crate::platform::create_webhook, crate::platform::delete_webhook,
        crate::platform::list_notifications, crate::platform::replace_notifications,
        crate::platform::project_report, crate::platform::list_audit_log,
        crate::platform::list_users, crate::platform::create_user, crate::platform::update_user,
        crate::platform::list_tokens, crate::platform::create_token, crate::platform::delete_token,
        crate::git_host::git_info_refs, crate::git_host::git_service_endpoint, crate::git_host::internal_git_push,
        crate::pulls::list_refs, crate::pulls::list_commits, crate::pulls::compare_refs,
        crate::pulls::list_pull_requests, crate::pulls::create_pull_request, crate::pulls::pr_action,
    ),
    components(schemas(
        crate::auth::LoginRequest, crate::auth::TokenPair,
        Project, CreateProject, UpdateProject, TriggerPipeline, Pipeline, Stage, Job,
        PipelineDetail, StageDetail, JobLog, ChangeStatus,
        crate::platform::Runner, crate::platform::RegisterRunner, crate::platform::RunnerHeartbeat,
        crate::platform::SecretMetadata, crate::platform::CreateSecret,
        crate::platform::Artifact,
        crate::platform::Environment, crate::platform::CreateEnvironment, crate::platform::UpdateEnvironment,
        crate::platform::Deployment, crate::platform::CreateDeployment,
        crate::platform::Schedule, crate::platform::ScheduleInput,
        crate::platform::Webhook, crate::platform::CreateWebhook,
        crate::platform::Notification, crate::platform::NotificationInput,
        crate::platform::Report, crate::platform::AuditEvent,
        crate::platform::User, crate::platform::UserInput,
        crate::platform::ApiToken, crate::platform::CreatedToken, crate::platform::CreateToken,
        crate::git_host::Repository, crate::git_host::GitPushEvent,
        crate::pulls::RefInfo, crate::pulls::CommitInfo, crate::pulls::DiffResult, crate::pulls::DiffFile,
        crate::pulls::PullRequest, crate::pulls::CreatePullRequest, crate::pulls::PrAction,
    )),
    tags(
        (name = "health", description = "Liveness/readiness"),
        (name = "auth", description = "Login and token refresh"),
        (name = "projects", description = "Project registry"),
        (name = "pipelines", description = "Pipeline lifecycle"),
        (name = "jobs", description = "Jobs, logs and retries"),
        (name = "runners", description = "Runner registration and heartbeats"),
        (name = "secrets", description = "Encrypted project secrets"),
        (name = "artifacts", description = "Job artifact upload and download"),
        (name = "environments", description = "Environments and deployments"),
        (name = "schedules", description = "Cron-style pipeline schedules"),
        (name = "webhooks", description = "Outgoing project webhooks"),
        (name = "notifications", description = "Notification channel configuration"),
        (name = "reports", description = "Project delivery reports"),
        (name = "audit", description = "Audit log"),
        (name = "users", description = "User management"),
        (name = "tokens", description = "Personal API tokens"),
        (name = "git", description = "Git Smart HTTP and internal push events"),
        (name = "repos", description = "Repository refs, commits and diff"),
        (name = "pulls", description = "Pull requests"),
    )
)]
pub struct ApiDoc;

pub(crate) async fn serve_openapi_json() -> Json<serde_json::Value> {
    use utoipa::OpenApi as _;
    Json(serde_json::to_value(ApiDoc::openapi()).expect("serialize openapi"))
}

/// Canonical YAML serialization of the OpenAPI document (openapi-dump bin).
pub fn openapi_yaml() -> Result<String, serde_yaml::Error> {
    use utoipa::OpenApi as _;
    serde_yaml::to_string(&ApiDoc::openapi())
}

#[derive(Debug)]
pub struct ApiError {
    pub(crate) status: StatusCode,
    pub(crate) message: String,
}
impl ApiError {
    pub(crate) fn unavailable() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: "database is unavailable".into(),
        }
    }
    pub(crate) fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }
    pub(crate) fn not_found_named(what: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: what.into(),
        }
    }
    pub(crate) fn not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: "resource not found".into(),
        }
    }
    pub(crate) fn too_many_requests() -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: "rate limit exceeded".into(),
        }
    }
    pub(crate) fn forbidden() -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: "forbidden".into(),
        }
    }
    pub(crate) fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: "unauthorized".into(),
        }
    }
    pub(crate) fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
        }
    }
    fn code(&self) -> &'static str {
        match self.status {
            StatusCode::BAD_REQUEST => "invalid_request",
            StatusCode::UNAUTHORIZED => "unauthorized",
            StatusCode::FORBIDDEN => "permission_denied",
            StatusCode::NOT_FOUND => "not_found",
            StatusCode::CONFLICT => "conflict",
            StatusCode::SERVICE_UNAVAILABLE => "unavailable",
            _ => "internal_error",
        }
    }
    pub(crate) fn internal(error: sqlx::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
        }
    }
}
impl From<crate::auth::AuthError> for ApiError {
    fn from(error: crate::auth::AuthError) -> Self {
        match error {
            crate::auth::AuthError::InvalidCredentials
            | crate::auth::AuthError::Expired
            | crate::auth::AuthError::Invalid
            | crate::auth::AuthError::NotConfigured => ApiError::unauthorized(),
            crate::auth::AuthError::Db(e) => ApiError::internal(e),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        // API_CONTRACT error envelope: request_id is required; code is stable snake_case.
        let request_id = REQUEST_ID
            .try_with(|u| u.to_string())
            .unwrap_or_else(|_| uuid::Uuid::nil().to_string());
        (
            self.status,
            [(
                axum::http::header::HeaderName::from_static("x-request-id"),
                request_id.clone(),
            )],
            Json(serde_json::json!({
                "error": {
                    "code": self.code(),
                    "message": self.message,
                    "request_id": request_id,
                }
            })),
        )
            .into_response()
    }
}

pub(crate) fn pool(state: &AppState) -> Result<&PgPool, ApiError> {
    state.pool.as_ref().ok_or_else(ApiError::unavailable)
}

pub fn app(pool: Option<PgPool>) -> Router {
    app_with(pool, None)
}

pub fn app_with_git(
    pool: Option<PgPool>,
    git: crate::git_host::GitConfig,
    running: Option<crate::runner::RunningJobs>,
) -> Router {
    build_router(pool, git, running)
}

#[allow(dead_code)]
fn app_with(pool: Option<PgPool>, running: Option<crate::runner::RunningJobs>) -> Router {
    build_router(pool, crate::git_host::GitConfig::default(), running)
}

/// AUTHZ_CONTRACT Phase 1: when CICD_AUTH_SECRET is configured, every
/// /api/v1 route except the public allowlist requires a valid Bearer JWT.
/// Without the secret the API stays in trusted-network mode (open), matching
/// CURRENT_STATE.
async fn request_id_mw(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::http::HeaderName;
    let id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| uuid::Uuid::parse_str(v).ok())
        .unwrap_or_else(uuid::Uuid::new_v4);
    let mut response = REQUEST_ID.scope(id, next.run(req)).await;
    response.headers_mut().insert(
        HeaderName::from_static("x-request-id"),
        id.to_string().parse().unwrap(),
    );
    response
}

async fn require_auth(
    State(state): State<Arc<AppState>>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, ApiError> {
    const PUBLIC: &[&str] = &[
        "/api/v1/health",
        "/api/v1/openapi.json",
        "/api/v1/auth/login",
        "/api/v1/auth/refresh",
        "/metrics",
    ];
    let path = req.uri().path();
    if PUBLIC.contains(&path) || path.starts_with("/git/") {
        return Ok(next.run(req).await);
    }
    if std::env::var("CICD_AUTH_SECRET")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .is_none()
    {
        return Ok(next.run(req).await); // trusted-network mode: no enforcement
    }
    let claims = bearer_identity(req.headers())
        .await
        .map_err(|_| ApiError::unauthorized())?;
    let role = crate::authz::Role::parse(&claims.role).ok_or_else(ApiError::unauthorized)?;
    if !user_enabled(&state, claims.sub).await? {
        return Err(ApiError::unauthorized());
    }
    let method = req.method().as_str().to_string();
    let path = req.uri().path().to_string();
    let (mut parts, body) = req.into_parts();
    parts.extensions.insert(claims.clone());
    let req = axum::extract::Request::from_parts(parts, body);
    if !crate::authz::allows(role, &method, &path) {
        if let Some(pool) = state.pool.as_ref() {
            let _ = audit(
                pool,
                "auth.denied",
                "route",
                claims.sub,
                Some(&format!("{method} {path}")),
            )
            .await;
        }
        return Err(ApiError::forbidden());
    }
    Ok(next.run(req).await)
}

/// JWT access tokens carry the role at issue time; PATs (`cicd_…`) are
/// resolved against api_tokens and assume the owner's role.
async fn bearer_identity(
    headers: &axum::http::HeaderMap,
) -> Result<crate::auth::AccessClaims, ApiError> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(ApiError::unauthorized)?;
    let token = value
        .strip_prefix("Bearer ")
        .ok_or_else(ApiError::unauthorized)?;
    if token.starts_with("cicd_") {
        let hash = crate::auth::hash_token(token);
        let row = sqlx::query_as::<_, (Uuid, String)>(
            "SELECT u.id, u.role FROM api_tokens t JOIN users u ON u.id = t.user_id \
             WHERE t.token_hash = $1 AND u.enabled \
               AND (t.expires_at IS NULL OR t.expires_at > now())",
        )
        .bind(&hash)
        .fetch_optional(STATE_POOL.get().ok_or_else(ApiError::unavailable)?)
        .await
        .map_err(ApiError::internal)?;
        let (sub, role) = row.ok_or_else(ApiError::unauthorized)?;
        // Touch last_used_at best-effort.
        let _ = sqlx::query("UPDATE api_tokens SET last_used_at = now() WHERE token_hash = $1")
            .bind(&hash)
            .execute(STATE_POOL.get().ok_or_else(ApiError::unavailable)?)
            .await;
        let now = chrono::Utc::now();
        Ok(crate::auth::AccessClaims {
            sub,
            role,
            iat: now.timestamp(),
            exp: now.timestamp() + 900,
        })
    } else {
        crate::auth::verify_access(token).map_err(|_| ApiError::unauthorized())
    }
}

async fn user_enabled(state: &AppState, user_id: uuid::Uuid) -> Result<bool, ApiError> {
    let Some(pool) = state.pool.as_ref() else {
        return Err(ApiError::unavailable());
    };
    let enabled = sqlx::query_scalar::<_, bool>("SELECT enabled FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal)?
        .unwrap_or(false);
    Ok(enabled)
}

fn build_router(
    pool: Option<PgPool>,
    git: crate::git_host::GitConfig,
    running: Option<crate::runner::RunningJobs>,
) -> Router {
    if let Some(p) = pool.as_ref() {
        let _ = STATE_POOL.set(p.clone());
    }
    Router::new()
        .route("/api/v1/health", get(health))
        .route(
            "/metrics",
            get(|| async {
                (
                    [(
                        axum::http::header::CONTENT_TYPE,
                        "text/plain; version=0.0.4",
                    )],
                    crate::metrics::render(),
                )
            }),
        )
        .route("/api/v1/openapi.json", get(serve_openapi_json))
        .route("/api/v1/auth/login", post(auth_login))
        .route("/api/v1/auth/refresh", post(auth_refresh))
        .merge(crate::platform::routes())
        .route("/api/v1/projects", get(list_projects).post(create_project))
        .route(
            "/api/v1/projects/{project_id}",
            get(get_project)
                .patch(update_project)
                .delete(delete_project),
        )
        .route(
            "/api/v1/projects/{project_id}/pipelines",
            get(list_pipelines).post(trigger_pipeline),
        )
        .route("/api/v1/pipelines/{pipeline_id}", get(get_pipeline))
        .route("/api/v1/jobs/{job_id}/status", post(change_job_status))
        .route(
            "/api/v1/pipelines/{pipeline_id}/cancel",
            post(cancel_pipeline),
        )
        .route(
            "/api/v1/pipelines/{pipeline_id}/retry",
            post(retry_pipeline),
        )
        .route("/api/v1/jobs/{job_id}/retry", post(retry_job))
        .route(
            "/api/v1/jobs/{job_id}/logs",
            get(list_logs).post(append_log),
        )
        .route("/api/v1/jobs/{job_id}/start", post(start_manual_job))
        .route("/api/v1/jobs/{job_id}/logs/stream", get(job_log_stream))
        .route(
            "/api/v1/repositories",
            get(list_repositories).post(create_repository),
        )
        .route(
            "/api/v1/repositories/{name}",
            axum::routing::delete(delete_repository),
        )
        .route("/git/{repo}/info/refs", get(git_info_refs))
        .route("/git/{repo}/git-upload-pack", post(git_service_endpoint))
        .route("/git/{repo}/git-receive-pack", post(git_service_endpoint))
        .route("/api/v1/internal/git-push", post(internal_git_push))
        .route("/api/v1/repos/{repo}/refs", get(list_refs))
        .route("/api/v1/repos/{repo}/tree", get(crate::pulls::list_tree))
        .route("/api/v1/repos/{repo}/blob", get(crate::pulls::get_blob))
        .route("/api/v1/repos/{repo}/tags", get(crate::pulls::list_tags))
        .route(
            "/api/v1/repos/{repo}/releases",
            get(list_releases).post(create_release),
        )
        .route(
            "/api/v1/repos/{repo}/releases/{tag}",
            get(get_release).delete(delete_release),
        )
        .route(
            "/api/v1/pipelines/{pipeline_id}/badge.svg",
            get(pipeline_badge),
        )
        .route(
            "/api/v1/jobs/{job_id}/test-report",
            get(get_test_report).post(upload_test_report),
        )
        .route(
            "/api/v1/pipelines/{pipeline_id}/variables",
            get(pipeline_variables),
        )
        .route("/api/v1/repos/{repo}/commits", get(list_commits))
        .route("/api/v1/repos/{repo}/compare", get(compare_refs))
        .route(
            "/api/v1/repos/{repo}/pulls",
            get(list_pull_requests).post(create_pull_request),
        )
        .route(
            "/api/v1/repos/{repo}/pulls/{number}/action",
            post(pr_action),
        )
        .layer(axum::middleware::from_fn_with_state(
            Arc::new(AppState {
                pool: pool.clone(),
                git: git.clone(),
                running_jobs: running.clone(),
            }),
            require_auth,
        ))
        .layer(axum::middleware::from_fn(request_id_mw))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(Arc::new(AppState {
            pool: pool.clone(),
            git,
            running_jobs: running,
        }))
}

#[utoipa::path(post, path="/api/v1/auth/login", tag="auth", request_body=crate::auth::LoginRequest, responses((status=200, body=crate::auth::TokenPair), (status=401)))]
async fn auth_login(
    State(state): State<Arc<AppState>>,
    Json(input): Json<crate::auth::LoginRequest>,
) -> Result<Json<crate::auth::TokenPair>, ApiError> {
    use crate::auth::*;
    crate::metrics::LOGIN_ATTEMPTS_TOTAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    {
        static LIMITER: std::sync::OnceLock<crate::rate_limit::RateLimiter> =
            std::sync::OnceLock::new();
        let limiter = LIMITER.get_or_init(crate::rate_limit::RateLimiter::default);
        limiter.prune(60);
        if !limiter.allow("login:global", 30, 60) {
            return Err(ApiError::too_many_requests());
        }
    }
    let pool = pool(&state)?;
    let row = sqlx::query_as::<_, (Uuid, String, bool, String)>(
        "SELECT u.id, u.role, u.enabled, c.password_hash FROM users u \
         JOIN user_credentials c ON c.user_id = u.id WHERE u.username = $1",
    )
    .bind(input.username.trim())
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::unauthorized)?;
    let (user_id, role, enabled, password_hash) = row;
    if !enabled || !verify_password(&password_hash, &input.password) {
        let _ = audit(
            pool,
            "auth.login_failed",
            "user",
            user_id,
            Some(input.username.trim()),
        )
        .await;
        return Err(ApiError::unauthorized());
    }
    let _ = audit(
        pool,
        "auth.login_success",
        "user",
        user_id,
        Some(input.username.trim()),
    )
    .await;
    let refresh = new_refresh_token();
    create_session(pool, user_id, &hash_token(&refresh))
        .await
        .map_err(ApiError::from)?;
    let mut pair = issue_access(user_id, &role).map_err(|_| ApiError::unauthorized())?;
    pair.refresh_token = refresh;
    Ok(Json(pair))
}

#[utoipa::path(post, path="/api/v1/auth/refresh", tag="auth", request_body=crate::auth::LoginRequest, responses((status=200, body=crate::auth::TokenPair), (status=401)))]
async fn auth_refresh(
    State(state): State<Arc<AppState>>,
    Json(input): Json<crate::auth::LoginRequest>,
) -> Result<Json<crate::auth::TokenPair>, ApiError> {
    // Reuses LoginRequest shape; only refresh_token matters here.
    use crate::auth::*;
    let pool = pool(&state)?;
    if input.refresh_token.is_empty() {
        return Err(ApiError::unauthorized());
    }
    let (user_id, new_refresh) = rotate_session(pool, &hash_token(&input.refresh_token))
        .await
        .map_err(|_| ApiError::unauthorized())?;
    let role = sqlx::query_scalar::<_, String>("SELECT role FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .map_err(ApiError::internal)?;
    let mut pair = issue_access(user_id, &role).map_err(|_| ApiError::unauthorized())?;
    pair.refresh_token = new_refresh;
    Ok(Json(pair))
}

#[utoipa::path(get, path="/api/v1/health", tag="health", responses((status=200, description="Liveness and readiness")))]
async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok", "service": "cicd"}))
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
struct Project {
    id: Uuid,
    name: String,
    repository_url: String,
    default_branch: String,
    created_at: DateTime<Utc>,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct CreateProject {
    name: String,
    repository_url: String,
    default_branch: Option<String>,
}

#[derive(serde::Deserialize, utoipa::IntoParams)]
struct PageParams {
    /// Max items (1..=200, default 50).
    #[serde(default)]
    limit: Option<i64>,
    /// Offset (default 0).
    #[serde(default)]
    offset: Option<i64>,
}

impl PageParams {
    fn bounded(&self) -> (i64, i64) {
        let limit = self.limit.unwrap_or(50).clamp(1, 200);
        let offset = self.offset.unwrap_or(0).max(0);
        (limit, offset)
    }
}

#[utoipa::path(post, path="/api/v1/projects", tag="projects", request_body=CreateProject, responses((status=200, body=Project), (status=400, description="validation error")))]
async fn create_project(
    State(state): State<Arc<AppState>>,
    Json(input): Json<CreateProject>,
) -> ApiResult<Project> {
    if input.name.trim().is_empty() || input.repository_url.trim().is_empty() {
        return Err(ApiError::bad_request(
            "name and repository_url are required",
        ));
    }
    let project = sqlx::query_as::<_, Project>(
        "INSERT INTO projects (id, name, repository_url, default_branch) VALUES ($1, $2, $3, $4) RETURNING id, name, repository_url, default_branch, created_at"
    ).bind(Uuid::new_v4()).bind(input.name.trim()).bind(input.repository_url.trim()).bind(input.default_branch.unwrap_or_else(|| "main".into())).fetch_one(pool(&state)?).await.map_err(ApiError::internal)?;
    Ok(Json(project))
}

#[utoipa::path(get, path="/api/v1/projects", tag="projects", params(PageParams), responses((status=200, body=[Project])))]
async fn list_projects(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(page): axum::extract::Query<PageParams>,
) -> ApiResult<Vec<Project>> {
    let (limit, offset) = page.bounded();
    let projects = sqlx::query_as::<_, Project>("SELECT id, name, repository_url, default_branch, created_at FROM projects ORDER BY created_at DESC LIMIT $1 OFFSET $2")
        .bind(limit)
        .bind(offset)
        .fetch_all(pool(&state)?)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(projects))
}

#[utoipa::path(get, path="/api/v1/projects/{project_id}", tag="projects", params(("project_id"=Uuid, Path)), responses((status=200, body=Project), (status=404)))]
async fn get_project(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
) -> ApiResult<Project> {
    let project = sqlx::query_as::<_, Project>(
        "SELECT id, name, repository_url, default_branch, created_at FROM projects WHERE id = $1",
    )
    .bind(project_id)
    .fetch_optional(pool(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::not_found)?;
    Ok(Json(project))
}

#[derive(Debug, Deserialize, Default, utoipa::ToSchema)]
struct UpdateProject {
    name: Option<String>,
    repository_url: Option<String>,
    default_branch: Option<String>,
}

#[utoipa::path(patch, path="/api/v1/projects/{project_id}", tag="projects", request_body=UpdateProject, params(("project_id"=Uuid, Path)), responses((status=200, body=Project), (status=404)))]
async fn update_project(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<UpdateProject>,
) -> ApiResult<Project> {
    if let (None, None, None) = (&input.name, &input.repository_url, &input.default_branch) {
        return Err(ApiError::bad_request(
            "at least one of name, repository_url, default_branch is required",
        ));
    }
    for field in [&input.name, &input.repository_url, &input.default_branch]
        .into_iter()
        .flatten()
    {
        if field.trim().is_empty() {
            return Err(ApiError::bad_request("fields cannot be empty"));
        }
    }
    let project = sqlx::query_as::<_, Project>(
        "UPDATE projects SET name = COALESCE($2, name), repository_url = COALESCE($3, repository_url), default_branch = COALESCE($4, default_branch) WHERE id = $1 RETURNING id, name, repository_url, default_branch, created_at",
    )
    .bind(project_id)
    .bind(input.name.as_deref().map(str::trim))
    .bind(input.repository_url.as_deref().map(str::trim))
    .bind(input.default_branch.as_deref().map(str::trim))
    .fetch_optional(pool(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::not_found)?;
    Ok(Json(project))
}

#[utoipa::path(delete, path="/api/v1/projects/{project_id}", tag="projects", params(("project_id"=Uuid, Path)), responses((status=200), (status=404)))]
async fn delete_project(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
) -> ApiResult<serde_json::Value> {
    let deleted = sqlx::query_scalar::<_, Uuid>("DELETE FROM projects WHERE id = $1 RETURNING id")
        .bind(project_id)
        .fetch_optional(pool(&state)?)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(serde_json::json!({"deleted": deleted})))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct TriggerPipeline {
    git_ref: Option<String>,
    /// Optional run variables; exposed to every job as CICD_VAR_<KEY>.
    #[serde(default)]
    variables: Option<std::collections::BTreeMap<String, String>>,
}
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct Pipeline {
    pub(crate) id: Uuid,
    pub(crate) project_id: Uuid,
    pub(crate) git_ref: String,
    pub(crate) status: String,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) started_at: Option<DateTime<Utc>>,
    pub(crate) finished_at: Option<DateTime<Utc>>,
}
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
struct Stage {
    id: Uuid,
    pipeline_id: Uuid,
    name: String,
    position: i32,
    status: String,
}
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
struct Job {
    id: Uuid,
    stage_id: Uuid,
    name: String,
    image: String,
    command: String,
    position: i32,
    status: String,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
struct PipelineDetail {
    pipeline: Pipeline,
    stages: Vec<StageDetail>,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
struct StageDetail {
    #[serde(flatten)]
    stage: Stage,
    jobs: Vec<Job>,
}

#[utoipa::path(post, path="/api/v1/projects/{project_id}/pipelines", tag="pipelines", request_body=TriggerPipeline, params(("project_id"=Uuid, Path)), responses((status=200, body=Pipeline), (status=404)))]
async fn trigger_pipeline(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<TriggerPipeline>,
) -> ApiResult<PipelineDetail> {
    let pool = pool(&state)?;
    let project_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM projects WHERE id = $1)")
            .bind(project_id)
            .fetch_one(pool)
            .await
            .map_err(ApiError::internal)?;
    if !project_exists {
        return Err(ApiError::not_found());
    }
    let git_ref = input.git_ref.unwrap_or_else(|| "main".into());
    let pipeline = create_pipeline_with_vars(
        pool,
        project_id,
        git_ref,
        serde_json::to_value(input.variables.clone().unwrap_or_default())
            .unwrap_or_else(|_| serde_json::json!({})),
    )
    .await?;
    pipeline_detail(pool, pipeline.id).await.map(Json)
}

/// Creates a queued pipeline. Stage/job structure comes from `.forge-ci.yml`
/// in the project repository at the given ref when available; otherwise the
/// default build/test/deploy template is used.
pub(crate) async fn create_pipeline(
    pool: &PgPool,
    project_id: Uuid,
    git_ref: String,
) -> Result<Pipeline, ApiError> {
    create_pipeline_with_vars(pool, project_id, git_ref, serde_json::json!({})).await
}

pub(crate) async fn create_pipeline_with_vars(
    pool: &PgPool,
    project_id: Uuid,
    git_ref: String,
    variables: serde_json::Value,
) -> Result<Pipeline, ApiError> {
    let repository_url: Option<String> =
        sqlx::query_scalar("SELECT repository_url FROM projects WHERE id = $1")
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(ApiError::internal)?
            .flatten();
    // Never clone here: this path is called by post-receive and must return
    // before git-receive-pack finishes. Local config is read from the bare repo.
    let config = read_local_forge_ci_config(repository_url.as_deref(), &git_ref).await;
    let stages = parse_forge_ci(config.as_deref()).map_err(ApiError::bad_request)?;
    let commit_sha = resolve_commit_sha(repository_url.as_deref(), &git_ref).await;

    let pipeline = sqlx::query_as::<_, Pipeline>(
        "INSERT INTO pipelines (id, project_id, git_ref, commit_sha, variables, status) VALUES ($1, $2, $3, $4, $5, 'queued') RETURNING id, project_id, git_ref, status, created_at, started_at, finished_at",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(&git_ref)
    .bind(&commit_sha)
    .bind(&variables)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)?;
    for (position, stage) in stages.iter().enumerate() {
        let stage_id = Uuid::new_v4();
        sqlx::query("INSERT INTO stages (id, pipeline_id, name, position, status) VALUES ($1, $2, $3, $4, 'queued')")
            .bind(stage_id).bind(pipeline.id).bind(&stage.name).bind(position as i32).execute(pool).await.map_err(ApiError::internal)?;
        for (job_position, job) in stage.jobs.iter().enumerate() {
            sqlx::query("INSERT INTO jobs (id, stage_id, name, image, command, position, status, timeout_seconds, allow_failure, manual) VALUES ($1, $2, $3, $4, $5, $6, 'queued', $7, $8, $9)")
                .bind(Uuid::new_v4()).bind(stage_id).bind(&job.name).bind(&job.image).bind(&job.command).bind(job_position as i32)
                .bind(job.timeout_seconds).bind(job.allow_failure).bind(job.manual)
                .execute(pool).await.map_err(ApiError::internal)?;
        }
    }
    Ok(pipeline)
}

#[derive(Debug, Default)]
struct CiStage {
    name: String,
    jobs: Vec<CiJob>,
}
#[derive(Debug)]
struct CiJob {
    name: String,
    image: String,
    command: String,
    timeout_seconds: Option<i32>,
    allow_failure: bool,
    manual: bool,
}

/// Reads `.forge-ci.yml` from an already-pushed local bare repository.
/// External URLs deliberately use the template: cloning during post-receive
/// could wait on the same Smart HTTP request that is still completing.
async fn read_local_forge_ci_config(repo_url: Option<&str>, git_ref: &str) -> Option<String> {
    let name = extract_repo_name_from_url(repo_url?)?;
    let git_root = std::env::var("CICD_GIT_ROOT").unwrap_or_else(|_| "/var/lib/forge/git".into());
    let bare_path = std::path::Path::new(&git_root).join(format!("{name}.git"));
    if !bare_path.is_dir() {
        return None;
    }

    let output = tokio::process::Command::new("git")
        .arg(format!("--git-dir={}", bare_path.display()))
        .args(["show", &format!("{git_ref}:.forge-ci.yml")])
        .output()
        .await
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Resolves a ref to a commit sha in the local bare repo (best-effort).
async fn resolve_commit_sha(repo_url: Option<&str>, git_ref: &str) -> Option<String> {
    let name = extract_repo_name_from_url(repo_url?)?;
    let git_root = std::env::var("CICD_GIT_ROOT").unwrap_or_else(|_| "/var/lib/forge/git".into());
    let bare_path = std::path::Path::new(&git_root).join(format!("{name}.git"));
    if !bare_path.is_dir() {
        return None;
    }
    let output = tokio::process::Command::new("git")
        .arg(format!("--git-dir={}", bare_path.display()))
        .args(["rev-parse", git_ref])
        .output()
        .await
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Extracts the repository name from a URL like `http://host/git/name.git`.
fn extract_repo_name_from_url(url: &str) -> Option<String> {
    let path = url.split('/').next_back()?;
    let name = path.strip_suffix(".git").unwrap_or(path);
    Some(name.to_string())
}

/// Parses `.forge-ci.yml`:
/// ```yaml
/// stages:
///   - name: build
///     jobs:
///       - name: compile
///         image: rust:1.86
///         command: cargo build --release
/// ```
fn parse_forge_ci(raw: Option<&str>) -> Result<Vec<CiStage>, String> {
    #[derive(serde::Deserialize)]
    struct YamlJob {
        name: String,
        #[serde(default = "default_image")]
        image: String,
        command: String,
        /// e.g. "30s", "5m", "1h" — default 1h.
        #[serde(default)]
        timeout: Option<String>,
        /// Job failure does not fail the stage/pipeline.
        #[serde(default)]
        allow_failure: bool,
        /// Waits for an explicit start (approval gate).
        #[serde(default)]
        when: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct YamlStage {
        name: String,
        #[serde(default)]
        jobs: Vec<YamlJob>,
    }
    #[derive(serde::Deserialize)]
    struct YamlConfig {
        #[serde(default)]
        stages: Vec<YamlStage>,
    }
    fn default_image() -> String {
        "alpine:3.21".into()
    }

    fn parse_timeout(raw: Option<&str>) -> Option<i32> {
        let raw = raw?.trim();
        let (value, unit) =
            raw.split_at(raw.find(|c: char| !c.is_ascii_digit()).unwrap_or(raw.len()));
        let value: i32 = value.parse().ok()?;
        match unit.trim() {
            "s" | "sec" | "secs" | "" => Some(value),
            "m" | "min" | "mins" => value.checked_mul(60),
            "h" | "hour" | "hours" => value.checked_mul(3600),
            _ => None,
        }
    }

    let Some(raw) = raw else {
        return Ok(default_pipeline());
    };
    let parsed: YamlConfig =
        serde_yaml::from_str(raw).map_err(|error| format!("invalid .forge-ci.yml: {error}"))?;
    let mut stages: Vec<CiStage> = parsed
        .stages
        .into_iter()
        .map(|stage| CiStage {
            name: stage.name,
            jobs: stage
                .jobs
                .into_iter()
                .map(|job| CiJob {
                    name: job.name,
                    image: job.image,
                    command: job.command,
                    timeout_seconds: parse_timeout(job.timeout.as_deref()),
                    allow_failure: job.allow_failure,
                    manual: job.when.as_deref() == Some("manual"),
                })
                .collect(),
        })
        .collect();
    // Drop stages without jobs (nothing to execute).
    stages.retain(|stage| !stage.jobs.is_empty());
    validate_ci_stages(&stages)?;
    Ok(stages)
}

fn default_pipeline() -> Vec<CiStage> {
    vec![
        CiStage {
            name: "build".into(),
            jobs: vec![CiJob {
                name: "checkout".into(),
                image: "alpine/git:latest".into(),
                command: "git fetch --all".into(),
                timeout_seconds: None,
                allow_failure: false,
                manual: false,
            }],
        },
        CiStage {
            name: "test".into(),
            jobs: vec![CiJob {
                name: "unit-tests".into(),
                image: "rust:1.86".into(),
                command: "cargo test".into(),
                timeout_seconds: None,
                allow_failure: false,
                manual: false,
            }],
        },
        CiStage {
            name: "deploy".into(),
            jobs: vec![CiJob {
                name: "deploy".into(),
                image: "alpine:3.21".into(),
                timeout_seconds: None,
                allow_failure: false,
                manual: false,
                command: "echo deploy".into(),
            }],
        },
    ]
}

fn validate_ci_stages(stages: &[CiStage]) -> Result<(), String> {
    if stages.is_empty() {
        return Err(".forge-ci.yml must define at least one stage".into());
    }
    for stage in stages {
        if stage.name.trim().is_empty() || stage.jobs.is_empty() {
            return Err("every stage must have a name and at least one job".into());
        }
        for job in &stage.jobs {
            if job.name.trim().is_empty()
                || job.command.trim().is_empty()
                || !is_safe_image_reference(&job.image)
            {
                return Err("every job needs a name, command, and safe image reference".into());
            }
        }
    }
    Ok(())
}

fn is_safe_image_reference(image: &str) -> bool {
    !image.is_empty()
        && !image.starts_with('.')
        && !image.contains("..")
        && image.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '/' | ':' | '.' | '_' | '-')
        })
}

// ---- Releases (P1 git-server parity) ----

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
struct Release {
    id: Uuid,
    repository_name: String,
    tag_name: String,
    name: String,
    description: String,
    prerelease: bool,
    created_by: Option<String>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct CreateRelease {
    tag_name: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    prerelease: bool,
}

#[utoipa::path(get, path="/api/v1/repos/{repo}/releases", tag="releases", params(("repo"=String, Path)), responses((status=200, body=[Release])))]
async fn list_releases(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(repo): axum::extract::Path<String>,
) -> ApiResult<Vec<Release>> {
    let rows = sqlx::query_as::<_, Release>(
        "SELECT id, repository_name, tag_name, name, description, prerelease, created_by, created_at \
         FROM releases WHERE repository_name = $1 ORDER BY created_at DESC",
    )
    .bind(repo)
    .fetch_all(pool(&state)?)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(rows))
}

#[utoipa::path(post, path="/api/v1/repos/{repo}/releases", tag="releases", params(("repo"=String, Path)), request_body=CreateRelease, responses((status=200, body=Release)))]
async fn create_release(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(repo): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
    Json(input): Json<CreateRelease>,
) -> ApiResult<Release> {
    let created_by = bearer_identity(&headers)
        .await
        .ok()
        .map(|c| c.sub.to_string());
    if input.tag_name.trim().is_empty() || input.name.trim().is_empty() {
        return Err(ApiError::bad_request("tag_name and name are required"));
    }
    let row = sqlx::query_as::<_, Release>(
        "INSERT INTO releases (id, repository_name, tag_name, name, description, prerelease, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT (repository_name, tag_name) DO UPDATE SET \
           name = EXCLUDED.name, description = EXCLUDED.description, prerelease = EXCLUDED.prerelease \
         RETURNING id, repository_name, tag_name, name, description, prerelease, created_by, created_at",
    )
    .bind(Uuid::new_v4())
    .bind(repo.trim())
    .bind(input.tag_name.trim())
    .bind(input.name.trim())
    .bind(input.description)
    .bind(input.prerelease)
    .bind(created_by)
    .fetch_one(pool(&state)?)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(row))
}

#[utoipa::path(get, path="/api/v1/repos/{repo}/releases/{tag}", tag="releases", params(("repo"=String, Path), ("tag"=String, Path)), responses((status=200, body=Release), (status=404)))]
async fn get_release(
    State(state): State<Arc<AppState>>,
    axum::extract::Path((repo, tag)): axum::extract::Path<(String, String)>,
) -> ApiResult<Release> {
    let row = sqlx::query_as::<_, Release>(
        "SELECT id, repository_name, tag_name, name, description, prerelease, created_by, created_at \
         FROM releases WHERE repository_name = $1 AND tag_name = $2",
    )
    .bind(repo)
    .bind(tag)
    .fetch_optional(pool(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::not_found)?;
    Ok(Json(row))
}

#[utoipa::path(delete, path="/api/v1/repos/{repo}/releases/{tag}", tag="releases", params(("repo"=String, Path), ("tag"=String, Path)), responses((status=200), (status=404)))]
async fn delete_release(
    State(state): State<Arc<AppState>>,
    axum::extract::Path((repo, tag)): axum::extract::Path<(String, String)>,
) -> ApiResult<serde_json::Value> {
    let deleted = sqlx::query_scalar::<_, Uuid>(
        "DELETE FROM releases WHERE repository_name = $1 AND tag_name = $2 RETURNING id",
    )
    .bind(repo)
    .bind(tag)
    .fetch_optional(pool(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::not_found)?;
    Ok(Json(serde_json::json!({ "deleted": deleted.to_string() })))
}

// ---- Pipeline badge (P1, public read-only) ----

#[utoipa::path(get, path="/api/v1/pipelines/{pipeline_id}/badge.svg", tag="pipelines", params(("pipeline_id"=Uuid, Path)), responses((status=200, description="SVG badge")))]
async fn pipeline_badge(
    State(state): State<Arc<AppState>>,
    Path(pipeline_id): Path<Uuid>,
) -> axum::response::Response {
    let status: String = match state.pool.as_ref() {
        Some(pool) => sqlx::query_scalar("SELECT status FROM pipelines WHERE id = $1")
            .bind(pipeline_id)
            .fetch_one(pool)
            .await
            .unwrap_or_else(|_| "unknown".into()),
        None => "unknown".into(),
    };
    let (label, color) = match status.as_str() {
        "success" => ("passing", "#2ea44f"),
        "failed" => ("failed", "#d73a4a"),
        "running" | "queued" => ("running", "#dfb317"),
        "canceled" => ("canceled", "#959da5"),
        _ => ("unknown", "#959da5"),
    };
    let width = 60 + label.len() * 7;
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"20\">\
<linearGradient id=\"s\" x2=\"0\" y2=\"100%\"><stop offset=\"0\" stop-color=\"#bbb\" stop-opacity=\".1\"/><stop offset=\"1\" stop-opacity=\".1\"/></linearGradient>\
<clipPath id=\"r\"><rect width=\"{width}\" height=\"20\" rx=\"3\" fill=\"#fff\"/></clipPath>\
<g clip-path=\"url(#r)\"><rect width=\"46\" height=\"20\" fill=\"#555\"/><rect x=\"46\" width=\"{rw}\" height=\"20\" fill=\"{color}\"/><rect width=\"{width}\" height=\"20\" fill=\"url(#s)\"/></g>\
<g fill=\"#fff\" text-anchor=\"middle\" font-family=\"Verdana,sans-serif\" font-size=\"11\">\
<text x=\"23\" y=\"15\">build</text><text x=\"{tx}\" y=\"15\">{label}</text></g></svg>",
        width = width,
        rw = width - 46,
        tx = 46 + (width - 46) / 2,
        color = color,
        label = label,
    );
    axum::response::Response::builder()
        .status(200)
        .header("content-type", "image/svg+xml")
        .header("cache-control", "no-cache")
        .body(axum::body::Body::from(svg))
        .unwrap()
}

// ---- Pipeline variables (P1) ----

#[utoipa::path(get, path="/api/v1/pipelines/{pipeline_id}/variables", tag="pipelines", params(("pipeline_id"=Uuid, Path)), responses((status=200)))]
async fn pipeline_variables(
    State(state): State<Arc<AppState>>,
    Path(pipeline_id): Path<Uuid>,
) -> ApiResult<serde_json::Value> {
    let vars: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT variables FROM pipelines WHERE id = $1")
            .bind(pipeline_id)
            .fetch_optional(pool(&state)?)
            .await
            .map_err(ApiError::internal)?
            .ok_or_else(ApiError::not_found)?;
    Ok(Json(vars.unwrap_or_else(|| serde_json::json!({}))))
}

// ---- JUnit test reports (P1) ----

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
struct TestReport {
    id: Uuid,
    job_id: Uuid,
    suite_name: String,
    tests_total: i32,
    tests_passed: i32,
    tests_failed: i32,
    tests_skipped: i32,
    duration_ms: Option<i32>,
    created_at: DateTime<Utc>,
}

#[utoipa::path(get, path="/api/v1/jobs/{job_id}/test-report", tag="jobs", params(("job_id"=Uuid, Path)), responses((status=200, body=[TestReport])))]
async fn get_test_report(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
) -> ApiResult<Vec<TestReport>> {
    let rows = sqlx::query_as::<_, TestReport>(
        "SELECT id, job_id, suite_name, tests_total, tests_passed, tests_failed, tests_skipped, duration_ms, created_at \
         FROM test_reports WHERE job_id = $1 ORDER BY suite_name",
    )
    .bind(job_id)
    .fetch_all(pool(&state)?)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(rows))
}

/// Minimal JUnit XML parser: sums testsuite/testcase counts and failures.
#[utoipa::path(post, path="/api/v1/jobs/{job_id}/test-report", tag="jobs", params(("job_id"=Uuid, Path)), request_body=String, responses((status=200, body=[TestReport])))]
async fn upload_test_report(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
    Json(body): Json<String>,
) -> ApiResult<Vec<TestReport>> {
    // Delete previous reports for the job (idempotent re-upload).
    sqlx::query("DELETE FROM test_reports WHERE job_id = $1")
        .bind(job_id)
        .execute(pool(&state)?)
        .await
        .map_err(ApiError::internal)?;
    let mut reports = Vec::new();
    for suite in parse_junit(&body) {
        let row = sqlx::query_as::<_, TestReport>(
            "INSERT INTO test_reports (id, job_id, suite_name, tests_total, tests_passed, tests_failed, tests_skipped, duration_ms) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
             RETURNING id, job_id, suite_name, tests_total, tests_passed, tests_failed, tests_skipped, duration_ms, created_at",
        )
        .bind(Uuid::new_v4())
        .bind(job_id)
        .bind(&suite.name)
        .bind(suite.total)
        .bind(suite.passed)
        .bind(suite.failed)
        .bind(suite.skipped)
        .bind(suite.duration_ms)
        .fetch_one(pool(&state)?)
        .await
        .map_err(ApiError::internal)?;
        reports.push(row);
    }
    Ok(Json(reports))
}

#[derive(Debug)]
struct JunitSuite {
    name: String,
    total: i32,
    passed: i32,
    failed: i32,
    skipped: i32,
    duration_ms: Option<i32>,
}

/// Extracts `<testsuite ...>` attributes and counts `<testcase>` children with
/// failure/skipped children. String-scanning keeps the parser allocation-light
/// and dependency-free.
fn parse_junit(xml: &str) -> Vec<JunitSuite> {
    let mut out = Vec::new();
    // Iterate over each <testsuite ...> ... </testsuite> block by scanning
    // opening tags and slicing to the matching close.
    let mut search_from = 0usize;
    let mut suite_idx = 0;
    while let Some(open_rel) = xml[search_from..]
        .find("<testsuite ")
        .or_else(|| xml[search_from..].find("<testsuite>"))
    {
        let open_at = search_from + open_rel;
        let attrs_start = open_at
            + if xml[open_at..].starts_with("<testsuite ") {
                "<testsuite ".len()
            } else {
                "<testsuite>".len()
            };
        let attrs_end = xml[attrs_start..]
            .find('>')
            .map(|e| attrs_start + e)
            .unwrap_or(xml.len());
        let attrs = &xml[attrs_start..attrs_end];
        let body_start = (attrs_end + 1).min(xml.len());
        let close_at = xml[body_start..]
            .find("</testsuite>")
            .map(|e| body_start + e)
            .unwrap_or(xml.len());
        let body = &xml[body_start..close_at];
        suite_idx += 1;

        let get_int = |key: &str| -> Option<i32> {
            let pat = format!("{key}=\"");
            let start = attrs.find(&pat)? + pat.len();
            let rest = &attrs[start..];
            let end = rest.find('"')?;
            rest[..end].parse().ok()
        };
        let get_float = |key: &str| -> Option<f64> {
            let pat = format!("{key}=\"");
            let start = attrs.find(&pat)? + pat.len();
            let rest = &attrs[start..];
            let end = rest.find('"')?;
            rest[..end].parse().ok()
        };
        let name = {
            let pat = "name=\"";
            attrs.find(pat).and_then(|p| {
                let rest = &attrs[p + pat.len()..];
                rest.find('"').map(|e| rest[..e].to_string())
            })
        }
        .unwrap_or_else(|| format!("suite-{suite_idx}"));

        let total_cases = body.matches("<testcase").count() as i32;
        let failed = (body.matches("<failure").count() + body.matches("<error").count()) as i32;
        let skipped = body.matches("<skipped").count() as i32;
        let total = get_int("tests").unwrap_or(total_cases);
        out.push(JunitSuite {
            name,
            total,
            passed: (total - failed - skipped).max(0),
            failed,
            skipped,
            duration_ms: get_float("time").map(|s| (s * 1000.0) as i32),
        });
        search_from = (close_at + "</testsuite>".len()).min(xml.len());
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_junit_extracts_suite_names_and_counts() {
        let xml = r#"<?xml version="1.0"?>
<testsuites>
  <testsuite name="unit::core" tests="4" time="0.845">
    <testcase name="a"/><testcase name="b"/>
    <testcase name="c"><failure message="x"/></testcase>
    <testcase name="d"><skipped/></testcase>
  </testsuite>
  <testsuite name="integration::api" tests="2" time="1.9">
    <testcase name="e"/><testcase name="f"><error message="y"/></testcase>
  </testsuite>
</testsuites>"#;
        let suites = parse_junit(xml);
        assert_eq!(suites.len(), 2, "expected 2 suites, got {suites:?}");
        assert_eq!(suites[0].name, "unit::core");
        assert_eq!(suites[0].total, 4);
        assert_eq!(suites[0].failed, 1);
        assert_eq!(suites[0].skipped, 1);
        assert_eq!(suites[0].passed, 2);
        assert_eq!(suites[0].duration_ms, Some(845));
        assert_eq!(suites[1].name, "integration::api");
        assert_eq!(suites[1].failed, 1);
        assert_eq!(suites[1].duration_ms, Some(1900));
    }

    use super::*;

    #[test]
    fn parses_execution_controls() {
        let stages = parse_forge_ci(Some(
            r#"
stages:
  - name: build
    jobs:
      - name: compile
        command: make
        timeout: 30s
      - name: lint
        command: make lint
        allow_failure: true
  - name: deploy
    jobs:
      - name: prod
        command: ./deploy.sh
        when: manual
"#,
        ))
        .expect("valid configuration");
        assert_eq!(stages[0].jobs[0].timeout_seconds, Some(30));
        assert!(!stages[0].jobs[0].allow_failure);
        assert!(stages[0].jobs[1].allow_failure);
        assert!(stages[1].jobs[0].manual);
        assert_eq!(stages[1].jobs[0].timeout_seconds, None);
    }

    #[test]
    fn timeout_units_parse() {
        // parse_timeout lives inside parse_forge_ci; verify via full config.
        let stages = parse_forge_ci(Some(
            "stages:\n  - name: a\n    jobs:\n      - name: j\n        command: x\n        timeout: 5m\n",
        ))
        .expect("valid");
        assert_eq!(stages[0].jobs[0].timeout_seconds, Some(300));
    }

    #[test]
    fn parses_multiple_jobs_and_normalizes_optional_values() {
        let stages = parse_forge_ci(Some(
            r#"
stages:
  - name: build
    jobs:
      - name: compile
        image: rust:1.86
        command: cargo build --release
      - name: lint
        command: cargo fmt --check
  - name: test
    jobs:
      - name: unit
        image: rust:1.86
        command: cargo test
"#,
        ))
        .expect("valid configuration");

        assert_eq!(stages.len(), 2);
        assert_eq!(stages[0].name, "build");
        assert_eq!(stages[0].jobs.len(), 2);
        assert_eq!(stages[0].jobs[1].image, "alpine:3.21");
        assert_eq!(stages[1].jobs[0].command, "cargo test");
    }

    #[test]
    fn uses_the_template_only_when_no_configuration_was_found() {
        let stages = parse_forge_ci(None).expect("missing config uses template");

        assert_eq!(
            stages
                .iter()
                .map(|stage| stage.name.as_str())
                .collect::<Vec<_>>(),
            ["build", "test", "deploy"]
        );
    }

    #[test]
    fn rejects_invalid_or_empty_configuration_instead_of_deploying_the_template() {
        for source in [
            "stages: []",
            "stages:\n  - name: build\n    jobs: []",
            "stages:\n  - name: build\n    jobs:\n      - name: compile\n        command: ''",
            "stages:\n  - name: build\n    jobs:\n      - name: compile\n        image: ../unsafe\n        command: echo nope",
        ] {
            assert!(parse_forge_ci(Some(source)).is_err(), "{source}");
        }
    }
}

#[utoipa::path(get, path="/api/v1/projects/{project_id}/pipelines", tag="pipelines", params(("project_id"=Uuid, Path)), responses((status=200, body=[Pipeline])))]
async fn list_pipelines(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    axum::extract::Query(page): axum::extract::Query<PageParams>,
) -> ApiResult<Vec<Pipeline>> {
    let (limit, offset) = page.bounded();
    let pipelines = sqlx::query_as::<_, Pipeline>("SELECT id, project_id, git_ref, status, created_at, started_at, finished_at FROM pipelines WHERE project_id = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3")
        .bind(project_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool(&state)?)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(pipelines))
}
#[utoipa::path(get, path="/api/v1/pipelines/{pipeline_id}", tag="pipelines", params(("pipeline_id"=Uuid, Path)), responses((status=200, body=PipelineDetail), (status=404)))]
async fn get_pipeline(
    State(state): State<Arc<AppState>>,
    Path(pipeline_id): Path<Uuid>,
) -> ApiResult<PipelineDetail> {
    pipeline_detail(pool(&state)?, pipeline_id).await.map(Json)
}

async fn pipeline_detail(pool: &PgPool, pipeline_id: Uuid) -> Result<PipelineDetail, ApiError> {
    let pipeline = sqlx::query_as::<_, Pipeline>("SELECT id, project_id, git_ref, status, created_at, started_at, finished_at FROM pipelines WHERE id = $1").bind(pipeline_id).fetch_optional(pool).await.map_err(ApiError::internal)?.ok_or_else(ApiError::not_found)?;
    let stages = sqlx::query_as::<_, Stage>("SELECT id, pipeline_id, name, position, status FROM stages WHERE pipeline_id = $1 ORDER BY position").bind(pipeline_id).fetch_all(pool).await.map_err(ApiError::internal)?;
    let mut details = Vec::with_capacity(stages.len());
    for stage in stages {
        let jobs = sqlx::query_as::<_, Job>("SELECT id, stage_id, name, image, command, position, status, started_at, finished_at FROM jobs WHERE stage_id = $1 ORDER BY position").bind(stage.id).fetch_all(pool).await.map_err(ApiError::internal)?;
        details.push(StageDetail { stage, jobs });
    }
    Ok(PipelineDetail {
        pipeline,
        stages: details,
    })
}

#[derive(Deserialize, utoipa::ToSchema)]
struct ChangeStatus {
    status: JobStatus,
}
#[utoipa::path(post, path="/api/v1/jobs/{job_id}/status", tag="jobs", params(("job_id"=Uuid, Path)), responses((status=200, body=Job), (status=404)))]
async fn change_job_status(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
    Json(input): Json<ChangeStatus>,
) -> ApiResult<Job> {
    let pool = pool(&state)?;
    let job = sqlx::query_as::<_, Job>("SELECT id, stage_id, name, image, command, position, status, started_at, finished_at FROM jobs WHERE id = $1").bind(job_id).fetch_optional(pool).await.map_err(ApiError::internal)?.ok_or_else(ApiError::not_found)?;
    let current = JobStatus::try_from(job.status.as_str()).map_err(ApiError::bad_request)?;
    current
        .transition_to(input.status)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let updated = sqlx::query_as::<_, Job>("UPDATE jobs SET status = $2, started_at = CASE WHEN $2 = 'running' THEN now() ELSE started_at END, finished_at = CASE WHEN $2 IN ('success','failed','canceled') THEN now() ELSE finished_at END WHERE id = $1 RETURNING id, stage_id, name, image, command, position, status, started_at, finished_at").bind(job_id).bind(input.status.as_str()).fetch_one(pool).await.map_err(ApiError::internal)?;
    refresh_statuses(pool, updated.stage_id).await?;
    Ok(Json(updated))
}

#[utoipa::path(post, path="/api/v1/pipelines/{pipeline_id}/cancel", tag="pipelines", params(("pipeline_id"=Uuid, Path)), responses((status=200, body=Pipeline), (status=404)))]
async fn cancel_pipeline(
    State(state): State<Arc<AppState>>,
    Path(pipeline_id): Path<Uuid>,
) -> ApiResult<serde_json::Value> {
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
        "UPDATE stages SET status = 'canceled' \
         WHERE status IN ('queued','running') AND pipeline_id = $1",
    )
    .bind(pipeline_id)
    .execute(pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(serde_json::json!({"canceled": pipeline_id})))
}

/// Kill a running job process: try Docker container stop by name, then
/// SIGTERM and SIGKILL the child PID as fallback.
async fn kill_running_job(job_id: Uuid, pid: u32) {
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

#[utoipa::path(post, path="/api/v1/pipelines/{pipeline_id}/retry", tag="pipelines", params(("pipeline_id"=Uuid, Path)), responses((status=200, body=Pipeline), (status=404)))]
async fn retry_pipeline(
    State(state): State<Arc<AppState>>,
    Path(pipeline_id): Path<Uuid>,
) -> ApiResult<serde_json::Value> {
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
    Ok(Json(serde_json::json!({"retried": pipeline_id})))
}

#[utoipa::path(post, path="/api/v1/jobs/{job_id}/retry", tag="jobs", params(("job_id"=Uuid, Path)), responses((status=200, body=Job), (status=404)))]
async fn retry_job(State(state): State<Arc<AppState>>, Path(job_id): Path<Uuid>) -> ApiResult<Job> {
    let pool = pool(&state)?;
    let job = sqlx::query_as::<_, Job>("SELECT id, stage_id, name, image, command, position, status, started_at, finished_at FROM jobs WHERE id = $1")
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
    sqlx::query("DELETE FROM job_logs WHERE job_id = $1")
        .bind(job_id)
        .execute(pool)
        .await
        .map_err(ApiError::internal)?;
    let updated = sqlx::query_as::<_, Job>("UPDATE jobs SET status = 'queued', started_at = NULL, finished_at = NULL WHERE id = $1 RETURNING id, stage_id, name, image, command, position, status, started_at, finished_at")
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

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
struct JobLog {
    id: i64,
    job_id: Uuid,
    sequence: i32,
    message: String,
    created_at: DateTime<Utc>,
}
#[derive(Deserialize, utoipa::ToSchema)]
struct AppendLog {
    message: String,
}
#[utoipa::path(post, path="/api/v1/jobs/{job_id}/start", tag="jobs", params(("job_id"=Uuid, Path)), responses((status=200), (status=409, description="job is not a waiting manual job")))]
/// Starts a manual (`when: manual`) job — approval gate (GitLab parity).
async fn start_manual_job(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let pool = pool(&state)?;
    let manual: Option<bool> = sqlx::query_scalar("SELECT manual FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal)?;
    match manual {
        Some(true) => {}
        Some(false) => return Err(ApiError::conflict("job is not manual")),
        None => return Err(ApiError::not_found()),
    }
    let updated = sqlx::query_scalar::<_, bool>(
        "UPDATE jobs SET manual = false WHERE id = $1 AND manual RETURNING TRUE",
    )
    .bind(job_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?;
    if !updated.unwrap_or(false) {
        return Err(ApiError::conflict("job already started"));
    }
    crate::metrics::PIPELINES_CREATED_TOTAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(Json(serde_json::json!({"started": true})))
}

#[utoipa::path(get, path="/api/v1/jobs/{job_id}/logs/stream", tag="jobs", params(("job_id"=Uuid, Path), ("after"=Option<i32>, Query)), responses((status=200, description="text/event-stream of job log lines")))]
/// SSE live log stream: emits existing lines, then polls for new ones.
async fn job_log_stream(
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
    let mut after = params.after.unwrap_or(-1);
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let p = pool.clone();
    let jid = job_id;
    tokio::spawn(async move {
        loop {
            let rows = sqlx::query_as::<_, (i32, String)>(
                "SELECT sequence, message FROM job_logs WHERE job_id = $1 AND sequence > $2 ORDER BY sequence",
            )
            .bind(jid)
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

#[derive(serde::Deserialize, utoipa::IntoParams)]
struct StreamParams {
    after: Option<i32>,
}

#[utoipa::path(get, path="/api/v1/jobs/{job_id}/logs", tag="jobs", params(("job_id"=Uuid, Path)), responses((status=200, body=[JobLog])))]
async fn list_logs(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
) -> ApiResult<Vec<JobLog>> {
    let logs = sqlx::query_as::<_, JobLog>("SELECT id, job_id, sequence, message, created_at FROM job_logs WHERE job_id = $1 ORDER BY sequence").bind(job_id).fetch_all(pool(&state)?).await.map_err(ApiError::internal)?;
    Ok(Json(logs))
}
#[utoipa::path(post, path="/api/v1/jobs/{job_id}/logs", tag="jobs", params(("job_id"=Uuid, Path)), responses((status=200, body=[JobLog])))]
async fn append_log(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
    Json(input): Json<AppendLog>,
) -> ApiResult<JobLog> {
    if input.message.trim().is_empty() {
        return Err(ApiError::bad_request("message is required"));
    }
    let pool = pool(&state)?;
    let log = sqlx::query_as::<_, JobLog>("INSERT INTO job_logs (job_id, sequence, message) VALUES ($1, $2, $3) RETURNING id, job_id, sequence, message, created_at").bind(job_id).bind(next_log_sequence(pool, job_id).await.map_err(ApiError::internal)?).bind(input.message.trim()).fetch_one(pool).await.map_err(ApiError::internal)?;
    Ok(Json(log))
}
