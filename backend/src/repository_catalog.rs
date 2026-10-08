//! Public names resolve to immutable catalog/storage identities, including legacy aliases.
use crate::api::{ApiError, AppState};
use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use std::sync::Arc;
use uuid::Uuid;

fn catalog_database_error(error: sqlx::Error) -> ApiError {
    match error
        .as_database_error()
        .and_then(|value| value.code())
        .as_deref()
    {
        Some("23505") => ApiError::conflict("repository_identity_or_slug_conflict"),
        Some("42501") => ApiError::conflict("namespace_resource_read_only"),
        _ => ApiError::internal(error),
    }
}

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateRepository {
    pub id: Uuid,
    pub slug: String,
    pub kind: String,
    pub visibility: String,
    pub external_url: Option<String>,
    #[serde(default)]
    pub provider_refs: serde_json::Value,
}
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub struct CatalogRepository {
    pub id: Uuid,
    pub group_id: Option<Uuid>,
    pub slug: String,
    pub kind: String,
    pub ready: bool,
    pub external_url: Option<String>,
    pub provider_refs: serde_json::Value,
    pub public_name: String,
    pub visibility: String,
    pub storage_name: Option<String>,
    pub namespace: Option<sdlc_shared::resource_context::NamespaceRef>,
    pub state: Option<String>,
    pub clone_url: Option<String>,
    pub availability: Option<String>,
}
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub struct CatalogPage {
    pub items: Vec<CatalogRepository>,
}
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AttachRepository {
    pub operation_id: Uuid,
    pub group_id: Uuid,
    pub slug: String,
}
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub struct AttachReadback {
    pub operation_id: Uuid,
    pub repository_id: Uuid,
    pub group_id: Uuid,
    pub slug: String,
}
#[derive(Deserialize, utoipa::IntoParams)]
pub struct Page {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}
#[derive(Deserialize, utoipa::IntoParams)]
pub struct NamespaceQuery {
    pub registry_instance_id: Uuid,
    pub namespace_id: Uuid,
    pub offset: Option<u32>,
}
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub struct VerifiedRepository {
    pub namespace: sdlc_shared::resource_context::NamespaceRef,
    pub forge_instance_id: Uuid,
    pub repository_id: Uuid,
    pub public_name: String,
    pub kind: String,
}
#[utoipa::path(get,operation_id="forge_namespace_repository_catalog",path="/api/v1/namespace-repositories",tag="namespaces",params(NamespaceQuery),responses((status=200,body=Vec<VerifiedRepository>)))]
pub async fn verified_repositories(
    State(state): State<Arc<AppState>>,
    Query(query): Query<NamespaceQuery>,
) -> Result<Json<Vec<VerifiedRepository>>, ApiError> {
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    let rows=sqlx::query("SELECT r.id,r.kind,g.slug||'/'||r.slug AS public_name,b.command FROM repository_catalog r JOIN git_groups g ON g.id=r.group_id JOIN forge_namespace_bindings b ON b.resource_id=g.id WHERE b.registry_instance_id=$1 AND b.namespace_id=$2 AND r.ready ORDER BY r.slug,r.id LIMIT 50 OFFSET $3").bind(query.registry_instance_id).bind(query.namespace_id).bind(i64::from(query.offset.unwrap_or(0))).fetch_all(pool).await.map_err(catalog_database_error)?;
    let mut result = Vec::new();
    for row in rows {
        let command: sdlc_shared::resource_context::OwnerCommand =
            serde_json::from_value(row.get("command"))
                .map_err(|_| ApiError::service_unavailable("invalid_namespace_projection"))?;
        crate::namespace::validate_projection(&command)?;
        if command.state != "active" {
            return Err(ApiError::forbidden());
        }
        result.push(VerifiedRepository {
            namespace: command.namespace,
            forge_instance_id: command.resource.instance_id,
            repository_id: row.get("id"),
            public_name: row.get("public_name"),
            kind: row.get("kind"),
        });
    }
    Ok(Json(result))
}
#[utoipa::path(get,operation_id="forge_verify_repository_ref",path="/api/v1/namespace-repositories/{id}",tag="namespaces",params(("id"=Uuid,Path),NamespaceQuery),responses((status=200,body=VerifiedRepository),(status=403,description="Foreign namespace")))]
pub async fn verified_repository(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Query(query): Query<NamespaceQuery>,
) -> Result<Json<VerifiedRepository>, ApiError> {
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    let row=sqlx::query("SELECT r.kind,g.slug||'/'||r.slug AS public_name,b.command FROM repository_catalog r JOIN git_groups g ON g.id=r.group_id JOIN forge_namespace_bindings b ON b.resource_id=g.id WHERE r.id=$1 AND r.ready").bind(id).fetch_optional(pool).await.map_err(catalog_database_error)?.ok_or_else(ApiError::not_found)?;
    let command: sdlc_shared::resource_context::OwnerCommand =
        serde_json::from_value(row.get("command"))
            .map_err(|_| ApiError::service_unavailable("invalid_namespace_projection"))?;
    crate::namespace::validate_projection(&command)?;
    if command.state != "active" {
        return Err(ApiError::forbidden());
    }
    if command.namespace.registry_instance_id != query.registry_instance_id
        || command.namespace.namespace_id != query.namespace_id
    {
        return Err(ApiError::forbidden());
    }
    Ok(Json(VerifiedRepository {
        namespace: command.namespace,
        forge_instance_id: command.resource.instance_id,
        repository_id: id,
        public_name: row.get("public_name"),
        kind: row.get("kind"),
    }))
}

pub async fn resolve_storage(pool: &PgPool, raw: &str) -> Result<String, ApiError> {
    let name = raw.trim_end_matches(".git");
    if name.contains('/') {
        let parts: Vec<_> = name.split('/').collect();
        if parts.len() != 2
            || !crate::namespace::valid_slug(parts[0])
            || crate::git_host::validate_repo_name(parts[1]).is_err()
        {
            return Err(ApiError::bad_request("invalid_repository_path"));
        }
    } else {
        crate::git_host::validate_repo_name(name).map_err(ApiError::bad_request)?;
    }
    let row = sqlx::query("SELECT r.storage_name,r.kind,r.ready FROM repository_aliases a JOIN repository_catalog r ON r.id=a.repository_id WHERE a.alias=$1")
        .bind(name).fetch_optional(pool).await.map_err(catalog_database_error)?.ok_or_else(ApiError::not_found)?;
    if row.get::<String, _>("kind") != "hosted" {
        return Err(ApiError::bad_request(
            "external_repository_has_no_local_git_storage",
        ));
    }
    if !row.get::<bool, _>("ready") {
        return Err(ApiError::service_unavailable(
            "repository_initialization_pending",
        ));
    }
    row.try_get("storage_name").map_err(ApiError::internal)
}

/// Held for the complete Git/merge operation. Archive takes the exclusive group lock.
pub async fn write_lease<'a>(
    state: &'a AppState,
    storage: &str,
) -> Result<sqlx::Transaction<'a, sqlx::Postgres>, ApiError> {
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    let repository: Uuid =
        sqlx::query_scalar("SELECT id FROM repository_catalog WHERE storage_name=$1")
            .bind(storage)
            .fetch_optional(pool)
            .await
            .map_err(catalog_database_error)?
            .ok_or_else(ApiError::not_found)?;
    let mut tx = state
        .namespace_admission_pool
        .as_ref()
        .ok_or_else(ApiError::unavailable)?
        .begin()
        .await
        .map_err(catalog_database_error)?;
    sqlx::query("SELECT pg_advisory_xact_lock_shared(hashtextextended($1,3))")
        .bind(repository.to_string())
        .execute(&mut *tx)
        .await
        .map_err(catalog_database_error)?;
    let group: Option<Uuid> =
        sqlx::query_scalar("SELECT group_id FROM repository_catalog WHERE id=$1")
            .bind(repository)
            .fetch_optional(&mut *tx)
            .await
            .map_err(catalog_database_error)?
            .ok_or_else(ApiError::not_found)?;
    if let Some(group) = group {
        sqlx::query("SELECT pg_advisory_xact_lock_shared(hashtextextended($1,0))")
            .bind(group.to_string())
            .execute(&mut *tx)
            .await
            .map_err(catalog_database_error)?;
        crate::namespace::binding_on(&mut *tx, group)
            .await?
            .ok_or_else(|| ApiError::service_unavailable("namespace_projection_missing"))?;
        let state: Option<String> =
            sqlx::query_scalar("SELECT state FROM forge_namespace_bindings WHERE resource_id=$1")
                .bind(group)
                .fetch_optional(&mut *tx)
                .await
                .map_err(catalog_database_error)?;
        if state.as_deref() != Some("active") {
            return Err(ApiError::conflict("namespace_resource_read_only"));
        }
    }
    Ok(tx)
}
pub async fn require_project_writable(pool: &PgPool, project: Uuid) -> Result<(), ApiError> {
    let group: Option<Uuid>=sqlx::query_scalar("SELECT r.group_id FROM projects p LEFT JOIN repository_catalog r ON r.id=p.repository_id WHERE p.id=$1").bind(project).fetch_optional(pool).await.map_err(catalog_database_error)?.flatten();
    if let Some(group) = group {
        crate::namespace::binding(pool, group).await?;
    }
    let lifecycle: Option<String> = sqlx::query_scalar("SELECT b.state FROM projects p JOIN repository_catalog r ON r.id=p.repository_id JOIN forge_namespace_bindings b ON b.resource_id=r.group_id WHERE p.id=$1").bind(project).fetch_optional(pool).await.map_err(catalog_database_error)?;
    if lifecycle.is_some_and(|s| s != "active") {
        return Err(ApiError::conflict("namespace_resource_read_only"));
    }
    Ok(())
}
pub async fn require_storage_writable(pool: &PgPool, storage: &str) -> Result<(), ApiError> {
    let group: Option<Uuid> =
        sqlx::query_scalar("SELECT group_id FROM repository_catalog WHERE storage_name=$1")
            .bind(storage)
            .fetch_optional(pool)
            .await
            .map_err(catalog_database_error)?
            .flatten();
    if let Some(group) = group {
        crate::namespace::binding(pool, group).await?;
    }
    let lifecycle: Option<String>=sqlx::query_scalar("SELECT b.state FROM repository_catalog r JOIN forge_namespace_bindings b ON b.resource_id=r.group_id WHERE r.storage_name=$1").bind(storage).fetch_optional(pool).await.map_err(catalog_database_error)?;
    if lifecycle.is_some_and(|state| state != "active") {
        return Err(ApiError::conflict("namespace_resource_read_only"));
    }
    Ok(())
}
/// Only an explicit delivery mapping permits access to local Git storage.
pub async fn local_storage_for_project(
    pool: &PgPool,
    project: Uuid,
) -> Result<Option<String>, ApiError> {
    let row=sqlx::query("SELECT r.kind,r.ready,r.storage_name FROM projects p LEFT JOIN repository_catalog r ON r.id=p.repository_id WHERE p.id=$1").bind(project).fetch_optional(pool).await.map_err(catalog_database_error)?.ok_or_else(ApiError::not_found)?;
    if row.get::<Option<String>, _>("kind").as_deref() != Some("hosted") {
        return Ok(None);
    }
    if row.get::<Option<bool>, _>("ready") != Some(true) {
        return Err(ApiError::service_unavailable(
            "repository_initialization_pending",
        ));
    }
    let name: String = row
        .try_get("storage_name")
        .map_err(catalog_database_error)?;
    crate::git_host::validate_repo_name(&name)
        .map(Some)
        .map_err(ApiError::bad_request)
}
pub fn public_git_url(group: &str, slug: &str) -> Result<String, ApiError> {
    let raw = std::env::var("CICD_NAMESPACE__PUBLIC_GIT_ORIGIN")
        .map_err(|_| ApiError::service_unavailable("public_git_origin_not_configured"))?;
    let origin = reqwest::Url::parse(&raw)
        .map_err(|_| ApiError::service_unavailable("invalid_public_git_origin"))?;
    if !matches!(origin.scheme(), "http" | "https")
        || origin.host_str().is_none()
        || !origin.username().is_empty()
        || origin.password().is_some()
        || origin.path() != "/"
        || origin.query().is_some()
        || origin.fragment().is_some()
    {
        return Err(ApiError::service_unavailable("invalid_public_git_origin"));
    }
    origin
        .join(&format!("git/{group}/{slug}.git"))
        .map(|url| url.to_string())
        .map_err(|_| ApiError::service_unavailable("invalid_public_git_origin"))
}
pub async fn checkout_url_for_project(pool: &PgPool, project: Uuid) -> Result<String, ApiError> {
    let row=sqlx::query("SELECT p.repository_url,r.kind,r.external_url,r.slug,g.slug AS group_slug FROM projects p LEFT JOIN repository_catalog r ON r.id=p.repository_id LEFT JOIN git_groups g ON g.id=r.group_id WHERE p.id=$1").bind(project).fetch_optional(pool).await.map_err(catalog_database_error)?.ok_or_else(ApiError::not_found)?;
    match row.get::<Option<String>, _>("kind").as_deref() {
        Some("external") => row.try_get("external_url").map_err(ApiError::internal),
        Some("hosted") if row.get::<Option<String>, _>("group_slug").is_some() => public_git_url(
            &row.get::<String, _>("group_slug"),
            &row.get::<String, _>("slug"),
        ),
        _ => row.try_get("repository_url").map_err(ApiError::internal),
    }
}

#[utoipa::path(get,operation_id="forge_repository_catalog_list",path="/api/v1/git-groups/{id}/repositories",tag="namespaces",params(("id"=Uuid,Path),Page),responses((status=200,body=CatalogPage)))]
pub async fn list(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Query(page): Query<Page>,
) -> Result<Json<CatalogPage>, ApiError> {
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    crate::namespace::binding(pool, id).await?;
    let items: Vec<serde_json::Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',r.id,'group_id',r.group_id,'slug',r.slug,'kind',r.kind,'ready',r.ready,'external_url',r.external_url,'provider_refs',r.provider_refs,'public_name',g.slug||'/'||r.slug,'storage_name',r.storage_name,'visibility',s.visibility) FROM repository_catalog r JOIN git_groups g ON g.id=r.group_id JOIN repositories s ON s.id=r.id WHERE r.group_id=$1 ORDER BY r.slug,r.id LIMIT $2 OFFSET $3")
        .bind(id).bind(page.limit.unwrap_or(50).clamp(1,100)).bind(page.offset.unwrap_or(0).max(0)).fetch_all(pool).await.map_err(catalog_database_error)?;
    let mut items = items
        .into_iter()
        .map(|value| {
            serde_json::from_value(value)
                .map_err(|_| ApiError::service_unavailable("invalid_repository_catalog"))
        })
        .collect::<Result<Vec<CatalogRepository>, _>>()?;
    for repository in &mut items {
        check_availability(&state, repository).await;
    }
    Ok(Json(CatalogPage { items }))
}

#[utoipa::path(get,operation_id="forge_unbound_repository_catalog",path="/api/v1/catalog/available-repositories",tag="namespaces",params(Page),responses((status=200,body=CatalogPage)))]
pub async fn available(
    State(state): State<Arc<AppState>>,
    Query(page): Query<Page>,
) -> Result<Json<CatalogPage>, ApiError> {
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    let values: Vec<serde_json::Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',r.id,'group_id',r.group_id,'slug',r.slug,'kind',r.kind,'ready',r.ready,'external_url',r.external_url,'provider_refs',r.provider_refs,'public_name',r.slug,'storage_name',r.storage_name,'visibility',s.visibility) FROM repository_catalog r JOIN repositories s ON s.id=r.id WHERE r.group_id IS NULL ORDER BY r.slug,r.id LIMIT $1 OFFSET $2").bind(page.limit.unwrap_or(50).clamp(1,100)).bind(page.offset.unwrap_or(0).max(0)).fetch_all(pool).await.map_err(catalog_database_error)?;
    Ok(Json(CatalogPage {
        items: values
            .into_iter()
            .map(|value| {
                serde_json::from_value(value)
                    .map_err(|_| ApiError::service_unavailable("invalid_repository_catalog"))
            })
            .collect::<Result<Vec<_>, _>>()?,
    }))
}

#[utoipa::path(put,operation_id="forge_attach_repository",path="/api/v1/catalog/repositories/{id}/group",tag="namespaces",params(("id"=Uuid,Path)),request_body=AttachRepository,responses((status=200,body=AttachReadback),(status=409,description="Already attached or conflicting original operation")))]
pub async fn attach(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    axum::Extension(actor): axum::Extension<crate::auth::AccessClaims>,
    Json(input): Json<AttachRepository>,
) -> Result<Json<AttachReadback>, ApiError> {
    if id.is_nil()
        || input.operation_id.is_nil()
        || input.group_id.is_nil()
        || crate::git_host::validate_repo_name(&input.slug).as_deref() != Ok(input.slug.as_str())
    {
        return Err(ApiError::bad_request("invalid_repository_attach"));
    }
    let payload = serde_json::to_value(&input)
        .map_err(|_| ApiError::bad_request("invalid_repository_attach"))?;
    let mut tx = state
        .namespace_admission_pool
        .as_ref()
        .ok_or_else(ApiError::unavailable)?
        .begin()
        .await
        .map_err(catalog_database_error)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,2))")
        .bind(input.operation_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(catalog_database_error)?;
    if let Some(old)=sqlx::query("SELECT repository_id,actor_id,command,readback FROM repository_attach_operations WHERE operation_id=$1").bind(input.operation_id).fetch_optional(&mut *tx).await.map_err(catalog_database_error)? {
        if old.get::<Uuid,_>("repository_id")!=id || old.get::<Uuid,_>("actor_id")!=actor.sub || old.get::<serde_json::Value,_>("command")!=payload {return Err(ApiError::conflict("repository_attach_payload_conflict"));}
        let readback=serde_json::from_value(old.get("readback")).map_err(|_| ApiError::service_unavailable("invalid_repository_attach_readback"))?;
        return Ok(Json(readback));
    }
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,3))")
        .bind(id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(catalog_database_error)?;
    sqlx::query("SELECT pg_advisory_xact_lock_shared(hashtextextended($1,0))")
        .bind(input.group_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(catalog_database_error)?;
    let binding = crate::namespace::binding_on(&mut *tx, input.group_id)
        .await?
        .ok_or_else(|| ApiError::conflict("managed_git_group_required"))?;
    if binding.state != "active" {
        return Err(ApiError::conflict("namespace_resource_read_only"));
    }
    let group_slug: String = sqlx::query_scalar("SELECT slug FROM git_groups WHERE id=$1")
        .bind(input.group_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(catalog_database_error)?;
    let repo = sqlx::query(
        "SELECT group_id,kind,storage_name,ready FROM repository_catalog WHERE id=$1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(catalog_database_error)?
    .ok_or_else(ApiError::not_found)?;
    if repo.get::<Option<Uuid>, _>("group_id").is_some() {
        return Err(ApiError::conflict("repository_already_attached"));
    }
    if !repo.get::<bool, _>("ready") {
        return Err(ApiError::conflict("repository_initialization_pending"));
    }
    if repo.get::<String, _>("kind") == "hosted" {
        let storage: String = repo
            .try_get("storage_name")
            .map_err(catalog_database_error)?;
        crate::git_host::validate_repo_name(&storage).map_err(ApiError::bad_request)?;
        if !tokio::fs::try_exists(state.git.root.join(format!("{storage}.git/HEAD")))
            .await
            .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?
        {
            return Err(ApiError::conflict("historical_repository_storage_missing"));
        }
    }
    let occupied: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM repository_catalog WHERE group_id=$1 AND slug=$2)",
    )
    .bind(input.group_id)
    .bind(&input.slug)
    .fetch_one(&mut *tx)
    .await
    .map_err(catalog_database_error)?;
    if occupied {
        return Err(ApiError::conflict("repository_slug_already_used"));
    }
    sqlx::query("UPDATE repository_catalog SET group_id=$2,slug=$3 WHERE id=$1")
        .bind(id)
        .bind(input.group_id)
        .bind(&input.slug)
        .execute(&mut *tx)
        .await
        .map_err(catalog_database_error)?;
    sqlx::query("INSERT INTO repository_aliases(alias,repository_id) VALUES($1,$2)")
        .bind(format!("{group_slug}/{}", input.slug))
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(catalog_database_error)?;
    let result = AttachReadback {
        operation_id: input.operation_id,
        repository_id: id,
        group_id: input.group_id,
        slug: input.slug,
    };
    sqlx::query("INSERT INTO repository_attach_operations(operation_id,repository_id,actor_id,command,readback) VALUES($1,$2,$3,$4,$5)").bind(result.operation_id).bind(id).bind(actor.sub).bind(payload).bind(serde_json::to_value(&result).map_err(|_| ApiError::bad_request("invalid_repository_attach"))?).execute(&mut *tx).await.map_err(catalog_database_error)?;
    tx.commit().await.map_err(catalog_database_error)?;
    Ok(Json(result))
}

#[utoipa::path(post,operation_id="forge_repository_catalog_create",path="/api/v1/git-groups/{id}/repositories",tag="namespaces",params(("id"=Uuid,Path)),request_body=CreateRepository,responses((status=200,description="Original ID replay or created catalog repository"),(status=409,description="Payload or namespace conflict")))]
pub async fn create(
    State(state): State<Arc<AppState>>,
    Path(group): Path<Uuid>,
    Json(input): Json<CreateRepository>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if input.id.is_nil()
        || crate::git_host::validate_repo_name(&input.slug).as_deref() != Ok(input.slug.as_str())
        || !matches!(input.kind.as_str(), "hosted" | "external")
        || !matches!(input.visibility.as_str(), "public" | "private")
    {
        return Err(ApiError::bad_request("invalid_repository_properties"));
    }
    if (input.kind == "external") != input.external_url.is_some() {
        return Err(ApiError::bad_request("repository_source_required"));
    }
    if let Some(raw) = &input.external_url {
        cicd_domain::repository_url::validate_repository_url(raw).map_err(ApiError::bad_request)?;
    }
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    let payload = serde_json::to_value(&input)
        .map_err(|_| ApiError::bad_request("invalid_repository_command"))?;
    if let Some(old)=sqlx::query("SELECT r.group_id,r.create_command,r.ready,g.slug||'/'||r.slug AS public_name FROM repository_catalog r LEFT JOIN git_groups g ON g.id=r.group_id WHERE r.id=$1").bind(input.id).fetch_optional(pool).await.map_err(catalog_database_error)? {
        if old.get::<Option<Uuid>,_>("group_id")!=Some(group) || old.get::<Option<serde_json::Value>,_>("create_command")!=Some(payload.clone()) {return Err(ApiError::conflict("repository_operation_payload_conflict"));}
        // A confirmed resource is never initialized again, even if its storage
        // is now missing. Recovery must restore the actual historical data.
        if old.get::<bool,_>("ready") {
            return Ok(Json(serde_json::json!({"id":input.id,"group_id":group,"slug":input.slug,"kind":input.kind,"ready":true,"public_name":old.get::<String,_>("public_name")})));
        }
    }
    let mut lease = state
        .namespace_admission_pool
        .as_ref()
        .ok_or_else(ApiError::unavailable)?
        .begin()
        .await
        .map_err(catalog_database_error)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,3))")
        .bind(input.id.to_string())
        .execute(&mut *lease)
        .await
        .map_err(catalog_database_error)?;
    sqlx::query("SELECT pg_advisory_xact_lock_shared(hashtextextended($1,0))")
        .bind(group.to_string())
        .execute(&mut *lease)
        .await
        .map_err(catalog_database_error)?;
    crate::namespace::binding_on(&mut *lease, group)
        .await?
        .ok_or_else(|| ApiError::service_unavailable("namespace_projection_missing"))?;
    let group_slug: Option<String> = sqlx::query_scalar("SELECT g.slug FROM git_groups g JOIN forge_namespace_bindings b ON b.resource_id=g.id WHERE g.id=$1 AND b.state='active'").bind(group).fetch_optional(&mut *lease).await.map_err(catalog_database_error)?;
    let group_slug =
        group_slug.ok_or_else(|| ApiError::conflict("namespace_resource_read_only"))?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,1))")
        .bind(input.id.to_string())
        .execute(&mut *lease)
        .await
        .map_err(catalog_database_error)?;
    // Catalog intent commits before filesystem work; replays use the same opaque path.
    let mut tx = pool.begin().await.map_err(catalog_database_error)?;
    let storage = format!("ns-{}", input.id.simple());
    if let Some(row) =
        sqlx::query("SELECT group_id,create_command,ready FROM repository_catalog WHERE id=$1")
            .bind(input.id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(catalog_database_error)?
    {
        if row.get::<Option<Uuid>, _>("group_id") != Some(group)
            || row.get::<Option<serde_json::Value>, _>("create_command") != Some(payload.clone())
        {
            return Err(ApiError::conflict("repository_operation_payload_conflict"));
        }
        if row.get::<bool, _>("ready") {
            tx.commit().await.map_err(catalog_database_error)?;
            lease.commit().await.map_err(catalog_database_error)?;
            return Ok(Json(
                serde_json::json!({"id":input.id,"group_id":group,"slug":input.slug,"kind":input.kind,"ready":true,"public_name":format!("{group_slug}/{}",input.slug)}),
            ));
        }
    } else {
        sqlx::query("INSERT INTO repositories(id,name,visibility) VALUES($1,$2,$3)")
            .bind(input.id)
            .bind(&storage)
            .bind(&input.visibility)
            .execute(&mut *tx)
            .await
            .map_err(catalog_database_error)?;
        sqlx::query("INSERT INTO repository_catalog(id,group_id,slug,kind,storage_name,external_url,provider_refs,create_command,ready) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT(id) DO UPDATE SET group_id=EXCLUDED.group_id,slug=EXCLUDED.slug,kind=EXCLUDED.kind,storage_name=EXCLUDED.storage_name,external_url=EXCLUDED.external_url,provider_refs=EXCLUDED.provider_refs,create_command=EXCLUDED.create_command,ready=EXCLUDED.ready")
            .bind(input.id).bind(group).bind(&input.slug).bind(&input.kind).bind(if input.kind=="hosted" {Some(storage.as_str())}else{None}).bind(&input.external_url).bind(&input.provider_refs).bind(payload).bind(input.kind=="external").execute(&mut *tx).await.map_err(catalog_database_error)?;
        sqlx::query("INSERT INTO repository_aliases(alias,repository_id) VALUES($1,$2)")
            .bind(format!("{group_slug}/{}", input.slug))
            .bind(input.id)
            .execute(&mut *tx)
            .await
            .map_err(catalog_database_error)?;
        if input.kind == "hosted" {
            sqlx::query("INSERT INTO repository_aliases(alias,repository_id) VALUES($1,$2) ON CONFLICT(alias) DO NOTHING").bind(&storage).bind(input.id).execute(&mut *tx).await.map_err(catalog_database_error)?;
        }
    }
    tx.commit().await.map_err(catalog_database_error)?;
    if input.kind == "hosted" {
        crate::git_host::ensure_owned_bare_repository(&state.git, &storage, input.id).await?;
        sqlx::query("UPDATE repository_catalog SET ready=true WHERE id=$1")
            .bind(input.id)
            .execute(pool)
            .await
            .map_err(catalog_database_error)?;
    }
    lease.commit().await.map_err(catalog_database_error)?;
    Ok(Json(
        serde_json::json!({"id":input.id,"group_id":group,"slug":input.slug,"kind":input.kind,"ready":true,"public_name":format!("{group_slug}/{}",input.slug)}),
    ))
}

#[utoipa::path(put,operation_id="forge_repository_catalog_connect_delivery",path="/api/v1/catalog/repositories/{id}/delivery-configs/{project_id}",tag="namespaces",params(("id"=Uuid,Path),("project_id"=Uuid,Path)),responses((status=200,description="Explicit stable repository/config mapping")))]
pub async fn connect_delivery(
    State(state): State<Arc<AppState>>,
    Path((id, project)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    let mut tx = pool.begin().await.map_err(catalog_database_error)?;
    let row = sqlx::query("SELECT r.group_id,b.state FROM repository_catalog r LEFT JOIN forge_namespace_bindings b ON b.resource_id=r.group_id WHERE r.id=$1").bind(id).fetch_optional(&mut *tx).await.map_err(catalog_database_error)?.ok_or_else(ApiError::not_found)?;
    if row.get::<Option<Uuid>, _>("group_id").is_some()
        && row.get::<Option<String>, _>("state").as_deref() != Some("active")
    {
        return Err(ApiError::conflict("namespace_resource_read_only"));
    }
    let affected = sqlx::query("UPDATE projects SET repository_id=$2 WHERE id=$1 AND (repository_id IS NULL OR repository_id=$2)").bind(project).bind(id).execute(&mut *tx).await.map_err(catalog_database_error)?.rows_affected();
    if affected != 1 {
        return Err(ApiError::conflict("delivery_repository_already_bound"));
    }
    tx.commit().await.map_err(catalog_database_error)?;
    Ok(Json(
        serde_json::json!({"repository_id":id,"project_id":project}),
    ))
}

pub async fn storage_by_id(pool: &PgPool, id: Uuid) -> Result<String, ApiError> {
    let row = sqlx::query("SELECT storage_name,kind,ready FROM repository_catalog WHERE id=$1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(catalog_database_error)?
        .ok_or_else(ApiError::not_found)?;
    if row.get::<String, _>("kind") != "hosted" {
        return Err(ApiError::bad_request(
            "external_repository_has_no_local_pull_requests",
        ));
    }
    if !row.get::<bool, _>("ready") {
        return Err(ApiError::service_unavailable(
            "repository_initialization_pending",
        ));
    }
    row.try_get("storage_name").map_err(ApiError::internal)
}
#[utoipa::path(get,operation_id="forge_repository_catalog_get",path="/api/v1/catalog/repositories/{id}",tag="namespaces",params(("id"=Uuid,Path)),responses((status=200,body=CatalogRepository)))]
pub async fn get(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<CatalogRepository>, ApiError> {
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    let value: serde_json::Value=sqlx::query_scalar("SELECT jsonb_build_object('id',r.id,'group_id',r.group_id,'slug',r.slug,'kind',r.kind,'ready',r.ready,'external_url',r.external_url,'provider_refs',r.provider_refs,'public_name',COALESCE(g.slug||'/'||r.slug,r.slug),'storage_name',r.storage_name,'visibility',s.visibility,'namespace',CASE WHEN b.resource_id IS NOT NULL THEN jsonb_build_object('registry_instance_id',b.registry_instance_id,'namespace_id',b.namespace_id) ELSE NULL END,'state',b.state) FROM repository_catalog r JOIN repositories s ON s.id=r.id LEFT JOIN git_groups g ON g.id=r.group_id LEFT JOIN forge_namespace_bindings b ON b.resource_id=r.group_id WHERE r.id=$1").bind(id).fetch_optional(pool).await.map_err(catalog_database_error)?.ok_or_else(ApiError::not_found)?;
    let mut repository: CatalogRepository = serde_json::from_value(value)
        .map_err(|_| ApiError::service_unavailable("invalid_repository_catalog"))?;
    if let Some(group) = repository.group_id {
        crate::namespace::binding(pool, group).await?;
    }
    check_availability(&state, &mut repository).await;
    repository.clone_url = if repository.kind == "external" {
        repository.external_url.clone()
    } else if repository.group_id.is_some() {
        let (group, slug) = repository
            .public_name
            .split_once('/')
            .ok_or_else(|| ApiError::service_unavailable("invalid_repository_catalog"))?;
        Some(public_git_url(group, slug)?)
    } else {
        None
    };
    Ok(Json(repository))
}

async fn check_availability(state: &AppState, repository: &mut CatalogRepository) {
    if repository.kind == "hosted" && repository.ready {
        let available = match repository.storage_name.as_deref() {
            Some(storage) if crate::git_host::validate_repo_name(storage).is_ok() => {
                tokio::fs::try_exists(state.git.root.join(format!("{storage}.git/HEAD")))
                    .await
                    .unwrap_or(false)
            }
            _ => false,
        };
        if !available {
            repository.availability = Some("storage_unavailable_restore_required".into());
        }
    }
}
#[utoipa::path(get,operation_id="forge_repository_catalog_pulls",path="/api/v1/catalog/repositories/{id}/pulls",tag="pulls",params(("id"=Uuid,Path)),responses((status=200,body=Vec<crate::pulls::PullRequest>)))]
pub async fn pulls(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<crate::pulls::PullRequest>>, ApiError> {
    let storage = storage_by_id(state.pool.as_ref().ok_or_else(ApiError::unavailable)?, id).await?;
    crate::pulls::list_pull_requests(State(state), Path(storage)).await
}
#[utoipa::path(get,operation_id="forge_repository_catalog_pull",path="/api/v1/catalog/repositories/{id}/pulls/{number}",tag="pulls",params(("id"=Uuid,Path),("number"=i32,Path)),responses((status=200,body=crate::pulls::PullRequest)))]
pub async fn pull(
    State(state): State<Arc<AppState>>,
    Path((id, number)): Path<(Uuid, i32)>,
) -> Result<Json<crate::pulls::PullRequest>, ApiError> {
    let storage = storage_by_id(state.pool.as_ref().ok_or_else(ApiError::unavailable)?, id).await?;
    crate::pulls::get_pull_request(State(state), Path((storage, number))).await
}
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreatePull {
    pub title: String,
    pub description: Option<String>,
    pub source_branch: String,
    pub target_branch: String,
}
#[utoipa::path(post,operation_id="forge_repository_catalog_create_pull",path="/api/v1/catalog/repositories/{id}/pulls",tag="pulls",params(("id"=Uuid,Path)),request_body=CreatePull,responses((status=200,body=crate::pulls::PullRequest)))]
pub async fn create_pull(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    claims: Option<axum::Extension<crate::auth::AccessClaims>>,
    Json(input): Json<CreatePull>,
) -> Result<Json<crate::pulls::PullRequest>, ApiError> {
    let storage = storage_by_id(state.pool.as_ref().ok_or_else(ApiError::unavailable)?, id).await?;
    crate::pulls::create_pull_request(
        State(state),
        Path(storage.clone()),
        claims,
        Json(crate::pulls::CreatePullRequest {
            repository_name: storage,
            title: input.title,
            description: input.description,
            source_branch: input.source_branch,
            target_branch: input.target_branch,
            author: None,
        }),
    )
    .await
}
#[utoipa::path(post,operation_id="forge_repository_catalog_pull_action",path="/api/v1/catalog/repositories/{id}/pulls/{number}/action",tag="pulls",params(("id"=Uuid,Path),("number"=i32,Path)),request_body=crate::pulls::PrAction,responses((status=200,body=crate::pulls::PullRequest)))]
pub async fn pull_action(
    State(state): State<Arc<AppState>>,
    Path((id, number)): Path<(Uuid, i32)>,
    Json(input): Json<crate::pulls::PrAction>,
) -> Result<Json<crate::pulls::PullRequest>, ApiError> {
    let storage = storage_by_id(state.pool.as_ref().ok_or_else(ApiError::unavailable)?, id).await?;
    crate::pulls::pr_action(State(state), Path((storage, number)), Json(input)).await
}
