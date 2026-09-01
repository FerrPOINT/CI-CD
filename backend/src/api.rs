use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode},
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
    store::{active_or_latest_attempt_id, append_job_log, open_attempt_id},
};

tokio::task_local! {
    static REQUEST_ID: uuid::Uuid;
}

pub struct AppState {
    pub pool: Option<PgPool>,
    pub auth_secret: Option<String>,
    pub git: crate::git_host::GitConfig,
    pub running_jobs: Option<crate::runner::RunningJobs>,
    pub rate_limiter: Arc<crate::rate_limit::RateLimiter>,
}

type ApiResult<T> = Result<Json<T>, ApiError>;
const IDEMPOTENCY_KEY_HEADER: &str = "idempotency-key";
const READINESS_TIMEOUT: Duration = Duration::from_secs(2);
const LEGACY_PIPELINE_PLAN_PARSER_VERSION: &str = "forge-legacy-linear/1";
const V1_PIPELINE_PLAN_PARSER_VERSION: &str = "forge-dsl/1.0.0";
const LEGACY_TEMPLATE_CONFIG: &str = r#"stages:
  - name: build
    jobs:
      - name: checkout
        image: alpine/git:latest
        command: git fetch --all
  - name: test
    jobs:
      - name: unit-tests
        image: rust:1.86
        command: cargo test
  - name: deploy
    jobs:
      - name: deploy
        image: alpine:3.21
        command: echo deploy
"#;
pub(crate) const PIPELINE_TRIGGER_SOURCE_API: &str = "api";
pub(crate) const PIPELINE_TRIGGER_SOURCE_GIT_PUSH: &str = "git-push";
pub(crate) const PIPELINE_TRIGGER_SOURCE_SCHEDULE: &str = "schedule";

/// OpenAPI 3 document for the current API surface (API_CONTRACT, utoipa).
#[derive(utoipa::OpenApi)]
#[openapi(
    info(
        title = "Forge CI/CD API",
        version = "0.1.0",
        description = "Self-hosted CI/CD control plane. Array responses, error envelope and in-process rate limits follow docs/contracts/API_CONTRACT.md and docs/API.md current compatibility mode.",
        license(
            name = "FerrPOINT Proprietary Source-Available Evaluation License v1.0",
            url = "https://github.com/FerrPOINT/CI-CD/blob/main/LICENSE"
        )
    ),
    paths(
        health, readiness, metrics, serve_openapi_json, auth_login, auth_refresh, auth_logout,
        list_projects, create_project, get_project, update_project, delete_project,
        list_project_memberships, upsert_project_membership, delete_project_membership,
        trigger_pipeline, list_pipelines, get_pipeline, cancel_pipeline, retry_pipeline,
        change_job_status, retry_job, start_manual_job, list_job_attempts, list_attempt_logs,
        list_attempt_logs_page, job_log_stream, list_logs, list_logs_page, append_log,
        crate::platform::list_runners, crate::platform::register_runner,
        crate::platform::runner_heartbeat, crate::platform::delete_runner,
        crate::runner_protocol::register_runner_protocol,
        crate::runner_protocol::runner_protocol_heartbeat,
        crate::runner_protocol::poll_runner_work, crate::runner_protocol::ack_runner_lease,
        crate::runner_protocol::renew_runner_lease, crate::runner_protocol::append_runner_lease_logs,
        crate::runner_protocol::complete_runner_lease,
        crate::platform::list_secrets, crate::platform::create_secret, crate::platform::delete_secret,
        crate::platform::list_artifacts, crate::platform::upload_artifact, crate::platform::download_artifact,
        crate::platform::list_environments, crate::platform::create_environment,
        crate::platform::update_environment, crate::platform::delete_environment,
        crate::platform::list_deployments, crate::platform::create_deployment,
        crate::platform::list_schedules, crate::platform::create_schedule,
        crate::platform::update_schedule, crate::platform::delete_schedule,
        crate::platform::list_webhooks, crate::platform::create_webhook, crate::platform::delete_webhook,
        crate::platform::list_outbox_deliveries, crate::platform::get_outbox_delivery,
        crate::platform::requeue_outbox_delivery,
        crate::platform::list_notifications, crate::platform::replace_notifications,
        crate::platform::list_notification_events, crate::platform::notification_stream,
        crate::platform::project_report, crate::platform::list_audit_log,
        crate::platform::list_users, crate::platform::create_user, crate::platform::update_user,
        crate::platform::list_tokens, crate::platform::create_token, crate::platform::delete_token,
        crate::git_host::list_repositories, crate::git_host::create_repository,
        crate::git_host::delete_repository,
        crate::git_host::git_info_refs, crate::git_host::git_service_endpoint,
        crate::git_host::git_receive_pack_openapi, crate::git_host::internal_git_push,
        crate::pulls::list_refs, crate::pulls::list_tree, crate::pulls::get_blob,
        crate::pulls::list_tags, crate::pulls::list_commits, crate::pulls::compare_refs,
        crate::pulls::list_pull_requests, crate::pulls::create_pull_request, crate::pulls::pr_action,
        list_releases, create_release, get_release, delete_release,
        pipeline_badge, pipeline_variables, get_test_report, upload_test_report,
    ),
    components(schemas(
        crate::auth::LoginRequest, crate::auth::LogoutRequest, crate::auth::LogoutResponse, crate::auth::RefreshRequest, crate::auth::TokenPair,
        Project, CreateProject, UpdateProject, ProjectMembership, ProjectMembershipInput,
        Readiness, MigrationReadiness,
        TriggerPipeline, Pipeline, Stage, Job,
        PipelineDetail, PipelinePlan, StageDetail, JobAttempt, JobLog, JobLogPage, ChangeStatus, AppendLog,
        CanceledPipelineResult, RetriedPipelineResult, ManualJobStartResult,
        Release, CreateRelease, TestReport,
        crate::runner_protocol::RunnerRegisterRequest, crate::runner_protocol::RunnerRegisterResponse,
        crate::runner_protocol::RunnerHeartbeatRequest, crate::runner_protocol::RunnerCapacity,
        crate::runner_protocol::RunnerPollRequest, crate::runner_protocol::RunnerPollCapacity,
        crate::runner_protocol::RunnerLeaseOffer, crate::runner_protocol::RunnerAttemptSpec,
        crate::runner_protocol::RunnerWorkspace, crate::runner_protocol::RunnerLeaseControlRequest,
        crate::runner_protocol::RunnerLeaseControlResponse, crate::runner_protocol::RunnerCompleteRequest,
        crate::runner_protocol::RunnerCompleteResponse, crate::runner_protocol::RunnerLogAppendRequest,
        crate::runner_protocol::RunnerLogLine, crate::runner_protocol::RunnerLogAppendResponse,
        crate::platform::Runner, crate::platform::RegisterRunner, crate::platform::RunnerHeartbeat,
        crate::platform::SecretMetadata, crate::platform::CreateSecret,
        crate::platform::Artifact,
        crate::platform::Environment, crate::platform::CreateEnvironment, crate::platform::UpdateEnvironment,
        crate::platform::Deployment, crate::platform::CreateDeployment,
        crate::platform::Schedule, crate::platform::ScheduleInput,
        crate::platform::Webhook, crate::platform::CreateWebhook,
        crate::platform::OutboxDelivery, crate::platform::OutboxDeliveryAttempt,
        crate::platform::OutboxDeliveryDetail, crate::platform::RequeuedOutboxDelivery,
        crate::platform::Notification, crate::platform::NotificationInput,
        crate::platform::NotificationEvent,
        crate::platform::Report, crate::platform::AuditEvent,
        crate::platform::User, crate::platform::UserInput,
        crate::platform::ApiToken, crate::platform::CreatedToken, crate::platform::CreateToken,
        crate::git_host::Repository, crate::git_host::CreateRepositoryBody,
        crate::git_host::DeletedRepository, crate::git_host::GitPushEvent,
        crate::pulls::RefInfo, crate::pulls::TreeEntry, crate::pulls::BlobContent,
        crate::pulls::TagInfo, crate::pulls::CommitInfo, crate::pulls::DiffResult, crate::pulls::DiffFile,
        crate::pulls::PullRequest, crate::pulls::CreatePullRequest, crate::pulls::PrAction,
    )),
    tags(
        (name = "health", description = "Liveness/readiness"),
        (name = "auth", description = "Login, token refresh and logout"),
        (name = "projects", description = "Project registry"),
        (name = "memberships", description = "Project-scoped user roles"),
        (name = "pipelines", description = "Pipeline lifecycle"),
        (name = "jobs", description = "Jobs, logs and retries"),
        (name = "runners", description = "Runner registration and heartbeats"),
        (name = "runner-protocol", description = "External runner protocol"),
        (name = "secrets", description = "Encrypted project secrets"),
        (name = "artifacts", description = "Job artifact upload and download"),
        (name = "environments", description = "Environments and deployments"),
        (name = "schedules", description = "Cron-style pipeline schedules"),
        (name = "webhooks", description = "Outgoing project webhooks"),
        (name = "outbox", description = "Outbox delivery history and replay"),
        (name = "notifications", description = "Notification channel configuration"),
        (name = "reports", description = "Project delivery reports"),
        (name = "audit", description = "Audit log"),
        (name = "users", description = "User management"),
        (name = "tokens", description = "Personal API tokens"),
        (name = "git", description = "Git Smart HTTP and internal push events"),
        (name = "repos", description = "Repository refs, commits and diff"),
        (name = "releases", description = "Repository release metadata"),
        (name = "pulls", description = "Pull requests"),
    )
)]
pub struct ApiDoc;

#[utoipa::path(
    get,
    path = "/api/v1/openapi.json",
    tag = "health",
    responses((status = 200, description = "OpenAPI JSON document"))
)]
pub(crate) async fn serve_openapi_json() -> Json<serde_json::Value> {
    use utoipa::OpenApi as _;
    Json(serde_json::to_value(ApiDoc::openapi()).expect("serialize openapi"))
}

/// Canonical YAML serialization of the OpenAPI document (openapi-dump bin).
pub fn openapi_yaml() -> Result<String, serde_yaml::Error> {
    use utoipa::OpenApi as _;
    serde_yaml::to_string(&ApiDoc::openapi())
}

#[utoipa::path(
    get,
    path = "/metrics",
    tag = "health",
    responses((status = 200, description = "Prometheus text exposition"))
)]
async fn metrics() -> impl IntoResponse {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4",
        )],
        crate::metrics::render(),
    )
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
    pub(crate) fn gone(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::GONE,
            message: message.into(),
        }
    }
    pub(crate) fn service_unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
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
            StatusCode::GONE => "expired",
            StatusCode::TOO_MANY_REQUESTS => "rate_limited",
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

fn attempt_lookup_error(error: sqlx::Error) -> ApiError {
    match error {
        sqlx::Error::RowNotFound => ApiError::not_found(),
        other => ApiError::internal(other),
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

fn auth_secret(state: &AppState) -> Result<&str, ApiError> {
    state
        .auth_secret
        .as_deref()
        .ok_or_else(ApiError::unauthorized)
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

/// AUTHZ_CONTRACT current mode: when CICD_AUTH_SECRET is configured, every
/// /api/v1 route except the public allowlist requires a valid Bearer JWT/PAT
/// and project-scoped resources require `project_memberships`. Without the
/// secret the API stays in trusted-network mode (open), matching CURRENT_STATE.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RateLimitRule {
    class: &'static str,
    limit: u32,
    window_secs: u64,
}

async fn rate_limit_mw(
    State(state): State<Arc<AppState>>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, ApiError> {
    if let Some(rule) = rate_limit_rule(req.method(), req.uri().path()) {
        state.rate_limiter.prune(rule.window_secs);
        let client = rate_limit_client(req.headers());
        let key = format!("{}:{}", rule.class, client);
        if !state.rate_limiter.allow(&key, rule.limit, rule.window_secs) {
            return Err(ApiError::too_many_requests());
        }
    }
    Ok(next.run(req).await)
}

fn rate_limit_rule(method: &Method, path: &str) -> Option<RateLimitRule> {
    if matches!(
        path,
        "/api/v1/health" | "/api/v1/readiness" | "/api/v1/openapi.json" | "/metrics"
    ) {
        return None;
    }
    if path == "/api/v1/auth/login" {
        return Some(RateLimitRule {
            class: "auth-login",
            limit: 30,
            window_secs: 60,
        });
    }
    if path == "/api/v1/auth/refresh" {
        return Some(RateLimitRule {
            class: "auth-refresh",
            limit: 120,
            window_secs: 60,
        });
    }
    if path == "/api/v1/auth/logout" {
        return Some(RateLimitRule {
            class: "auth-logout",
            limit: 120,
            window_secs: 60,
        });
    }
    if path == "/api/v1/internal/git-push" {
        return Some(RateLimitRule {
            class: "internal-git-push",
            limit: 120,
            window_secs: 60,
        });
    }
    if path.starts_with("/api/v1/runner/") {
        return Some(RateLimitRule {
            class: "runner-protocol",
            limit: 1200,
            window_secs: 60,
        });
    }
    if path.starts_with("/git/") {
        return Some(RateLimitRule {
            class: if method == Method::POST && path.ends_with("/git-receive-pack") {
                "git-push"
            } else {
                "git-read"
            },
            limit: 240,
            window_secs: 60,
        });
    }
    if method == Method::POST && path.starts_with("/api/v1/jobs/") && path.ends_with("/artifacts") {
        return Some(RateLimitRule {
            class: "artifact-upload",
            limit: 60,
            window_secs: 60,
        });
    }
    if path.starts_with("/api/") {
        return Some(RateLimitRule {
            class: if matches!(
                *method,
                Method::POST | Method::PUT | Method::PATCH | Method::DELETE
            ) {
                "api-write"
            } else {
                "api-read"
            },
            limit: if matches!(
                *method,
                Method::POST | Method::PUT | Method::PATCH | Method::DELETE
            ) {
                600
            } else {
                1200
            },
            window_secs: 60,
        });
    }
    None
}

fn rate_limit_client(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .unwrap_or("unknown")
        .to_string()
}

async fn require_auth(
    State(state): State<Arc<AppState>>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, ApiError> {
    const PUBLIC: &[&str] = &[
        "/api/v1/health",
        "/api/v1/readiness",
        "/api/v1/openapi.json",
        "/api/v1/auth/login",
        "/api/v1/auth/refresh",
        "/api/v1/auth/logout",
        "/api/v1/internal/git-push",
        "/metrics",
    ];
    let path = req.uri().path();
    if PUBLIC.contains(&path) || path.starts_with("/git/") || path.starts_with("/api/v1/runner/") {
        return Ok(next.run(req).await);
    }
    let Some(auth_secret) = state.auth_secret.as_deref() else {
        return Ok(next.run(req).await); // trusted-network mode: no enforcement
    };
    let pool = pool(&state)?;
    let claims = bearer_identity(pool, auth_secret, req.headers())
        .await
        .map_err(|_| ApiError::unauthorized())?;
    let role = crate::authz::Role::parse(&claims.role).ok_or_else(ApiError::unauthorized)?;
    let method = req.method().as_str().to_string();
    let path = req.uri().path().to_string();
    let (mut parts, body) = req.into_parts();
    parts.extensions.insert(claims.clone());
    let req = axum::extract::Request::from_parts(parts, body);
    let allowed = api_token_scope_allows(&claims, &method)
        && crate::authz::allows(role, &method, &path)
        && project_scope_allows(&state, &claims, role, &method, &path).await?;
    if !allowed {
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProjectScopeRef {
    Project(Uuid),
    Pipeline(Uuid),
    Job(Uuid),
    Artifact(Uuid),
    Secret(Uuid),
    Environment(Uuid),
    Schedule(Uuid),
    Webhook(Uuid),
    OutboxDelivery(Uuid),
    Repository(String),
}

async fn project_scope_allows(
    state: &AppState,
    claims: &crate::auth::AccessClaims,
    global_role: crate::authz::Role,
    method: &str,
    path: &str,
) -> Result<bool, ApiError> {
    let Some(scope_ref) = project_scope_ref(path) else {
        if claims.token_id.is_some() && claims.token_project_id.is_some() {
            return Ok(method == "GET" && path == "/api/v1/projects");
        }
        return Ok(true);
    };
    let Some(pool) = state.pool.as_ref() else {
        return Err(ApiError::unavailable());
    };
    let scope_ref = match scope_ref {
        ProjectScopeRef::Repository(name) => {
            let (_, min_role) = crate::authz::required_role(method, path);
            return repository_scope_allows(
                pool,
                &name,
                claims.sub,
                global_role,
                min_role,
                claims.token_project_id,
            )
            .await;
        }
        other => other,
    };
    let Some(project_id) = project_id_for_scope_ref(pool, scope_ref).await? else {
        return Ok(true);
    };
    if let Some(token_project_id) = claims.token_project_id {
        if project_id != token_project_id {
            return Ok(false);
        }
    }
    if global_role == crate::authz::Role::Admin {
        return Ok(true);
    }
    let (_, min_role) = crate::authz::required_role(method, path);
    let Some(project_role) = project_membership_role(pool, claims.sub, project_id).await? else {
        return Ok(false);
    };
    Ok(project_role >= min_role)
}

fn api_token_scope_allows(claims: &crate::auth::AccessClaims, method: &str) -> bool {
    if claims.token_id.is_none() {
        return true;
    }
    let required_scope = if matches!(method, "GET" | "HEAD" | "OPTIONS") {
        "api:read"
    } else {
        "api:write"
    };
    bearer_token_has_scope(claims, required_scope)
}

pub(crate) fn bearer_token_has_scope(
    claims: &crate::auth::AccessClaims,
    required_scope: &str,
) -> bool {
    claims.token_id.is_none()
        || claims
            .token_scopes
            .iter()
            .any(|scope| scope == required_scope)
}

fn project_scope_ref(path: &str) -> Option<ProjectScopeRef> {
    let mut segments = path.trim_start_matches('/').split('/');
    if segments.next()? != "api" || segments.next()? != "v1" {
        return None;
    }
    let resource = segments.next()?;
    match resource {
        "repos" | "repositories" => segments
            .next()
            .map(|name| ProjectScopeRef::Repository(name.to_string())),
        _ => {
            let id = segments.next().and_then(|raw| Uuid::parse_str(raw).ok())?;
            match resource {
                "projects" => Some(ProjectScopeRef::Project(id)),
                "pipelines" => Some(ProjectScopeRef::Pipeline(id)),
                "jobs" => Some(ProjectScopeRef::Job(id)),
                "artifacts" => Some(ProjectScopeRef::Artifact(id)),
                "secrets" => Some(ProjectScopeRef::Secret(id)),
                "environments" => Some(ProjectScopeRef::Environment(id)),
                "schedules" => Some(ProjectScopeRef::Schedule(id)),
                "webhooks" => Some(ProjectScopeRef::Webhook(id)),
                "outbox-deliveries" => Some(ProjectScopeRef::OutboxDelivery(id)),
                _ => None,
            }
        }
    }
}

async fn repository_scope_allows(
    pool: &PgPool,
    repo: &str,
    user_id: Uuid,
    global_role: crate::authz::Role,
    min_role: crate::authz::Role,
    token_project_id: Option<Uuid>,
) -> Result<bool, ApiError> {
    let name = crate::git_host::validate_repo_name(repo).map_err(ApiError::bad_request)?;
    if global_role == crate::authz::Role::Admin {
        return match token_project_id {
            Some(project_id) => repository_linked_to_project(pool, &name, project_id).await,
            None => Ok(true),
        };
    }
    let patterns = crate::git_host::repository_url_like_patterns(&name);
    let roles = sqlx::query_scalar::<_, String>(
        "SELECT m.role FROM projects p \
         JOIN project_memberships m ON m.project_id = p.id \
         WHERE (p.repository_url ILIKE $1 ESCAPE '\\' \
             OR p.repository_url ILIKE $2 ESCAPE '\\' \
             OR p.repository_url ILIKE $3 ESCAPE '\\') \
           AND m.user_id = $4 \
           AND ($5::uuid IS NULL OR p.id = $5)",
    )
    .bind(&patterns.path)
    .bind(&patterns.scp)
    .bind(&patterns.exact)
    .bind(user_id)
    .bind(token_project_id)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(roles
        .iter()
        .filter_map(|role| crate::authz::Role::parse(role))
        .any(|role| role >= min_role))
}

async fn repository_linked_to_project(
    pool: &PgPool,
    repo: &str,
    project_id: Uuid,
) -> Result<bool, ApiError> {
    let patterns = crate::git_host::repository_url_like_patterns(repo);
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM projects \
         WHERE id = $1 AND (repository_url ILIKE $2 ESCAPE '\\' \
             OR repository_url ILIKE $3 ESCAPE '\\' \
             OR repository_url ILIKE $4 ESCAPE '\\'))",
    )
    .bind(project_id)
    .bind(&patterns.path)
    .bind(&patterns.scp)
    .bind(&patterns.exact)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)
}

async fn list_projects_for_claims(
    pool: &PgPool,
    claims: &crate::auth::AccessClaims,
    role: crate::authz::Role,
    limit: i64,
    offset: i64,
) -> Result<Vec<Project>, ApiError> {
    match (role, claims.token_project_id) {
        (crate::authz::Role::Admin, Some(project_id)) => sqlx::query_as::<_, Project>(
            "SELECT id, name, repository_url, default_branch, created_at \
                 FROM projects WHERE id = $3 ORDER BY created_at DESC LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .bind(project_id)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal),
        (crate::authz::Role::Admin, None) => sqlx::query_as::<_, Project>(
            "SELECT id, name, repository_url, default_branch, created_at \
                 FROM projects ORDER BY created_at DESC LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal),
        (_, Some(project_id)) => sqlx::query_as::<_, Project>(
            "SELECT p.id, p.name, p.repository_url, p.default_branch, p.created_at \
                 FROM projects p \
                 JOIN project_memberships m ON m.project_id = p.id \
                 WHERE m.user_id = $3 AND p.id = $4 \
                 ORDER BY p.created_at DESC LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .bind(claims.sub)
        .bind(project_id)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal),
        (_, None) => sqlx::query_as::<_, Project>(
            "SELECT p.id, p.name, p.repository_url, p.default_branch, p.created_at \
                 FROM projects p \
                 JOIN project_memberships m ON m.project_id = p.id \
                 WHERE m.user_id = $3 \
                 ORDER BY p.created_at DESC LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .bind(claims.sub)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal),
    }
}

async fn project_id_for_scope_ref(
    pool: &PgPool,
    scope_ref: ProjectScopeRef,
) -> Result<Option<Uuid>, ApiError> {
    match scope_ref {
        ProjectScopeRef::Project(id) => Ok(Some(id)),
        ProjectScopeRef::Pipeline(id) => {
            sqlx::query_scalar("SELECT project_id FROM pipelines WHERE id = $1")
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(ApiError::internal)
        }
        ProjectScopeRef::Job(id) => sqlx::query_scalar(
            "SELECT p.project_id FROM jobs j \
             JOIN stages s ON s.id = j.stage_id \
             JOIN pipelines p ON p.id = s.pipeline_id \
             WHERE j.id = $1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal),
        ProjectScopeRef::Artifact(id) => sqlx::query_scalar(
            "SELECT p.project_id FROM artifacts a \
             JOIN jobs j ON j.id = a.job_id \
             JOIN stages s ON s.id = j.stage_id \
             JOIN pipelines p ON p.id = s.pipeline_id \
             WHERE a.id = $1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal),
        ProjectScopeRef::Secret(id) => {
            sqlx::query_scalar("SELECT project_id FROM project_secrets WHERE id = $1")
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(ApiError::internal)
        }
        ProjectScopeRef::Environment(id) => {
            sqlx::query_scalar("SELECT project_id FROM environments WHERE id = $1")
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(ApiError::internal)
        }
        ProjectScopeRef::Schedule(id) => {
            sqlx::query_scalar("SELECT project_id FROM schedules WHERE id = $1")
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(ApiError::internal)
        }
        ProjectScopeRef::Webhook(id) => {
            sqlx::query_scalar("SELECT project_id FROM webhooks WHERE id = $1")
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(ApiError::internal)
        }
        ProjectScopeRef::OutboxDelivery(id) => sqlx::query_scalar(
            "SELECT COALESCE( \
                m.project_id, \
                CASE WHEN m.payload->>'project_id' ~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$' THEN (m.payload->>'project_id')::uuid END, \
                CASE WHEN e.payload->>'project_id' ~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$' THEN (e.payload->>'project_id')::uuid END, \
                p.project_id \
            ) \
             FROM outbox_messages m \
             JOIN domain_events e ON e.id = m.event_id \
             LEFT JOIN pipelines p ON p.id = e.aggregate_id AND e.aggregate_type = 'pipeline' \
             WHERE m.id = $1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal),
        ProjectScopeRef::Repository(_) => Ok(None),
    }
}

pub(crate) async fn project_membership_role(
    pool: &PgPool,
    user_id: Uuid,
    project_id: Uuid,
) -> Result<Option<crate::authz::Role>, ApiError> {
    let role = sqlx::query_scalar::<_, String>(
        "SELECT role FROM project_memberships WHERE user_id = $1 AND project_id = $2",
    )
    .bind(user_id)
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(role.and_then(|value| crate::authz::Role::parse(&value)))
}

/// JWT access tokens are bound to `sessions.id`; PATs (`cicd_...`) are
/// resolved against api_tokens and carry explicit token scopes/bindings.
pub(crate) async fn bearer_identity(
    pool: &PgPool,
    auth_secret: &str,
    headers: &axum::http::HeaderMap,
) -> Result<crate::auth::AccessClaims, ApiError> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(ApiError::unauthorized)?;
    let token = value
        .strip_prefix("Bearer ")
        .ok_or_else(ApiError::unauthorized)?;
    identity_for_bearer_token(pool, auth_secret, token).await
}

pub(crate) async fn identity_for_bearer_token(
    pool: &PgPool,
    auth_secret: &str,
    token: &str,
) -> Result<crate::auth::AccessClaims, ApiError> {
    if token.starts_with("cicd_") {
        let hash = crate::auth::hash_token(token);
        let row = sqlx::query_as::<_, (Uuid, Uuid, String, Option<Uuid>, Vec<String>)>(
            "SELECT t.id, u.id, u.role, t.project_id, t.scopes \
             FROM api_tokens t JOIN users u ON u.id = t.user_id \
             WHERE t.token_hash = $1 AND u.enabled \
               AND t.revoked_at IS NULL \
               AND (t.expires_at IS NULL OR t.expires_at > now())",
        )
        .bind(&hash)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal)?;
        let (token_id, sub, role, token_project_id, token_scopes) =
            row.ok_or_else(ApiError::unauthorized)?;
        // Touch last_used_at best-effort.
        let _ = sqlx::query("UPDATE api_tokens SET last_used_at = now() WHERE id = $1")
            .bind(token_id)
            .execute(pool)
            .await;
        let now = chrono::Utc::now();
        Ok(crate::auth::AccessClaims {
            sub,
            sid: None,
            token_id: Some(token_id),
            token_project_id,
            token_scopes,
            role,
            iat: now.timestamp(),
            exp: now.timestamp() + 900,
        })
    } else {
        let mut claims = crate::auth::verify_access_with_secret(token, auth_secret)
            .map_err(|_| ApiError::unauthorized())?;
        let session_id = claims.sid.ok_or_else(ApiError::unauthorized)?;
        let current = crate::auth::access_session_user(pool, session_id, claims.sub)
            .await
            .map_err(|_| ApiError::unauthorized())?;
        claims.role = current.role;
        Ok(claims)
    }
}

fn build_router(
    pool: Option<PgPool>,
    git: crate::git_host::GitConfig,
    running: Option<crate::runner::RunningJobs>,
) -> Router {
    build_router_with_auth_secret(pool, git, running, crate::auth::configured_secret().ok())
}

#[cfg(any(test, feature = "integration"))]
pub fn app_with_auth_secret(pool: Option<PgPool>, auth_secret: Option<String>) -> Router {
    build_router_with_auth_secret(
        pool,
        crate::git_host::GitConfig::default(),
        None,
        auth_secret,
    )
}

#[cfg(any(test, feature = "integration"))]
pub fn app_with_git_and_auth_secret(
    pool: Option<PgPool>,
    git: crate::git_host::GitConfig,
    auth_secret: Option<String>,
) -> Router {
    build_router_with_auth_secret(pool, git, None, auth_secret)
}

fn build_router_with_auth_secret(
    pool: Option<PgPool>,
    git: crate::git_host::GitConfig,
    running: Option<crate::runner::RunningJobs>,
    auth_secret: Option<String>,
) -> Router {
    let state = Arc::new(AppState {
        pool: pool.clone(),
        auth_secret,
        git,
        running_jobs: running,
        rate_limiter: Arc::new(crate::rate_limit::RateLimiter::default()),
    });
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/readiness", get(readiness))
        .route("/metrics", get(metrics))
        .route("/api/v1/openapi.json", get(serve_openapi_json))
        .route("/api/v1/auth/login", post(auth_login))
        .route("/api/v1/auth/refresh", post(auth_refresh))
        .route("/api/v1/auth/logout", post(auth_logout))
        .merge(crate::platform::routes())
        .merge(crate::runner_protocol::routes())
        .route("/api/v1/projects", get(list_projects).post(create_project))
        .route(
            "/api/v1/projects/{project_id}",
            get(get_project)
                .patch(update_project)
                .delete(delete_project),
        )
        .route(
            "/api/v1/projects/{project_id}/memberships",
            get(list_project_memberships).post(upsert_project_membership),
        )
        .route(
            "/api/v1/projects/{project_id}/memberships/{user_id}",
            axum::routing::delete(delete_project_membership),
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
        .route("/api/v1/jobs/{job_id}/attempts", get(list_job_attempts))
        .route(
            "/api/v1/jobs/{job_id}/attempts/{attempt_id}/logs",
            get(list_attempt_logs),
        )
        .route(
            "/api/v1/jobs/{job_id}/attempts/{attempt_id}/logs/page",
            get(list_attempt_logs_page),
        )
        .route(
            "/api/v1/jobs/{job_id}/logs",
            get(list_logs).post(append_log),
        )
        .route("/api/v1/jobs/{job_id}/logs/page", get(list_logs_page))
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
            state.clone(),
            require_auth,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            rate_limit_mw,
        ))
        .layer(axum::middleware::from_fn(request_id_mw))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[utoipa::path(post, path="/api/v1/auth/login", tag="auth", request_body=crate::auth::LoginRequest, responses((status=200, body=crate::auth::TokenPair), (status=401)))]
async fn auth_login(
    State(state): State<Arc<AppState>>,
    Json(input): Json<crate::auth::LoginRequest>,
) -> Result<Json<crate::auth::TokenPair>, ApiError> {
    use crate::auth::*;
    crate::metrics::LOGIN_ATTEMPTS_TOTAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
    let session_id = create_session(pool, user_id, &hash_token(&refresh))
        .await
        .map_err(ApiError::from)?;
    let mut pair = issue_access_with_secret(user_id, &role, session_id, auth_secret(&state)?)
        .map_err(|_| ApiError::unauthorized())?;
    pair.refresh_token = refresh;
    Ok(Json(pair))
}

#[utoipa::path(post, path="/api/v1/auth/refresh", tag="auth", request_body=crate::auth::RefreshRequest, responses((status=200, body=crate::auth::TokenPair), (status=401)))]
async fn auth_refresh(
    State(state): State<Arc<AppState>>,
    Json(input): Json<crate::auth::RefreshRequest>,
) -> Result<Json<crate::auth::TokenPair>, ApiError> {
    use crate::auth::*;
    let pool = pool(&state)?;
    if input.refresh_token.is_empty() {
        return Err(ApiError::unauthorized());
    }
    let (user_id, session_id, new_refresh) =
        rotate_session(pool, &hash_token(&input.refresh_token))
            .await
            .map_err(|_| ApiError::unauthorized())?;
    let role = sqlx::query_scalar::<_, String>("SELECT role FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .map_err(ApiError::internal)?;
    let mut pair = issue_access_with_secret(user_id, &role, session_id, auth_secret(&state)?)
        .map_err(|_| ApiError::unauthorized())?;
    pair.refresh_token = new_refresh;
    Ok(Json(pair))
}

#[utoipa::path(post, path="/api/v1/auth/logout", tag="auth", request_body=crate::auth::LogoutRequest, responses((status=200, body=crate::auth::LogoutResponse)))]
async fn auth_logout(
    State(state): State<Arc<AppState>>,
    Json(input): Json<crate::auth::LogoutRequest>,
) -> Result<Json<crate::auth::LogoutResponse>, ApiError> {
    use crate::auth::*;
    if input.refresh_token.trim().is_empty() {
        return Ok(Json(LogoutResponse { revoked: false }));
    }
    let pool = pool(&state)?;
    let user_id = revoke_session(pool, &hash_token(input.refresh_token.trim()))
        .await
        .map_err(ApiError::from)?;
    if let Some(user_id) = user_id {
        let _ = audit(pool, "auth.logout", "session", user_id, None).await;
    }
    Ok(Json(LogoutResponse {
        revoked: user_id.is_some(),
    }))
}

#[utoipa::path(get, path="/api/v1/health", tag="health", responses((status=200, description="Liveness")))]
async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok", "service": "cicd"}))
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
struct Readiness {
    status: String,
    service: String,
    database: String,
    migrations: MigrationReadiness,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
struct MigrationReadiness {
    status: String,
    latest_applied_version: Option<i64>,
    latest_required_version: i64,
    pending_versions: Vec<i64>,
    checksum_mismatches: Vec<i64>,
    unknown_applied_versions: Vec<i64>,
    error: Option<String>,
}

#[derive(Debug)]
struct ExpectedMigration {
    version: i64,
    checksum: Vec<u8>,
}

#[utoipa::path(
    get,
    path = "/api/v1/readiness",
    tag = "health",
    responses(
        (status = 200, description = "Database-aware readiness", body = Readiness),
        (status = 503, description = "Database or migrations are not ready", body = Readiness)
    )
)]
async fn readiness(State(state): State<Arc<AppState>>) -> axum::response::Response {
    let expected = expected_migrations();
    let Some(db) = state.pool.as_ref() else {
        return readiness_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "not_ready",
            "unavailable",
            migration_readiness_error(&expected, "unknown", "database pool is not configured"),
        );
    };

    let database_probe = tokio::time::timeout(
        READINESS_TIMEOUT,
        sqlx::query_scalar::<_, i64>("SELECT 1::BIGINT").fetch_one(db),
    )
    .await;
    match database_probe {
        Ok(Ok(_)) => {}
        Ok(Err(_)) => {
            return readiness_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "not_ready",
                "unavailable",
                migration_readiness_error(&expected, "unknown", "database query failed"),
            );
        }
        Err(_) => {
            return readiness_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "not_ready",
                "unavailable",
                migration_readiness_error(&expected, "unknown", "database query timed out"),
            );
        }
    }

    let migrations =
        match tokio::time::timeout(READINESS_TIMEOUT, migration_readiness(db, &expected)).await {
            Ok(readiness) => readiness,
            Err(_) => migration_readiness_error(&expected, "unknown", "migration check timed out"),
        };
    let ready = migrations.status == "ok";
    readiness_response(
        if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        if ready { "ready" } else { "not_ready" },
        "ok",
        migrations,
    )
}

fn readiness_response(
    status: StatusCode,
    readiness_status: &str,
    database: &str,
    migrations: MigrationReadiness,
) -> axum::response::Response {
    (
        status,
        Json(Readiness {
            status: readiness_status.to_string(),
            service: "cicd".to_string(),
            database: database.to_string(),
            migrations,
        }),
    )
        .into_response()
}

fn expected_migrations() -> Vec<ExpectedMigration> {
    crate::migrations()
        .iter()
        .filter(|migration| migration.migration_type.is_up_migration())
        .map(|migration| ExpectedMigration {
            version: migration.version,
            checksum: migration.checksum.to_vec(),
        })
        .collect()
}

async fn migration_readiness(db: &PgPool, expected: &[ExpectedMigration]) -> MigrationReadiness {
    let applied: Vec<(i64, Vec<u8>)> = match sqlx::query_as(
        "SELECT version, checksum FROM _sqlx_migrations WHERE success ORDER BY version",
    )
    .fetch_all(db)
    .await
    {
        Ok(rows) => rows,
        Err(_) => {
            return migration_readiness_error(
                expected,
                "unknown",
                "migration history query failed",
            );
        }
    };

    let latest_applied_version = applied.iter().map(|(version, _)| *version).max();
    let applied_by_version: HashMap<i64, Vec<u8>> = applied.into_iter().collect();
    let expected_versions: HashSet<i64> =
        expected.iter().map(|migration| migration.version).collect();
    let pending_versions = expected
        .iter()
        .filter(|migration| !applied_by_version.contains_key(&migration.version))
        .map(|migration| migration.version)
        .collect::<Vec<_>>();
    let checksum_mismatches = expected
        .iter()
        .filter_map(|migration| {
            applied_by_version
                .get(&migration.version)
                .filter(|checksum| checksum.as_slice() != migration.checksum.as_slice())
                .map(|_| migration.version)
        })
        .collect::<Vec<_>>();
    let mut unknown_applied_versions = applied_by_version
        .keys()
        .copied()
        .filter(|version| !expected_versions.contains(version))
        .collect::<Vec<_>>();
    unknown_applied_versions.sort_unstable();

    let status = if checksum_mismatches.is_empty()
        && pending_versions.is_empty()
        && unknown_applied_versions.is_empty()
    {
        "ok"
    } else if !checksum_mismatches.is_empty() || !unknown_applied_versions.is_empty() {
        "mismatch"
    } else {
        "pending"
    };

    MigrationReadiness {
        status: status.to_string(),
        latest_applied_version,
        latest_required_version: latest_required_version(expected),
        pending_versions,
        checksum_mismatches,
        unknown_applied_versions,
        error: None,
    }
}

fn migration_readiness_error(
    expected: &[ExpectedMigration],
    status: &str,
    error: &str,
) -> MigrationReadiness {
    MigrationReadiness {
        status: status.to_string(),
        latest_applied_version: None,
        latest_required_version: latest_required_version(expected),
        pending_versions: Vec::new(),
        checksum_mismatches: Vec::new(),
        unknown_applied_versions: Vec::new(),
        error: Some(error.to_string()),
    }
}

fn latest_required_version(expected: &[ExpectedMigration]) -> i64 {
    expected
        .iter()
        .map(|migration| migration.version)
        .max()
        .unwrap_or_default()
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
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
struct ProjectMembership {
    project_id: Uuid,
    user_id: Uuid,
    username: String,
    user_enabled: bool,
    role: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct ProjectMembershipInput {
    user_id: Uuid,
    role: String,
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
    claims: Option<axum::Extension<crate::auth::AccessClaims>>,
    Json(input): Json<CreateProject>,
) -> ApiResult<Project> {
    if input.name.trim().is_empty() || input.repository_url.trim().is_empty() {
        return Err(ApiError::bad_request(
            "name and repository_url are required",
        ));
    }
    let db = pool(&state)?;
    let project = sqlx::query_as::<_, Project>(
        "INSERT INTO projects (id, name, repository_url, default_branch) VALUES ($1, $2, $3, $4) RETURNING id, name, repository_url, default_branch, created_at"
    ).bind(Uuid::new_v4()).bind(input.name.trim()).bind(input.repository_url.trim()).bind(input.default_branch.unwrap_or_else(|| "main".into())).fetch_one(db).await.map_err(ApiError::internal)?;
    if let Some(axum::Extension(claims)) = claims {
        if let Some(role) = default_project_role(&claims.role) {
            upsert_project_membership_record(db, project.id, claims.sub, role).await?;
        }
    }
    Ok(Json(project))
}

#[utoipa::path(get, path="/api/v1/projects", tag="projects", params(PageParams), responses((status=200, body=[Project])))]
async fn list_projects(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::auth::AccessClaims>>,
    axum::extract::Query(page): axum::extract::Query<PageParams>,
) -> ApiResult<Vec<Project>> {
    let (limit, offset) = page.bounded();
    let db = pool(&state)?;
    let projects = if state.auth_secret.is_some() {
        let claims = claims.ok_or_else(ApiError::unauthorized)?.0;
        let role = crate::authz::Role::parse(&claims.role).ok_or_else(ApiError::unauthorized)?;
        list_projects_for_claims(db, &claims, role, limit, offset).await?
    } else {
        sqlx::query_as::<_, Project>("SELECT id, name, repository_url, default_branch, created_at FROM projects ORDER BY created_at DESC LIMIT $1 OFFSET $2")
            .bind(limit)
            .bind(offset)
            .fetch_all(db)
            .await
            .map_err(ApiError::internal)?
    };
    Ok(Json(projects))
}

#[utoipa::path(get, path="/api/v1/projects/{project_id}/memberships", tag="memberships", params(("project_id"=Uuid, Path)), responses((status=200, body=[ProjectMembership]), (status=404)))]
async fn list_project_memberships(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
) -> ApiResult<Vec<ProjectMembership>> {
    let db = pool(&state)?;
    ensure_project_exists(db, project_id).await?;
    let rows = sqlx::query_as::<_, ProjectMembership>(
        "SELECT m.project_id, m.user_id, u.username, u.enabled AS user_enabled, \
                m.role, m.created_at, m.updated_at \
         FROM project_memberships m \
         JOIN users u ON u.id = m.user_id \
         WHERE m.project_id = $1 \
         ORDER BY m.role, u.username",
    )
    .bind(project_id)
    .fetch_all(db)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(rows))
}

#[utoipa::path(post, path="/api/v1/projects/{project_id}/memberships", tag="memberships", request_body=ProjectMembershipInput, params(("project_id"=Uuid, Path)), responses((status=200, body=ProjectMembership), (status=400), (status=404)))]
async fn upsert_project_membership(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<ProjectMembershipInput>,
) -> ApiResult<ProjectMembership> {
    let role = input.role.trim();
    if !valid_project_role(role) {
        return Err(ApiError::bad_request(
            "role (maintainer, developer, viewer) is required",
        ));
    }
    let db = pool(&state)?;
    ensure_project_exists(db, project_id).await?;
    ensure_user_exists(db, input.user_id).await?;
    let membership = upsert_project_membership_record(db, project_id, input.user_id, role).await?;
    audit(
        db,
        "project_membership.upserted",
        "project",
        project_id,
        Some(&format!("{}:{}", membership.user_id, membership.role)),
    )
    .await?;
    Ok(Json(membership))
}

#[utoipa::path(delete, path="/api/v1/projects/{project_id}/memberships/{user_id}", tag="memberships", params(("project_id"=Uuid, Path), ("user_id"=Uuid, Path)), responses((status=200), (status=404), (status=409)))]
async fn delete_project_membership(
    State(state): State<Arc<AppState>>,
    Path((project_id, user_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<serde_json::Value> {
    let db = pool(&state)?;
    ensure_project_exists(db, project_id).await?;
    let Some(role) = sqlx::query_scalar::<_, String>(
        "SELECT role FROM project_memberships WHERE project_id = $1 AND user_id = $2",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(db)
    .await
    .map_err(ApiError::internal)?
    else {
        return Err(ApiError::not_found());
    };
    if role == "maintainer" {
        let maintainer_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM project_memberships WHERE project_id = $1 AND role = 'maintainer'",
        )
        .bind(project_id)
        .fetch_one(db)
        .await
        .map_err(ApiError::internal)?;
        if maintainer_count <= 1 {
            return Err(ApiError::conflict(
                "project must keep at least one maintainer",
            ));
        }
    }
    sqlx::query("DELETE FROM project_memberships WHERE project_id = $1 AND user_id = $2")
        .bind(project_id)
        .bind(user_id)
        .execute(db)
        .await
        .map_err(ApiError::internal)?;
    audit(
        db,
        "project_membership.deleted",
        "project",
        project_id,
        Some(&user_id.to_string()),
    )
    .await?;
    Ok(Json(
        serde_json::json!({"deleted": user_id, "project_id": project_id}),
    ))
}

async fn ensure_project_exists(db: &PgPool, project_id: Uuid) -> Result<(), ApiError> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM projects WHERE id = $1)")
        .bind(project_id)
        .fetch_one(db)
        .await
        .map_err(ApiError::internal)?;
    if exists {
        Ok(())
    } else {
        Err(ApiError::not_found())
    }
}

async fn ensure_user_exists(db: &PgPool, user_id: Uuid) -> Result<(), ApiError> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)")
        .bind(user_id)
        .fetch_one(db)
        .await
        .map_err(ApiError::internal)?;
    if exists {
        Ok(())
    } else {
        Err(ApiError::not_found_named("user"))
    }
}

async fn upsert_project_membership_record(
    db: &PgPool,
    project_id: Uuid,
    user_id: Uuid,
    role: &str,
) -> Result<ProjectMembership, ApiError> {
    sqlx::query_as::<_, ProjectMembership>(
        "WITH upsert AS ( \
             INSERT INTO project_memberships (project_id, user_id, role) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (project_id, user_id) DO UPDATE \
                 SET role = EXCLUDED.role, updated_at = now() \
             RETURNING project_id, user_id, role, created_at, updated_at \
         ) \
         SELECT upsert.project_id, upsert.user_id, u.username, u.enabled AS user_enabled, \
                upsert.role, upsert.created_at, upsert.updated_at \
         FROM upsert JOIN users u ON u.id = upsert.user_id",
    )
    .bind(project_id)
    .bind(user_id)
    .bind(role)
    .fetch_one(db)
    .await
    .map_err(ApiError::internal)
}

fn default_project_role(global_role: &str) -> Option<&'static str> {
    match crate::authz::Role::parse(global_role)? {
        crate::authz::Role::Admin | crate::authz::Role::Maintainer => Some("maintainer"),
        crate::authz::Role::Developer => Some("developer"),
        crate::authz::Role::Viewer => None,
    }
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
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
struct PipelinePlan {
    pipeline_id: Uuid,
    config_source: String,
    parser_version: String,
    git_ref: String,
    resolved_commit_sha: Option<String>,
    config_sha256: String,
    plan_sha256: String,
    raw_config: String,
    plan: serde_json::Value,
    created_at: DateTime<Utc>,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
struct PipelineDetail {
    pipeline: Pipeline,
    plan: Option<PipelinePlan>,
    stages: Vec<StageDetail>,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
struct StageDetail {
    #[serde(flatten)]
    stage: Stage,
    jobs: Vec<Job>,
}

#[utoipa::path(
    post,
    path="/api/v1/projects/{project_id}/pipelines",
    tag="pipelines",
    request_body=TriggerPipeline,
    params(
        ("project_id"=Uuid, Path),
        ("Idempotency-Key" = Option<Uuid>, Header, description = "Optional UUID idempotency key for retry-safe pipeline trigger")
    ),
    responses((status=200, body=PipelineDetail), (status=400), (status=404), (status=409))
)]
async fn trigger_pipeline(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<TriggerPipeline>,
) -> Result<(HeaderMap, Json<PipelineDetail>), ApiError> {
    let idempotency_key = pipeline_idempotency_key(&headers)?;
    let pool = pool(&state)?;
    let git_ref = input.git_ref.unwrap_or_else(|| "main".into());
    let outcome = create_pipeline_with_vars_idempotent(
        pool,
        project_id,
        git_ref,
        serde_json::to_value(input.variables.clone().unwrap_or_default())
            .unwrap_or_else(|_| serde_json::json!({})),
        PIPELINE_TRIGGER_SOURCE_API,
        idempotency_key.as_deref(),
    )
    .await?;
    let mut response_headers = HeaderMap::new();
    if outcome.replayed {
        response_headers.insert("idempotency-replayed", HeaderValue::from_static("true"));
    }
    pipeline_detail(pool, outcome.pipeline.id)
        .await
        .map(Json)
        .map(|body| (response_headers, body))
}

#[derive(Debug)]
pub(crate) struct PipelineTriggerOutcome {
    pub(crate) pipeline: Pipeline,
    pub(crate) replayed: bool,
}

pub(crate) async fn create_pipeline_with_vars_idempotent(
    pool: &PgPool,
    project_id: Uuid,
    git_ref: String,
    variables: serde_json::Value,
    source: &str,
    idempotency_key: Option<&str>,
) -> Result<PipelineTriggerOutcome, ApiError> {
    let repository_url: String =
        sqlx::query_scalar("SELECT repository_url FROM projects WHERE id = $1")
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(ApiError::internal)?
            .ok_or_else(ApiError::not_found)?;
    // Never clone here: this path is called by post-receive and must return
    // before git-receive-pack finishes. Local config is read from the bare repo.
    let commit_sha = resolve_commit_sha(Some(repository_url.as_str()), &git_ref).await;
    let config_ref = commit_sha.as_deref().unwrap_or(&git_ref);
    let config = read_local_forge_ci_config(Some(repository_url.as_str()), config_ref).await;
    let (config_source, raw_config) = match config {
        Some(raw_config) => ("repository", raw_config),
        None => ("legacy_template", LEGACY_TEMPLATE_CONFIG.to_string()),
    };
    let parsed_config = parse_pipeline_config(Some(&raw_config)).map_err(ApiError::bad_request)?;
    let plan_snapshot = build_pipeline_plan_snapshot(
        &git_ref,
        commit_sha.as_deref(),
        config_source,
        raw_config,
        &parsed_config,
    );
    let fingerprint = pipeline_trigger_fingerprint(&git_ref, &variables);
    let mut tx = pool.begin().await.map_err(ApiError::internal)?;

    if let Some(idempotency_key) = idempotency_key {
        let lock_key = format!("pipeline-trigger:{project_id}:{source}:{idempotency_key}");
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(&lock_key)
            .execute(&mut *tx)
            .await
            .map_err(ApiError::internal)?;

        if let Some((pipeline_id, existing_fingerprint)) = sqlx::query_as::<_, (Uuid, String)>(
            "SELECT pipeline_id, request_fingerprint \
                 FROM pipeline_triggers \
                 WHERE project_id = $1 AND source = $2 AND idempotency_key = $3",
        )
        .bind(project_id)
        .bind(source)
        .bind(idempotency_key)
        .fetch_optional(&mut *tx)
        .await
        .map_err(ApiError::internal)?
        {
            if existing_fingerprint != fingerprint {
                return Err(ApiError::conflict(
                    "idempotency key was already used for a different pipeline trigger",
                ));
            }
            let pipeline = sqlx::query_as::<_, Pipeline>(
                "SELECT id, project_id, git_ref, status, created_at, started_at, finished_at \
                 FROM pipelines WHERE id = $1",
            )
            .bind(pipeline_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(ApiError::internal)?;
            tx.commit().await.map_err(ApiError::internal)?;
            return Ok(PipelineTriggerOutcome {
                pipeline,
                replayed: true,
            });
        }
    }

    let pipeline = sqlx::query_as::<_, Pipeline>(
        "INSERT INTO pipelines (id, project_id, git_ref, commit_sha, variables, status) VALUES ($1, $2, $3, $4, $5, 'queued') RETURNING id, project_id, git_ref, status, created_at, started_at, finished_at",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(&git_ref)
    .bind(&commit_sha)
    .bind(&variables)
    .fetch_one(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    sqlx::query(
        "INSERT INTO pipeline_plans \
         (pipeline_id, config_source, parser_version, git_ref, resolved_commit_sha, config_sha256, plan_sha256, raw_config, plan) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(pipeline.id)
    .bind(plan_snapshot.config_source)
    .bind(plan_snapshot.parser_version)
    .bind(&plan_snapshot.git_ref)
    .bind(&plan_snapshot.resolved_commit_sha)
    .bind(&plan_snapshot.config_sha256)
    .bind(&plan_snapshot.plan_sha256)
    .bind(&plan_snapshot.raw_config)
    .bind(&plan_snapshot.plan)
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    for (position, stage) in parsed_config.stages.iter().enumerate() {
        let stage_id = Uuid::new_v4();
        sqlx::query("INSERT INTO stages (id, pipeline_id, name, position, status) VALUES ($1, $2, $3, $4, 'queued')")
            .bind(stage_id).bind(pipeline.id).bind(&stage.name).bind(position as i32).execute(&mut *tx).await.map_err(ApiError::internal)?;
        for (job_position, job) in stage.jobs.iter().enumerate() {
            let job_id = Uuid::new_v4();
            sqlx::query("INSERT INTO jobs (id, stage_id, name, image, command, position, status, timeout_seconds, allow_failure, manual) VALUES ($1, $2, $3, $4, $5, $6, 'queued', $7, $8, $9)")
                .bind(job_id).bind(stage_id).bind(&job.name).bind(&job.image).bind(&job.command).bind(job_position as i32)
                .bind(job.timeout_seconds).bind(job.allow_failure).bind(job.manual)
                .execute(&mut *tx).await.map_err(ApiError::internal)?;
            sqlx::query("INSERT INTO execution_attempts (id, job_id, attempt_no, status, trigger) VALUES ($1, $2, 1, 'queued', 'initial')")
                .bind(Uuid::new_v4()).bind(job_id).execute(&mut *tx).await.map_err(ApiError::internal)?;
        }
    }
    if let Some(idempotency_key) = idempotency_key {
        sqlx::query(
            "INSERT INTO pipeline_triggers \
             (id, project_id, source, idempotency_key, request_fingerprint, pipeline_id) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(Uuid::new_v4())
        .bind(project_id)
        .bind(source)
        .bind(idempotency_key)
        .bind(&fingerprint)
        .bind(pipeline.id)
        .execute(&mut *tx)
        .await
        .map_err(ApiError::internal)?;
    }
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(PipelineTriggerOutcome {
        pipeline,
        replayed: false,
    })
}

fn pipeline_idempotency_key(headers: &HeaderMap) -> Result<Option<String>, ApiError> {
    let Some(value) = headers.get(IDEMPOTENCY_KEY_HEADER) else {
        return Ok(None);
    };
    let raw = value
        .to_str()
        .map_err(|_| ApiError::bad_request("idempotency key must be a UUID"))?
        .trim();
    if raw.is_empty() {
        return Err(ApiError::bad_request("idempotency key must be a UUID"));
    }
    Uuid::parse_str(raw)
        .map(|uuid| Some(uuid.to_string()))
        .map_err(|_| ApiError::bad_request("idempotency key must be a UUID"))
}

fn pipeline_trigger_fingerprint(git_ref: &str, variables: &serde_json::Value) -> String {
    let payload = serde_json::json!({
        "git_ref": git_ref,
        "variables": variables,
    });
    sha256_hex(&serde_json::to_vec(&payload).unwrap_or_default())
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Debug)]
struct ParsedPipelineConfig {
    stages: Vec<CiStage>,
    plan: ParsedPipelinePlan,
}

#[derive(Debug)]
enum ParsedPipelinePlan {
    Legacy,
    V1(V1PlanData),
}

impl ParsedPipelineConfig {
    fn parser_version(&self) -> &'static str {
        match self.plan {
            ParsedPipelinePlan::Legacy => LEGACY_PIPELINE_PLAN_PARSER_VERSION,
            ParsedPipelinePlan::V1(_) => V1_PIPELINE_PLAN_PARSER_VERSION,
        }
    }
}

#[derive(Debug)]
struct PipelinePlanSnapshot {
    config_source: &'static str,
    parser_version: &'static str,
    git_ref: String,
    resolved_commit_sha: Option<String>,
    config_sha256: String,
    plan_sha256: String,
    raw_config: String,
    plan: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct LegacyExecutionPlan {
    format: &'static str,
    parser_version: &'static str,
    config_source: &'static str,
    git_ref: String,
    resolved_commit_sha: Option<String>,
    stages: Vec<LegacyPlanStage>,
    dependencies: Vec<LegacyPlanDependency>,
}

#[derive(Debug, Serialize)]
struct LegacyPlanStage {
    name: String,
    position: i32,
    jobs: Vec<LegacyPlanJob>,
}

#[derive(Debug, Serialize)]
struct LegacyPlanJob {
    key: String,
    name: String,
    stage: String,
    stage_position: i32,
    position: i32,
    image: String,
    command: String,
    timeout_seconds: Option<i32>,
    allow_failure: bool,
    manual: bool,
    needs: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
struct LegacyPlanDependency {
    from: String,
    to: String,
}

#[derive(Debug)]
struct V1PlanData {
    jobs: Vec<V1PlanJob>,
    dependencies: Vec<LegacyPlanDependency>,
}

#[derive(Debug, Serialize)]
struct V1ExecutionPlan {
    format: &'static str,
    version: u8,
    parser_version: &'static str,
    config_source: &'static str,
    git_ref: String,
    resolved_commit_sha: Option<String>,
    jobs: Vec<V1PlanJob>,
    dependencies: Vec<LegacyPlanDependency>,
}

#[derive(Clone, Debug, Serialize)]
struct V1PlanJob {
    key: String,
    stage: String,
    stage_position: i32,
    position: i32,
    image: String,
    commands: Vec<String>,
    command: String,
    timeout_seconds: Option<i32>,
    allow_failure: bool,
    needs: Vec<String>,
}

fn build_pipeline_plan_snapshot(
    git_ref: &str,
    resolved_commit_sha: Option<&str>,
    config_source: &'static str,
    raw_config: String,
    parsed_config: &ParsedPipelineConfig,
) -> PipelinePlanSnapshot {
    let parser_version = parsed_config.parser_version();
    let plan = match &parsed_config.plan {
        ParsedPipelinePlan::Legacy => serde_json::to_value(build_legacy_execution_plan(
            git_ref,
            resolved_commit_sha,
            config_source,
            &parsed_config.stages,
        ))
        .expect("serialize legacy execution plan"),
        ParsedPipelinePlan::V1(v1_plan) => serde_json::to_value(build_v1_execution_plan(
            git_ref,
            resolved_commit_sha,
            config_source,
            v1_plan,
        ))
        .expect("serialize v1 execution plan"),
    };
    let plan_bytes = serde_json::to_vec(&plan).expect("serialize execution plan value");
    PipelinePlanSnapshot {
        config_source,
        parser_version,
        git_ref: git_ref.to_string(),
        resolved_commit_sha: resolved_commit_sha.map(ToOwned::to_owned),
        config_sha256: sha256_hex(raw_config.as_bytes()),
        plan_sha256: sha256_hex(&plan_bytes),
        raw_config,
        plan,
    }
}

fn build_legacy_execution_plan(
    git_ref: &str,
    resolved_commit_sha: Option<&str>,
    config_source: &'static str,
    stages: &[CiStage],
) -> LegacyExecutionPlan {
    let mut planned_stages = Vec::with_capacity(stages.len());
    let mut dependencies = Vec::new();
    let mut previous_stage_keys: Vec<String> = Vec::new();

    for (stage_position, stage) in stages.iter().enumerate() {
        let stage_position = stage_position as i32;
        let mut current_stage_keys = Vec::with_capacity(stage.jobs.len());
        let mut planned_jobs = Vec::with_capacity(stage.jobs.len());
        for (job_position, job) in stage.jobs.iter().enumerate() {
            let job_position = job_position as i32;
            let key = format!("stage-{stage_position}/job-{job_position}");
            let needs = previous_stage_keys.clone();
            for predecessor in &needs {
                dependencies.push(LegacyPlanDependency {
                    from: predecessor.clone(),
                    to: key.clone(),
                });
            }
            current_stage_keys.push(key.clone());
            planned_jobs.push(LegacyPlanJob {
                key,
                name: job.name.clone(),
                stage: stage.name.clone(),
                stage_position,
                position: job_position,
                image: job.image.clone(),
                command: job.command.clone(),
                timeout_seconds: job.timeout_seconds,
                allow_failure: job.allow_failure,
                manual: job.manual,
                needs,
            });
        }
        previous_stage_keys = current_stage_keys;
        planned_stages.push(LegacyPlanStage {
            name: stage.name.clone(),
            position: stage_position,
            jobs: planned_jobs,
        });
    }

    LegacyExecutionPlan {
        format: "legacy-linear",
        parser_version: LEGACY_PIPELINE_PLAN_PARSER_VERSION,
        config_source,
        git_ref: git_ref.to_string(),
        resolved_commit_sha: resolved_commit_sha.map(ToOwned::to_owned),
        stages: planned_stages,
        dependencies,
    }
}

fn build_v1_execution_plan(
    git_ref: &str,
    resolved_commit_sha: Option<&str>,
    config_source: &'static str,
    plan: &V1PlanData,
) -> V1ExecutionPlan {
    V1ExecutionPlan {
        format: "v1-dag",
        version: 1,
        parser_version: V1_PIPELINE_PLAN_PARSER_VERSION,
        config_source,
        git_ref: git_ref.to_string(),
        resolved_commit_sha: resolved_commit_sha.map(ToOwned::to_owned),
        jobs: plan.jobs.clone(),
        dependencies: plan.dependencies.clone(),
    }
}

#[derive(Clone, Debug, Default)]
struct CiStage {
    name: String,
    jobs: Vec<CiJob>,
}
#[derive(Clone, Debug)]
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
#[cfg(test)]
fn parse_forge_ci(raw: Option<&str>) -> Result<Vec<CiStage>, String> {
    Ok(parse_pipeline_config(raw)?.stages)
}

fn parse_pipeline_config(raw: Option<&str>) -> Result<ParsedPipelineConfig, String> {
    let Some(raw) = raw else {
        return Ok(ParsedPipelineConfig {
            stages: default_pipeline(),
            plan: ParsedPipelinePlan::Legacy,
        });
    };
    if raw.len() > 1024 * 1024 {
        return Err(".forge-ci.yml must be no larger than 1 MiB".into());
    }

    let root: serde_yaml::Value =
        serde_yaml::from_str(raw).map_err(|error| format!("invalid .forge-ci.yml: {error}"))?;
    let has_version = match root {
        serde_yaml::Value::Mapping(mapping) => {
            mapping.contains_key(serde_yaml::Value::String("version".into()))
        }
        _ => return Err(".forge-ci.yml must be a YAML object".into()),
    };
    if has_version {
        parse_v1_pipeline_config(raw)
    } else {
        parse_legacy_pipeline_config(raw)
    }
}

fn parse_legacy_pipeline_config(raw: &str) -> Result<ParsedPipelineConfig, String> {
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
    Ok(ParsedPipelineConfig {
        stages,
        plan: ParsedPipelinePlan::Legacy,
    })
}

#[derive(Clone, Debug)]
struct NormalizedV1Job {
    key: String,
    image: String,
    commands: Vec<String>,
    timeout_seconds: Option<i32>,
    allow_failure: bool,
    needs: Vec<String>,
}

fn parse_v1_pipeline_config(raw: &str) -> Result<ParsedPipelineConfig, String> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct YamlV1Config {
        version: u8,
        #[serde(default)]
        defaults: YamlV1Defaults,
        jobs: BTreeMap<String, YamlV1Job>,
    }

    #[derive(Default, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct YamlV1Defaults {
        image: Option<String>,
        timeout: Option<String>,
    }

    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct YamlV1Job {
        #[serde(default)]
        needs: Vec<String>,
        image: Option<String>,
        commands: Vec<String>,
        timeout: Option<String>,
        #[serde(default)]
        allow_failure: bool,
    }

    let parsed: YamlV1Config =
        serde_yaml::from_str(raw).map_err(|error| format!("invalid .forge-ci.yml v1: {error}"))?;
    if parsed.version != 1 {
        return Err("unsupported .forge-ci.yml version; only version: 1 is supported".into());
    }
    if parsed.jobs.is_empty() {
        return Err(".forge-ci.yml v1 must define at least one job".into());
    }
    if parsed.jobs.len() > 500 {
        return Err(".forge-ci.yml v1 supports at most 500 jobs".into());
    }

    let default_image = parsed
        .defaults
        .image
        .unwrap_or_else(|| "alpine:3.21".into());
    if !is_safe_image_reference(&default_image) {
        return Err(".forge-ci.yml v1 defaults.image must be a safe image reference".into());
    }
    let default_timeout = parse_v1_timeout("defaults.timeout", parsed.defaults.timeout.as_deref())?;

    let mut jobs = BTreeMap::new();
    let mut edge_count = 0usize;
    for (key, job) in parsed.jobs {
        if !is_valid_v1_job_key(&key) {
            return Err(format!(
                ".forge-ci.yml v1 job key '{key}' must match ^[a-zA-Z][a-zA-Z0-9_.-]{{0,62}}$"
            ));
        }
        if job.commands.is_empty() || job.commands.len() > 64 {
            return Err(format!(
                ".forge-ci.yml v1 job '{key}' must define 1..64 commands"
            ));
        }
        for command in &job.commands {
            if command.trim().is_empty() || command.len() > 16 * 1024 || command.contains('\0') {
                return Err(format!(
                    ".forge-ci.yml v1 job '{key}' has an empty or oversized command"
                ));
            }
        }

        let image = job.image.unwrap_or_else(|| default_image.clone());
        if !is_safe_image_reference(&image) {
            return Err(format!(
                ".forge-ci.yml v1 job '{key}' image must be a safe image reference"
            ));
        }
        let timeout_seconds = match job.timeout.as_deref() {
            Some(raw) => parse_v1_timeout(&format!("jobs.{key}.timeout"), Some(raw))?,
            None => default_timeout,
        };

        if job.needs.len() > 64 {
            return Err(format!(
                ".forge-ci.yml v1 job '{key}' can depend on at most 64 jobs"
            ));
        }
        let mut unique_needs = BTreeSet::new();
        let mut needs = Vec::with_capacity(job.needs.len());
        for need in job.needs {
            if !is_valid_v1_job_key(&need) {
                return Err(format!(
                    ".forge-ci.yml v1 job '{key}' has invalid need key '{need}'"
                ));
            }
            if !unique_needs.insert(need.clone()) {
                return Err(format!(
                    ".forge-ci.yml v1 job '{key}' lists dependency '{need}' more than once"
                ));
            }
            needs.push(need);
        }
        needs.sort();
        edge_count += needs.len();
        if edge_count > 10_000 {
            return Err(".forge-ci.yml v1 supports at most 10000 DAG edges".into());
        }

        jobs.insert(
            key.clone(),
            NormalizedV1Job {
                key,
                image,
                commands: job.commands,
                timeout_seconds,
                allow_failure: job.allow_failure,
                needs,
            },
        );
    }

    for job in jobs.values() {
        for need in &job.needs {
            if need == &job.key {
                return Err(format!(
                    ".forge-ci.yml v1 job '{}' cannot depend on itself",
                    job.key
                ));
            }
            if !jobs.contains_key(need) {
                return Err(format!(
                    ".forge-ci.yml v1 job '{}' depends on missing job '{need}'",
                    job.key
                ));
            }
        }
    }

    let levels = compute_v1_job_levels(&jobs)?;
    let mut grouped: BTreeMap<i32, Vec<NormalizedV1Job>> = BTreeMap::new();
    for (key, job) in jobs {
        let level = *levels
            .get(&key)
            .expect("every v1 job must have a computed topological level");
        grouped.entry(level).or_default().push(job);
    }

    let mut stages = Vec::with_capacity(grouped.len());
    let mut plan_jobs = Vec::new();
    for (stage_position, jobs) in grouped.into_values().enumerate() {
        let stage_position = stage_position as i32;
        let stage_name = format!("dag-{stage_position}");
        let mut ci_jobs = Vec::with_capacity(jobs.len());
        for (job_position, job) in jobs.into_iter().enumerate() {
            let job_position = job_position as i32;
            let command = v1_runtime_command(&job.commands);
            ci_jobs.push(CiJob {
                name: job.key.clone(),
                image: job.image.clone(),
                command: command.clone(),
                timeout_seconds: job.timeout_seconds,
                allow_failure: job.allow_failure,
                manual: false,
            });
            plan_jobs.push(V1PlanJob {
                key: job.key,
                stage: stage_name.clone(),
                stage_position,
                position: job_position,
                image: job.image,
                commands: job.commands,
                command,
                timeout_seconds: job.timeout_seconds,
                allow_failure: job.allow_failure,
                needs: job.needs,
            });
        }
        stages.push(CiStage {
            name: stage_name,
            jobs: ci_jobs,
        });
    }
    validate_ci_stages(&stages)?;

    let mut dependencies = Vec::with_capacity(edge_count);
    for job in &plan_jobs {
        for need in &job.needs {
            dependencies.push(LegacyPlanDependency {
                from: need.clone(),
                to: job.key.clone(),
            });
        }
    }

    Ok(ParsedPipelineConfig {
        stages,
        plan: ParsedPipelinePlan::V1(V1PlanData {
            jobs: plan_jobs,
            dependencies,
        }),
    })
}

fn v1_runtime_command(commands: &[String]) -> String {
    format!("set -e\n{}", commands.join("\n"))
}

fn compute_v1_job_levels(
    jobs: &BTreeMap<String, NormalizedV1Job>,
) -> Result<BTreeMap<String, i32>, String> {
    fn visit(
        key: &str,
        jobs: &BTreeMap<String, NormalizedV1Job>,
        levels: &mut BTreeMap<String, i32>,
        visiting: &mut BTreeSet<String>,
    ) -> Result<i32, String> {
        if let Some(level) = levels.get(key) {
            return Ok(*level);
        }
        if !visiting.insert(key.to_string()) {
            return Err(format!(
                ".forge-ci.yml v1 jobs.needs contains a dependency cycle at '{key}'"
            ));
        }
        let job = jobs
            .get(key)
            .ok_or_else(|| format!(".forge-ci.yml v1 references missing job '{key}'"))?;
        let mut level = 0;
        for need in &job.needs {
            level = level.max(visit(need, jobs, levels, visiting)? + 1);
        }
        visiting.remove(key);
        levels.insert(key.to_string(), level);
        Ok(level)
    }

    let mut levels = BTreeMap::new();
    let mut visiting = BTreeSet::new();
    for key in jobs.keys() {
        visit(key, jobs, &mut levels, &mut visiting)?;
    }
    Ok(levels)
}

fn parse_timeout(raw: Option<&str>) -> Option<i32> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    let (value, unit) = raw.split_at(raw.find(|c: char| !c.is_ascii_digit()).unwrap_or(raw.len()));
    let value: i32 = value.parse().ok()?;
    match unit.trim() {
        "s" | "sec" | "secs" | "" => Some(value),
        "m" | "min" | "mins" => value.checked_mul(60),
        "h" | "hour" | "hours" => value.checked_mul(3600),
        _ => None,
    }
}

fn parse_v1_timeout(field: &str, raw: Option<&str>) -> Result<Option<i32>, String> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let digit_count = raw
        .char_indices()
        .find(|(_, character)| !character.is_ascii_digit())
        .map(|(index, _)| index)
        .unwrap_or(raw.len());
    if digit_count == 0 || digit_count == raw.len() {
        return Err(format!(
            ".forge-ci.yml v1 {field} must use a positive duration with unit s, m, h, or d"
        ));
    }
    let value: i32 = raw[..digit_count]
        .parse()
        .map_err(|_| format!(".forge-ci.yml v1 {field} duration is too large"))?;
    if value <= 0 {
        return Err(format!(
            ".forge-ci.yml v1 {field} must use a positive duration"
        ));
    }
    let seconds = match &raw[digit_count..] {
        "s" => value,
        "m" => value
            .checked_mul(60)
            .ok_or_else(|| format!(".forge-ci.yml v1 {field} duration is too large"))?,
        "h" => value
            .checked_mul(3600)
            .ok_or_else(|| format!(".forge-ci.yml v1 {field} duration is too large"))?,
        "d" => value
            .checked_mul(86_400)
            .ok_or_else(|| format!(".forge-ci.yml v1 {field} duration is too large"))?,
        _ => {
            return Err(format!(
                ".forge-ci.yml v1 {field} must use duration unit s, m, h, or d"
            ));
        }
    };
    if seconds > 86_400 {
        return Err(format!(".forge-ci.yml v1 {field} must not exceed 24h"));
    }
    Ok(Some(seconds))
}

fn is_valid_v1_job_key(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    key.len() <= 63
        && first.is_ascii_alphabetic()
        && chars.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '-')
        })
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
    claims: Option<axum::Extension<crate::auth::AccessClaims>>,
    Json(input): Json<CreateRelease>,
) -> ApiResult<Release> {
    let created_by = claims.map(|c| c.0.sub.to_string());
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

fn valid_project_role(role: &str) -> bool {
    crate::authz::Role::parse(role).is_some_and(crate::authz::Role::is_project_role)
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use tower::ServiceExt;

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
    fn project_scope_ref_maps_project_owned_routes() {
        let project_id = Uuid::new_v4();
        let pipeline_id = Uuid::new_v4();
        let job_id = Uuid::new_v4();
        let artifact_id = Uuid::new_v4();
        let secret_id = Uuid::new_v4();
        let environment_id = Uuid::new_v4();
        let schedule_id = Uuid::new_v4();
        let webhook_id = Uuid::new_v4();

        assert_eq!(
            project_scope_ref(&format!("/api/v1/projects/{project_id}/secrets")),
            Some(ProjectScopeRef::Project(project_id))
        );
        assert_eq!(
            project_scope_ref(&format!("/api/v1/projects/{project_id}/memberships")),
            Some(ProjectScopeRef::Project(project_id))
        );
        assert_eq!(
            project_scope_ref(&format!(
                "/api/v1/projects/{project_id}/notification-events"
            )),
            Some(ProjectScopeRef::Project(project_id))
        );
        assert_eq!(
            project_scope_ref(&format!("/api/v1/projects/{project_id}/outbox-deliveries")),
            Some(ProjectScopeRef::Project(project_id))
        );
        assert_eq!(
            project_scope_ref(&format!(
                "/api/v1/projects/{project_id}/notifications/stream"
            )),
            Some(ProjectScopeRef::Project(project_id))
        );
        assert_eq!(
            project_scope_ref(&format!("/api/v1/outbox-deliveries/{webhook_id}")),
            Some(ProjectScopeRef::OutboxDelivery(webhook_id))
        );
        assert_eq!(
            project_scope_ref(&format!("/api/v1/outbox-deliveries/{webhook_id}/requeue")),
            Some(ProjectScopeRef::OutboxDelivery(webhook_id))
        );
        assert_eq!(
            project_scope_ref(&format!("/api/v1/pipelines/{pipeline_id}/retry")),
            Some(ProjectScopeRef::Pipeline(pipeline_id))
        );
        assert_eq!(
            project_scope_ref(&format!("/api/v1/jobs/{job_id}/attempts")),
            Some(ProjectScopeRef::Job(job_id))
        );
        assert_eq!(
            project_scope_ref(&format!("/api/v1/artifacts/{artifact_id}/download")),
            Some(ProjectScopeRef::Artifact(artifact_id))
        );
        assert_eq!(
            project_scope_ref(&format!("/api/v1/secrets/{secret_id}")),
            Some(ProjectScopeRef::Secret(secret_id))
        );
        assert_eq!(
            project_scope_ref(&format!(
                "/api/v1/environments/{environment_id}/deployments"
            )),
            Some(ProjectScopeRef::Environment(environment_id))
        );
        assert_eq!(
            project_scope_ref(&format!("/api/v1/schedules/{schedule_id}")),
            Some(ProjectScopeRef::Schedule(schedule_id))
        );
        assert_eq!(
            project_scope_ref(&format!("/api/v1/webhooks/{webhook_id}")),
            Some(ProjectScopeRef::Webhook(webhook_id))
        );
        assert_eq!(
            project_scope_ref("/api/v1/repos/demo/refs"),
            Some(ProjectScopeRef::Repository("demo".to_string()))
        );
        assert_eq!(
            project_scope_ref("/api/v1/repositories/demo"),
            Some(ProjectScopeRef::Repository("demo".to_string()))
        );
        assert_eq!(project_scope_ref("/api/v1/projects"), None);
        assert_eq!(project_scope_ref("/api/v1/repositories"), None);
    }

    #[test]
    fn pat_scopes_are_enforced_by_method() {
        let claims = crate::auth::AccessClaims {
            sub: Uuid::new_v4(),
            sid: None,
            token_id: Some(Uuid::new_v4()),
            token_project_id: Some(Uuid::new_v4()),
            token_scopes: vec!["api:read".to_string()],
            role: "admin".to_string(),
            iat: 0,
            exp: 900,
        };
        assert!(api_token_scope_allows(&claims, "GET"));
        assert!(!api_token_scope_allows(&claims, "POST"));
    }

    #[test]
    fn log_page_params_are_bounded_and_search_is_escaped() {
        let params = LogPageParams {
            after: Some(0),
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
                limit: Some(50),
                q: None,
            }
            .after_sequence()
            .is_err()
        );
        assert!(
            LogPageParams {
                after: None,
                limit: Some(201),
                q: None,
            }
            .bounded_limit()
            .is_err()
        );
        assert!(
            LogPageParams {
                after: None,
                limit: Some(50),
                q: Some("x".repeat(129)),
            }
            .search_pattern()
            .is_err()
        );
    }

    #[test]
    fn project_membership_roles_exclude_instance_admin() {
        assert!(valid_project_role("maintainer"));
        assert!(valid_project_role("developer"));
        assert!(valid_project_role("viewer"));
        assert!(!valid_project_role("admin"));
        assert_eq!(default_project_role("admin"), Some("maintainer"));
        assert_eq!(default_project_role("developer"), Some("developer"));
        assert_eq!(default_project_role("viewer"), None);
    }

    #[test]
    fn rate_limit_rules_cover_api_git_and_artifact_routes() {
        assert_eq!(rate_limit_rule(&Method::GET, "/api/v1/health"), None);
        assert_eq!(rate_limit_rule(&Method::GET, "/api/v1/readiness"), None);
        assert_eq!(
            rate_limit_rule(&Method::POST, "/api/v1/auth/login"),
            Some(RateLimitRule {
                class: "auth-login",
                limit: 30,
                window_secs: 60
            })
        );
        assert_eq!(
            rate_limit_rule(&Method::POST, "/api/v1/internal/git-push")
                .expect("internal hook limit")
                .class,
            "internal-git-push"
        );
        assert_eq!(
            rate_limit_rule(&Method::POST, "/api/v1/auth/logout")
                .expect("logout limit")
                .class,
            "auth-logout"
        );
        assert_eq!(
            rate_limit_rule(&Method::POST, "/git/demo.git/git-receive-pack")
                .expect("git push limit")
                .class,
            "git-push"
        );
        assert_eq!(
            rate_limit_rule(
                &Method::POST,
                "/api/v1/jobs/00000000-0000-0000-0000-000000000001/artifacts"
            )
            .expect("artifact upload limit")
            .class,
            "artifact-upload"
        );
        assert_eq!(
            rate_limit_rule(
                &Method::PATCH,
                "/api/v1/projects/00000000-0000-0000-0000-000000000001"
            )
            .expect("api write limit")
            .class,
            "api-write"
        );
    }

    #[test]
    fn rate_limit_client_uses_forwarded_headers() {
        let headers = HeaderMap::new();
        assert_eq!(rate_limit_client(&headers), "unknown");

        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", HeaderValue::from_static("198.51.100.7"));
        assert_eq!(rate_limit_client(&headers), "198.51.100.7");

        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("203.0.113.9, 10.0.0.1"),
        );
        assert_eq!(rate_limit_client(&headers), "203.0.113.9");
    }

    #[tokio::test]
    async fn login_rate_limit_returns_429_per_forwarded_client() {
        let app = app(None);
        for _ in 0..30 {
            let response = app
                .clone()
                .oneshot(login_request("203.0.113.10"))
                .await
                .unwrap();
            assert_ne!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        }

        let response = app
            .clone()
            .oneshot(login_request("203.0.113.10"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

        let response = app.oneshot(login_request("203.0.113.11")).await.unwrap();
        assert_ne!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    fn login_request(client: &'static str) -> axum::http::Request<Body> {
        axum::http::Request::post("/api/v1/auth/login")
            .header("content-type", "application/json")
            .header("x-forwarded-for", client)
            .body(Body::from(r#"{"username":"nobody","password":"bad"}"#))
            .unwrap()
    }

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

    #[test]
    fn parses_v1_dag_into_topological_stages() {
        let parsed = parse_pipeline_config(Some(
            r#"
version: 1
defaults:
  image: alpine:3.21
  timeout: 20m
jobs:
  build:
    commands:
      - cargo build --release
  lint:
    commands:
      - cargo fmt --check
    allow_failure: true
  test:
    needs: [build, lint]
    image: rust:1.86
    timeout: 45m
    commands:
      - cargo test
      - cargo clippy --all-targets
"#,
        ))
        .expect("valid v1 configuration");

        assert_eq!(parsed.parser_version(), "forge-dsl/1.0.0");
        assert_eq!(
            parsed
                .stages
                .iter()
                .map(|stage| stage.name.as_str())
                .collect::<Vec<_>>(),
            ["dag-0", "dag-1"]
        );
        assert_eq!(
            parsed.stages[0]
                .jobs
                .iter()
                .map(|job| job.name.as_str())
                .collect::<Vec<_>>(),
            ["build", "lint"]
        );
        assert_eq!(parsed.stages[0].jobs[0].timeout_seconds, Some(1200));
        assert!(parsed.stages[0].jobs[1].allow_failure);
        assert_eq!(parsed.stages[1].jobs[0].name, "test");
        assert_eq!(parsed.stages[1].jobs[0].image, "rust:1.86");
        assert_eq!(parsed.stages[1].jobs[0].timeout_seconds, Some(2700));
        assert_eq!(
            parsed.stages[1].jobs[0].command,
            "set -e\ncargo test\ncargo clippy --all-targets"
        );
    }

    #[test]
    fn v1_pipeline_plan_is_deterministic_and_records_needs() {
        let raw_config = r#"
version: 1
jobs:
  package:
    needs: [test]
    commands: ["tar -cf app.tar target/release/app"]
  build:
    commands: ["cargo build --release"]
  test:
    needs: [build]
    commands: ["cargo test"]
"#;
        let parsed = parse_pipeline_config(Some(raw_config)).expect("valid v1");
        let first = build_pipeline_plan_snapshot(
            "main",
            Some("abc123"),
            "repository",
            raw_config.into(),
            &parsed,
        );
        let second = build_pipeline_plan_snapshot(
            "main",
            Some("abc123"),
            "repository",
            raw_config.into(),
            &parsed,
        );

        assert_eq!(first.parser_version, "forge-dsl/1.0.0");
        assert_eq!(first.plan_sha256, second.plan_sha256);
        assert_eq!(first.config_sha256, second.config_sha256);
        assert_eq!(first.plan["format"], "v1-dag");
        assert_eq!(first.plan["version"], 1);
        assert_eq!(first.plan["jobs"].as_array().unwrap().len(), 3);
        assert_eq!(first.plan["dependencies"].as_array().unwrap().len(), 2);
        assert_eq!(first.plan["dependencies"][0]["from"], "build");
        assert_eq!(first.plan["dependencies"][0]["to"], "test");
        assert_eq!(first.plan["dependencies"][1]["from"], "test");
        assert_eq!(first.plan["dependencies"][1]["to"], "package");
    }

    #[test]
    fn rejects_invalid_v1_dag_configuration() {
        for source in [
            "version: 1\njobs: {}\n",
            "version: 2\njobs:\n  build:\n    commands: [echo build]\n",
            "version: 1\njobs:\n  build:\n    needs: [missing]\n    commands: [echo build]\n",
            "version: 1\njobs:\n  build:\n    needs: [test]\n    commands: [echo build]\n  test:\n    needs: [build]\n    commands: [echo test]\n",
            "version: 1\njobs:\n  build:\n    commands: []\n",
            "version: 1\njobs:\n  build:\n    timeout: 25h\n    commands: [echo build]\n",
            "version: 1\njobs:\n  build:\n    artifacts:\n      paths: [target]\n    commands: [echo build]\n",
        ] {
            assert!(parse_pipeline_config(Some(source)).is_err(), "{source}");
        }
    }

    #[test]
    fn legacy_pipeline_plan_is_deterministic_and_records_stage_edges() {
        let parsed = ParsedPipelineConfig {
            stages: default_pipeline(),
            plan: ParsedPipelinePlan::Legacy,
        };
        let first = build_pipeline_plan_snapshot(
            "main",
            Some("abc123"),
            "legacy_template",
            LEGACY_TEMPLATE_CONFIG.to_string(),
            &parsed,
        );
        let second = build_pipeline_plan_snapshot(
            "main",
            Some("abc123"),
            "legacy_template",
            LEGACY_TEMPLATE_CONFIG.to_string(),
            &parsed,
        );

        assert_eq!(first.plan_sha256, second.plan_sha256);
        assert_eq!(first.config_sha256, second.config_sha256);
        assert_eq!(first.plan["format"], "legacy-linear");
        assert_eq!(first.plan["stages"].as_array().unwrap().len(), 3);
        assert_eq!(first.plan["dependencies"].as_array().unwrap().len(), 2);
        assert_eq!(first.plan["dependencies"][0]["from"], "stage-0/job-0");
        assert_eq!(first.plan["dependencies"][0]["to"], "stage-1/job-0");
        assert_eq!(first.plan["dependencies"][1]["from"], "stage-1/job-0");
        assert_eq!(first.plan["dependencies"][1]["to"], "stage-2/job-0");
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
    let plan = sqlx::query_as::<_, PipelinePlan>(
        "SELECT pipeline_id, config_source, parser_version, git_ref, resolved_commit_sha, config_sha256, plan_sha256, raw_config, plan, created_at \
         FROM pipeline_plans WHERE pipeline_id = $1",
    )
    .bind(pipeline_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?;
    let stages = sqlx::query_as::<_, Stage>("SELECT id, pipeline_id, name, position, status FROM stages WHERE pipeline_id = $1 ORDER BY position").bind(pipeline_id).fetch_all(pool).await.map_err(ApiError::internal)?;
    let mut details = Vec::with_capacity(stages.len());
    for stage in stages {
        let jobs = sqlx::query_as::<_, Job>("SELECT id, stage_id, name, image, command, position, status, started_at, finished_at FROM jobs WHERE stage_id = $1 ORDER BY position").bind(stage.id).fetch_all(pool).await.map_err(ApiError::internal)?;
        details.push(StageDetail { stage, jobs });
    }
    Ok(PipelineDetail {
        pipeline,
        plan,
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
    transition_open_attempt(pool, job_id, input.status.as_str(), "manual_status").await?;
    let updated = sqlx::query_as::<_, Job>("UPDATE jobs SET status = $2, started_at = CASE WHEN $2 = 'running' THEN now() ELSE started_at END, finished_at = CASE WHEN $2 IN ('success','failed','canceled') THEN now() ELSE finished_at END WHERE id = $1 RETURNING id, stage_id, name, image, command, position, status, started_at, finished_at").bind(job_id).bind(input.status.as_str()).fetch_one(pool).await.map_err(ApiError::internal)?;
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
    Ok(attempt_id)
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
struct CanceledPipelineResult {
    #[schema(value_type = String, format = Uuid)]
    canceled: Uuid,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
struct RetriedPipelineResult {
    #[schema(value_type = String, format = Uuid)]
    retried: Uuid,
}

#[utoipa::path(post, path="/api/v1/pipelines/{pipeline_id}/cancel", tag="pipelines", params(("pipeline_id"=Uuid, Path)), responses((status=200, body=CanceledPipelineResult), (status=404), (status=409)))]
async fn cancel_pipeline(
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

#[utoipa::path(post, path="/api/v1/pipelines/{pipeline_id}/retry", tag="pipelines", params(("pipeline_id"=Uuid, Path)), responses((status=200, body=RetriedPipelineResult), (status=404), (status=409)))]
async fn retry_pipeline(
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
    Ok(Json(RetriedPipelineResult {
        retried: pipeline_id,
    }))
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
struct JobAttempt {
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
struct JobLog {
    id: i64,
    job_id: Uuid,
    attempt_id: Uuid,
    sequence: i32,
    message: String,
    created_at: DateTime<Utc>,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
struct JobLogPage {
    items: Vec<JobLog>,
    next_after: Option<i32>,
}
#[derive(Debug, Deserialize, utoipa::IntoParams)]
struct LogPageParams {
    /// Return log rows with sequence greater than this value.
    after: Option<i32>,
    /// Page size. Default and maximum are 200 rows.
    limit: Option<i64>,
    /// Optional case-insensitive substring filter for message text.
    q: Option<String>,
}
impl LogPageParams {
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
#[derive(Deserialize, utoipa::ToSchema)]
struct AppendLog {
    message: String,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
struct ManualJobStartResult {
    started: bool,
}

#[utoipa::path(post, path="/api/v1/jobs/{job_id}/start", tag="jobs", params(("job_id"=Uuid, Path)), responses((status=200, body=ManualJobStartResult), (status=404), (status=409, description="job is not a waiting manual job")))]
/// Starts a manual (`when: manual`) job — approval gate (GitLab parity).
async fn start_manual_job(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
) -> ApiResult<ManualJobStartResult> {
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
    Ok(Json(ManualJobStartResult { started: true }))
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

#[derive(serde::Deserialize, utoipa::IntoParams)]
struct StreamParams {
    after: Option<i32>,
}

#[utoipa::path(get, path="/api/v1/jobs/{job_id}/attempts", tag="jobs", params(("job_id"=Uuid, Path)), responses((status=200, body=[JobAttempt]), (status=404)))]
async fn list_job_attempts(
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
async fn list_attempt_logs(
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
async fn list_attempt_logs_page(
    State(state): State<Arc<AppState>>,
    Path((job_id, attempt_id)): Path<(Uuid, Uuid)>,
    axum::extract::Query(params): axum::extract::Query<LogPageParams>,
) -> ApiResult<JobLogPage> {
    let db = pool(&state)?;
    ensure_attempt_belongs_to_job(db, job_id, attempt_id).await?;
    Ok(Json(log_page(db, job_id, attempt_id, params).await?))
}

#[utoipa::path(get, path="/api/v1/jobs/{job_id}/logs", tag="jobs", params(("job_id"=Uuid, Path)), responses((status=200, body=[JobLog])))]
async fn list_logs(
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
async fn list_logs_page(
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
#[utoipa::path(post, path="/api/v1/jobs/{job_id}/logs", tag="jobs", request_body=AppendLog, params(("job_id"=Uuid, Path)), responses((status=200, body=JobLog)))]
async fn append_log(
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
    let after = params.after_sequence()?;
    let search_pattern = params.search_pattern()?;
    let mut items = if let Some(pattern) = search_pattern {
        sqlx::query_as::<_, JobLog>(
            "SELECT id, job_id, attempt_id, sequence, message, created_at \
             FROM job_logs \
             WHERE job_id = $1 AND attempt_id = $2 AND sequence > $3 \
               AND message ILIKE $4 ESCAPE '\\' \
             ORDER BY sequence LIMIT $5",
        )
        .bind(job_id)
        .bind(attempt_id)
        .bind(after)
        .bind(pattern)
        .bind(fetch_limit)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)?
    } else {
        sqlx::query_as::<_, JobLog>(
            "SELECT id, job_id, attempt_id, sequence, message, created_at \
             FROM job_logs \
             WHERE job_id = $1 AND attempt_id = $2 AND sequence > $3 \
             ORDER BY sequence LIMIT $4",
        )
        .bind(job_id)
        .bind(attempt_id)
        .bind(after)
        .bind(fetch_limit)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)?
    };
    let next_after = if items.len() as i64 > limit {
        items.pop();
        items.last().map(|log| log.sequence)
    } else {
        None
    };
    Ok(JobLogPage { items, next_after })
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
