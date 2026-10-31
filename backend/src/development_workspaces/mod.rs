//! Human-managed project environments over existing identity and Forge execution.

mod catalog;
mod native;
pub mod worker;
use crate::api::{ApiError, AppState, authz_mw::VerifiedRequestIdentity};
use crate::space_admission::RepositoryWrite;
use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
pub use catalog::{SourceRepository, TargetChoice};
use cicd_domain::development_workspace::WorkspaceAction;
use cicd_infra::development_workspace::{self as store, Operation, Session, Workspace};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Serialize, utoipa::ToSchema)]
pub struct DevelopmentWorkspacePage {
    pub items: Vec<Workspace>,
    pub total: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct DevelopmentWorkspaceDetail {
    pub workspace: Workspace,
    pub session: Option<Session>,
    pub latest_operation: Option<Operation>,
    pub actor_id: Uuid,
    pub actions: Vec<ActionAvailability>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ActionAvailability {
    pub action: String,
    pub allowed: bool,
    pub reason: Option<String>,
}
#[derive(Serialize, utoipa::ToSchema)]
pub struct DevelopmentWorkspaceTargets {
    pub items: Vec<TargetChoice>,
    pub can_register: bool,
    pub denied_reason: Option<String>,
}

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RegisterWorkspace {
    pub id: Uuid,
    pub operation_id: Uuid,
    pub catalog_target_id: Uuid,
    pub name: String,
    pub create: bool,
}

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OperationRequest {
    pub operation_id: Uuid,
    pub expected_version: i64,
    pub command: WorkspaceAction,
}

#[derive(Default, Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}
impl Page {
    fn bounds(&self) -> Result<(i64, i64), ApiError> {
        let (limit, offset) = (self.limit.unwrap_or(50), self.offset.unwrap_or(0));
        if !(1..=100).contains(&limit) || offset < 0 {
            return Err(ApiError::bad_request("invalid_page"));
        }
        Ok((limit, offset))
    }
}

pub(crate) fn error(error: store::Error) -> ApiError {
    match error {
        store::Error::Conflict(message) => ApiError::conflict(message),
        store::Error::NotFound => ApiError::not_found(),
        store::Error::Database(error)
            if error
                .as_database_error()
                .is_some_and(|error| error.code().as_deref() == Some("23505")) =>
        {
            ApiError::conflict("workspace_concurrent_operation")
        }
        store::Error::Database(error) => ApiError::internal(error),
    }
}

fn pool(state: &AppState) -> Result<&sqlx::PgPool, ApiError> {
    state.pool.as_ref().ok_or_else(ApiError::unavailable)
}

fn human(identity: &VerifiedRequestIdentity, write: bool) -> Result<(), ApiError> {
    if identity.claims.service_account_name.is_some() || identity.claims.role == "service_account" {
        return Err(ApiError::forbidden());
    }
    identity.require_scope(if write { "ci-cd:write" } else { "ci-cd:read" })
}

async fn require_maintainer(
    pool: &sqlx::PgPool,
    actor: Uuid,
    role: &str,
    configuration: Uuid,
) -> Result<(), ApiError> {
    if crate::authz::Role::parse(role).is_some_and(|role| role >= crate::authz::Role::Maintainer)
        || crate::api::authz_mw::project_membership_role(pool, actor, configuration)
            .await?
            .is_some_and(|role| role >= crate::authz::Role::Maintainer)
    {
        Ok(())
    } else {
        Err(ApiError::forbidden())
    }
}

#[allow(clippy::too_many_arguments)]
async fn enqueue(
    state: &AppState,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    workspace: &Workspace,
    id: Uuid,
    actor: Uuid,
    owner: Uuid,
    version: i64,
    action: &WorkspaceAction,
) -> Result<Operation, ApiError> {
    let request = serde_json::to_value(action)
        .map_err(|_| ApiError::bad_request("invalid_workspace_command"))?;
    let hash =
        store::request_hash(&serde_json::json!({"action":request,"expected_version":version}))
            .map_err(error)?;
    if let Some(original) = store::original(tx, workspace.id, id, actor, owner, &hash)
        .await
        .map_err(error)?
    {
        return Ok(original);
    }
    let mut plan = catalog::operation_plan(state, workspace, action)?;
    if !plan.as_object().is_some_and(|value| value.is_empty()) {
        let environment =
            catalog::execution_target(&mut **tx, workspace.environment_id, workspace.project_id)
                .await?;
        if store::request_hash(&environment).map_err(error)? != workspace.environment_hash {
            return Err(ApiError::conflict("workspace_environment_target_drift"));
        }
        plan["environment"] = environment;
        plan["environmentHash"] = serde_json::json!(workspace.environment_hash);
    }
    store::enqueue(tx, workspace.id, id, actor, owner, version, action, &plan)
        .await
        .map_err(error)
}

async fn configuration_repository(state: &AppState, project: Uuid) -> Result<Uuid, ApiError> {
    sqlx::query_scalar::<_, Option<Uuid>>("SELECT repository_id FROM projects WHERE id=$1")
        .bind(project)
        .fetch_optional(pool(state)?)
        .await
        .map_err(ApiError::internal)?
        .flatten()
        .ok_or_else(ApiError::not_found)
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/api/v1/projects/{project_id}/development-workspaces",
            get(list).post(register),
        )
        .route(
            "/api/v1/projects/{project_id}/development-workspace-profiles",
            get(profiles),
        )
        .route("/api/v1/development-workspaces/{workspace_id}", get(detail))
        .route(
            "/api/v1/development-workspaces/{workspace_id}/operations",
            get(operations).post(submit),
        )
        .route(
            "/api/v1/development-workspaces/{workspace_id}/operations/{operation_id}",
            get(operation),
        )
        .route(
            "/api/v1/development-workspaces/{workspace_id}/operations/{operation_id}/reconcile",
            post(reconcile),
        )
        .layer(axum::extract::DefaultBodyLimit::max(32 * 1024))
        .layer(axum::middleware::map_response(
            |mut response: axum::response::Response| async move {
                response.headers_mut().insert(
                    "cache-control",
                    axum::http::HeaderValue::from_static("private, no-store"),
                );
                response
            },
        ))
}

#[utoipa::path(get,path="/api/v1/projects/{project_id}/development-workspaces",params(("project_id"=Uuid,Path),Page),responses((status=200,body=DevelopmentWorkspacePage)),tag="development-workspaces")]
pub(crate) async fn list(
    State(state): State<Arc<AppState>>,
    Path(project): Path<Uuid>,
    Query(page): Query<Page>,
    Extension(identity): Extension<VerifiedRequestIdentity>,
) -> Result<Json<DevelopmentWorkspacePage>, ApiError> {
    human(&identity, false)?;
    let (limit, offset) = page.bounds()?;
    let (items, total) = store::list(pool(&state)?, project, limit, offset)
        .await
        .map_err(error)?;
    Ok(Json(DevelopmentWorkspacePage { items, total }))
}

#[utoipa::path(get,path="/api/v1/projects/{project_id}/development-workspace-profiles",params(("project_id"=Uuid,Path)),responses((status=200,body=DevelopmentWorkspaceTargets)),tag="development-workspaces")]
pub(crate) async fn profiles(
    State(state): State<Arc<AppState>>,
    Path(project): Path<Uuid>,
    Extension(identity): Extension<VerifiedRequestIdentity>,
) -> Result<Json<DevelopmentWorkspaceTargets>, ApiError> {
    human(&identity, false)?;
    let can_register = can_maintain(pool(&state)?, &identity, project).await?;
    Ok(Json(DevelopmentWorkspaceTargets {
        items: catalog::choices(
            &state,
            configuration_repository(&state, project).await?,
            project,
        )?,
        can_register,
        denied_reason: (!can_register)
            .then(|| "Требуются права сопровождающего проекта".to_owned()),
    }))
}

#[utoipa::path(get,path="/api/v1/development-workspaces/{workspace_id}",params(("workspace_id"=Uuid,Path)),responses((status=200,body=DevelopmentWorkspaceDetail)),tag="development-workspaces")]
pub(crate) async fn detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Extension(identity): Extension<VerifiedRequestIdentity>,
) -> Result<Json<DevelopmentWorkspaceDetail>, ApiError> {
    human(&identity, false)?;
    let latest_operation = sqlx::query_as("SELECT * FROM development_workspace_operations WHERE workspace_id=$1 ORDER BY created_at DESC,id DESC LIMIT 1")
        .bind(id).fetch_optional(pool(&state)?).await.map_err(ApiError::internal)?;
    let workspace = store::workspace(pool(&state)?, id).await.map_err(error)?;
    let session = store::session(pool(&state)?, id).await.map_err(error)?;
    let maintainer = can_maintain(pool(&state)?, &identity, workspace.project_id).await?;
    let storage: String =
        sqlx::query_scalar("SELECT storage_name FROM repository_catalog WHERE id=$1")
            .bind(workspace.repository_id)
            .fetch_one(pool(&state)?)
            .await
            .map_err(ApiError::internal)?;
    let writable = crate::api::authz_mw::repository_scope_allows(
        pool(&state)?,
        &storage,
        identity.claims.sub,
        crate::authz::Role::parse(&identity.claims.role).unwrap_or(crate::authz::Role::Viewer),
        crate::authz::Role::Developer,
        Some(workspace.project_id),
    )
    .await?;
    let mut tx = pool(&state)?.begin().await.map_err(ApiError::internal)?;
    let jobs = store::workspace_busy(&mut tx, id).await.map_err(error)?;
    tx.commit().await.map_err(ApiError::internal)?;
    let busy = latest_operation
        .as_ref()
        .is_some_and(|operation: &Operation| {
            matches!(operation.state.as_str(), "queued" | "running" | "unknown")
        });
    let actions = action_availability(
        &workspace,
        session.as_ref(),
        identity.claims.sub,
        writable,
        maintainer,
        busy,
        jobs,
    );
    Ok(Json(DevelopmentWorkspaceDetail {
        workspace,
        session,
        latest_operation,
        actor_id: identity.claims.sub,
        actions,
    }))
}

#[utoipa::path(post,path="/api/v1/projects/{project_id}/development-workspaces",params(("project_id"=Uuid,Path)),request_body=RegisterWorkspace,responses((status=200,body=Operation)),tag="development-workspaces")]
pub(crate) async fn register(
    State(state): State<Arc<AppState>>,
    Path(project): Path<Uuid>,
    Extension(identity): Extension<VerifiedRequestIdentity>,
    permit: Option<Extension<RepositoryWrite>>,
    Json(input): Json<RegisterWorkspace>,
) -> Result<Json<Operation>, ApiError> {
    human(&identity, true)?;
    if input.id.is_nil()
        || input.operation_id.is_nil()
        || input.catalog_target_id.is_nil()
        || input.name.trim() != input.name
        || input.name.is_empty()
        || input.name.len() > 128
        || input.name.chars().any(char::is_control)
    {
        return Err(ApiError::bad_request("invalid_workspace_registration"));
    }
    let repository = configuration_repository(&state, project).await?;
    let mut tx = crate::repository_catalog::catalog_write_lease(&state, repository).await?;
    if let Some(permit) = &permit {
        permit.lock(&mut tx).await?;
    }
    let origin = crate::pipeline_authority::require_creation(false)?;
    let owner = origin
        .map(|origin| origin.subject())
        .unwrap_or(identity.claims.sub);
    if let Some(origin) = origin {
        origin.bind(&mut tx).await?;
    }
    let existing: Option<Workspace> =
        sqlx::query_as("SELECT * FROM development_workspaces WHERE id=$1 FOR UPDATE")
            .bind(input.id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(ApiError::internal)?;
    if let Some(existing) = existing {
        if existing.project_id != project
            || existing.name != input.name
            || existing.catalog_target_id != input.catalog_target_id
        {
            return Err(ApiError::conflict("workspace_registration_conflict"));
        }
    } else {
        let choice = catalog::target(&state, input.catalog_target_id)?;
        if choice.repository_id != repository
            || choice.configuration_id.is_some_and(|id| id != project)
            || (choice.kind == "template") != input.create
        {
            return Err(ApiError::conflict("workspace_target_binding_mismatch"));
        }
        let environment_matches: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM environments WHERE id=$1 AND project_id=$2)",
        )
        .bind(choice.environment_id)
        .bind(project)
        .fetch_one(&mut *tx)
        .await
        .map_err(ApiError::internal)?;
        if !environment_matches {
            return Err(ApiError::conflict(
                "workspace_execution_environment_binding_mismatch",
            ));
        }
        let target = if input.create {
            Uuid::now_v7()
        } else {
            choice.id
        };
        let tracker: Option<(Uuid,Uuid)> = sqlx::query_as(
            "SELECT w.tracker_instance_id,w.tracker_project_id FROM repository_catalog r JOIN forge_namespace_bindings b ON b.resource_id=r.group_id JOIN forge_workspace_projects w ON w.registry_instance_id=b.registry_instance_id AND w.namespace_id=b.namespace_id WHERE r.id=$1 AND w.state='active'",
        ).bind(repository).fetch_optional(&mut *tx).await.map_err(ApiError::internal)?;
        let (tracker_instance, tracker_project) = tracker
            .ok_or_else(|| ApiError::conflict("workspace_tracker_project_binding_required"))?;
        let catalog_hash = store::request_hash(
            &serde_json::to_value(&choice)
                .map_err(|_| ApiError::conflict("invalid_workspace_catalog"))?,
        )
        .map_err(error)?;
        sqlx::query("INSERT INTO development_workspaces(id,project_id,repository_id,name,profile_id,target_id,catalog_target_id,profile_hash,profile,created_by,tracker_instance_id,tracker_project_id,environment_id,catalog_hash,public_origin,environment_hash) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)")
            .bind(input.id).bind(project).bind(repository).bind(&input.name).bind(choice.profile_id).bind(target)
            .bind(choice.id).bind(choice.profile_hash).bind(choice.profile).bind(identity.claims.sub)
            .bind(tracker_instance).bind(tracker_project).bind(choice.environment_id).bind(catalog_hash).bind(choice.public_origin).bind(choice.environment_hash)
            .execute(&mut *tx).await.map_err(ApiError::internal)?;
    }
    let action = if input.create {
        WorkspaceAction::Create {}
    } else {
        WorkspaceAction::Register {}
    };
    let workspace: Workspace = sqlx::query_as("SELECT * FROM development_workspaces WHERE id=$1")
        .bind(input.id)
        .fetch_one(&mut *tx)
        .await
        .map_err(ApiError::internal)?;
    let operation = enqueue(
        &state,
        &mut tx,
        &workspace,
        input.operation_id,
        identity.claims.sub,
        owner,
        1,
        &action,
    )
    .await?;
    if let Some(permit) = &permit {
        permit.require()?;
    }
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(operation))
}

#[utoipa::path(post,path="/api/v1/development-workspaces/{workspace_id}/operations",params(("workspace_id"=Uuid,Path)),request_body=OperationRequest,responses((status=200,body=Operation)),tag="development-workspaces")]
pub(crate) async fn submit(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Extension(identity): Extension<VerifiedRequestIdentity>,
    permit: Option<Extension<RepositoryWrite>>,
    Json(input): Json<OperationRequest>,
) -> Result<Json<Operation>, ApiError> {
    human(&identity, true)?;
    if input.operation_id.is_nil()
        || input.expected_version < 1
        || matches!(
            input.command,
            WorkspaceAction::Register {} | WorkspaceAction::Create {}
        )
    {
        return Err(ApiError::bad_request("invalid_workspace_operation"));
    }
    let workspace = store::workspace(pool(&state)?, id).await.map_err(error)?;
    if matches!(input.command, WorkspaceAction::RecoverSession { .. }) {
        require_maintainer(
            pool(&state)?,
            identity.claims.sub,
            &identity.claims.role,
            workspace.project_id,
        )
        .await?;
    }
    let mut tx =
        crate::repository_catalog::catalog_write_lease(&state, workspace.repository_id).await?;
    if let Some(permit) = &permit {
        permit.lock(&mut tx).await?;
    }
    let origin = crate::pipeline_authority::require_creation(false)?;
    let owner = origin
        .map(|origin| origin.subject())
        .unwrap_or(identity.claims.sub);
    if let Some(origin) = origin {
        origin.bind(&mut tx).await?;
    }
    let operation = enqueue(
        &state,
        &mut tx,
        &workspace,
        input.operation_id,
        identity.claims.sub,
        owner,
        input.expected_version,
        &input.command,
    )
    .await?;
    if let Some(permit) = &permit {
        permit.require()?;
    }
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(operation))
}

#[utoipa::path(get,path="/api/v1/development-workspaces/{workspace_id}/operations/{operation_id}",params(("workspace_id"=Uuid,Path),("operation_id"=Uuid,Path)),responses((status=200,body=Operation)),tag="development-workspaces")]
pub(crate) async fn operation(
    State(state): State<Arc<AppState>>,
    Path((id, operation)): Path<(Uuid, Uuid)>,
    Extension(identity): Extension<VerifiedRequestIdentity>,
) -> Result<Json<Operation>, ApiError> {
    human(&identity, false)?;
    store::operation(pool(&state)?, id, operation)
        .await
        .map(Json)
        .map_err(error)
}

#[utoipa::path(get,path="/api/v1/development-workspaces/{workspace_id}/operations",params(("workspace_id"=Uuid,Path),Page),responses((status=200,body=Vec<Operation>)),tag="development-workspaces")]
pub(crate) async fn operations(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Query(page): Query<Page>,
    Extension(identity): Extension<VerifiedRequestIdentity>,
) -> Result<Json<Vec<Operation>>, ApiError> {
    human(&identity, false)?;
    let (limit, offset) = page.bounds()?;
    sqlx::query_as("SELECT * FROM development_workspace_operations WHERE workspace_id=$1 ORDER BY created_at DESC,id DESC LIMIT $2 OFFSET $3")
        .bind(id).bind(limit).bind(offset).fetch_all(pool(&state)?).await.map(Json).map_err(ApiError::internal)
}

#[utoipa::path(post,path="/api/v1/development-workspaces/{workspace_id}/operations/{operation_id}/reconcile",params(("workspace_id"=Uuid,Path),("operation_id"=Uuid,Path)),responses((status=200,body=Operation)),tag="development-workspaces")]
pub(crate) async fn reconcile(
    State(state): State<Arc<AppState>>,
    Path((id, operation)): Path<(Uuid, Uuid)>,
    Extension(identity): Extension<VerifiedRequestIdentity>,
    permit: Option<Extension<RepositoryWrite>>,
) -> Result<Json<Operation>, ApiError> {
    human(&identity, true)?;
    let workspace = store::workspace(pool(&state)?, id).await.map_err(error)?;
    let mut tx =
        crate::repository_catalog::catalog_write_lease(&state, workspace.repository_id).await?;
    if let Some(permit) = &permit {
        permit.lock(&mut tx).await?;
    }
    let origin = crate::pipeline_authority::require_creation(false)?;
    if let Some(origin) = origin {
        origin.bind(&mut tx).await?;
    }
    sqlx::query("SELECT id FROM development_workspaces WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .map_err(ApiError::internal)?;
    let row: Operation = sqlx::query_as(
        "SELECT * FROM development_workspace_operations WHERE workspace_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(id)
    .bind(operation)
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::not_found)?;
    if row.state == "unknown" {
        sqlx::query("UPDATE development_workspace_operations SET state='queued',reconcile_only=true,worker_id=NULL,recovery_actor_id=$2,recovery_owner_subject=$3,recovery_requested_at=clock_timestamp() WHERE id=$1")
            .bind(operation).bind(identity.claims.sub).bind(origin.map(|origin|origin.subject()).unwrap_or(identity.claims.sub))
            .execute(&mut *tx).await.map_err(ApiError::internal)?;
    }
    if let Some(permit) = &permit {
        permit.require()?;
    }
    tx.commit().await.map_err(ApiError::internal)?;
    store::operation(pool(&state)?, id, operation)
        .await
        .map(Json)
        .map_err(error)
}

async fn can_maintain(
    pool: &sqlx::PgPool,
    identity: &VerifiedRequestIdentity,
    project: Uuid,
) -> Result<bool, ApiError> {
    Ok(crate::authz::Role::parse(&identity.claims.role)
        .is_some_and(|role| role >= crate::authz::Role::Maintainer)
        || crate::api::authz_mw::project_membership_role(pool, identity.claims.sub, project)
            .await?
            .is_some_and(|role| role >= crate::authz::Role::Maintainer))
}
fn action_availability(
    workspace: &Workspace,
    session: Option<&Session>,
    actor: Uuid,
    writable: bool,
    maintainer: bool,
    busy: bool,
    jobs: bool,
) -> Vec<ActionAvailability> {
    [
        "observe",
        "start",
        "stop",
        "begin_session",
        "end_session",
        "prepare",
        "check",
        "build",
        "recover_session",
        "reconcile",
    ]
    .into_iter()
    .map(|action| {
        let reason = if matches!(action, "recover_session" | "reconcile") && !maintainer {
            Some("Требуются права сопровождающего проекта")
        } else if !writable {
            Some("Доступно только чтение")
        } else if busy && action != "reconcile" {
            Some("Сначала завершите или восстановите текущую операцию")
        } else if jobs
            && matches!(
                action,
                "start" | "stop" | "begin_session" | "end_session" | "recover_session"
            )
        {
            Some("Проверка, сборка или доставка ещё удерживает окружение")
        } else if action == "stop" && session.is_some() {
            Some("Сначала закончите рабочую сессию")
        } else if action == "begin_session" && session.is_some() {
            Some("В окружении уже есть рабочая сессия")
        } else if !matches!(action, "observe" | "reconcile" | "recover_session")
            && session.is_some_and(|session| session.actor_id != actor)
        {
            Some("Окружение занято другим исполнителем")
        } else if matches!(action, "prepare" | "check" | "build" | "end_session")
            && session.is_none()
        {
            Some("Сначала начните рабочую сессию")
        } else if matches!(action, "check" | "build") && workspace.preparation != "prepared" {
            Some("Сначала подготовьте исходники и инструменты")
        } else if action == "prepare"
            && workspace
                .local_preparation
                .as_object()
                .is_none_or(|value| value.is_empty())
        {
            Some("Подготовьте локальные исходники командой cicd-cli dev-workspace prepare")
        } else if action == "recover_session" && session.is_none() {
            Some("Активной сессии нет")
        } else {
            None
        };
        ActionAvailability {
            action: action.to_owned(),
            allowed: reason.is_none(),
            reason: reason.map(str::to_owned),
        }
    })
    .collect()
}
