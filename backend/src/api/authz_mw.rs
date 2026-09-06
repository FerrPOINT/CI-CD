//! AuthN/AuthZ middleware + project/repository scope checks (ADR-0012).

use super::dto::Project;
use super::{ApiError, AppState, REQUEST_ID, pool};
use crate::platform::audit;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::Method;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

/// AUTHZ_CONTRACT current mode: when CICD_AUTH_SECRET is configured, every
/// /api/v1 route except the public allowlist requires a valid Bearer JWT/PAT
/// and project-scoped resources require `project_memberships`. Without the
/// secret the API stays in trusted-network mode (open), matching CURRENT_STATE.
pub(crate) async fn request_id_mw(
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
pub(crate) struct RateLimitRule {
    pub(crate) class: &'static str,
    pub(crate) limit: u32,
    pub(crate) window_secs: u64,
}

pub(crate) async fn rate_limit_mw(
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

pub(crate) fn rate_limit_rule(method: &Method, path: &str) -> Option<RateLimitRule> {
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

pub(crate) fn rate_limit_client(headers: &HeaderMap) -> String {
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

pub(crate) async fn require_auth(
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
pub(crate) enum ProjectScopeRef {
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

pub(crate) fn api_token_scope_allows(claims: &crate::auth::AccessClaims, method: &str) -> bool {
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

pub(crate) fn project_scope_ref(path: &str) -> Option<ProjectScopeRef> {
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
            "SELECT id, name, repository_url, default_branch, max_running_jobs, created_at \
                 FROM projects WHERE id = $3 ORDER BY created_at DESC LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .bind(project_id)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal),
        (crate::authz::Role::Admin, None) => sqlx::query_as::<_, Project>(
            "SELECT id, name, repository_url, default_branch, max_running_jobs, created_at \
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
