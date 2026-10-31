//! One durable command owner per instance; CI execution remains in Forge's runner.
use super::native;
use crate::{
    api::{ApiError, AppState},
    authz::Role,
    pipeline_authority,
};
use cicd_domain::development_workspace::WorkspaceAction;
use cicd_infra::development_workspace::{self as store, Workspace};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

#[derive(sqlx::FromRow)]
struct Work {
    id: Uuid,
    workspace_id: Uuid,
    actor_id: Uuid,
    owner_subject: Uuid,
    recovery_actor_id: Option<Uuid>,
    recovery_owner_subject: Option<Uuid>,
    request: Value,
    execution_plan: Value,
    state: String,
    pipeline_id: Option<Uuid>,
    effect_id: Option<Uuid>,
    reconcile_only: bool,
}

pub async fn supervisor(pool: PgPool, config: crate::config::RuntimeConfig) {
    if config.development_workspaces.catalog_root.is_none() {
        return;
    }
    let state = Arc::new(AppState {
        namespace_admission_pool: Some(
            pool.options()
                .clone()
                .max_connections(2)
                .min_connections(0)
                .connect_lazy_with((*pool.connect_options()).clone()),
        ),
        pool: Some(pool),
        auth_secret: config.auth.secret.clone(),
        git: config.git.to_git_config(),
        config,
        running_jobs: None,
        rate_limiter: Arc::new(crate::rate_limit::RateLimiter::default()),
    });
    loop {
        if let Err(error) = tick(&state).await {
            tracing::warn!(status=%error.status,"development workspace reconciliation pending");
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

pub(super) async fn tick(state: &AppState) -> Result<(), ApiError> {
    let _continuation = crate::quiescence::enter(false)
        .map_err(|_| ApiError::service_unavailable("workspace_execution_admission_closed"))?;
    let pool = super::pool(state)?;
    let ids:Vec<Uuid>=sqlx::query_scalar("SELECT id FROM development_workspace_operations WHERE state IN ('queued','running') ORDER BY created_at,id LIMIT 32")
        .fetch_all(pool).await.map_err(ApiError::internal)?;
    for id in ids {
        // Transaction-scoped lock cannot leak through a pooled connection.
        let mut ownership = pool.begin().await.map_err(ApiError::internal)?;
        let acquired: bool =
            sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1,473912))")
                .bind(id.to_string())
                .fetch_one(&mut *ownership)
                .await
                .map_err(ApiError::internal)?;
        if !acquired {
            continue;
        }
        let Some(work):Option<Work>=sqlx::query_as("SELECT * FROM development_workspace_operations WHERE id=$1 AND state IN ('queued','running')")
            .bind(id).fetch_optional(pool).await.map_err(ApiError::internal)? else { continue; };
        if work.state == "running" && work.pipeline_id.is_none() {
            finish(
                pool,
                &work,
                "unknown",
                json!({"reason":"worker_interrupted","requiresReconciliation":true}),
            )
            .await?;
            continue;
        }
        if work.pipeline_id.is_some() {
            poll_pipeline(pool, &work).await?;
            continue;
        }
        let effect = crate::quiescence::enter(true)
            .map_err(|_| ApiError::service_unavailable("workspace_execution_admission_closed"))?;
        // Persist custody before any native process or pipeline can be admitted.
        sqlx::query("UPDATE development_workspace_operations SET state='running',worker_id=$2,effect_id=COALESCE(effect_id,$3),started_at=COALESCE(started_at,clock_timestamp()) WHERE id=$1 AND state='queued'")
            .bind(id).bind(Uuid::new_v4()).bind(effect.workspace_effect_id()).execute(pool).await.map_err(ApiError::internal)?;
        let outcome = execute(state, &work, &effect).await;
        match outcome {
            Ok(Some(result)) => {
                let terminal = result["state"]
                    .as_str()
                    .filter(|value| matches!(*value, "succeeded" | "failed" | "unknown"))
                    .unwrap_or("unknown")
                    .to_owned();
                finish(pool, &work, &terminal, result).await?;
                if terminal != "unknown" && work.reconcile_only {
                    if let Some(original) = work.effect_id {
                        crate::quiescence::retire_workspace_effect(original).map_err(|_| {
                            ApiError::service_unavailable(
                                "workspace_original_custody_retirement_pending",
                            )
                        })?;
                    }
                }
            }
            Ok(None) => {}
            Err(error) => {
                // A native adapter error may follow publication. Preserve its key;
                // permission/preflight errors cannot be mistaken for success.
                let native = matches!(
                    work.request["action"].as_str(),
                    Some("register" | "create" | "observe" | "start" | "stop")
                );
                finish(pool,&work,if native || error.status.is_server_error() {"unknown"}else{"failed"},json!({"reason":if native {"native_result_unconfirmed"}else{"command_rejected"},"status":error.status.as_u16()})).await?;
            }
        }
    }
    Ok(())
}

async fn execute(
    state: &AppState,
    work: &Work,
    effect: &crate::quiescence::Lease,
) -> Result<Option<Value>, ApiError> {
    let pool = super::pool(state)?;
    let workspace = store::workspace(pool, work.workspace_id)
        .await
        .map_err(super::error)?;
    if work.execution_plan.get("environmentHash").is_some() {
        let environment =
            super::catalog::execution_target(pool, workspace.environment_id, workspace.project_id)
                .await?;
        if store::request_hash(&environment).map_err(super::error)? != workspace.environment_hash {
            return Err(ApiError::conflict("workspace_environment_target_drift"));
        }
    }
    let action: WorkspaceAction = serde_json::from_value(work.request.clone())
        .map_err(|_| ApiError::conflict("workspace_saved_command_invalid"))?;
    let actor = if work.reconcile_only {
        work.recovery_actor_id.ok_or_else(ApiError::forbidden)?
    } else {
        work.actor_id
    };
    let subject = if work.reconcile_only {
        work.recovery_owner_subject
            .ok_or_else(ApiError::forbidden)?
    } else {
        work.owner_subject
    };
    let user: Option<(bool, String, Option<String>)> =
        sqlx::query_as("SELECT enabled,role,central_sub FROM users WHERE id=$1")
            .bind(actor)
            .fetch_optional(pool)
            .await
            .map_err(ApiError::internal)?;
    let (enabled, local_role, central) = user.ok_or_else(ApiError::forbidden)?;
    if !enabled || local_role == "service_account" {
        return Err(ApiError::forbidden());
    }
    let owner = if crate::central_auth::application_policy_required()? {
        if central.as_deref() != Some(subject.to_string().as_str()) {
            return Err(ApiError::forbidden());
        }
        Some(crate::product_tokens::current_owner(subject).await?)
    } else {
        None
    };
    let origin = owner
        .as_ref()
        .map(pipeline_authority::Origin::from_owner)
        .transpose()?;
    let global = if let Some(owner) = &owner {
        if owner.decision.role == pdlc_auth_core::application_access::HumanRole::Admin {
            Role::Admin
        } else {
            Role::Developer
        }
    } else {
        Role::parse(&local_role).ok_or_else(ApiError::forbidden)?
    };
    let minimum = if work.reconcile_only
        || matches!(
            action,
            WorkspaceAction::Create {}
                | WorkspaceAction::Register {}
                | WorkspaceAction::RecoverSession { .. }
        ) {
        Role::Maintainer
    } else {
        Role::Developer
    };
    let storage: String =
        sqlx::query_scalar("SELECT storage_name FROM repository_catalog WHERE id=$1")
            .bind(workspace.repository_id)
            .fetch_one(pool)
            .await
            .map_err(ApiError::internal)?;
    if !crate::api::authz_mw::repository_scope_allows(
        pool,
        &storage,
        actor,
        global,
        minimum,
        Some(workspace.project_id),
    )
    .await?
    {
        return Err(ApiError::forbidden());
    }
    let permit =
        crate::space_admission::saved_repository(pool, &storage, actor, owner.as_ref(), minimum)
            .await?;
    pipeline_authority::with_origin(origin,async {
        let mut tx=crate::repository_catalog::catalog_write_lease(state,workspace.repository_id).await?;
        if let Some(permit)=&permit {permit.lock(&mut tx).await?;}
        if let Some(origin)=origin {origin.bind(&mut tx).await?;}
        sqlx::query("SELECT id FROM development_workspaces WHERE id=$1 FOR UPDATE").bind(workspace.id).fetch_one(&mut *tx).await.map_err(ApiError::internal)?;
        if work.reconcile_only && matches!(action,WorkspaceAction::Prepare{..}|WorkspaceAction::Check{..}|WorkspaceAction::Build{..}) {
            let key=format!("pipeline-trigger:{}:development-workspace:{}",workspace.project_id,work.id);
            sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))").bind(key).execute(&mut *tx).await.map_err(ApiError::internal)?;
            let original:Option<Uuid>=sqlx::query_scalar("SELECT t.pipeline_id FROM pipeline_triggers t JOIN pipelines p ON p.id=t.pipeline_id WHERE t.project_id=$1 AND t.source='development-workspace' AND t.idempotency_key=$2 AND p.variables->>'development_workspace_id'=$3 AND p.variables->>'development_workspace_operation_id'=$2")
                .bind(workspace.project_id).bind(work.id.to_string()).bind(workspace.id.to_string()).fetch_optional(&mut *tx).await.map_err(ApiError::internal)?;
            if let Some(pipeline)=original {
                sqlx::query("UPDATE development_workspace_operations SET pipeline_id=$2 WHERE id=$1 AND state='running'").bind(work.id).bind(pipeline).execute(&mut *tx).await.map_err(ApiError::internal)?;
            }else{
                finish_tx(&mut tx,work,&json!({"state":"failed","reason":"pipeline_submission_not_recorded","reconciled":true})).await?;
            }
            if let Some(original)=work.effect_id {crate::quiescence::retire_workspace_effect(original).map_err(|_|ApiError::service_unavailable("workspace_original_custody_retirement_pending"))?;}
            if let Some(permit)=&permit {permit.require()?;}
            tx.commit().await.map_err(ApiError::internal)?;
            return Ok(None);
        }
        if work.reconcile_only && matches!(action,WorkspaceAction::BeginSession{..}|WorkspaceAction::EndSession{..}|WorkspaceAction::RecoverSession{..}) {
            // These effects and their terminal receipt commit in one SQL transaction.
            // An unresolved row proves no metadata transaction committed; recovery
            // observes that fact instead of replaying under a different principal.
            if let Some(original)=work.effect_id {crate::quiescence::retire_workspace_effect(original).map_err(|_|ApiError::service_unavailable("workspace_original_custody_retirement_pending"))?;}
            finish_tx(&mut tx,work,&json!({"state":"failed","reason":"metadata_transaction_not_committed","reconciled":true})).await?;
            if let Some(permit)=&permit {permit.require()?;}
            tx.commit().await.map_err(ApiError::internal)?;
            return Ok(None);
        }
        let result = match &action {
            WorkspaceAction::BeginSession{task} => {
                let project:String=sqlx::query_scalar("SELECT project_key FROM forge_workspace_projects WHERE tracker_instance_id=$1 AND tracker_project_id=$2 AND state='active'")
                    .bind(workspace.tracker_instance_id).bind(workspace.tracker_project_id).fetch_one(&mut *tx).await.map_err(ApiError::internal)?;
                let (task_id,task_ref)=crate::task_delivery::tracker::read_workspace_issue(&state.config.sdlc_workspace,task,&project,workspace.tracker_project_id).await?;
                sqlx::query("INSERT INTO development_workspace_sessions(id,workspace_id,actor_id,owner_subject,task_id,task_ref,sources) VALUES($1,$2,$3,$4,$5,$6,$7)")
                    .bind(work.id).bind(workspace.id).bind(work.actor_id).bind(work.owner_subject).bind(task_id).bind(task_ref).bind(&workspace.sources).execute(&mut *tx).await.map_err(ApiError::internal)?;
                json!({"state":"succeeded","sessionId":work.id})
            }
            WorkspaceAction::EndSession{session_id}|WorkspaceAction::RecoverSession{session_id} => {
                if store::workspace_busy(&mut tx,workspace.id).await.map_err(super::error)? {return Err(ApiError::conflict("workspace_has_active_pipeline"));}
                let changed=sqlx::query("UPDATE development_workspace_sessions SET ended_at=clock_timestamp() WHERE id=$1 AND workspace_id=$2 AND ended_at IS NULL")
                    .bind(session_id).bind(workspace.id).execute(&mut *tx).await.map_err(ApiError::internal)?;
                if changed.rows_affected()!=1 {return Err(ApiError::conflict("workspace_session_changed"));}
                json!({"state":"succeeded","sessionId":session_id,"recovered":matches!(action,WorkspaceAction::RecoverSession{..})})
            }
            WorkspaceAction::Register {}|WorkspaceAction::Observe {}|WorkspaceAction::Start {}|WorkspaceAction::Stop {}|WorkspaceAction::Create {} => {
                let command=match action {WorkspaceAction::Start {}=>"start",WorkspaceAction::Stop {}=>"stop",WorkspaceAction::Create {}=>"create",_=>"observe"};
                let result=native::operate(state,&workspace,work.id,command,work.reconcile_only,effect,&work.execution_plan).await?;
                if let Some((runtime,desired))=native_projection(work.request["action"].as_str().unwrap_or(""),&result) {
                    sqlx::query("UPDATE development_workspaces SET runtime=$2,observed_at=clock_timestamp(),observation=$3,desired_runtime=COALESCE($4,desired_runtime) WHERE id=$1")
                        .bind(workspace.id).bind(runtime).bind(&result).bind(desired).execute(&mut *tx).await.map_err(ApiError::internal)?;
                }
                result
            }
            WorkspaceAction::Prepare{sources,local} => {
                if local.profile_hash!=workspace.profile_hash {return Err(ApiError::conflict("workspace_local_profile_mismatch"));}
                let expected_tools=workspace.profile["tools"].as_array().ok_or_else(||ApiError::conflict("workspace_profile_tools_missing"))?.iter().filter(|tool|tool["location"]!="forge").collect::<Vec<_>>();
                if expected_tools.len()!=local.tools.len() || expected_tools.iter().any(|expected|local.tools.iter().filter(|actual|expected["name"]==actual.name && expected["version"]==actual.requirement).count()!=1) {return Err(ApiError::conflict("workspace_local_tool_report_incomplete"));}
                let bindings=work.execution_plan["repositories"].as_array().ok_or_else(||ApiError::conflict("workspace_source_bindings_missing"))?;
                if bindings.len()!=sources.len() || sources.iter().any(|source|!bindings.iter().any(|item|item["key"]==source.key && item["repository_id"]==source.repository_id.to_string())) {return Err(ApiError::conflict("workspace_source_bindings_mismatch"));}
                for source in sources {
                    let repository:String=sqlx::query_scalar("SELECT storage_name FROM repository_catalog WHERE id=$1").bind(source.repository_id).fetch_one(&mut *tx).await.map_err(ApiError::internal)?;
                    if !crate::api::authz_mw::repository_scope_allows(pool,&repository,actor,global,Role::Developer,None).await? {return Err(ApiError::forbidden());}
                    confirm_commit(state,&repository,&source.commit).await?;
                }
                sqlx::query("UPDATE development_workspaces SET sources=$2,local_preparation=$3,preparation='unprepared' WHERE id=$1")
                    .bind(workspace.id).bind(serde_json::to_value(sources).unwrap()).bind(serde_json::to_value(local).unwrap()).execute(&mut *tx).await.map_err(ApiError::internal)?;
                let selected=sqlx::query("UPDATE development_workspace_sessions SET sources=$2 WHERE workspace_id=$1 AND actor_id=$3 AND owner_subject=$4 AND ended_at IS NULL")
                    .bind(workspace.id).bind(serde_json::to_value(sources).unwrap()).bind(work.actor_id).bind(work.owner_subject).execute(&mut *tx).await.map_err(ApiError::internal)?;
                if selected.rows_affected()!=1 {return Err(ApiError::conflict("workspace_session_changed"));}
                tx.commit().await.map_err(ApiError::internal)?;
                return launch_pipeline(state,work,&workspace,sources.iter().find(|source|source.repository_id==workspace.repository_id).map(|source|source.commit.as_str()).ok_or_else(||ApiError::conflict("workspace_primary_source_missing"))?).await;
            }
            WorkspaceAction::Check{source_commit,..}|WorkspaceAction::Build{source_commit} => {
                if workspace.preparation!="prepared" || !workspace.sources.as_array().is_some_and(|sources|sources.iter().any(|source|source["repository_id"]==workspace.repository_id.to_string() && source["commit"]==*source_commit)) {return Err(ApiError::conflict("workspace_selected_source_not_prepared"));}
                tx.commit().await.map_err(ApiError::internal)?;
                return launch_pipeline(state,work,&workspace,source_commit).await;
            }
        };
        if let Some(permit)=&permit {permit.require()?;}
        if work.reconcile_only && result["state"]!="unknown" && matches!(action,WorkspaceAction::Register {}|WorkspaceAction::Create {}|WorkspaceAction::Observe {}|WorkspaceAction::Start {}|WorkspaceAction::Stop {}) {
            if let Some(original)=work.effect_id {crate::quiescence::retire_workspace_effect(original).map_err(|_|ApiError::service_unavailable("workspace_original_custody_retirement_pending"))?;}
        }
        finish_tx(&mut tx,work,&result).await?;
        tx.commit().await.map_err(ApiError::internal)?;
        // SQL-only effects and their result commit together. No second update
        // is allowed by the immutable terminal history trigger.
        Ok(None)
    }).await
}

async fn confirm_commit(state: &AppState, storage: &str, commit: &str) -> Result<(), ApiError> {
    let name = crate::git_host::validate_repo_name(storage)
        .map_err(|_| ApiError::conflict("workspace_repository_identity_invalid"))?;
    let mut command = tokio::process::Command::new("git");
    command
        .arg(format!(
            "--git-dir={}",
            state.git.root.join(format!("{name}.git")).display()
        ))
        .args([
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{commit}^{{commit}}"),
        ])
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(5), command.output())
        .await
        .map_err(|_| ApiError::service_unavailable("workspace_source_observation_timeout"))?
        .map_err(|_| ApiError::conflict("workspace_source_unavailable"))?;
    if !output.status.success() || String::from_utf8_lossy(&output.stdout).trim() != commit {
        return Err(ApiError::conflict("workspace_source_commit_missing"));
    }
    Ok(())
}

async fn launch_pipeline(
    state: &AppState,
    work: &Work,
    workspace: &Workspace,
    commit: &str,
) -> Result<Option<Value>, ApiError> {
    let pool = super::pool(state)?;
    let pipeline = &work.execution_plan["pipeline"];
    let raw = serde_json::to_string(pipeline)
        .map_err(|_| ApiError::conflict("workspace_pipeline_invalid"))?;
    let outcome=crate::api::pipelines_routes::create_development_pipeline(pool,workspace.project_id,commit,&raw,
        json!({"development_workspace_id":workspace.id,"development_workspace_operation_id":work.id,"development_workspace_profile_hash":workspace.profile_hash}),&work.id.to_string(),&state.git.root).await?;
    sqlx::query("UPDATE development_workspace_operations SET pipeline_id=$2 WHERE id=$1 AND state='running'")
        .bind(work.id).bind(outcome.pipeline.id).execute(pool).await.map_err(ApiError::internal)?;
    Ok(None)
}
async fn poll_pipeline(pool: &PgPool, work: &Work) -> Result<(), ApiError> {
    let pipeline = work
        .pipeline_id
        .ok_or_else(|| ApiError::conflict("workspace_pipeline_missing"))?;
    let status: String = sqlx::query_scalar("SELECT status FROM pipelines WHERE id=$1")
        .bind(pipeline)
        .fetch_one(pool)
        .await
        .map_err(ApiError::internal)?;
    if !matches!(status.as_str(), "success" | "failed" | "canceled") {
        return Ok(());
    }
    let mut tx = pool.begin().await.map_err(ApiError::internal)?;
    sqlx::query("SELECT id FROM development_workspaces WHERE id=$1 FOR UPDATE")
        .bind(work.workspace_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(ApiError::internal)?;
    if store::workspace_busy(&mut tx, work.workspace_id)
        .await
        .map_err(super::error)?
    {
        return Ok(());
    }
    if work.request["action"] == "prepare" {
        sqlx::query("UPDATE development_workspaces SET preparation=$2 WHERE id=$1")
            .bind(work.workspace_id)
            .bind(if status == "success" {
                "prepared"
            } else {
                "degraded"
            })
            .execute(&mut *tx)
            .await
            .map_err(ApiError::internal)?;
    }
    finish_tx(&mut tx,work,&json!({"state":if status=="success" {"succeeded"}else{"failed"},"pipelineId":pipeline,"pipelineStatus":status})).await?;
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(())
}
async fn finish_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    work: &Work,
    result: &Value,
) -> Result<(), ApiError> {
    let state = result["state"]
        .as_str()
        .filter(|value| matches!(*value, "succeeded" | "failed" | "unknown"))
        .unwrap_or("unknown");
    let changed=sqlx::query("UPDATE development_workspace_operations SET state=$2,result=$3,finished_at=CASE WHEN $2 IN ('succeeded','failed') THEN clock_timestamp() ELSE NULL END WHERE id=$1 AND state IN ('queued','running','unknown')")
        .bind(work.id).bind(state).bind(result).execute(&mut **tx).await.map_err(ApiError::internal)?;
    if changed.rows_affected() == 1 {
        sqlx::query("UPDATE development_workspaces SET version=version+1 WHERE id=$1")
            .bind(work.workspace_id)
            .execute(&mut **tx)
            .await
            .map_err(ApiError::internal)?;
    }
    Ok(())
}
async fn finish(
    pool: &PgPool,
    work: &Work,
    state: &str,
    mut result: Value,
) -> Result<(), ApiError> {
    result["state"] = json!(state);
    let mut tx = pool.begin().await.map_err(ApiError::internal)?;
    finish_tx(&mut tx, work, &result).await?;
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(())
}

/// Preserve accepted lifecycle intent even when a fenced readback finds a
/// partial failure. In particular a failed Stop must never authorize delivery.
fn native_projection<'a>(action: &str, result: &'a Value) -> Option<(&'a str, Option<&'a str>)> {
    let runtime = if result["state"] == "unknown" {
        "unknown"
    } else {
        result["result"]["observation"]["runtime"]
            .as_str()
            .or_else(|| result["observation"]["runtime"].as_str())?
    };
    if !matches!(runtime, "running" | "stopped" | "degraded" | "unknown") {
        return None;
    }
    let desired = match action {
        "start" => Some("running"),
        "stop" | "create" => Some("stopped"),
        "register" if matches!(runtime, "running" | "stopped") => Some(runtime),
        _ => None,
    };
    Some((runtime, desired))
}

#[cfg(test)]
mod tests {
    use super::native_projection;
    use serde_json::json;
    #[test]
    fn fenced_failed_stop_keeps_stopped_intent_and_unknown_is_not_a_running_observation() {
        assert_eq!(
            native_projection(
                "stop",
                &json!({"state":"failed","observation":{"runtime":"running"}})
            ),
            Some(("running", Some("stopped")))
        );
        assert_eq!(
            native_projection(
                "start",
                &json!({"state":"failed","observation":{"runtime":"degraded"}})
            ),
            Some(("degraded", Some("running")))
        );
        assert_eq!(
            native_projection(
                "observe",
                &json!({"state":"unknown","observation":{"runtime":"running"}})
            ),
            Some(("unknown", None))
        );
        assert_eq!(
            native_projection(
                "register",
                &json!({"state":"succeeded","result":{"observation":{"runtime":"stopped"}}})
            ),
            Some(("stopped", Some("stopped")))
        );
    }
}
