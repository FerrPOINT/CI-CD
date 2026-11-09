//! Assignment-scoped Developer access, never a substitute human administrator.
use super::{
    assignment::{AssignmentAccess, DeveloperAssignment, developer_action},
    tracker,
};
use crate::{
    api::{ApiError, AppState, authz_mw::VerifiedRequestIdentity},
    auth::AccessClaims,
    git_host::GitDeadline,
};
use cicd_domain::development_workspace::WorkspaceAction;
use cicd_infra::development_workspace::{self as store, Workspace};
use pdlc_auth_core::service_bridge::VerifiedCentralToken;
use pdlc_shared::resource_context::NamespaceRef;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::future::Future;
use uuid::Uuid;

tokio::task_local! { static CREATION: (Uuid, GitDeadline); }

pub(crate) fn creation_allowed(variables: &Value) -> bool {
    CREATION
        .try_with(|(operation, clock)| {
            clock.require().is_ok()
                && variables["development_workspace_operation_id"] == operation.to_string()
        })
        .unwrap_or(false)
}

pub(super) async fn with_creation<T>(
    operation: Uuid,
    clock: GitDeadline,
    future: impl Future<Output = T>,
) -> T {
    CREATION.scope((operation, clock), future).await
}

pub(crate) fn creation_clock() -> Option<GitDeadline> {
    CREATION.try_with(|(_, clock)| *clock).ok()
}

pub(crate) async fn bind_pipeline(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    variables: &Value,
    pipeline: Uuid,
) -> Result<(), ApiError> {
    if let Some(clock) = creation_clock() {
        if !creation_allowed(variables) {
            return Err(ApiError::forbidden());
        }
        let operation = variables["development_workspace_operation_id"]
            .as_str()
            .and_then(canonical)
            .ok_or_else(ApiError::forbidden)?;
        let workspace = variables["development_workspace_id"]
            .as_str()
            .and_then(canonical)
            .ok_or_else(ApiError::forbidden)?;
        clock.bind_commit_clock(tx).await?;
        let changed=sqlx::query("UPDATE development_workspace_operations SET pipeline_id=$3 WHERE id=$1 AND workspace_id=$2 AND state='running' AND pipeline_id IS NULL AND execution_plan ? 'developerAuthority'")
            .bind(operation).bind(workspace).bind(pipeline).execute(&mut **tx).await.map_err(ApiError::internal)?;
        if changed.rows_affected() != 1 {
            return Err(ApiError::conflict(
                "workspace_machine_pipeline_binding_changed",
            ));
        }
    }
    Ok(())
}

fn canonical(value: &str) -> Option<Uuid> {
    Uuid::parse_str(value)
        .ok()
        .filter(|id| !id.is_nil() && id.to_string() == value)
}

enum Target {
    Identity,
    Project(Uuid),
    Workspace(Uuid),
    Pipeline(Uuid),
    Job(Uuid),
    Artifact(Uuid),
}

/// Match workspace operations and their own result readers. Resolution below
/// requires an actual owned workspace operation, not just a project token.
fn target(method: &str, path: &str) -> Option<Target> {
    let parts: Vec<_> = path.strip_prefix("/api/v1/")?.split('/').collect();
    match parts.as_slice() {
        ["auth", "principal"] if method == "GET" => Some(Target::Identity),
        [
            "projects",
            id,
            "development-workspaces" | "development-workspace-profiles",
        ] if method == "GET" => Some(Target::Project(canonical(id)?)),
        ["development-workspaces", id] if method == "GET" => {
            Some(Target::Workspace(canonical(id)?))
        }
        ["development-workspaces", id, "operations"] if matches!(method, "GET" | "POST") => {
            Some(Target::Workspace(canonical(id)?))
        }
        ["development-workspaces", id, "operations", operation]
            if method == "GET" && canonical(operation).is_some() =>
        {
            Some(Target::Workspace(canonical(id)?))
        }
        ["pipelines", id] if method == "GET" => Some(Target::Pipeline(canonical(id)?)),
        ["jobs", id] if method == "GET" => Some(Target::Job(canonical(id)?)),
        ["jobs", id, "logs" | "attempts" | "artifacts"] if method == "GET" => {
            Some(Target::Job(canonical(id)?))
        }
        ["artifacts", id, "download"] if method == "GET" => Some(Target::Artifact(canonical(id)?)),
        _ => None,
    }
}

pub(crate) fn handles(method: &str, path: &str) -> bool {
    target(method, path).is_some()
}

pub(super) fn principal(identity: &VerifiedRequestIdentity) -> bool {
    identity.claims.role == "service_account"
        && identity
            .central
            .as_ref()
            .is_some_and(|v| v.subject_role.as_deref() == Some("service_account"))
}

fn require_token(verified: &VerifiedCentralToken, write: bool) -> Result<Uuid, ApiError> {
    let ctx = &verified.context;
    if !verified.lifetime.is_current()
        || verified.subject_role.as_deref() != Some("service_account")
        || ctx.session_id.is_some()
        || !pdlc_auth_core::is_personal_token(&ctx.token)
        || !ctx.has_scope(if write { "ci-cd:write" } else { "ci-cd:read" })
        || crate::namespace::registered_machine(&ctx.user_id)
        || crate::delegated_credentials::has_grant(ctx)
    {
        return Err(ApiError::forbidden());
    }
    canonical(&ctx.user_id).ok_or_else(ApiError::forbidden)
}

async fn project_access(
    pool: &PgPool,
    actor: Uuid,
    project: Uuid,
    write: bool,
) -> Result<(), ApiError> {
    let allowed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM project_memberships m JOIN users u ON u.id=m.user_id WHERE m.project_id=$1 AND m.user_id=$2 AND u.enabled AND u.role='service_account' AND (NOT $3 OR m.role IN ('developer','maintainer')))")
        .bind(project).bind(actor).bind(write).fetch_one(pool).await.map_err(ApiError::internal)?;
    if allowed {
        Ok(())
    } else {
        Err(ApiError::forbidden())
    }
}

pub(crate) async fn authenticate(
    pool: &PgPool,
    verified: VerifiedCentralToken,
    method: &str,
    path: &str,
) -> Result<VerifiedRequestIdentity, ApiError> {
    let selected = target(method, path).ok_or_else(ApiError::forbidden)?;
    let subject = require_token(&verified, method != "GET")?;
    let actor: Option<(Uuid, String, bool)> =
        sqlx::query_as("SELECT id,role,enabled FROM users WHERE central_sub=$1")
            .bind(subject.to_string())
            .fetch_optional(pool)
            .await
            .map_err(ApiError::internal)?;
    let actor = match actor {
        Some((id, kind, true)) if kind == "service_account" => id,
        Some(_) => return Err(ApiError::forbidden()),
        None => {
            let id = Uuid::new_v4();
            sqlx::query("INSERT INTO users(id,username,role,enabled,central_sub) VALUES($1,$2,'service_account',true,$3) ON CONFLICT(central_sub) WHERE central_sub IS NOT NULL DO NOTHING")
                .bind(id).bind(format!("machine-{}",subject.simple())).bind(subject.to_string()).execute(pool).await.map_err(ApiError::internal)?;
            let (id, kind, enabled): (Uuid, String, bool) =
                sqlx::query_as("SELECT id,role,enabled FROM users WHERE central_sub=$1")
                    .bind(subject.to_string())
                    .fetch_one(pool)
                    .await
                    .map_err(ApiError::internal)?;
            if kind != "service_account" || !enabled {
                return Err(ApiError::forbidden());
            }
            id
        }
    };
    let project = match selected {
        Target::Identity => None,
        Target::Project(id) => Some(id),
        Target::Workspace(id) => Some(sqlx::query_scalar("SELECT project_id FROM development_workspaces WHERE id=$1")
            .bind(id).fetch_optional(pool).await.map_err(ApiError::internal)?.ok_or_else(ApiError::not_found)?),
        Target::Pipeline(id) => Some(sqlx::query_scalar("SELECT w.project_id FROM development_workspace_operations o JOIN development_workspaces w ON w.id=o.workspace_id WHERE o.pipeline_id=$1 AND o.actor_id=$2 AND o.execution_plan ? 'developerAuthority'")
            .bind(id).bind(actor).fetch_optional(pool).await.map_err(ApiError::internal)?.ok_or_else(ApiError::forbidden)?),
        Target::Job(id) => Some(sqlx::query_scalar("SELECT w.project_id FROM jobs j JOIN stages s ON s.id=j.stage_id JOIN development_workspace_operations o ON o.pipeline_id=s.pipeline_id JOIN development_workspaces w ON w.id=o.workspace_id WHERE j.id=$1 AND o.actor_id=$2 AND o.execution_plan ? 'developerAuthority'")
            .bind(id).bind(actor).fetch_optional(pool).await.map_err(ApiError::internal)?.ok_or_else(ApiError::forbidden)?),
        Target::Artifact(id) => Some(sqlx::query_scalar("SELECT w.project_id FROM artifacts a JOIN jobs j ON j.id=a.job_id JOIN stages s ON s.id=j.stage_id JOIN development_workspace_operations o ON o.pipeline_id=s.pipeline_id JOIN development_workspaces w ON w.id=o.workspace_id WHERE a.id=$1 AND o.actor_id=$2 AND o.execution_plan ? 'developerAuthority'")
            .bind(id).bind(actor).fetch_optional(pool).await.map_err(ApiError::internal)?.ok_or_else(ApiError::forbidden)?),
    };
    if let Some(project) = project {
        project_access(pool, actor, project, method != "GET").await?;
    }
    let now = chrono::Utc::now().timestamp();
    let claims = AccessClaims {
        sub: actor,
        sid: None,
        token_id: None,
        token_project_id: project,
        token_scopes: verified.context.scopes.iter().cloned().collect(),
        role: "service_account".into(),
        service_account_name: Some(format!("central:{subject}")),
        ver: 0,
        iat: now,
        exp: verified.lifetime.local_valid_until(),
    };
    Ok(VerifiedRequestIdentity {
        claims,
        central: Some(verified),
        product_personal: None,
    })
}

async fn namespace(pool: &PgPool, workspace: &Workspace) -> Result<NamespaceRef, ApiError> {
    let row:Option<(Uuid,Uuid)>=sqlx::query_as("SELECT registry_instance_id,namespace_id FROM forge_workspace_projects WHERE tracker_instance_id=$1 AND tracker_project_id=$2 AND state='active'")
        .bind(workspace.tracker_instance_id).bind(workspace.tracker_project_id).fetch_optional(pool).await.map_err(ApiError::internal)?;
    let (registry_instance_id, namespace_id) = row.ok_or_else(ApiError::forbidden)?;
    let namespace = NamespaceRef {
        registry_instance_id,
        namespace_id,
    };
    let row:Option<(Uuid,Value)>=sqlx::query_as("SELECT r.group_id,b.command FROM repository_catalog r JOIN forge_namespace_bindings b ON b.resource_id=r.group_id WHERE r.id=$1 AND r.kind='hosted' AND r.ready AND b.state='active'")
        .bind(workspace.repository_id).fetch_optional(pool).await.map_err(ApiError::internal)?;
    let (group, command) = row.ok_or_else(ApiError::forbidden)?;
    let command: pdlc_shared::resource_context::OwnerCommand =
        serde_json::from_value(command).map_err(|_| ApiError::forbidden())?;
    crate::namespace::validate_projection(&command)?;
    if command.namespace != namespace
        || command.resource.resource_id != group
        || command.state != "active"
    {
        return Err(ApiError::forbidden());
    }
    Ok(namespace)
}

fn allowed_check(workspace: &Workspace, action: &WorkspaceAction) -> bool {
    if let WorkspaceAction::Check { recipe, .. } = action {
        return workspace.profile["checks"]
            .as_array()
            .is_some_and(|checks| {
                checks.iter().any(|c| {
                    c["key"] == *recipe && c["executor"] == "forge" && c["requiresRuntime"] == false
                })
            });
    }
    true
}

// No Debug: the ciphertext and decrypted bearer must never enter API DTOs/logs.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Credential {
    workspace: Uuid,
    operation: Uuid,
    actor: Uuid,
    subject: Uuid,
    issuer: String,
    assignment: DeveloperAssignment,
    token: String,
}

pub(super) async fn seal(
    state: &AppState,
    workspace: &Workspace,
    operation: Uuid,
    identity: &VerifiedRequestIdentity,
    action: &WorkspaceAction,
) -> Result<Value, ApiError> {
    if !principal(identity) || !developer_action(action) || !allowed_check(workspace, action) {
        return Err(ApiError::forbidden());
    }
    let verified = identity.central.as_ref().ok_or_else(ApiError::forbidden)?;
    let subject = require_token(verified, true)?;
    let pool = super::pool(state)?;
    project_access(pool, identity.claims.sub, workspace.project_id, true).await?;
    let namespace = namespace(pool, workspace).await?;
    let task = if let WorkspaceAction::BeginSession { task } = action {
        tracker::read_task(
            &namespace,
            workspace.tracker_instance_id,
            workspace.tracker_project_id,
            task,
        )
        .await?
        .0
    } else {
        store::session(pool, workspace.id)
            .await
            .map_err(super::error)?
            .filter(|s| s.actor_id == identity.claims.sub && s.owner_subject == subject)
            .ok_or_else(ApiError::forbidden)?
            .task_id
    };
    let (assignment, preparation) = tracker::read_assignment(
        &namespace,
        workspace,
        task,
        subject,
        AssignmentAccess::for_action(action),
    )
    .await?;
    if let Some(preparation) = preparation {
        preparation.require_source(action)?;
    }
    if !matches!(action, WorkspaceAction::BeginSession { .. }) {
        let saved:Option<Value>=sqlx::query_scalar("SELECT machine_assignment FROM development_workspace_sessions WHERE workspace_id=$1 AND ended_at IS NULL AND actor_id=$2 AND owner_subject=$3")
            .bind(workspace.id).bind(identity.claims.sub).bind(subject).fetch_optional(pool).await.map_err(ApiError::internal)?.flatten();
        let saved: DeveloperAssignment =
            serde_json::from_value(saved.ok_or_else(ApiError::forbidden)?)
                .map_err(|_| ApiError::forbidden())?;
        assignment.require_same(&saved)?;
    }
    let credential = Credential {
        workspace: workspace.id,
        operation,
        actor: identity.claims.sub,
        subject,
        issuer: verified.issuer.clone(),
        assignment,
        token: verified.context.token.clone(),
    };
    let plain = serde_json::to_string(&credential).map_err(|_| ApiError::forbidden())?;
    let encrypted =
        crate::platform::encrypt_secret(&plain, &state.config.secrets).map_err(|_| {
            ApiError::service_unavailable("workspace_machine_credential_storage_unavailable")
        })?;
    identity.original_deadline()?;
    Ok(json!({"sealed":encrypted}))
}

pub(super) struct Confirmed {
    pub assignment: DeveloperAssignment,
    pub clock: GitDeadline,
    preparation: Option<super::assignment::WorkspacePreparation>,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn verify(
    pool: &PgPool,
    workspace: &Workspace,
    operation: Uuid,
    actor: Uuid,
    subject: Uuid,
    action: &WorkspaceAction,
    saved: &Value,
    secrets: &crate::config::SecretsConfig,
) -> Result<Confirmed, ApiError> {
    if !developer_action(action) || !allowed_check(workspace, action) {
        return Err(ApiError::forbidden());
    }
    let encrypted = saved["sealed"]
        .as_str()
        .filter(|s| s.len() < 65536)
        .ok_or_else(ApiError::forbidden)?;
    let plain =
        crate::platform::decrypt_secret(encrypted, secrets).map_err(|_| ApiError::forbidden())?;
    let credential: Credential = serde_json::from_str(&plain).map_err(|_| ApiError::forbidden())?;
    if credential.workspace != workspace.id
        || credential.operation != operation
        || credential.actor != actor
        || credential.subject != subject
    {
        return Err(ApiError::forbidden());
    }
    let verified = crate::central_auth::try_verified_central(&credential.token)
        .await?
        .ok_or_else(ApiError::forbidden)?;
    if require_token(&verified, true)? != subject || verified.issuer != credential.issuer {
        return Err(ApiError::forbidden());
    }
    let proof = confirm_identity(
        pool,
        workspace,
        actor,
        verified,
        &credential.assignment,
        true,
        AssignmentAccess::for_action(action),
    )
    .await?;
    if let Some(preparation) = &proof.preparation {
        preparation.require_source(action)?;
    }
    Ok(proof)
}

pub(super) async fn confirm_identity(
    pool: &PgPool,
    workspace: &Workspace,
    actor: Uuid,
    verified: VerifiedCentralToken,
    saved: &DeveloperAssignment,
    write: bool,
    access: AssignmentAccess,
) -> Result<Confirmed, ApiError> {
    let subject = require_token(&verified, write)?;
    let profile: Option<(String, Option<String>, bool)> =
        sqlx::query_as("SELECT role,central_sub,enabled FROM users WHERE id=$1")
            .bind(actor)
            .fetch_optional(pool)
            .await
            .map_err(ApiError::internal)?;
    if profile != Some(("service_account".into(), Some(subject.to_string()), true)) {
        return Err(ApiError::forbidden());
    }
    project_access(pool, actor, workspace.project_id, write).await?;
    let namespace = namespace(pool, workspace).await?;
    let (assignment, preparation) =
        tracker::read_assignment(&namespace, workspace, saved.task_id, subject, access).await?;
    assignment.require_same(saved)?;
    let clock = GitDeadline::identity(&verified);
    let clock = match &preparation {
        Some(preparation) => clock.cap_until(preparation.expires_at),
        None => clock,
    };
    clock.require()?;
    Ok(Confirmed {
        assignment,
        clock,
        preparation,
    })
}

pub(crate) async fn pipeline_clock(
    pool: &PgPool,
    pipeline: Uuid,
    attempt: Uuid,
) -> Result<Option<GitDeadline>, ApiError> {
    let row:Option<(Uuid,Uuid,Uuid,Uuid,Value,Value)>=sqlx::query_as("SELECT o.id,o.workspace_id,o.actor_id,o.owner_subject,o.request,o.execution_plan FROM development_workspace_operations o JOIN pipelines p ON p.id=o.pipeline_id JOIN stages s ON s.pipeline_id=p.id JOIN jobs j ON j.stage_id=s.id JOIN execution_attempts a ON a.job_id=j.id WHERE p.id=$1 AND a.id=$2 AND o.execution_plan ? 'developerAuthority'")
        .bind(pipeline).bind(attempt).fetch_optional(pool).await.map_err(ApiError::internal)?;
    let Some((operation, workspace, actor, subject, request, plan)) = row else {
        return Ok(None);
    };
    let workspace = store::workspace(pool, workspace)
        .await
        .map_err(super::error)?;
    let action: WorkspaceAction =
        serde_json::from_value(request).map_err(|_| ApiError::forbidden())?;
    let secrets = crate::config::secrets_config_from_env().map_err(|_| {
        ApiError::service_unavailable("workspace_machine_credential_storage_unavailable")
    })?;
    let proof = verify(
        pool,
        &workspace,
        operation,
        actor,
        subject,
        &action,
        &plan["developerAuthority"],
        &secrets,
    )
    .await?;
    Ok(Some(proof.clock))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workspace_routes_do_not_open_registration_recovery_git_or_deployment() {
        let id = Uuid::new_v4();
        for (method, path) in [
            ("GET", format!("/api/v1/development-workspaces/{id}")),
            (
                "GET",
                format!("/api/v1/projects/{id}/development-workspace-profiles"),
            ),
            ("GET", format!("/api/v1/pipelines/{id}")),
            ("GET", format!("/api/v1/jobs/{id}/logs")),
            (
                "POST",
                format!("/api/v1/development-workspaces/{id}/operations"),
            ),
            (
                "GET",
                format!("/api/v1/projects/{id}/development-workspaces"),
            ),
            (
                "GET",
                format!(
                    "/api/v1/development-workspaces/{id}/operations/{}",
                    Uuid::new_v4()
                ),
            ),
        ] {
            assert!(handles(method, &path));
        }
        for (method, path) in [
            (
                "POST",
                format!("/api/v1/projects/{id}/development-workspaces"),
            ),
            (
                "POST",
                format!("/api/v1/development-workspaces/{id}/operations/{id}/reconcile"),
            ),
            ("POST", format!("/api/v1/projects/{id}/pipelines")),
            ("GET", format!("/api/v1/catalog/repositories/{id}")),
            (
                "GET",
                format!("/api/v1/development-workspaces/{id}/../{id}"),
            ),
        ] {
            assert!(!handles(method, &path));
        }
    }
}
