//! Admin-owned Space barrier, with optional ID-preserving legacy Git group mapping.
use crate::api::{ApiError, AppState};
use axum::{
    Extension, Json, Router,
    extract::{Path, Request, State},
    middleware::Next,
    response::Response,
};
use pdlc_shared::{
    resource_context::{ResourceKind, ResourceRef},
    space_access::{AdmissionValidity, CentralIdentity, SpaceRef},
    space_lifecycle::{
        DesiredSpaceState, SpaceOwnerAuthority, SpaceOwnerCommandV2, SpaceOwnerReadbackV2,
    },
};
use sqlx::{PgPool, Row};
use std::sync::Arc;
use uuid::Uuid;

fn current(authority: &SpaceOwnerAuthority) -> Result<(), ApiError> {
    if authority.is_current() {
        Ok(())
    } else {
        Err(ApiError::unauthorized())
    }
}
fn configured(key: &str) -> Result<Uuid, ApiError> {
    std::env::var(key)
        .ok()
        .and_then(|s| s.parse::<Uuid>().ok())
        .filter(|id| !id.is_nil())
        .ok_or_else(|| ApiError::service_unavailable("space_owner_not_configured"))
}
fn database(error: sqlx::Error) -> ApiError {
    match error.as_database_error().and_then(|e| e.code()).as_deref() {
        Some("23505" | "23514" | "42501" | "40001" | "40P01") => {
            ApiError::conflict("space_owner_state_conflict")
        }
        _ => ApiError::internal(error),
    }
}
fn value<T: serde::Serialize>(value: &T) -> Result<serde_json::Value, ApiError> {
    serde_json::to_value(value)
        .map_err(|_| ApiError::service_unavailable("invalid_space_owner_value"))
}
fn parse<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T, ApiError> {
    serde_json::from_value(value)
        .map_err(|_| ApiError::service_unavailable("invalid_space_owner_receipt"))
}
async fn owner_auth(mut req: Request, next: Next) -> Result<Response, ApiError> {
    let token = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(ApiError::unauthorized)?;
    let verified = crate::central_auth::try_verified_central(token)
        .await?
        .ok_or_else(ApiError::unauthorized)?;
    let ctx = &verified.context;
    let registered = std::env::var("CICD_NAMESPACE__OWNER_SUBJECTS")
        .unwrap_or_default()
        .split(',')
        .any(|s| !s.trim().is_empty() && s.trim() == ctx.user_id);
    if verified.subject_role.as_deref() != Some("service_account")
        || !pdlc_auth_core::is_personal_token(token)
        || ctx.session_id.is_some()
        || !registered
        || !ctx.has_scope("ci-cd:write")
        || crate::delegated_credentials::has_grant(ctx)
    {
        return Err(ApiError::forbidden());
    }
    let validity = AdmissionValidity::confirmed(
        verified.lifetime.source_observed_at,
        verified.lifetime.local_valid_until(),
        i64::MAX,
    )
    .ok_or_else(ApiError::unauthorized)?;
    let proof = SpaceOwnerAuthority::confirmed(
        CentralIdentity {
            issuer: verified.issuer,
            subject: verified.context.user_id,
        },
        validity,
    )
    .ok_or_else(ApiError::unauthorized)?
    .restrict_deadline(verified.lifetime.monotonic_deadline());
    current(&proof)?;
    req.extensions_mut().insert(proof);
    Ok(next.run(req).await)
}

async fn apply_on(
    pool: &PgPool,
    command: &SpaceOwnerCommandV2,
    authority: &SpaceOwnerAuthority,
) -> Result<SpaceOwnerReadbackV2, ApiError> {
    current(authority)?;
    let registry = configured("CICD_NAMESPACE__REGISTRY_INSTANCE_ID")?;
    let instance = configured("CICD_NAMESPACE__INSTANCE_ID")?;
    if !command.valid_for(registry, instance) {
        return Err(ApiError::bad_request("invalid_space_owner_v2_command"));
    }
    let mut tx = pool.begin().await.map_err(database)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("space-owner-operation:{}", command.operation_id))
        .execute(&mut *tx)
        .await
        .map_err(database)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!(
            "shared-space:{registry}:{}",
            command.space.space_id
        ))
        .execute(&mut *tx)
        .await
        .map_err(database)?;
    current(authority)?;
    let mut replay = false;
    if let Some(row) = sqlx::query(
        "SELECT command,readback FROM forge_space_owner_operations WHERE operation_id=$1",
    )
    .bind(command.operation_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(database)?
    {
        let old: SpaceOwnerCommandV2 = parse(row.try_get("command").map_err(database)?)?;
        let ack: SpaceOwnerReadbackV2 = parse(row.try_get("readback").map_err(database)?)?;
        let mut checked = ack.clone();
        checked.drained = true;
        if old != *command
            || !checked.matches(command)
            || ack
                .local_resource
                .as_ref()
                .is_some_and(|r| r.kind != ResourceKind::GitGroup)
        {
            return Err(ApiError::conflict("space_operation_payload_conflict"));
        }
        if ack.matches(command) {
            current(authority)?;
            tx.commit().await.map_err(database)?;
            return Ok(ack);
        }
        replay = true;
    }
    // The shared ID lock excludes legacy apply/attach. A v2 Space without a
    // legacy group remains a barrier; human labels never discover a resource.
    let legacy=sqlx::query("SELECT resource_id,command FROM forge_namespace_bindings WHERE registry_instance_id=$1 AND namespace_id=$2")
        .bind(registry).bind(command.space.space_id).fetch_optional(&mut *tx).await.map_err(database)?;
    let (group, legacy_archived): (Option<Uuid>, bool) = if let Some(row) = legacy {
        let binding: pdlc_shared::resource_context::OwnerCommand =
            parse(row.try_get("command").map_err(database)?)?;
        crate::namespace::validate_projection(&binding)?;
        let group: Uuid = row.try_get("resource_id").map_err(database)?;
        if binding.resource.resource_id != group
            || binding.namespace.registry_instance_id != registry
            || binding.namespace.namespace_id != command.space.space_id
        {
            return Err(ApiError::conflict("invalid_legacy_space_mapping"));
        }
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(group.to_string())
            .execute(&mut *tx)
            .await
            .map_err(database)?;
        sqlx::query(
            "SELECT resource_id FROM forge_namespace_bindings WHERE resource_id=$1 FOR UPDATE",
        )
        .bind(group)
        .fetch_one(&mut *tx)
        .await
        .map_err(database)?;
        (Some(group), binding.state == "archived")
    } else {
        (None, false)
    };
    current(authority)?;
    let existing=sqlx::query("SELECT group_id,command FROM forge_space_catalog WHERE registry_instance_id=$1 AND space_id=$2 FOR UPDATE")
        .bind(registry).bind(command.space.space_id).fetch_optional(&mut *tx).await.map_err(database)?;
    if let Some(row) = existing {
        let previous: SpaceOwnerCommandV2 = parse(row.try_get("command").map_err(database)?)?;
        let mapped: Option<Uuid> = row.try_get("group_id").map_err(database)?;
        if mapped != group
            || !command.follows(&previous)
            || (replay && previous.operation_id != command.operation_id)
        {
            return Err(ApiError::conflict("space_owner_revision_conflict"));
        }
        if previous.operation_id != command.operation_id {
            let receipt: serde_json::Value = sqlx::query_scalar(
                "SELECT readback FROM forge_space_owner_operations WHERE operation_id=$1",
            )
            .bind(previous.operation_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(database)?;
            let receipt: SpaceOwnerReadbackV2 = parse(receipt)?;
            if !receipt.matches(&previous) {
                return Err(ApiError::conflict("space_owner_operation_pending"));
            }
        }
    } else {
        if command.desired_state != DesiredSpaceState::Active {
            return Err(ApiError::conflict("space_catalog_entry_required"));
        }
        if legacy_archived
            && let Some(group) = group
            && !crate::namespace::confirmed_group_drain_on(&mut *tx, group).await?
        {
            return Err(ApiError::conflict("legacy_archive_drain_unconfirmed"));
        }
        sqlx::query("INSERT INTO forge_space_catalog(registry_instance_id,space_id,group_id,revision,state,operation_id,command) VALUES($1,$2,$3,$4,'active',$5,$6)")
            .bind(registry).bind(command.space.space_id).bind(group).bind(command.revision).bind(command.operation_id).bind(value(command)?)
            .execute(&mut *tx).await.map_err(database)?;
    }
    current(authority)?;
    let archived = command.desired_state == DesiredSpaceState::Archived;
    let state = if archived { "archiving" } else { "active" };
    sqlx::query("UPDATE forge_space_catalog SET revision=$3,state=$4,operation_id=$5,command=$6 WHERE registry_instance_id=$1 AND space_id=$2")
        .bind(registry).bind(command.space.space_id).bind(command.revision).bind(state).bind(command.operation_id).bind(value(command)?)
        .execute(&mut *tx).await.map_err(database)?;
    if let Some(group) = group {
        sqlx::query("SET LOCAL pdlc.forge.space_owner='admin_v2'")
            .execute(&mut *tx)
            .await
            .map_err(database)?;
        sqlx::query("UPDATE git_groups SET name=$2 WHERE id=$1")
            .bind(group)
            .bind(&command.metadata.name)
            .execute(&mut *tx)
            .await
            .map_err(database)?;
    }
    let drained = if !archived {
        true
    } else if let Some(group) = group {
        crate::namespace::confirmed_group_drain_on(&mut *tx, group).await?
    } else {
        true
    };
    if archived && drained {
        sqlx::query("UPDATE forge_space_catalog SET state='archived' WHERE registry_instance_id=$1 AND space_id=$2")
            .bind(registry).bind(command.space.space_id).execute(&mut *tx).await.map_err(database)?;
    }
    let ack = SpaceOwnerReadbackV2 {
        schema_version: 2,
        space: command.space.clone(),
        participant_instance_id: instance,
        operation_id: command.operation_id,
        revision: command.revision,
        state: command.desired_state,
        drained,
        local_resource: group.map(|resource_id| ResourceRef {
            kind: ResourceKind::GitGroup,
            instance_id: instance,
            resource_id,
        }),
    };
    if replay {
        sqlx::query("UPDATE forge_space_owner_operations SET readback=$2 WHERE operation_id=$1")
            .bind(command.operation_id)
            .bind(value(&ack)?)
            .execute(&mut *tx)
            .await
            .map_err(database)?;
    } else {
        sqlx::query("INSERT INTO forge_space_owner_operations(operation_id,registry_instance_id,space_id,revision,command,readback,owner_issuer,owner_subject) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(command.operation_id).bind(registry).bind(command.space.space_id).bind(command.revision).bind(value(command)?).bind(value(&ack)?).bind(&authority.identity().issuer).bind(&authority.identity().subject)
            .execute(&mut *tx).await.map_err(database)?;
    }
    current(authority)?;
    tx.commit().await.map_err(database)?;
    Ok(ack)
}

#[utoipa::path(put,operation_id="forge_space_owner_v2_apply",path="/api/v2/spaces/{registry}/{space}/owner-state",tag="spaces-v2",params(("registry"=Uuid,Path),("space"=Uuid,Path)),request_body=SpaceOwnerCommandV2,responses((status=200,body=SpaceOwnerReadbackV2),(status=403,description="Registered service account and exact ci-cd write scope required"),(status=409,description="Original mapping, operation or revision conflict")))]
pub(crate) async fn apply(
    State(state): State<Arc<AppState>>,
    Extension(authority): Extension<SpaceOwnerAuthority>,
    Path((registry, space)): Path<(Uuid, Uuid)>,
    Json(command): Json<SpaceOwnerCommandV2>,
) -> Result<Json<SpaceOwnerReadbackV2>, ApiError> {
    if command.space
        != (SpaceRef {
            registry_instance_id: registry,
            space_id: space,
        })
    {
        return Err(ApiError::bad_request("space_owner_path_mismatch"));
    }
    let pool = state
        .namespace_admission_pool
        .as_ref()
        .ok_or_else(ApiError::unavailable)?;
    Ok(Json(apply_on(pool, &command, &authority).await?))
}
#[utoipa::path(get,operation_id="forge_space_owner_v2_readback",path="/api/v2/spaces/{registry}/{space}/owner-state",tag="spaces-v2",params(("registry"=Uuid,Path),("space"=Uuid,Path)),responses((status=200,body=SpaceOwnerReadbackV2),(status=404,description="No current v2 operation")))]
pub(crate) async fn readback(
    State(state): State<Arc<AppState>>,
    Extension(authority): Extension<SpaceOwnerAuthority>,
    Path((registry, space)): Path<(Uuid, Uuid)>,
) -> Result<Json<SpaceOwnerReadbackV2>, ApiError> {
    current(&authority)?;
    if registry != configured("CICD_NAMESPACE__REGISTRY_INSTANCE_ID")? || space.is_nil() {
        return Err(ApiError::bad_request("invalid_space_ref"));
    }
    let pool = state
        .namespace_admission_pool
        .as_ref()
        .ok_or_else(ApiError::unavailable)?;
    let row=sqlx::query("SELECT c.command,o.readback FROM forge_space_catalog c JOIN forge_space_owner_operations o ON o.operation_id=c.operation_id WHERE c.registry_instance_id=$1 AND c.space_id=$2")
        .bind(registry).bind(space).fetch_optional(pool).await.map_err(database)?.ok_or_else(ApiError::not_found)?;
    let command: SpaceOwnerCommandV2 = parse(row.try_get("command").map_err(database)?)?;
    let ack: SpaceOwnerReadbackV2 = parse(row.try_get("readback").map_err(database)?)?;
    let mut checked = ack.clone();
    checked.drained = true;
    if !command.valid_for(registry, configured("CICD_NAMESPACE__INSTANCE_ID")?)
        || !checked.matches(&command)
        || command.space.space_id != space
        || ack
            .local_resource
            .as_ref()
            .is_some_and(|r| r.kind != ResourceKind::GitGroup)
    {
        return Err(ApiError::service_unavailable("invalid_space_owner_receipt"));
    }
    current(&authority)?;
    Ok(Json(ack))
}
pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/api/v2/spaces/{registry}/{space}/owner-state",
            axum::routing::get(readback).put(apply),
        )
        .route_layer(axum::middleware::from_fn(owner_auth))
}
