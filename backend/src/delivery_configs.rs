//! CI configurations retain legacy project IDs; repository identity owns checkout.
use crate::api::{ApiError, AppState};
use axum::{
    Extension, Json,
    extract::{Path, Query, State},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct DeliveryConfig {
    pub id: Uuid,
    pub name: String,
    pub repository_id: Option<Uuid>,
    pub repository_url: String,
    pub default_branch: String,
    pub max_running_jobs: Option<i32>,
    pub created_at: DateTime<Utc>,
}
#[derive(Serialize, utoipa::ToSchema)]
pub struct ConfigPage {
    pub items: Vec<DeliveryConfig>,
    pub total: i64,
}
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateConfig {
    pub operation_id: Uuid,
    pub name: String,
    pub default_branch: String,
}
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PushConfig {
    pub configuration_id: Option<Uuid>,
}
#[derive(Default, Deserialize, utoipa::IntoParams)]
pub struct ConfigQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub search: Option<String>,
    pub unbound: Option<bool>,
    pub registry_instance_id: Option<Uuid>,
    pub namespace_id: Option<Uuid>,
}
/// Capture the push decision before execution; retry keeps that choice even after a policy edit.
pub async fn push_decision(
    pool: &sqlx::PgPool,
    repository: Uuid,
    key: Option<&str>,
) -> Result<Option<Uuid>, ApiError> {
    if let Some(key) = key {
        sqlx::query("INSERT INTO repository_push_operations(repository_id,trigger_key,configuration_id) VALUES($1,$2,(SELECT configuration_id FROM repository_push_configs WHERE repository_id=$1)) ON CONFLICT DO NOTHING")
            .bind(repository).bind(key).execute(pool).await.map_err(ApiError::internal)?;
        return sqlx::query_scalar("SELECT configuration_id FROM repository_push_operations WHERE repository_id=$1 AND trigger_key=$2")
            .bind(repository).bind(key).fetch_one(pool).await.map_err(ApiError::internal);
    }
    sqlx::query_scalar(
        "SELECT configuration_id FROM repository_push_configs WHERE repository_id=$1",
    )
    .bind(repository)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)
    .map(Option::flatten)
}
pub async fn configuration_visible(
    pool: &sqlx::PgPool,
    id: Uuid,
    claims: &crate::auth::AccessClaims,
) -> Result<bool, ApiError> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM projects p WHERE p.id=$1 AND ($2 OR EXISTS(SELECT 1 FROM project_memberships m WHERE m.project_id=p.id AND m.user_id=$3) OR EXISTS(SELECT 1 FROM tenant_memberships tm JOIN tenants t ON t.id=tm.tenant_id WHERE tm.tenant_id=p.tenant_id AND tm.user_id=$3 AND t.status='active') OR EXISTS(SELECT 1 FROM repository_catalog r JOIN forge_namespace_bindings b ON b.resource_id=r.group_id WHERE r.id=p.repository_id)))").bind(id).bind(claims.role=="admin").bind(claims.sub).fetch_one(pool).await.map_err(ApiError::internal)
}
#[utoipa::path(get,operation_id="forge_delivery_get",path="/api/v1/delivery-configurations/{id}",tag="delivery-configurations",params(("id"=Uuid,Path)),responses((status=200,body=DeliveryConfig)))]
pub async fn get(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Extension(claims): Extension<crate::auth::AccessClaims>,
) -> Result<Json<DeliveryConfig>, ApiError> {
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    if !configuration_visible(pool, id, &claims).await? {
        return Err(ApiError::not_found());
    }
    sqlx::query_as("SELECT id,name,repository_id,repository_url,default_branch,max_running_jobs,created_at FROM projects WHERE id=$1")
        .bind(id).fetch_optional(pool).await.map_err(ApiError::internal)?.map(Json).ok_or_else(ApiError::not_found)
}
#[utoipa::path(get,operation_id="forge_delivery_readback",path="/api/v1/catalog/repositories/{id}/delivery-config-operations/{operation}",tag="delivery-configurations",params(("id"=Uuid,Path),("operation"=Uuid,Path)),responses((status=200,body=DeliveryConfig),(status=404,description="Original operation not found")))]
pub async fn readback(
    State(state): State<Arc<AppState>>,
    Path((id, operation)): Path<(Uuid, Uuid)>,
    Extension(claims): Extension<crate::auth::AccessClaims>,
) -> Result<Json<DeliveryConfig>, ApiError> {
    let value:serde_json::Value=sqlx::query_scalar("SELECT readback FROM delivery_configuration_operations WHERE operation_id=$1 AND repository_id=$2 AND actor_id=$3").bind(operation).bind(id).bind(claims.sub).fetch_optional(state.pool.as_ref().ok_or_else(ApiError::unavailable)?).await.map_err(ApiError::internal)?.ok_or_else(ApiError::not_found)?;
    Ok(Json(serde_json::from_value(value).map_err(|_| {
        ApiError::service_unavailable("invalid_delivery_readback")
    })?))
}
async fn read_configs(
    pool: &sqlx::PgPool,
    repo: Option<Uuid>,
    query: ConfigQuery,
    claims: Option<&crate::auth::AccessClaims>,
) -> Result<ConfigPage, ApiError> {
    let user = claims.map(|c| c.sub);
    let admin = claims.is_some_and(|c| c.role == "admin");
    if !query.unbound.unwrap_or(false) {
        crate::workspace_projects::validate_catalog(pool).await?;
    }
    if query.registry_instance_id.is_some() != query.namespace_id.is_some() {
        return Err(ApiError::bad_request("invalid_namespace_ref"));
    }
    let filter = " WHERE ($1::uuid IS NULL OR p.repository_id=$1) AND (NOT $2 OR p.repository_id IS NULL) AND position(lower($3) in lower(p.name))>0 AND ($4 OR EXISTS(SELECT 1 FROM project_memberships m WHERE m.project_id=p.id AND m.user_id=$5) OR EXISTS(SELECT 1 FROM tenant_memberships tm JOIN tenants t ON t.id=tm.tenant_id WHERE tm.tenant_id=p.tenant_id AND tm.user_id=$5 AND t.status='active') OR EXISTS(SELECT 1 FROM repository_catalog r JOIN forge_namespace_bindings b ON b.resource_id=r.group_id WHERE r.id=p.repository_id))";
    let search = query.search.unwrap_or_default();
    let unbound = query.unbound.unwrap_or(false);
    let filter = format!(
        "{filter} AND ($6::uuid IS NULL OR EXISTS(SELECT 1 FROM repository_catalog rc JOIN forge_namespace_bindings nb ON nb.resource_id=rc.group_id WHERE rc.id=p.repository_id AND nb.registry_instance_id=$6 AND nb.namespace_id=$7))"
    );
    let total = sqlx::query_scalar(&format!("SELECT count(*) FROM projects p{filter}"))
        .bind(repo)
        .bind(unbound)
        .bind(&search)
        .bind(admin)
        .bind(user)
        .bind(query.registry_instance_id)
        .bind(query.namespace_id)
        .fetch_one(pool)
        .await
        .map_err(ApiError::internal)?;
    let sql = format!(
        "SELECT p.id,p.name,p.repository_id,p.repository_url,p.default_branch,p.max_running_jobs,p.created_at FROM projects p{filter} ORDER BY p.name,p.id LIMIT $8 OFFSET $9"
    );
    let items = sqlx::query_as(&sql)
        .bind(repo)
        .bind(unbound)
        .bind(search)
        .bind(admin)
        .bind(user)
        .bind(query.registry_instance_id)
        .bind(query.namespace_id)
        .bind(query.limit.unwrap_or(50).clamp(1, 100))
        .bind(query.offset.unwrap_or(0).max(0))
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)?;
    Ok(ConfigPage { items, total })
}
#[utoipa::path(get,operation_id="forge_delivery_unbound",path="/api/v1/delivery-configurations",tag="delivery-configurations",params(ConfigQuery),responses((status=200,body=ConfigPage)))]
pub async fn unbound(
    State(state): State<Arc<AppState>>,
    claims: Option<Extension<crate::auth::AccessClaims>>,
    Query(query): Query<ConfigQuery>,
) -> Result<Json<ConfigPage>, ApiError> {
    read_configs(
        state.pool.as_ref().ok_or_else(ApiError::unavailable)?,
        None,
        query,
        claims.as_ref().map(|c| &c.0),
    )
    .await
    .map(Json)
}
#[utoipa::path(get,operation_id="forge_delivery_list",path="/api/v1/catalog/repositories/{id}/delivery-configs",tag="delivery-configurations",params(("id"=Uuid,Path),ConfigQuery),responses((status=200,body=ConfigPage)))]
pub async fn list(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    claims: Option<Extension<crate::auth::AccessClaims>>,
    Query(query): Query<ConfigQuery>,
) -> Result<Json<ConfigPage>, ApiError> {
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    read_configs(pool, Some(id), query, claims.as_ref().map(|c| &c.0))
        .await
        .map(Json)
}
#[utoipa::path(post,operation_id="forge_delivery_create",path="/api/v1/catalog/repositories/{id}/delivery-configs",tag="delivery-configurations",params(("id"=Uuid,Path)),request_body=CreateConfig,responses((status=200,body=DeliveryConfig),(status=409,description="Original operation conflict")))]
pub async fn create(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Extension(claims): Extension<crate::auth::AccessClaims>,
    Json(input): Json<CreateConfig>,
) -> Result<Json<DeliveryConfig>, ApiError> {
    if input.operation_id.is_nil()
        || input.name.trim().is_empty()
        || input.name.len() > 255
        || input.default_branch.trim().is_empty()
        || input.default_branch.len() > 255
    {
        return Err(ApiError::bad_request("invalid_delivery_configuration"));
    }
    let payload = serde_json::to_value(&input)
        .map_err(|_| ApiError::bad_request("invalid_delivery_configuration"))?;
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    if let Some(row)=sqlx::query("SELECT repository_id,actor_id,payload,readback FROM delivery_configuration_operations WHERE operation_id=$1").bind(input.operation_id).fetch_optional(pool).await.map_err(ApiError::internal)? {
  if row.get::<Uuid,_>("repository_id")!=id || row.get::<Uuid,_>("actor_id")!=claims.sub || row.get::<serde_json::Value,_>("payload")!=payload{return Err(ApiError::conflict("delivery_operation_payload_conflict"));}
  return Ok(Json(serde_json::from_value(row.get("readback")).map_err(|_|ApiError::service_unavailable("invalid_delivery_readback"))?));
 }
    let mut tx = crate::repository_catalog::catalog_write_lease(&state, id).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,5))")
        .bind(input.operation_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(ApiError::internal)?;
    if let Some(row)=sqlx::query("SELECT repository_id,actor_id,payload,readback FROM delivery_configuration_operations WHERE operation_id=$1").bind(input.operation_id).fetch_optional(&mut *tx).await.map_err(ApiError::internal)?{
  if row.get::<Uuid,_>("repository_id")!=id || row.get::<Uuid,_>("actor_id")!=claims.sub || row.get::<serde_json::Value,_>("payload")!=payload{return Err(ApiError::conflict("delivery_operation_payload_conflict"));}
  return Ok(Json(serde_json::from_value(row.get("readback")).map_err(|_|ApiError::service_unavailable("invalid_delivery_readback"))?));
 }
    let row=sqlx::query("SELECT r.kind,r.external_url,r.slug,g.slug AS group_slug FROM repository_catalog r JOIN git_groups g ON g.id=r.group_id WHERE r.id=$1 AND r.ready").bind(id).fetch_optional(&mut *tx).await.map_err(ApiError::internal)?.ok_or_else(||ApiError::conflict("repository_not_ready_or_attached"))?;
    let url = if row.get::<String, _>("kind") == "external" {
        row.get::<String, _>("external_url")
    } else {
        crate::repository_catalog::public_git_url(
            &row.get::<String, _>("group_slug"),
            &row.get::<String, _>("slug"),
        )?
    };
    let configuration = Uuid::new_v4();
    let result:DeliveryConfig=sqlx::query_as("INSERT INTO projects(id,name,repository_id,repository_url,default_branch) VALUES($1,$2,$3,$4,$5) RETURNING id,name,repository_id,repository_url,default_branch,max_running_jobs,created_at").bind(configuration).bind(input.name.trim()).bind(id).bind(url).bind(input.default_branch.trim()).fetch_one(&mut *tx).await.map_err(ApiError::internal)?;
    sqlx::query("INSERT INTO project_memberships(project_id,user_id,role) VALUES($1,$2,'maintainer') ON CONFLICT DO NOTHING").bind(configuration).bind(claims.sub).execute(&mut *tx).await.map_err(ApiError::internal)?;
    sqlx::query("INSERT INTO delivery_configuration_operations(operation_id,repository_id,actor_id,payload,configuration_id,readback) VALUES($1,$2,$3,$4,$5,$6)").bind(input.operation_id).bind(id).bind(claims.sub).bind(payload).bind(configuration).bind(serde_json::to_value(&result).map_err(|_|ApiError::bad_request("invalid_delivery_configuration"))?).execute(&mut *tx).await.map_err(ApiError::internal)?;
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(result))
}
#[utoipa::path(get,operation_id="forge_delivery_push_get",path="/api/v1/catalog/repositories/{id}/push-config",tag="delivery-configurations",params(("id"=Uuid,Path)),responses((status=200,body=PushConfig)))]
pub async fn push_get(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<PushConfig>, ApiError> {
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    let configuration_id = sqlx::query_scalar(
        "SELECT configuration_id FROM repository_push_configs WHERE repository_id=$1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?
    .flatten();
    Ok(Json(PushConfig { configuration_id }))
}
#[utoipa::path(put,operation_id="forge_delivery_push_put",path="/api/v1/catalog/repositories/{id}/push-config",tag="delivery-configurations",params(("id"=Uuid,Path)),request_body=PushConfig,responses((status=200,body=PushConfig)))]
pub async fn push_put(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(input): Json<PushConfig>,
) -> Result<Json<PushConfig>, ApiError> {
    let mut tx = crate::repository_catalog::catalog_write_lease(&state, id).await?;
    if let Some(config) = input.configuration_id {
        let matches: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE id=$1 AND repository_id=$2)",
        )
        .bind(config)
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .map_err(ApiError::internal)?;
        if !matches {
            return Err(ApiError::conflict("push_configuration_repository_mismatch"));
        }
    }
    sqlx::query("INSERT INTO repository_push_configs(repository_id,configuration_id) VALUES($1,$2) ON CONFLICT(repository_id) DO UPDATE SET configuration_id=EXCLUDED.configuration_id").bind(id).bind(input.configuration_id).execute(&mut *tx).await.map_err(ApiError::internal)?;
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(input))
}
