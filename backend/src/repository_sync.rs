//! Thin HTTP/composition adapter over synchronization application and infra ports.
use crate::api::{ApiError, AppState};
use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use cicd_app::repository_sync::{self as app, Access, Error};
use cicd_domain::repository_sync::*;
use cicd_infra::{
    repository_sync::PgSyncStore,
    secrets::Secrets,
    sync_git::{GitSession, MutationGuard},
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, atomic::AtomicBool};
use uuid::Uuid;
#[path = "repository_sync_worker.rs"]
mod worker;
pub use worker::supervisor;

#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub struct ConnectionPage {
    pub items: Vec<Connection>,
    pub total: i64,
}
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub struct RunPage {
    pub items: Vec<SyncRun>,
    pub total: i64,
}
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub struct RefPage {
    pub items: Vec<RefResult>,
    pub total: i64,
}
#[derive(Default, Deserialize, utoipa::IntoParams)]
pub struct SyncQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub search: Option<String>,
    pub provider: Option<String>,
    pub state: Option<String>,
    pub group_id: Option<Uuid>,
    pub repository_id: Option<Uuid>,
    pub connection_id: Option<Uuid>,
}
pub(crate) fn api_error(error: Error) -> ApiError {
    match error {
        Error::Invalid(message) => ApiError::bad_request(message),
        Error::Conflict(message) => ApiError::conflict(message),
        Error::NotFound => ApiError::not_found(),
        Error::Forbidden => ApiError::forbidden(),
        Error::Unavailable(message) => ApiError::service_unavailable(message),
    }
}
pub(crate) fn store(state: &AppState) -> Result<PgSyncStore, ApiError> {
    Ok(PgSyncStore {
        pool: state.pool.clone().ok_or_else(ApiError::unavailable)?,
        worker_enabled: state.config.git_sync.enabled,
        interval_seconds: state.config.git_sync.interval_seconds,
    })
}
fn access(claims: &crate::auth::AccessClaims) -> Access {
    let role = crate::authz::Role::parse(&claims.role).unwrap_or(crate::authz::Role::Viewer);
    Access {
        actor_id: claims.sub,
        project_id: claims.token_project_id,
        admin: role == crate::authz::Role::Admin,
        machine: claims.role == "service_account",
        writable: role >= crate::authz::Role::Developer,
    }
}
async fn authorize(
    state: &Arc<AppState>,
    repository: Uuid,
    claims: &crate::auth::AccessClaims,
    write: bool,
    common_read: bool,
) -> Result<Access, ApiError> {
    let access = access(claims);
    if write || !common_read {
        store(state)?
            .authorize(repository, access, write)
            .await
            .map_err(api_error)?;
    }
    let repo = crate::repository_catalog::get_core(state, repository, None)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if write
        && (!repo.ready
            || repo.availability.is_some()
            || repo.state.as_deref().is_some_and(|s| s != "active"))
    {
        return Err(ApiError::conflict("repository_read_only"));
    }
    if write && !crate::api::bearer_token_has_scope(claims, "git:write") {
        return Err(ApiError::forbidden());
    }
    Ok(access)
}
fn operation(headers: &HeaderMap) -> Result<Uuid, ApiError> {
    headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<Uuid>().ok())
        .filter(|v| !v.is_nil())
        .ok_or_else(|| ApiError::bad_request("Нужен UUID Idempotency-Key"))
}
async fn writable_lease<'a>(
    state: &'a AppState,
    repository: Uuid,
    permit: Option<&crate::space_admission::RepositoryWrite>,
) -> Result<sqlx::Transaction<'a, sqlx::Postgres>, ApiError> {
    let mut lease = crate::repository_catalog::catalog_write_lease(state, repository).await?;
    if let Some(permit) = permit {
        permit.lock(&mut lease).await?;
    }
    Ok(lease)
}

#[utoipa::path(get,operation_id="forge_repository_sync_list",path="/api/v1/repository-syncs",tag="repository-sync",params(SyncQuery),responses((status=200,body=ConnectionPage),(status=403)))]
pub(crate) async fn list(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<crate::auth::AccessClaims>,
    Query(query): Query<SyncQuery>,
    identity: Option<Extension<crate::api::authz_mw::VerifiedRequestIdentity>>,
) -> Result<Response, ApiError> {
    let sync_store = store(&state)?;
    let admission = if claims.role == "service_account" {
        None
    } else {
        crate::space_admission::catalog_read(&sync_store.pool, identity.map(|value| value.0))
            .await?
    };
    let groups = admission.as_ref().map(|scope| scope.groups.as_slice());
    let provider = query.provider.as_deref().unwrap_or("");
    let status = query.state.as_deref().unwrap_or("");
    if !["", "gitlab", "github"].contains(&provider)
        || ![
            "",
            "enabled",
            "disabled",
            "queued",
            "running",
            "stopping",
            "reconciling",
            "success",
            "partial",
            "conflict",
            "failed",
            "canceled",
        ]
        .contains(&status)
    {
        return Err(ApiError::bad_request("Некорректный фильтр синхронизаций"));
    }
    let (mut items, total) = store(&state)?
        .list_admitted_at(
            access(&claims),
            query.search.as_deref().unwrap_or(""),
            provider,
            status,
            query.group_id,
            query.limit.unwrap_or(20),
            query.offset.unwrap_or(0),
            query.repository_id,
            groups,
        )
        .await
        .map_err(api_error)?;
    // Apply the owner's signed namespace projection validation, not just display data.
    for item in &mut items {
        let repo = crate::repository_catalog::get_core(
            &state,
            item.repository_id,
            groups.map(|value| value.to_vec()),
        )
        .await?
        .ok_or_else(ApiError::not_found)?;
        item.writable = item.writable
            && repo.availability.is_none()
            && store(&state)?
                .authorize(item.repository_id, access(&claims), true)
                .await
                .is_ok()
            && crate::api::bearer_token_has_scope(&claims, "git:write");
        if repo.availability.is_some() {
            item.paused_reason = Some("storage_unavailable".into());
        }
    }
    let response = Json(ConnectionPage { items, total }).into_response();
    if let Some(admission) = admission {
        admission.response(response)
    } else {
        Ok(response)
    }
}
#[utoipa::path(get,operation_id="forge_repository_sync_get",path="/api/v1/catalog/repositories/{id}/sync",tag="repository-sync",params(("id"=Uuid,Path),SyncQuery),responses((status=200,body=Connection),(status=403)))]
pub(crate) async fn get_connection(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<crate::auth::AccessClaims>,
    common: Option<Extension<crate::space_admission::RepositoryRead>>,
    Path(id): Path<Uuid>,
    Query(query): Query<SyncQuery>,
) -> Result<Json<Connection>, ApiError> {
    let access = authorize(&state, id, &claims, false, common.is_some()).await?;
    let repo = crate::repository_catalog::get_core(&state, id, None)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let sync_store = store(&state)?;
    let mut connection = sync_store
        .connection_at(id, query.connection_id.unwrap_or(id), access)
        .await
        .map_err(api_error)?
        .unwrap_or(Connection {
            id: query.connection_id.unwrap_or(id),
            repository_id: id,
            repository_name: repo.public_name,
            namespace: repo.namespace.map(|reference| RepositoryNamespace {
                registry_instance_id: reference.registry_instance_id,
                namespace_id: reference.namespace_id,
            }),
            version: 0,
            settings: Settings {
                provider: Provider::Github,
                instance_url: "https://github.com".into(),
                repository_path: String::new(),
                direction: Direction::Disabled,
                automatic: false,
            },
            has_credential: false,
            writable: false,
            paused_reason: None,
            worker_enabled: state.config.git_sync.enabled,
            next_check_at: None,
            active_run: None,
            last_run: None,
        });
    connection.writable = sync_store.authorize(id, access, true).await.is_ok()
        && crate::api::bearer_token_has_scope(&claims, "git:write")
        && repo.ready
        && repo.availability.is_none()
        && repo.state.as_deref().is_none_or(|s| s == "active");
    if repo.availability.is_some() {
        connection.paused_reason = Some("storage_unavailable".into());
    }
    Ok(Json(connection))
}

#[utoipa::path(put,operation_id="forge_repository_sync_save",path="/api/v1/catalog/repositories/{id}/sync",tag="repository-sync",params(("id"=Uuid,Path),("Idempotency-Key"=Uuid,Header),SyncQuery),request_body=SaveConnection,responses((status=200,body=Connection),(status=409)))]
pub(crate) async fn save_connection(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<crate::auth::AccessClaims>,
    permit: Option<Extension<crate::space_admission::RepositoryWrite>>,
    Path(id): Path<Uuid>,
    Query(query): Query<SyncQuery>,
    headers: HeaderMap,
    Json(input): Json<SaveConnection>,
) -> Result<Json<Connection>, ApiError> {
    let access = authorize(&state, id, &claims, true, false).await?;
    let _lease = writable_lease(&state, id, permit.as_ref().map(|value| &value.0)).await?;
    let sync_store = store(&state)?;
    let result = app::save(
        &sync_store
            .scoped(query.connection_id.unwrap_or(id))
            .with_owner(crate::pipeline_authority::current()?.map(|origin| origin.subject())),
        &Secrets(state.config.secrets.key),
        id,
        operation(&headers)?,
        access,
        input,
        &state.config.git_sync.allowed_gitlab_origins,
    )
    .await
    .map_err(api_error)?;
    Ok(Json(result))
}
#[utoipa::path(post,operation_id="forge_repository_sync_check",path="/api/v1/catalog/repositories/{id}/sync/check",tag="repository-sync",params(("id"=Uuid,Path),SyncQuery),request_body=CheckConnection,responses((status=200,body=CheckResult),(status=503)))]
pub(crate) async fn check(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<crate::auth::AccessClaims>,
    permit: Option<Extension<crate::space_admission::RepositoryWrite>>,
    Path(id): Path<Uuid>,
    Query(query): Query<SyncQuery>,
    Json(mut input): Json<CheckConnection>,
) -> Result<Json<CheckResult>, ApiError> {
    authorize(&state, id, &claims, true, false).await?;
    input
        .settings
        .validate(&state.config.git_sync.allowed_gitlab_origins)
        .map_err(ApiError::bad_request)?;
    let sync_store = store(&state)?;
    let token = app::credential(
        &sync_store.scoped(query.connection_id.unwrap_or(id)),
        &Secrets(state.config.secrets.key),
        id,
        &input.settings,
        input.token.as_deref(),
    )
    .await
    .map_err(api_error)?;
    let storage = crate::repository_catalog::storage_by_id(
        state.pool.as_ref().ok_or_else(ApiError::unavailable)?,
        id,
    )
    .await?;
    let _lease = writable_lease(&state, id, permit.as_ref().map(|value| &value.0)).await?;
    let guard = Arc::new(
        MutationGuard::acquire(&state.git.root, id)
            .await
            .map_err(api_error)?,
    );
    crate::space_admission::require_write(permit.as_ref().map(|value| &value.0))?;
    let write = matches!(input.settings.direction, Direction::Push | Direction::Both);
    // Checks have no durable run UUID. Scope their scratch to the repository so
    // the next check can recover it after an API crash, under the same OS guard.
    let check_root = state.git.root.join(".forge-sync-checks");
    GitSession::clean_recovered_scratch(&check_root, id).map_err(api_error)?;
    let canceled = Arc::new(AtomicBool::new(false));
    let mut git = GitSession::new(
        &check_root,
        state.git.root.join(format!("{storage}.git")),
        input.settings,
        token,
        id,
        0,
        canceled.clone(),
    )
    .await
    .map_err(api_error)?;
    git.guard = Some(guard);
    let action = git.check(write);
    tokio::pin!(action);
    let result = if let Some(permit) = permit.as_ref() {
        tokio::select! {
            result = &mut action => result.map_err(api_error)?,
            _ = tokio::time::sleep_until(permit.proof().monotonic_deadline().into()) => {
                canceled.store(true, std::sync::atomic::Ordering::SeqCst);
                // Drain the owned Git process before releasing its guard/lease.
                let _ = action.await;
                return Err(ApiError::forbidden());
            }
        }
    } else {
        action.await.map_err(api_error)?
    };
    Ok(Json(result))
}
#[utoipa::path(post,operation_id="forge_repository_sync_start",path="/api/v1/catalog/repositories/{id}/sync/runs",tag="repository-sync",params(("id"=Uuid,Path),("Idempotency-Key"=Uuid,Header),SyncQuery),responses((status=202,body=SyncRun),(status=409),(status=503)))]
pub(crate) async fn start(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<crate::auth::AccessClaims>,
    permit: Option<Extension<crate::space_admission::RepositoryWrite>>,
    Path(id): Path<Uuid>,
    Query(query): Query<SyncQuery>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<SyncRun>), ApiError> {
    let access = authorize(&state, id, &claims, true, false).await?;
    if !state.config.git_sync.enabled {
        return Err(ApiError::service_unavailable(
            "Исполнитель синхронизации отключён оператором",
        ));
    }
    let _lease = writable_lease(&state, id, permit.as_ref().map(|value| &value.0)).await?;
    let run = store(&state)?
        .enqueue_at(
            id,
            query.connection_id.unwrap_or(id),
            "manual",
            Some((operation(&headers)?, access)),
        )
        .await
        .map_err(api_error)?;
    Ok((StatusCode::ACCEPTED, Json(run)))
}
#[utoipa::path(get,operation_id="forge_repository_sync_runs",path="/api/v1/catalog/repositories/{id}/sync/runs",tag="repository-sync",params(("id"=Uuid,Path),SyncQuery),responses((status=200,body=RunPage)))]
pub(crate) async fn runs(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<crate::auth::AccessClaims>,
    common: Option<Extension<crate::space_admission::RepositoryRead>>,
    Path(id): Path<Uuid>,
    Query(query): Query<SyncQuery>,
) -> Result<Json<RunPage>, ApiError> {
    authorize(&state, id, &claims, false, common.is_some()).await?;
    let (items, total) = store(&state)?
        .runs_at(
            id,
            query.connection_id.unwrap_or(id),
            query.limit.unwrap_or(20),
            query.offset.unwrap_or(0),
        )
        .await
        .map_err(api_error)?;
    Ok(Json(RunPage { items, total }))
}
#[utoipa::path(get,operation_id="forge_repository_sync_run",path="/api/v1/catalog/repositories/{id}/sync/runs/{run_id}",tag="repository-sync",params(("id"=Uuid,Path),("run_id"=Uuid,Path),SyncQuery),responses((status=200,body=SyncRun),(status=404)))]
pub(crate) async fn run(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<crate::auth::AccessClaims>,
    common: Option<Extension<crate::space_admission::RepositoryRead>>,
    Path((id, run_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<SyncQuery>,
) -> Result<Json<SyncRun>, ApiError> {
    authorize(&state, id, &claims, false, common.is_some()).await?;
    let run = store(&state)?.run(id, run_id).await.map_err(api_error)?;
    if query
        .connection_id
        .is_some_and(|connection| connection != run.connection_id)
    {
        return Err(ApiError::not_found());
    }
    Ok(Json(run))
}
#[utoipa::path(get,operation_id="forge_repository_sync_refs",path="/api/v1/catalog/repositories/{id}/sync/runs/{run_id}/refs",tag="repository-sync",params(("id"=Uuid,Path),("run_id"=Uuid,Path),SyncQuery),responses((status=200,body=RefPage)))]
pub(crate) async fn refs(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<crate::auth::AccessClaims>,
    common: Option<Extension<crate::space_admission::RepositoryRead>>,
    Path((id, run_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<SyncQuery>,
) -> Result<Json<RefPage>, ApiError> {
    authorize(&state, id, &claims, false, common.is_some()).await?;
    let run = store(&state)?.run(id, run_id).await.map_err(api_error)?;
    if query
        .connection_id
        .is_some_and(|connection| connection != run.connection_id)
    {
        return Err(ApiError::not_found());
    }
    let (items, total) = store(&state)?
        .refs(run_id, query.limit.unwrap_or(50), query.offset.unwrap_or(0))
        .await
        .map_err(api_error)?;
    Ok(Json(RefPage { items, total }))
}
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/repository-syncs", get(list))
        .route(
            "/api/v1/catalog/repositories/{id}/sync",
            get(get_connection).put(save_connection),
        )
        .route("/api/v1/catalog/repositories/{id}/sync/check", post(check))
        .route(
            "/api/v1/catalog/repositories/{id}/sync/runs",
            get(runs).post(start),
        )
        .route(
            "/api/v1/catalog/repositories/{id}/sync/runs/{run_id}",
            get(run),
        )
        .route(
            "/api/v1/catalog/repositories/{id}/sync/runs/{run_id}/refs",
            get(refs),
        )
}

pub(crate) async fn signal(pool: &sqlx::PgPool, repository: Uuid) {
    let store = PgSyncStore {
        pool: pool.clone(),
        worker_enabled: false,
        interval_seconds: 300,
    };
    if store.signal(repository).await.is_err() {
        tracing::warn!(%repository,"sync change signal unavailable; periodic reconciliation will recover");
    }
}
