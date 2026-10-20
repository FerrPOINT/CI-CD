//! AuthN/AuthZ middleware + project/repository scope checks (ADR-0012).

use super::dto::Project;
use super::{ApiError, AppState, REQUEST_ID, pool};
use crate::platform::audit;
use axum::extract::ConnectInfo;
use axum::extract::State;
use axum::http::Extensions;
use axum::http::Method;
use sqlx::PgPool;
use std::net::SocketAddr;
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
    let id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| uuid::Uuid::parse_str(v).ok())
        .unwrap_or_else(uuid::Uuid::new_v4);
    REQUEST_ID
        .scope(
            id,
            pdlc_telemetry::run_with_request_id(req, next, id.to_string()),
        )
        .await
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
        let client = rate_limit_client(req.extensions());
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
        "/health" | "/api/v1/health" | "/api/v1/readiness" | "/api/v1/openapi.json" | "/metrics"
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
    if matches!(
        path,
        "/api/v1/internal/git-push" | "/api/v1/internal/task-git-check"
    ) {
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

pub(crate) fn rate_limit_client(extensions: &Extensions) -> String {
    extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|peer| peer.0.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string())
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
            | crate::authz::RouteAccess::NamespaceOwner
            | crate::authz::RouteAccess::NamespaceReader
            | crate::authz::RouteAccess::Git { .. },
        ) => return Ok(next.run(req).await),
        Some(crate::authz::RouteAccess::User { .. }) => {}
        None if path.starts_with("/api/v1/") || path.starts_with("/git/") || path == "/metrics" => {
            return Err(ApiError::not_found());
        }
        None => return Ok(next.run(req).await),
    }
    let Some(auth_secret) = state.auth_secret.as_deref() else {
        return Err(ApiError::service_unavailable(
            "CI/CD authentication is not configured",
        ));
    };
    let pool = pool(&state)?;
    let bearer = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(pdlc_auth_core::bearer_from_header);
    let mut checked_central = None;
    if let Some(token) = bearer.filter(|token| pdlc_auth_core::is_personal_token(token))
        && let Some(verified) = crate::central_auth::try_verified_central(token).await?
    {
        let central = &verified.context;
        if crate::delegated_credentials::has_grant(&central)
            || state
                .config
                .sdlc_workspace
                .delegated_subject
                .is_some_and(|id| id.to_string() == central.user_id)
        {
            let identity = crate::delegated_credentials::Identity::verified(
                &central,
                &state.config.sdlc_workspace,
            )?;
            identity.route(&method, req.uri().path())?;
            let mut req = req;
            req.extensions_mut().insert(identity);
            return Ok(next.run(req).await);
        }
        checked_central = Some(VerifiedRequestIdentity {
            claims: central_identity(pool, &verified).await?,
            central: Some(verified),
        });
    }
    let is_platform_token = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            pdlc_auth_core::bearer_from_header(value).is_some_and(pdlc_auth_core::is_personal_token)
        });
    let identity = match checked_central {
        Some(identity) => identity,
        None => verified_bearer_identity(pool, auth_secret, req.headers()).await?,
    };
    let claims = identity.claims.clone();
    if is_platform_token {
        let required = if matches!(method.as_str(), "GET" | "HEAD" | "OPTIONS") {
            "ci-cd:read"
        } else {
            "ci-cd:write"
        };
        if !claims.token_scopes.iter().any(|scope| scope == required) {
            return Err(ApiError::forbidden());
        }
    }
    // Role::parse maps machine principals ("service_account") to the
    // developer class; token scopes still constrain every route.
    let role = crate::authz::Role::parse(&claims.role).ok_or_else(ApiError::unauthorized)?;
    let path = req.uri().path().to_string();
    if claims.role == "service_account"
        && (path.starts_with("/api/v1/git-groups/")
            || (path.starts_with("/api/v1/catalog/repositories")
                && !repository_sync_route(&method, &path))
            || path.starts_with("/api/v1/namespace-contexts")
            || path.starts_with("/api/v1/workspace-")
            || path.starts_with("/api/v1/delivery-configurations"))
    {
        return Err(ApiError::forbidden());
    }
    let (mut parts, body) = req.into_parts();
    let read_storage = crate::space_admission::repository_request_storage(pool, &mut parts).await?;
    let write_route = parts
        .extensions
        .get::<axum::extract::MatchedPath>()
        .is_some_and(|route| {
            crate::space_admission::repository_write_route(&parts.method, route.as_str())
        });
    let mut write_permit = None;
    let read_proof = match read_storage.as_ref() {
        Some(storage) if crate::space_admission::active(pool).await? => {
            identity.require_scope(if write_route {
                "ci-cd:write"
            } else {
                "ci-cd:read"
            })?;
            let actor = identity.central.as_ref().ok_or_else(ApiError::forbidden)?;
            let proof =
                crate::space_admission::repository(pool, storage, actor, write_route).await?;
            if write_route {
                let (_, min_role) = crate::authz::required_role(&method, &path);
                write_permit = Some(crate::space_admission::RepositoryWrite::confirmed(
                    proof,
                    storage.clone(),
                    claims.sub,
                    min_role,
                ));
                None
            } else {
                Some(proof)
            }
        }
        _ => None,
    };
    let scope_ref = read_storage
        .as_ref()
        .map(|storage| ProjectScopeRef::Repository(storage.clone()))
        .or_else(|| project_scope_ref(&path));
    let scope_allowed = if read_proof.is_some() {
        // Confirmed common visibility replaces legacy read membership, not
        // credential assignment. An explicitly project-bound token stays bound.
        match claims.token_project_id {
            Some(project) => {
                repository_linked_to_project(
                    pool,
                    read_storage.as_deref().ok_or_else(ApiError::forbidden)?,
                    project,
                )
                .await?
            }
            None => true,
        }
    } else {
        project_scope_allows(&state, &claims, role, &method, &path, scope_ref).await?
    };
    let allowed = api_token_scope_allows(&claims, &method)
        && crate::authz::allows(role, &method, &path)
        && scope_allowed;
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
    if let Some(permit) = write_permit.as_ref() {
        parts.extensions.insert(permit.clone());
    }
    parts.extensions.insert(claims);
    parts.extensions.insert(identity);
    let req = axum::extract::Request::from_parts(parts, body);
    let response = next.run(req).await;
    match (read_proof, write_permit) {
        (Some(proof), _) => crate::space_admission::read_response(proof, response),
        (_, Some(permit)) => permit.response(response),
        _ => Ok(response),
    }
}

// This exception reaches the existing project/token checks, never the wider catalog API.
fn repository_sync_route(method: &str, path: &str) -> bool {
    let Some(tail) = path.strip_prefix("/api/v1/catalog/repositories/") else {
        return false;
    };
    let parts: Vec<_> = tail.split('/').collect();
    let Some(repository) = parts.first() else {
        return false;
    };
    if Uuid::parse_str(repository).is_err() {
        return false;
    }
    match (method, &parts[1..]) {
        ("GET" | "PUT", ["sync"])
        | ("POST", ["sync", "check"])
        | ("GET" | "POST", ["sync", "runs"]) => true,
        ("GET", ["sync", "runs", run]) | ("GET", ["sync", "runs", run, "refs"]) => {
            Uuid::parse_str(run).is_ok()
        }
        _ => false,
    }
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
    CatalogRepository(Uuid),
}

async fn project_scope_allows(
    state: &AppState,
    claims: &crate::auth::AccessClaims,
    global_role: crate::authz::Role,
    method: &str,
    path: &str,
    scope_ref: Option<ProjectScopeRef>,
) -> Result<bool, ApiError> {
    let Some(scope_ref) = scope_ref else {
        if claims.token_id.is_some() && claims.token_project_id.is_some() {
            return Ok(method == "GET"
                && matches!(
                    path,
                    "/api/v1/projects" | "/api/v1/auth/principal" | "/api/v1/repository-syncs"
                ));
        }
        return Ok(true);
    };
    let Some(pool) = state.pool.as_ref() else {
        return Err(ApiError::unavailable());
    };
    let scope_ref = match scope_ref {
        ProjectScopeRef::CatalogRepository(id) => {
            ProjectScopeRef::Repository(crate::repository_catalog::storage_by_id(pool, id).await?)
        }
        other => other,
    };
    let scope_ref = match scope_ref {
        ProjectScopeRef::Repository(name) => {
            if claims.role == "service_account" {
                return match claims.token_project_id {
                    Some(project_id) => repository_linked_to_project(pool, &name, project_id).await,
                    None => Ok(false),
                };
            }
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
    if claims.role == "service_account" {
        return Ok(claims.token_project_id == Some(project_id));
    }
    if global_role == crate::authz::Role::Admin {
        return Ok(true);
    }
    let (_, min_role) = crate::authz::required_role(method, path);
    let storage: Option<String> = sqlx::query_scalar(
        "SELECT r.storage_name FROM projects p JOIN repository_catalog r ON r.id=p.repository_id WHERE p.id=$1",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?
    .flatten();
    if let Some(storage) = storage {
        if global_role >= min_role
            && crate::repository_catalog::shared_human_repository(
                pool,
                &storage,
                claims.token_project_id,
            )
            .await?
        {
            return Ok(true);
        }
    }
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
        "catalog" => {
            if segments.next()? != "repositories" {
                return None;
            }
            let id = segments.next()?.parse().ok()?;
            if segments.next()? != "sync" {
                return None;
            }
            Some(ProjectScopeRef::CatalogRepository(id))
        }
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
    let name = crate::repository_catalog::resolve_storage(pool, repo).await?;
    if global_role >= min_role
        && crate::repository_catalog::shared_human_repository(pool, &name, token_project_id).await?
    {
        return Ok(true);
    }
    if global_role == crate::authz::Role::Admin {
        return match token_project_id {
            Some(project_id) => repository_linked_to_project(pool, &name, project_id).await,
            None => Ok(true),
        };
    }
    let roles = sqlx::query_scalar::<_, String>(
        "SELECT m.role FROM projects p JOIN project_memberships m ON m.project_id=p.id JOIN repository_catalog r ON r.id=p.repository_id WHERE r.storage_name=$1 AND m.user_id=$2 AND ($3::uuid IS NULL OR p.id=$3)",
    )
    .bind(&name)
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

pub(crate) async fn repository_linked_to_project(
    pool: &PgPool,
    repo: &str,
    project_id: Uuid,
) -> Result<bool, ApiError> {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM projects p JOIN repository_catalog r ON r.id=p.repository_id WHERE p.id=$1 AND r.storage_name=$2)",
    )
    .bind(project_id)
    .bind(repo)
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
    if claims.role == "service_account" {
        let Some(project_id) = claims.token_project_id else {
            return Ok(Vec::new());
        };
        return sqlx::query_as::<_, Project>(
            "SELECT id, name, repository_url, default_branch, max_running_jobs, created_at \
             FROM projects WHERE id = $3 ORDER BY created_at DESC LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .bind(project_id)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal);
    }
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
            "SELECT p.id, p.name, p.repository_url, p.default_branch, p.max_running_jobs, p.created_at \
                 FROM projects p \
                 LEFT JOIN project_memberships m ON m.project_id = p.id AND m.user_id = $3 \
                 WHERE p.id = $4 AND (m.user_id = $3 OR EXISTS(SELECT 1 FROM repository_catalog r JOIN forge_namespace_bindings b ON b.resource_id=r.group_id WHERE r.id=p.repository_id)) \
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
            // Tenant visibility (AUTHORIZATION target, bounded step): active
            // tenant membership implies read access to that tenant's projects
            // alongside the user's direct project memberships.
            "SELECT DISTINCT p.id, p.name, p.repository_url, p.default_branch, p.max_running_jobs, p.created_at \
                 FROM projects p \
                 LEFT JOIN project_memberships m ON m.project_id = p.id AND m.user_id = $3 \
                 LEFT JOIN tenant_memberships tm ON tm.user_id = $3 AND tm.tenant_id = p.tenant_id \
                 LEFT JOIN tenants t ON t.id = p.tenant_id \
                 WHERE m.user_id = $3 \
                    OR (p.tenant_id IS NOT NULL AND tm.user_id = $3 AND t.status = 'active') \
                    OR EXISTS(SELECT 1 FROM repository_catalog r JOIN forge_namespace_bindings b ON b.resource_id=r.group_id WHERE r.id=p.repository_id) \
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
        ProjectScopeRef::Repository(_) | ProjectScopeRef::CatalogRepository(_) => Ok(None),
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
async fn verified_bearer_identity(
    pool: &PgPool,
    auth_secret: &str,
    headers: &axum::http::HeaderMap,
) -> Result<VerifiedRequestIdentity, ApiError> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(ApiError::unauthorized)?;
    let token = value
        .strip_prefix("Bearer ")
        .ok_or_else(ApiError::unauthorized)?;
    verified_identity_for_bearer_token(pool, auth_secret, token).await
}

async fn central_identity(
    pool: &PgPool,
    verified: &pdlc_auth_core::service_bridge::VerifiedCentralToken,
) -> Result<crate::auth::AccessClaims, ApiError> {
    let central = &verified.context;
    if crate::namespace::registered_machine(&central.user_id)
        || verified.subject_role.as_deref() == Some("service_account")
        || crate::delegated_credentials::has_grant(central)
        || crate::delegated_credentials::configured_subject(&central.user_id)
    {
        return Err(ApiError::forbidden());
    }
    crate::central_auth::link_central_user(pool, verified).await
}

/// Preserve the authenticated origin in memory; central PATs have no local token ID.
#[derive(Clone)]
pub(crate) struct VerifiedRequestIdentity {
    pub(crate) claims: crate::auth::AccessClaims,
    pub(crate) central: Option<pdlc_auth_core::service_bridge::VerifiedCentralToken>,
}
impl VerifiedRequestIdentity {
    pub(crate) fn require_scope(&self, required: &str) -> Result<(), ApiError> {
        if let Some(source) = self.central.as_ref() {
            if !source.lifetime.is_current() {
                return Err(ApiError::unauthorized());
            }
            if source.context.session_id.is_none() && !source.context.has_scope(required) {
                return Err(ApiError::forbidden());
            }
        } else if !bearer_token_has_scope(&self.claims, required) {
            return Err(ApiError::forbidden());
        }
        Ok(())
    }
}
pub(crate) async fn verified_identity_for_bearer_token(
    pool: &PgPool,
    auth_secret: &str,
    token: &str,
) -> Result<VerifiedRequestIdentity, ApiError> {
    if let Some(verified) = crate::central_auth::try_verified_central(token).await? {
        let claims = central_identity(pool, &verified).await?;
        return Ok(VerifiedRequestIdentity {
            claims,
            central: Some(verified),
        });
    }
    let claims = local_identity_for_bearer_token(pool, auth_secret, token).await?;
    Ok(VerifiedRequestIdentity {
        claims,
        central: None,
    })
}
pub(crate) async fn identity_for_bearer_token(
    pool: &PgPool,
    auth_secret: &str,
    token: &str,
) -> Result<crate::auth::AccessClaims, ApiError> {
    Ok(verified_identity_for_bearer_token(pool, auth_secret, token)
        .await?
        .claims)
}
async fn local_identity_for_bearer_token(
    pool: &PgPool,
    auth_secret: &str,
    token: &str,
) -> Result<crate::auth::AccessClaims, ApiError> {
    if token.starts_with("forge_sat_") {
        let hash = crate::auth::hash_token(token);
        let row = sqlx::query_as::<_, (Uuid, Uuid, String, Option<Uuid>, Vec<String>)>(
            "SELECT t.id, sa.id, sa.name, t.project_id, t.scopes \
             FROM api_tokens t JOIN service_accounts sa ON sa.id = t.service_account_id \
             WHERE t.token_hash = $1 AND t.principal_type = 'service_account' \
               AND sa.enabled AND t.revoked_at IS NULL \
               AND (t.expires_at IS NULL OR t.expires_at > now())",
        )
        .bind(&hash)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal)?;
        let (token_id, service_account_id, sa_name, token_project_id, token_scopes) =
            row.ok_or_else(ApiError::unauthorized)?;
        let _ = sqlx::query("UPDATE api_tokens SET last_used_at = now() WHERE id = $1")
            .bind(token_id)
            .execute(pool)
            .await;
        let now = chrono::Utc::now();
        Ok(crate::auth::AccessClaims {
            sub: service_account_id,
            sid: None,
            token_id: Some(token_id),
            token_project_id,
            token_scopes,
            role: "service_account".to_string(),
            ver: 0,
            iat: now.timestamp(),
            exp: now.timestamp() + 900,
            service_account_name: Some(sa_name),
        })
    } else if token.starts_with("cicd_") {
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
            service_account_name: None,
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
        if std::env::var_os("CICD_AUTH__CENTRAL_JWKS_URI").is_some() {
            return Err(ApiError::unauthorized());
        }
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
