use std::sync::Arc;
use std::time::Duration;

use axum::{Json, Router, http::StatusCode, response::IntoResponse};
use sqlx::PgPool;
use uuid::Uuid;

tokio::task_local! {
    static REQUEST_ID: uuid::Uuid;
}

pub(crate) mod auth_routes;
pub(crate) mod authz_mw;
pub(crate) mod dto;
pub(crate) mod jobs_routes;
pub(crate) mod pipelines_routes;
pub(crate) mod projects_routes;
pub(crate) mod readiness;
pub(crate) mod router;
#[cfg(test)]
use authz_mw::{
    ProjectScopeRef, RateLimitRule, api_token_scope_allows, project_scope_ref, rate_limit_client,
    rate_limit_rule,
};
pub(crate) use authz_mw::{
    bearer_token_has_scope, identity_for_bearer_token, list_projects_for_claims,
    project_membership_role, rate_limit_mw, request_id_mw,
};
#[cfg(test)]
use axum::http::{HeaderMap, HeaderValue, Method, header};
use dto::{Job, Pipeline, PipelineDetail, PipelinePlan, Stage, StageDetail};
pub(crate) use jobs_routes::refresh_statuses;
pub(crate) use pipelines_routes::create_pipeline_with_vars_idempotent;
pub(crate) use readiness::PageParams;
use router::build_router_from_env;
pub use router::{app_with_auth_secret, app_with_git_and_auth_secret, app_with_git_and_config};
#[cfg(test)]
use router::{build_router_with_cors, cors_layer_from_allowed_origins};

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
        crate::api::readiness::health, crate::api::readiness::readiness, metrics, serve_openapi_json,
        crate::api::auth_routes::auth_login, crate::api::auth_routes::auth_refresh,
        crate::api::auth_routes::auth_logout,
        crate::api::projects_routes::list_projects, crate::api::projects_routes::create_project,
        crate::api::projects_routes::get_project, crate::api::projects_routes::update_project,
        crate::api::projects_routes::delete_project,
        crate::api::projects_routes::list_project_memberships,
        crate::api::projects_routes::upsert_project_membership,
        crate::api::projects_routes::delete_project_membership,
        crate::api::pipelines_routes::trigger_pipeline, crate::api::pipelines_routes::list_pipelines,
        crate::api::pipelines_routes::get_pipeline,
        crate::api::jobs_routes::cancel_pipeline, crate::api::jobs_routes::retry_pipeline,
        crate::api::jobs_routes::change_job_status, crate::api::jobs_routes::retry_job,
        crate::api::jobs_routes::start_manual_job, crate::api::jobs_routes::list_job_attempts,
        crate::api::jobs_routes::list_attempt_logs,
        crate::api::jobs_routes::list_attempt_logs_page, crate::api::jobs_routes::job_log_stream,
        crate::api::jobs_routes::list_logs, crate::api::jobs_routes::list_logs_page,
        crate::api::jobs_routes::append_log,
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
        crate::api::readiness::Readiness,
        crate::api::readiness::MigrationReadiness,
        dto::TriggerPipeline, dto::Pipeline, dto::Stage, dto::Job,
        dto::PipelineDetail, dto::PipelinePlan, dto::StageDetail, jobs_routes::JobAttempt, jobs_routes::JobLog, jobs_routes::JobLogPage, jobs_routes::ChangeStatus, jobs_routes::AppendLog,
        jobs_routes::CanceledPipelineResult, jobs_routes::RetriedPipelineResult, jobs_routes::ManualJobStartResult,
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
