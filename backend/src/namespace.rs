//! Forge owns Git groups and its confirmed resource projection.
use crate::api::{ApiError, AppState};
use axum::{
    Json,
    extract::{Path, Request, State},
    middleware::Next,
    response::Response,
};
use sdlc_shared::resource_context::*;
use sqlx::{PgPool, Row};
use std::sync::Arc;
use uuid::Uuid;

pub fn registered_machine(subject: &str) -> bool {
    [
        "CICD_NAMESPACE__OWNER_SUBJECTS",
        "CICD_NAMESPACE__READER_SUBJECTS",
    ]
    .iter()
    .any(|key| {
        std::env::var(key)
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .any(|s| !s.is_empty() && s == subject)
    })
}
pub async fn owner_auth(req: Request, next: Next) -> Result<Response, ApiError> {
    machine_auth(req, next, "CICD_NAMESPACE__OWNER_SUBJECTS").await
}
pub async fn reader_auth(req: Request, next: Next) -> Result<Response, ApiError> {
    machine_auth(req, next, "CICD_NAMESPACE__READER_SUBJECTS").await
}
async fn machine_auth(req: Request, next: Next, key: &str) -> Result<Response, ApiError> {
    let token = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(ApiError::unauthorized)?;
    let ctx = crate::central_auth::try_central(token)
        .await?
        .ok_or_else(ApiError::unauthorized)?;
    let registered = std::env::var(key)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .any(|s| !s.is_empty() && s == ctx.user_id);
    if ctx.session_id.is_some()
        || !registered
        || !ctx.allows_service("ci-cd", req.method().as_str())
    {
        return Err(ApiError::forbidden());
    }
    Ok(next.run(req).await)
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateSpec {
    slug: String,
    name: String,
}
pub fn validate_projection(command: &OwnerCommand) -> Result<(), ApiError> {
    let instance = std::env::var("CICD_NAMESPACE__INSTANCE_ID")
        .ok()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| ApiError::service_unavailable("namespace_owner_not_configured"))?;
    let registry = std::env::var("CICD_NAMESPACE__REGISTRY_INSTANCE_ID")
        .ok()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| ApiError::service_unavailable("namespace_owner_not_configured"))?;
    if !command.valid_for(ResourceKind::GitGroup, instance, registry) {
        return Err(ApiError::service_unavailable(
            "invalid_namespace_projection",
        ));
    }
    Ok(())
}
pub async fn binding(pool: &PgPool, id: Uuid) -> Result<Option<OwnerCommand>, ApiError> {
    binding_on(pool, id).await
}
pub async fn binding_on<'e, E>(executor: E, id: Uuid) -> Result<Option<OwnerCommand>, ApiError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let Some(row)=sqlx::query("SELECT g.namespace_managed,b.command FROM git_groups g LEFT JOIN forge_namespace_bindings b ON b.resource_id=g.id WHERE g.id=$1").bind(id).fetch_optional(executor).await.map_err(ApiError::internal)? else {return Ok(None);};
    let value: Option<serde_json::Value> = row.get("command");
    if value.is_none() && row.get::<bool, _>("namespace_managed") {
        return Err(ApiError::service_unavailable(
            "namespace_projection_missing",
        ));
    }
    value
        .map(|v| {
            let command: OwnerCommand = serde_json::from_value(v)
                .map_err(|_| ApiError::service_unavailable("invalid_namespace_projection"))?;
            validate_projection(&command)?;
            Ok(command)
        })
        .transpose()
}
#[utoipa::path(put,operation_id="forge_namespace_apply",path="/api/v1/namespace-resources/git_group/{id}",tag="namespaces",params(("id"=Uuid,Path)),request_body=OwnerCommand,responses((status=200,body=OwnerReadback),(status=409,description="Binding/fence conflict")))]
pub async fn apply(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(command): Json<OwnerCommand>,
) -> Result<Json<OwnerReadback>, ApiError> {
    let instance = std::env::var("CICD_NAMESPACE__INSTANCE_ID")
        .ok()
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| ApiError::service_unavailable("namespace_owner_not_configured"))?;
    let registry = std::env::var("CICD_NAMESPACE__REGISTRY_INSTANCE_ID")
        .ok()
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| ApiError::service_unavailable("namespace_owner_not_configured"))?;
    if id != command.resource.resource_id
        || !command.valid_for(ResourceKind::GitGroup, instance, registry)
    {
        return Err(ApiError::bad_request("invalid_namespace_owner_command"));
    }
    let mut tx = state
        .namespace_admission_pool
        .as_ref()
        .ok_or_else(ApiError::unavailable)?
        .begin()
        .await
        .map_err(ApiError::internal)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(ApiError::internal)?;
    if let Some(row) =
        sqlx::query("SELECT command FROM forge_namespace_bindings WHERE resource_id=$1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(ApiError::internal)?
    {
        let old: OwnerCommand = serde_json::from_value(row.get("command"))
            .map_err(|_| ApiError::service_unavailable("invalid_namespace_projection"))?;
        if !command.follows(&old) {
            return Err(ApiError::conflict("namespace_binding_conflict"));
        }
    } else {
        if command.state != "active" {
            return Err(ApiError::conflict("binding_required_before_lifecycle"));
        }
        let exists = sqlx::query("SELECT id FROM git_groups WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(ApiError::internal)?
            .is_some();
        if exists && command.create_spec.is_some() {
            return Err(ApiError::conflict("existing_resource_requires_attach"));
        }
        if !exists {
            let spec: CreateSpec = serde_json::from_value(
                command
                    .create_spec
                    .clone()
                    .ok_or_else(ApiError::not_found)?,
            )
            .map_err(|_| ApiError::bad_request("invalid_group_create_spec"))?;
            if !valid_slug(&spec.slug)
                || spec.name.trim().is_empty()
                || spec.name.chars().count() > 200
            {
                return Err(ApiError::bad_request("invalid_group_properties"));
            }
            sqlx::query("INSERT INTO git_groups(id,slug,name) VALUES($1,$2,$3)")
                .bind(id)
                .bind(spec.slug)
                .bind(spec.name)
                .execute(&mut *tx)
                .await
                .map_err(ApiError::internal)?;
        }
    }
    sqlx::query("INSERT INTO forge_namespace_bindings(resource_id,registry_instance_id,namespace_id,generation,state,command) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(resource_id) DO UPDATE SET generation=EXCLUDED.generation,state=EXCLUDED.state,command=EXCLUDED.command")
        .bind(id).bind(command.namespace.registry_instance_id).bind(command.namespace.namespace_id).bind(command.generation).bind(&command.state).bind(serde_json::to_value(&command).map_err(|_| ApiError::bad_request("invalid_command"))?).execute(&mut *tx).await.map_err(ApiError::internal)?;
    let running: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs j JOIN stages s ON s.id=j.stage_id JOIN pipelines pl ON pl.id=s.pipeline_id JOIN projects p ON p.id=pl.project_id JOIN repository_catalog r ON r.id=p.repository_id WHERE r.group_id=$1 AND (j.status='running' OR EXISTS(SELECT 1 FROM job_leases l WHERE l.job_id=j.id AND l.lease_status='active' AND l.lease_expires_at>now()))").bind(id).fetch_one(&mut *tx).await.map_err(ApiError::internal)?;
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(command.readback(running == 0)))
}
pub fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= 64
        && slug.starts_with(|c: char| c.is_ascii_lowercase())
        && !slug.ends_with('-')
        && !slug.contains("--")
        && slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}
#[utoipa::path(get,operation_id="forge_namespace_readback",path="/api/v1/namespace-resources/git_group/{id}",tag="namespaces",params(("id"=Uuid,Path)),responses((status=200,body=OwnerReadback),(status=404,description="Binding not found")))]
pub async fn readback(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<OwnerReadback>, ApiError> {
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    let command = binding(pool, id).await?.ok_or_else(ApiError::not_found)?;
    let running: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs j JOIN stages s ON s.id=j.stage_id JOIN pipelines pl ON pl.id=s.pipeline_id JOIN projects p ON p.id=pl.project_id JOIN repository_catalog r ON r.id=p.repository_id WHERE r.group_id=$1 AND (j.status='running' OR EXISTS(SELECT 1 FROM job_leases l WHERE l.job_id=j.id AND l.lease_status='active' AND l.lease_expires_at>now()))").bind(id).fetch_one(pool).await.map_err(ApiError::internal)?;
    Ok(Json(command.readback(running == 0)))
}
pub fn routes() -> axum::Router<Arc<AppState>> {
    axum::Router::new()
        .route(
            "/api/v1/namespace-resources/git_group/{id}",
            axum::routing::get(readback).put(apply),
        )
        .route_layer(axum::middleware::from_fn(owner_auth))
        .merge(
            axum::Router::new()
                .route(
                    "/api/v1/namespace-repositories/{id}",
                    axum::routing::get(crate::repository_catalog::verified_repository),
                )
                .route(
                    "/api/v1/namespace-repositories",
                    axum::routing::get(crate::repository_catalog::verified_repositories),
                )
                .route(
                    "/api/v1/namespace-task-evidence/{tracker}/{task}",
                    axum::routing::get(crate::task_links::evidence),
                )
                .route_layer(axum::middleware::from_fn(reader_auth)),
        )
}

#[derive(serde::Deserialize, utoipa::IntoParams)]
pub struct Page {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}
#[utoipa::path(get,operation_id="forge_namespace_available_resources",path="/api/v1/namespace-available-resources",tag="namespaces",params(Page),responses((status=200,body=Vec<ResourceCatalogItem>)))]
pub async fn available_resources(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(page): axum::extract::Query<Page>,
) -> Result<Json<Vec<ResourceCatalogItem>>, ApiError> {
    let instance = std::env::var("CICD_NAMESPACE__INSTANCE_ID")
        .ok()
        .and_then(|value| value.parse::<Uuid>().ok())
        .filter(|id| !id.is_nil())
        .ok_or_else(|| ApiError::service_unavailable("namespace_owner_not_configured"))?;
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    let rows=sqlx::query("SELECT g.id,g.slug,g.name FROM git_groups g WHERE NOT EXISTS(SELECT 1 FROM forge_namespace_bindings b WHERE b.resource_id=g.id) ORDER BY g.slug,g.id LIMIT $1 OFFSET $2").bind(page.limit.unwrap_or(50).clamp(1,100)).bind(page.offset.unwrap_or(0).max(0)).fetch_all(pool).await.map_err(ApiError::internal)?;
    Ok(Json(
        rows.into_iter()
            .map(|row| ResourceCatalogItem {
                resource: ResourceRef {
                    kind: ResourceKind::GitGroup,
                    instance_id: instance,
                    resource_id: row.get("id"),
                },
                label: row.get("name"),
                resource_key: row.get("slug"),
            })
            .collect(),
    ))
}
#[utoipa::path(get,operation_id="forge_namespace_stats",path="/api/v1/namespace-stats/{registry}/{namespace}",tag="namespaces",params(("registry"=Uuid,Path),("namespace"=Uuid,Path)),responses((status=200,body=ResourceStats)))]
pub async fn stats(
    State(state): State<Arc<AppState>>,
    Path((registry, namespace)): Path<(Uuid, Uuid)>,
) -> Result<Json<ResourceStats>, ApiError> {
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    let resources = local_contexts(
        pool,
        Some(NamespaceRef {
            registry_instance_id: registry,
            namespace_id: namespace,
        }),
        1,
        0,
    )
    .await?;
    let resource = resources
        .into_iter()
        .next()
        .ok_or_else(ApiError::not_found)?;
    let row=sqlx::query("SELECT count(*) AS repositories,count(*) FILTER(WHERE kind='hosted') AS hosted,count(*) FILTER(WHERE kind='external') AS external FROM repository_catalog WHERE group_id=$1").bind(resource.binding.resource.resource_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    Ok(Json(ResourceStats {
        binding: resource.binding,
        counters: std::collections::BTreeMap::from([
            ("repositories".into(), row.get("repositories")),
            ("hosted".into(), row.get("hosted")),
            ("external".into(), row.get("external")),
        ]),
    }))
}
async fn local_contexts(
    pool: &PgPool,
    namespace: Option<NamespaceRef>,
    limit: i64,
    offset: i64,
) -> Result<Vec<ResourceContextSummary>, ApiError> {
    let rows=sqlx::query("SELECT b.command,g.name,g.slug FROM forge_namespace_bindings b JOIN git_groups g ON g.id=b.resource_id WHERE ($1::uuid IS NULL OR (b.registry_instance_id=$1 AND b.namespace_id=$2)) ORDER BY g.name,g.id LIMIT $3 OFFSET $4").bind(namespace.as_ref().map(|n| n.registry_instance_id)).bind(namespace.as_ref().map(|n| n.namespace_id)).bind(limit.clamp(1,100)).bind(offset.max(0)).fetch_all(pool).await.map_err(ApiError::internal)?;
    rows.into_iter()
        .map(|row| {
            let command: OwnerCommand = serde_json::from_value(row.get("command"))
                .map_err(|_| ApiError::service_unavailable("invalid_namespace_projection"))?;
            validate_projection(&command)?;
            Ok(ResourceContextSummary {
                binding: command.readback(true),
                label: row.get("name"),
                resource_key: row.get("slug"),
            })
        })
        .collect()
}
#[utoipa::path(get,operation_id="forge_namespace_contexts",path="/api/v1/namespace-contexts",tag="namespaces",params(Page),responses((status=200,body=Vec<ResourceContextSummary>)))]
pub async fn contexts(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(page): axum::extract::Query<Page>,
) -> Result<Json<Vec<ResourceContextSummary>>, ApiError> {
    Ok(Json(
        local_contexts(
            state.pool.as_ref().ok_or_else(ApiError::unavailable)?,
            None,
            page.limit.unwrap_or(50),
            page.offset.unwrap_or(0),
        )
        .await?,
    ))
}
#[utoipa::path(get,operation_id="forge_namespace_context",path="/api/v1/namespace-contexts/{registry}/{namespace}",tag="namespaces",params(("registry"=Uuid,Path),("namespace"=Uuid,Path)),responses((status=200,body=ResourceContextSummary),(status=404,description="No confirmed local binding")))]
pub async fn context(
    State(state): State<Arc<AppState>>,
    Path((registry, namespace)): Path<(Uuid, Uuid)>,
) -> Result<Json<ResourceContextSummary>, ApiError> {
    let mut resources = local_contexts(
        state.pool.as_ref().ok_or_else(ApiError::unavailable)?,
        Some(NamespaceRef {
            registry_instance_id: registry,
            namespace_id: namespace,
        }),
        1,
        0,
    )
    .await?;
    Ok(Json(resources.pop().ok_or_else(ApiError::not_found)?))
}
