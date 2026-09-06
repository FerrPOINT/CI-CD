use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, header},
    response::IntoResponse,
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    trace::TraceLayer,
};
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

pub(crate) mod auth_routes;
pub(crate) mod dto;
pub(crate) mod pipelines_routes;
pub(crate) mod projects_routes;
use auth_routes::{auth_login, auth_logout, auth_refresh};
use dto::{Job, Pipeline, PipelineDetail, PipelinePlan, Project, Stage, StageDetail};
pub(crate) use pipelines_routes::create_pipeline_with_vars_idempotent;
use pipelines_routes::{get_pipeline, list_pipelines, trigger_pipeline};
use projects_routes::{
    create_project, delete_project, delete_project_membership, get_project,
    list_project_memberships, list_projects, update_project, upsert_project_membership,
};

pub struct AppState {
    pub pool: Option<PgPool>,
    pub auth_secret: Option<String>,
    pub git: crate::git_host::GitConfig,
    pub config: crate::config::RuntimeConfig,
    pub running_jobs: Option<crate::runner::RunningJobs>,
    pub rate_limiter: Arc<crate::rate_limit::RateLimiter>,
}

type ApiResult<T> = Result<Json<T>, ApiError>;
const IDEMPOTENCY_KEY_HEADER: &str = "idempotency-key";
const READINESS_TIMEOUT: Duration = Duration::from_secs(2);
const LEGACY_PIPELINE_PLAN_PARSER_VERSION: &str = "forge-legacy-linear/1";
const V1_PIPELINE_PLAN_PARSER_VERSION: &str = "forge-dsl/1.0.0";
pub(super) const AUTH_REFRESH_COOKIE: &str = "forge_refresh";
pub(super) const AUTH_CSRF_COOKIE: &str = "forge_csrf";
pub(super) const AUTH_CSRF_HEADER: &str = "x-csrf-token";
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
        health, readiness, metrics, serve_openapi_json,
        crate::api::auth_routes::auth_login, crate::api::auth_routes::auth_refresh,
        crate::api::auth_routes::auth_logout,
        crate::api::projects_routes::list_projects, crate::api::projects_routes::create_project,
        crate::api::projects_routes::get_project, crate::api::projects_routes::update_project,
        crate::api::projects_routes::delete_project,
        crate::api::projects_routes::list_project_memberships,
        crate::api::projects_routes::upsert_project_membership,
        crate::api::projects_routes::delete_project_membership,
        crate::api::pipelines_routes::trigger_pipeline, crate::api::pipelines_routes::list_pipelines,
        crate::api::pipelines_routes::get_pipeline, cancel_pipeline, retry_pipeline,
        change_job_status, retry_job, start_manual_job, list_job_attempts, list_attempt_logs,
        list_attempt_logs_page, job_log_stream, list_logs, list_logs_page, append_log,
        crate::platform::list_runners, crate::platform::register_runner,
        crate::platform::runner_heartbeat, crate::platform::delete_runner,
        crate::runner_protocol::register_runner_protocol,
        crate::runner_protocol::runner_protocol_heartbeat,
        crate::runner_protocol::poll_runner_work, crate::runner_protocol::ack_runner_lease,
        crate::runner_protocol::renew_runner_lease, crate::runner_protocol::poll_runner_lease_control,
        crate::runner_protocol::resolve_runner_lease_secrets,
        crate::runner_protocol::upload_runner_lease_artifact,
        crate::runner_protocol::append_runner_lease_logs,
        crate::runner_protocol::complete_runner_lease,
        crate::platform::list_secrets, crate::platform::create_secret, crate::platform::delete_secret,
        crate::platform::list_artifacts, crate::platform::upload_artifact, crate::platform::download_artifact,
        crate::platform::list_environments, crate::platform::create_environment,
        crate::platform::update_environment, crate::platform::delete_environment,
        crate::platform::list_deployments, crate::platform::create_deployment,
        crate::platform::list_deployment_approvals,
        crate::platform::record_deployment_approval,
        crate::platform::rollback_deployment,
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
        crate::api::pipelines_routes::list_releases, crate::api::pipelines_routes::create_release,
        crate::api::pipelines_routes::get_release, crate::api::pipelines_routes::delete_release,
        crate::api::pipelines_routes::pipeline_badge, crate::api::pipelines_routes::pipeline_variables,
        crate::api::pipelines_routes::get_test_report, crate::api::pipelines_routes::upload_test_report,
    ),
    components(schemas(
        crate::auth::LoginRequest, crate::auth::LogoutRequest, crate::auth::LogoutResponse, crate::auth::RefreshRequest, crate::auth::TokenPair,
        dto::Project, dto::CreateProject, projects_routes::UpdateProject, dto::ProjectMembership, dto::ProjectMembershipInput,
        Readiness, MigrationReadiness,
        dto::TriggerPipeline, dto::Pipeline, dto::Stage, dto::Job,
        dto::PipelineDetail, dto::PipelinePlan, dto::StageDetail, JobAttempt, JobLog, JobLogPage, ChangeStatus, AppendLog,
        CanceledPipelineResult, RetriedPipelineResult, ManualJobStartResult,
        pipelines_routes::Release, pipelines_routes::CreateRelease, pipelines_routes::TestReport,
        crate::runner_protocol::RunnerRegisterRequest, crate::runner_protocol::RunnerRegisterResponse,
        crate::runner_protocol::RunnerHeartbeatRequest, crate::runner_protocol::RunnerCapacity,
        crate::runner_protocol::RunnerPollRequest, crate::runner_protocol::RunnerPollCapacity,
        crate::runner_protocol::RunnerLeaseOffer, crate::runner_protocol::RunnerAttemptSpec,
        crate::runner_protocol::RunnerWorkspace, crate::runner_protocol::RunnerLeaseControlRequest,
        crate::runner_protocol::RunnerLeaseControlResponse, crate::runner_protocol::RunnerCompleteRequest,
        crate::runner_protocol::RunnerSecretResolveRequest, crate::runner_protocol::RunnerSecretResolveResponse,
        crate::runner_protocol::RunnerSecretItem,
        crate::runner_protocol::RunnerCompleteResponse, crate::runner_protocol::RunnerLogAppendRequest,
        crate::runner_protocol::RunnerLogLine, crate::runner_protocol::RunnerLogAppendResponse,
        crate::platform::Runner, crate::platform::RegisterRunner, crate::platform::RunnerHeartbeat,
        crate::platform::SecretMetadata, crate::platform::CreateSecret,
        crate::platform::Artifact,
        crate::platform::Environment, crate::platform::CreateEnvironment, crate::platform::UpdateEnvironment,
        crate::platform::Deployment, crate::platform::CreateDeployment,
        crate::platform::DeploymentApproval, crate::platform::RecordDeploymentApproval,
        crate::platform::RollbackDeployment,
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
    pub(crate) fn payload_too_large(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::PAYLOAD_TOO_LARGE,
            message: message.into(),
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
            StatusCode::PAYLOAD_TOO_LARGE => "payload_too_large",
            StatusCode::SERVICE_UNAVAILABLE => "unavailable",
            _ => "internal_error",
        }
    }
    pub(crate) fn internal(error: sqlx::Error) -> Self {
        tracing::error!(%error, "internal API error");
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "internal server error".into(),
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

pub(super) fn auth_secret(state: &AppState) -> Result<&str, ApiError> {
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
    build_router_from_env(pool, git, running)
}

#[allow(dead_code)]
fn app_with(pool: Option<PgPool>, running: Option<crate::runner::RunningJobs>) -> Router {
    build_router_from_env(pool, crate::git_host::GitConfig::default(), running)
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
    let path = req.uri().path();
    let method = req.method().as_str().to_string();
    match crate::authz::route_access(&method, path) {
        Some(
            crate::authz::RouteAccess::Public
            | crate::authz::RouteAccess::Runner
            | crate::authz::RouteAccess::System
            | crate::authz::RouteAccess::Git { .. },
        ) => return Ok(next.run(req).await),
        Some(crate::authz::RouteAccess::User { .. }) => {}
        None if path.starts_with("/api/v1/") || path.starts_with("/git/") || path == "/metrics" => {
            return Err(ApiError::not_found());
        }
        None => return Ok(next.run(req).await),
    }
    let Some(auth_secret) = state.auth_secret.as_deref() else {
        return Ok(next.run(req).await); // trusted-network mode: no enforcement
    };
    let pool = pool(&state)?;
    let claims = bearer_identity(pool, auth_secret, req.headers())
        .await
        .map_err(|_| ApiError::unauthorized())?;
    let role = crate::authz::Role::parse(&claims.role).ok_or_else(ApiError::unauthorized)?;
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
    Deployment(Uuid),
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
                "deployments" => Some(ProjectScopeRef::Deployment(id)),
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

pub(crate) async fn list_projects_for_claims(
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
        ProjectScopeRef::Deployment(id) => sqlx::query_scalar(
            "SELECT e.project_id FROM deployments d \
             JOIN environments e ON e.id = d.environment_id \
             WHERE d.id = $1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal),
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
    // Central fleet auth-server first (ES256 via JWKS); legacy session JWTs
    // and cicd_ PATs remain valid during the migration window.
    if let Some(central) = crate::central_auth::try_central(token).await {
        return crate::central_auth::link_central_user(pool, &central).await;
    }
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
            ver: 0,
            iat: now.timestamp(),
            exp: now.timestamp() + 900,
        })
    } else {
        let mut claims = crate::auth::verify_access_with_secret(token, auth_secret)
            .map_err(|_| ApiError::unauthorized())?;
        let session_id = claims.sid.ok_or_else(ApiError::unauthorized)?;
        let current = crate::auth::access_session_user_with_version(
            pool,
            session_id,
            claims.sub,
            Some(claims.ver),
        )
        .await
        .map_err(|_| ApiError::unauthorized())?;
        claims.role = current.role;
        claims.ver = current.token_version;
        Ok(claims)
    }
}

fn build_router_from_env(
    pool: Option<PgPool>,
    git: crate::git_host::GitConfig,
    running: Option<crate::runner::RunningJobs>,
) -> Router {
    let config = runtime_config_from_env_for_router().with_git_config(git);
    build_router_with_config(pool, running, config).expect("invalid CICD_ HTTP configuration")
}

/// Build a router with an explicit auth secret for tests and integration harnesses.
pub fn app_with_auth_secret(pool: Option<PgPool>, auth_secret: Option<String>) -> Router {
    build_router_with_auth_secret(
        pool,
        crate::git_host::GitConfig::default(),
        None,
        auth_secret,
    )
}

/// Build a Git-aware router with an explicit auth secret for tests and integration harnesses.
pub fn app_with_git_and_auth_secret(
    pool: Option<PgPool>,
    git: crate::git_host::GitConfig,
    auth_secret: Option<String>,
) -> Router {
    build_router_with_auth_secret(pool, git, None, auth_secret)
}

/// Build a Git-aware router from already validated typed runtime config.
pub fn app_with_git_and_config(
    pool: Option<PgPool>,
    git: crate::git_host::GitConfig,
    running: Option<crate::runner::RunningJobs>,
    config: crate::config::RuntimeConfig,
) -> Result<Router, String> {
    build_router_with_config(pool, running, config.with_git_config(git))
}

fn build_router_with_auth_secret(
    pool: Option<PgPool>,
    git: crate::git_host::GitConfig,
    running: Option<crate::runner::RunningJobs>,
    auth_secret: Option<String>,
) -> Router {
    let config = runtime_config_from_env_for_router()
        .with_git_config(git)
        .with_auth_secret(auth_secret);
    build_router_with_config(pool, running, config).expect("invalid CICD_ HTTP configuration")
}

fn runtime_config_from_env_for_router() -> crate::config::RuntimeConfig {
    crate::config::RuntimeConfig::from_env_for_app().expect("invalid CICD_ runtime configuration")
}

fn build_router_with_config(
    pool: Option<PgPool>,
    running: Option<crate::runner::RunningJobs>,
    config: crate::config::RuntimeConfig,
) -> Result<Router, String> {
    let cors = cors_layer_from_allowed_origins(config.http.cors_allowed_origins.as_deref())?;
    let git = config.git.to_git_config();
    Ok(build_router_with_cors(pool, git, running, config, cors))
}

fn build_router_with_cors(
    pool: Option<PgPool>,
    git: crate::git_host::GitConfig,
    running: Option<crate::runner::RunningJobs>,
    config: crate::config::RuntimeConfig,
    cors: CorsLayer,
) -> Router {
    let state = Arc::new(AppState {
        pool: pool.clone(),
        auth_secret: config.auth.secret.clone(),
        git,
        config,
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
            get(list_logs).post(append_log).layer(DefaultBodyLimit::max(
                crate::body_limits::JOB_LOG_APPEND_BYTES,
            )),
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
        .route(
            "/git/{repo}/git-upload-pack",
            post(git_service_endpoint).layer(DefaultBodyLimit::max(
                crate::body_limits::GIT_SMART_HTTP_RPC_BYTES,
            )),
        )
        .route(
            "/git/{repo}/git-receive-pack",
            post(git_service_endpoint).layer(DefaultBodyLimit::max(
                crate::body_limits::GIT_SMART_HTTP_RPC_BYTES,
            )),
        )
        .route("/api/v1/internal/git-push", post(internal_git_push))
        .route("/api/v1/repos/{repo}/refs", get(list_refs))
        .route("/api/v1/repos/{repo}/tree", get(crate::pulls::list_tree))
        .route("/api/v1/repos/{repo}/blob", get(crate::pulls::get_blob))
        .route("/api/v1/repos/{repo}/tags", get(crate::pulls::list_tags))
        .route(
            "/api/v1/repos/{repo}/releases",
            get(crate::api::pipelines_routes::list_releases)
                .post(crate::api::pipelines_routes::create_release),
        )
        .route(
            "/api/v1/repos/{repo}/releases/{tag}",
            get(crate::api::pipelines_routes::get_release)
                .delete(crate::api::pipelines_routes::delete_release),
        )
        .route(
            "/api/v1/pipelines/{pipeline_id}/badge.svg",
            get(crate::api::pipelines_routes::pipeline_badge),
        )
        .route(
            "/api/v1/jobs/{job_id}/test-report",
            get(crate::api::pipelines_routes::get_test_report)
                .post(crate::api::pipelines_routes::upload_test_report)
                .layer(DefaultBodyLimit::max(
                    crate::body_limits::TEST_REPORT_UPLOAD_BYTES,
                )),
        )
        .route(
            "/api/v1/pipelines/{pipeline_id}/variables",
            get(crate::api::pipelines_routes::pipeline_variables),
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
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

fn cors_layer_from_allowed_origins(raw: Option<&str>) -> Result<CorsLayer, String> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(CorsLayer::permissive());
    };

    let mut origins = Vec::new();
    for value in raw
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if value == "*" {
            return Err(
                "wildcard is not allowed; leave CICD_CORS_ALLOWED_ORIGINS empty only for isolated development"
                    .to_string(),
            );
        }
        let uri = value
            .parse::<Uri>()
            .map_err(|_| format!("origin must be a valid URI: {value}"))?;
        if !matches!(uri.scheme_str(), Some("http" | "https")) {
            return Err(format!(
                "origin must start with http:// or https://: {value}"
            ));
        }
        let authority = uri
            .authority()
            .ok_or_else(|| format!("origin must include a host: {value}"))?;
        let rest = value
            .strip_prefix("https://")
            .or_else(|| value.strip_prefix("http://"))
            .ok_or_else(|| format!("origin must start with http:// or https://: {value}"))?;
        if rest != authority.as_str() || authority.as_str().contains('@') {
            return Err(format!(
                "origin must include only scheme, host and optional port: {value}"
            ));
        }
        origins.push(
            value
                .parse::<HeaderValue>()
                .map_err(|_| format!("origin is not a valid header value: {value}"))?,
        );
    }
    if origins.is_empty() {
        return Err("CICD_CORS_ALLOWED_ORIGINS did not contain any origins".to_string());
    }

    Ok(CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::ACCEPT,
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            HeaderName::from_static(IDEMPOTENCY_KEY_HEADER),
            HeaderName::from_static("x-internal-token"),
            HeaderName::from_static("x-runner-protocol-version"),
            HeaderName::from_static("x-lease-token"),
            HeaderName::from_static("x-fencing-token"),
            HeaderName::from_static("x-attempt-id"),
            HeaderName::from_static("x-artifact-path"),
            HeaderName::from_static("x-artifact-name"),
            HeaderName::from_static(AUTH_CSRF_HEADER),
        ])
        .allow_credentials(true))
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
    let expected = match expected_migrations().await {
        Ok(expected) => expected,
        Err(_) => {
            return readiness_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "not_ready",
                "unknown",
                migration_readiness_error(&[], "unknown", "migration source failed"),
            );
        }
    };
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

async fn expected_migrations() -> Result<Vec<ExpectedMigration>, sqlx::migrate::MigrateError> {
    Ok(crate::migrations()
        .await?
        .iter()
        .filter(|migration| migration.migration_type.is_up_migration())
        .map(|migration| ExpectedMigration {
            version: migration.version,
            checksum: migration.checksum.to_vec(),
        })
        .collect())
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

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema, serde::Deserialize, utoipa::IntoParams)]
pub(crate) struct PageParams {
    /// Max items (1..=200, default 50).
    #[serde(default)]
    pub(crate) limit: Option<i64>,
    /// Offset (default 0).
    #[serde(default)]
    pub(crate) offset: Option<i64>,
}

impl PageParams {
    fn bounded(&self) -> (i64, i64) {
        let limit = self.limit.unwrap_or(50).clamp(1, 200);
        let offset = self.offset.unwrap_or(0).max(0);
        (limit, offset)
    }
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use tower::ServiceExt;

    use super::*;

    #[test]
    fn internal_errors_hide_database_details_from_clients() {
        let error = ApiError::internal(sqlx::Error::Protocol(
            "duplicate key value violates unique constraint \"users_username_key\"".to_string(),
        ));

        assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(error.message, "internal server error");
        assert_eq!(error.code(), "internal_error");
    }

    #[test]
    fn project_scope_ref_maps_project_owned_routes() {
        let project_id = Uuid::new_v4();
        let pipeline_id = Uuid::new_v4();
        let job_id = Uuid::new_v4();
        let artifact_id = Uuid::new_v4();
        let secret_id = Uuid::new_v4();
        let environment_id = Uuid::new_v4();
        let deployment_id = Uuid::new_v4();
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
            project_scope_ref(&format!("/api/v1/deployments/{deployment_id}/approvals")),
            Some(ProjectScopeRef::Deployment(deployment_id))
        );
        assert_eq!(
            project_scope_ref(&format!("/api/v1/deployments/{deployment_id}/rollback")),
            Some(ProjectScopeRef::Deployment(deployment_id))
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
            ver: 0,
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

    #[test]
    fn cors_allowlist_rejects_wildcard_and_invalid_origins() {
        assert!(cors_layer_from_allowed_origins(None).is_ok());
        assert!(cors_layer_from_allowed_origins(Some(" ")).is_ok());
        assert!(cors_layer_from_allowed_origins(Some("*")).is_err());
        assert!(cors_layer_from_allowed_origins(Some("cicd.example.com")).is_err());
        assert!(cors_layer_from_allowed_origins(Some("https://cicd.example.com/app")).is_err());
        assert!(
            cors_layer_from_allowed_origins(Some(
                "https://cicd.example.com, http://localhost:22802",
            ))
            .is_ok()
        );
    }

    #[tokio::test]
    async fn cors_allowlist_marks_only_configured_origins() {
        let config = crate::config::RuntimeConfig::test_default();
        let app = build_router_with_cors(
            None,
            crate::git_host::GitConfig::default(),
            None,
            config,
            cors_layer_from_allowed_origins(Some("https://cicd.example.com")).unwrap(),
        );

        let response = app
            .clone()
            .oneshot(cors_preflight("https://cicd.example.com"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "https://cicd.example.com"
        );
        assert_eq!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS)
                .unwrap(),
            "true"
        );

        let response = app
            .oneshot(cors_preflight("https://evil.example.com"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .is_none()
        );
    }

    #[tokio::test]
    async fn cors_allowlist_preflight_bypasses_auth_on_protected_routes() {
        let config = crate::config::RuntimeConfig::test_default()
            .with_auth_secret(Some("test-auth-secret".to_string()));
        let app = build_router_with_cors(
            None,
            crate::git_host::GitConfig::default(),
            None,
            config,
            cors_layer_from_allowed_origins(Some("https://cicd.example.com")).unwrap(),
        );

        let response = app
            .oneshot(cors_preflight_for(
                "/api/v1/projects",
                "https://cicd.example.com",
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "https://cicd.example.com"
        );
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

    fn cors_preflight(origin: &'static str) -> axum::http::Request<Body> {
        cors_preflight_for("/api/v1/health", origin)
    }

    fn cors_preflight_for(path: &'static str, origin: &'static str) -> axum::http::Request<Body> {
        axum::http::Request::options(path)
            .header(header::ORIGIN, origin)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
            .body(Body::empty())
            .unwrap()
    }
}

pub(crate) async fn pipeline_detail(
    pool: &PgPool,
    pipeline_id: Uuid,
) -> Result<PipelineDetail, ApiError> {
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
        let jobs = sqlx::query_as::<_, Job>("SELECT id, stage_id, name, image, command, required_tags, required_secrets, artifact_paths, position, status, started_at, finished_at FROM jobs WHERE stage_id = $1 ORDER BY position").bind(stage.id).fetch_all(pool).await.map_err(ApiError::internal)?;
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
async fn retry_job(State(state): State<Arc<AppState>>, Path(job_id): Path<Uuid>) -> ApiResult<Job> {
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
#[utoipa::path(post, path="/api/v1/jobs/{job_id}/logs", tag="jobs", request_body=AppendLog, params(("job_id"=Uuid, Path)), responses((status=200, body=JobLog), (status=413, description="log append body exceeds 1 MiB")))]
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
