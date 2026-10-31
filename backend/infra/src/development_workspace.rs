//! Durable instance/session/operation storage. Resource admission belongs to ingress.

use chrono::{DateTime, Utc};
use cicd_domain::development_workspace::WorkspaceAction;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Conflict(&'static str),
    #[error("workspace_not_found")]
    NotFound,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
#[schema(as = DevelopmentWorkspace)]
pub struct Workspace {
    pub id: Uuid,
    pub project_id: Uuid,
    pub tracker_instance_id: Uuid,
    pub tracker_project_id: Uuid,
    pub repository_id: Uuid,
    pub environment_id: Uuid,
    pub environment_hash: String,
    pub name: String,
    pub profile_id: Uuid,
    pub target_id: Uuid,
    pub catalog_target_id: Uuid,
    pub profile_hash: String,
    pub catalog_hash: String,
    pub public_origin: Option<String>,
    pub profile: Value,
    pub sources: Value,
    pub local_preparation: Value,
    pub preparation: String,
    pub runtime: String,
    pub desired_runtime: String,
    pub observation: Value,
    pub observed_at: Option<DateTime<Utc>>,
    pub version: i64,
    pub created_by: Uuid,
    #[sqlx(default)]
    pub owner_name: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
#[schema(as = DevelopmentWorkspaceSession)]
pub struct Session {
    pub id: Uuid,
    pub owner_subject: Uuid,
    pub workspace_id: Uuid,
    pub actor_id: Uuid,
    #[sqlx(default)]
    pub actor_name: Option<String>,
    pub task_id: Uuid,
    pub task_ref: String,
    pub sources: Value,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
#[schema(as = DevelopmentWorkspaceOperation)]
pub struct Operation {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub actor_id: Uuid,
    pub request: Value,
    pub pipeline_id: Option<Uuid>,
    pub recovery_actor_id: Option<Uuid>,
    pub recovery_requested_at: Option<DateTime<Utc>>,
    pub expected_version: i64,
    pub state: String,
    pub result: Option<Value>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

pub async fn workspace(pool: &PgPool, id: Uuid) -> Result<Workspace, Error> {
    sqlx::query_as("SELECT w.*,u.username AS owner_name FROM development_workspaces w JOIN users u ON u.id=w.created_by WHERE w.id=$1")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(Error::NotFound)
}

pub async fn session(pool: &PgPool, id: Uuid) -> Result<Option<Session>, Error> {
    Ok(sqlx::query_as(
        "SELECT s.*,u.username AS actor_name FROM development_workspace_sessions s JOIN users u ON u.id=s.actor_id WHERE s.workspace_id=$1 AND s.ended_at IS NULL",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?)
}

pub async fn operation(pool: &PgPool, workspace: Uuid, id: Uuid) -> Result<Operation, Error> {
    sqlx::query_as("SELECT * FROM development_workspace_operations WHERE workspace_id=$1 AND id=$2")
        .bind(workspace)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(Error::NotFound)
}

pub async fn list(
    pool: &PgPool,
    project: Uuid,
    limit: i64,
    offset: i64,
) -> Result<(Vec<Workspace>, i64), Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await?;
    let total =
        sqlx::query_scalar("SELECT count(*) FROM development_workspaces WHERE project_id=$1")
            .bind(project)
            .fetch_one(&mut *tx)
            .await?;
    let items = sqlx::query_as("SELECT * FROM development_workspaces WHERE project_id=$1 ORDER BY created_at,id LIMIT $2 OFFSET $3")
        .bind(project).bind(limit).bind(offset).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    Ok((items, total))
}

pub async fn original(
    tx: &mut Transaction<'_, Postgres>,
    workspace: Uuid,
    operation: Uuid,
    actor: Uuid,
    owner: Uuid,
    hash: &str,
) -> Result<Option<Operation>, Error> {
    let row: Option<(Uuid, Uuid, Uuid, String)> = sqlx::query_as(
        "SELECT workspace_id,actor_id,owner_subject,request_hash FROM development_workspace_operations WHERE id=$1")
        .bind(operation).fetch_optional(&mut **tx).await?;
    if let Some(row) = row {
        if row != (workspace, actor, owner, hash.to_owned()) {
            return Err(Error::Conflict("operation_key_payload_conflict"));
        }
        return Ok(
            sqlx::query_as("SELECT * FROM development_workspace_operations WHERE id=$1")
                .bind(operation)
                .fetch_optional(&mut **tx)
                .await?,
        );
    }
    Ok(None)
}

pub fn request_hash(value: &Value) -> Result<String, Error> {
    // serde_json's default map is ordered; caller DTO is serialized once before hashing.
    let bytes =
        serde_json::to_vec(value).map_err(|_| Error::Conflict("invalid_operation_payload"))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[allow(clippy::too_many_arguments)]
pub async fn enqueue(
    tx: &mut Transaction<'_, Postgres>,
    workspace: Uuid,
    operation: Uuid,
    actor: Uuid,
    owner: Uuid,
    expected_version: i64,
    action: &WorkspaceAction,
    execution_plan: &Value,
) -> Result<Operation, Error> {
    action.validate().map_err(Error::Conflict)?;
    let request = serde_json::to_value(action).map_err(|_| Error::Conflict("invalid_action"))?;
    let hash = request_hash(
        &serde_json::json!({"action": request, "expected_version": expected_version}),
    )?;
    if let Some(original) = original(tx, workspace, operation, actor, owner, &hash).await? {
        return Ok(original);
    }
    let actual: Option<i64> =
        sqlx::query_scalar("SELECT version FROM development_workspaces WHERE id=$1 FOR UPDATE")
            .bind(workspace)
            .fetch_optional(&mut **tx)
            .await?;
    if let Some(original) = original(tx, workspace, operation, actor, owner, &hash).await? {
        return Ok(original);
    }
    if actual != Some(expected_version) {
        return Err(if actual.is_none() {
            Error::NotFound
        } else {
            Error::Conflict("workspace_version_conflict")
        });
    }
    let busy: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM development_workspace_operations WHERE workspace_id=$1 AND state IN ('queued','running','unknown'))")
        .bind(workspace).fetch_one(&mut **tx).await?;
    if busy {
        return Err(Error::Conflict("workspace_operation_in_progress"));
    }
    let session: Option<(Uuid, Uuid, Uuid)> = sqlx::query_as("SELECT id,actor_id,owner_subject FROM development_workspace_sessions WHERE workspace_id=$1 AND ended_at IS NULL")
        .bind(workspace).fetch_optional(&mut **tx).await?;
    if action.requires_free_workspace() && session.is_some() {
        return Err(Error::Conflict("workspace_has_active_session"));
    }
    if !matches!(
        action,
        WorkspaceAction::Observe {} | WorkspaceAction::RecoverSession { .. }
    ) && session.is_some_and(|(_, user, subject)| user != actor || subject != owner)
    {
        return Err(Error::Conflict("workspace_session_owned_by_another_actor"));
    }
    if action.requires_session()
        && !session.is_some_and(|(_, user, subject)| user == actor && subject == owner)
    {
        return Err(Error::Conflict("workspace_session_required"));
    }
    if let WorkspaceAction::RecoverSession { session_id } = action {
        if session.is_none_or(|(id, _, _)| id != *session_id) {
            return Err(Error::Conflict("workspace_session_changed"));
        }
    }
    if let WorkspaceAction::EndSession { session_id } = action {
        if session.is_none_or(|(id, _, _)| id != *session_id) {
            return Err(Error::Conflict("workspace_session_changed"));
        }
    }
    let jobs = workspace_busy(tx, workspace).await?;
    if matches!(
        action,
        WorkspaceAction::Stop {}
            | WorkspaceAction::Start {}
            | WorkspaceAction::EndSession { .. }
            | WorkspaceAction::RecoverSession { .. }
            | WorkspaceAction::BeginSession { .. }
    ) && jobs
    {
        return Err(Error::Conflict("workspace_has_active_pipeline"));
    }
    let operation: Operation = sqlx::query_as("INSERT INTO development_workspace_operations(id,workspace_id,actor_id,owner_subject,request_hash,request,expected_version,execution_plan) VALUES($1,$2,$3,$4,$5,$6,$7,$8) RETURNING *")
        .bind(operation).bind(workspace).bind(actor).bind(owner).bind(hash).bind(request)
        .bind(expected_version).bind(execution_plan).fetch_one(&mut **tx).await?;
    sqlx::query("UPDATE development_workspaces SET version=version+1 WHERE id=$1")
        .bind(workspace)
        .execute(&mut **tx)
        .await?;
    Ok(operation)
}

/// Scope native custody to this instance, including legacy delivery to its
/// environment. A terminal projection is not a completion acknowledgement.
pub async fn workspace_busy(
    tx: &mut Transaction<'_, Postgres>,
    workspace: Uuid,
) -> Result<bool, Error> {
    Ok(sqlx::query_scalar(
        "WITH selected AS (
           SELECT pipeline_id FROM development_workspace_operations WHERE workspace_id=$1 AND pipeline_id IS NOT NULL
           UNION SELECT d.pipeline_id FROM deployments d JOIN development_workspaces w ON w.environment_id=d.environment_id WHERE w.id=$1 AND d.pipeline_id IS NOT NULL
         ), selected_jobs AS (
           SELECT j.id FROM jobs j JOIN stages s ON s.id=j.stage_id WHERE s.pipeline_id IN (SELECT pipeline_id FROM selected)
         )
         SELECT EXISTS(SELECT 1 FROM pipelines WHERE id IN (SELECT pipeline_id FROM selected) AND status IN ('queued','running'))
         OR EXISTS(SELECT 1 FROM deployments d JOIN development_workspaces w ON w.environment_id=d.environment_id WHERE w.id=$1 AND (d.status IN ('pending','running') OR d.runtime_phase IN ('prepared','running','held')))
         OR EXISTS(SELECT 1 FROM execution_attempts a LEFT JOIN job_leases l ON l.attempt_id=a.id WHERE a.job_id IN (SELECT id FROM selected_jobs) AND (a.status='running' OR l.lease_status='active' OR (l.runner_id IS NOT NULL AND l.completion_received_at IS NULL)))
         OR EXISTS(SELECT 1 FROM job_queue WHERE job_id IN (SELECT id FROM selected_jobs) AND state='leased')
         OR EXISTS(SELECT 1 FROM artifact_upload_sessions WHERE job_id IN (SELECT id FROM selected_jobs) AND status='open')"
    ).bind(workspace).fetch_one(&mut **tx).await?)
}
