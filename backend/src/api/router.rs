//! Router assembly: routes, layers, CORS (ADR-0012).

use super::readiness::{health, readiness};
use axum::extract::DefaultBodyLimit;

use super::auth_routes::AUTH_CSRF_HEADER;
use super::auth_routes::{auth_login, auth_logout, auth_refresh};
use super::authz_mw::require_auth;
use super::jobs_routes::{
    append_log, cancel_pipeline, change_job_status, job_log_stream, list_attempt_logs,
    list_attempt_logs_page, list_job_attempts, list_logs, list_logs_page, retry_job,
    retry_pipeline, start_manual_job,
};
use super::pipelines_routes::{get_pipeline, list_pipelines, trigger_pipeline};
use super::projects_routes::{
    create_project, delete_project, delete_project_membership, get_project,
    list_project_memberships, list_projects, update_project, upsert_project_membership,
};
use super::{
    AppState, IDEMPOTENCY_KEY_HEADER, metrics, rate_limit_mw, request_id_mw, serve_openapi_json,
};
use crate::git_host::{
    create_repository, delete_repository, git_info_refs, git_service_endpoint, internal_git_push,
    list_repositories,
};
use crate::pulls::{
    compare_refs, create_pull_request, list_commits, list_pull_requests, list_refs, pr_action,
};
use axum::Router;
use axum::http::{HeaderName, HeaderValue, Method, Uri, header};
use axum::routing::{get, post};
use sqlx::PgPool;
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;

/// Build a router from CICD_* environment (host mode entrypoint).
pub(crate) fn build_router_from_env(
    pool: Option<PgPool>,
    git: crate::git_host::GitConfig,
    running: Option<crate::runner::RunningJobs>,
) -> Router {
    wire_default_notify_hook();
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
    wire_default_notify_hook();
    let cors = cors_layer_from_allowed_origins(config.http.cors_allowed_origins.as_deref())?;
    let git = config.git.to_git_config();
    Ok(build_router_with_cors(pool, git, running, config, cors))
}

/// Wire the in-process dispatch signal to the store enqueue hook exactly once.
fn wire_default_notify_hook() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        cicd_infra::set_notify_hook(crate::dispatch_signal::notify_runner_work_available);
    });
}

pub(crate) fn build_router_with_cors(
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

pub(crate) fn cors_layer_from_allowed_origins(raw: Option<&str>) -> Result<CorsLayer, String> {
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
