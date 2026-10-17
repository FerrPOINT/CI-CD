//! Existing pipeline/embedded-runner bridge. No runner credential or effect authority in readback.
use super::*;
use crate::{
    config::{SdlcWorkspaceConfig, SecretsConfig},
    delegated_credentials::{Identity, StoredCredential, TaskCredentialContext},
    runner_workspace::{OwnedWorkspace, preparation},
};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{path::Path as FsPath, time::Instant};

pub(crate) struct Submission {
    pub subject: Uuid,
    pub token: Option<Uuid>,
    pub delegated: Option<StoredCredential>,
    pub intent: TaskWorkspaceIntent,
    pub config: SdlcWorkspaceConfig,
    pub secrets: SecretsConfig,
}

fn hash(value: &impl serde::Serialize) -> Result<String, ApiError> {
    Ok(format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(value).map_err(|_| ApiError::conflict("Task intent невалиден"))?
        )
    ))
}

fn canonical(value: Value) -> Value {
    match value {
        Value::Object(v) => Value::Object(
            v.into_iter()
                .map(|(k, v)| (k, canonical(v)))
                .collect::<std::collections::BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Value::Array(v) => Value::Array(v.into_iter().map(canonical).collect()),
        v => v,
    }
}

fn current(value: &Value, intent: &TaskWorkspaceIntent) -> anyhow::Result<Duration> {
    use anyhow::ensure;
    intent.validate().map_err(anyhow::Error::msg)?;
    let b = &intent.binding;
    let task = &value["receipt"]["task"];
    let assignment = &value["receipt"]["assignment"];
    let lease = &value["receipt"]["lease"];
    let routing = &task["routing"];
    let native = &value["receipt"]["binding"];
    let (role, stage) = match intent.role {
        WorkspaceRole::Analyst => ("analyst", "Analysis"),
        WorkspaceRole::Architect => ("architect", "Architecture"),
        WorkspaceRole::Developer => (
            "developer",
            task["stage"]
                .as_str()
                .filter(|s| matches!(*s, "Development" | "Rework"))
                .unwrap_or(""),
        ),
        WorkspaceRole::Reviewer => ("reviewer", "Review"),
        WorkspaceRole::Tester => ("tester", "Testing"),
        WorkspaceRole::Devops => ("devops", "Deployment"),
    };
    ensure!(
        !stage.is_empty()
            && value["runtime_state"] == "running"
            && task["runtime"] == "running"
            && task["task_id"] == json!(b.task_id)
            && task["stage"] == stage
            && task["status"] == "InProgress"
            && task.get("waiting_reason") == Some(&Value::Null)
            && task["owner_subject"]
                .as_str()
                .is_some_and(|s| !s.trim().is_empty())
            && task["owner_subject"] == assignment["owner_subject"]
            && task["cycle"].as_u64().is_some_and(|n| n <= 3),
        "task held or changed"
    );
    ensure!(
        assignment["task_id"] == json!(b.task_id)
            && assignment["assignment_id"] == json!(b.assignment_id)
            && assignment["execution_id"] == json!(b.execution_id)
            && assignment["execution"]["key"] == b.workflow_task_ref
            && assignment["agent_id"] == json!(intent.agent_id)
            && assignment["lease_id"] == json!(intent.tracker_lease_id)
            && assignment["role"] == role
            && assignment["stage"] == stage
            && assignment["cycle"] == task["cycle"]
            && assignment["attempt"].as_u64().is_some_and(|n| n > 0)
            && assignment["fencing_token"] == b.fencing_token
            && assignment["assignment_hash"] == b.assignment_hash
            && assignment["routing"] == *routing,
        "assignment changed"
    );
    let mut body = assignment.clone();
    body.as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("assignment absent"))?
        .remove("assignment_hash");
    ensure!(
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&canonical(body))?)
        ) == b.assignment_hash,
        "assignment hash mismatch"
    );
    ensure!(
        routing["tracker_instance_id"] == b.tracker_instance_id
            && routing["project_id"] == json!(b.tracker_project_id)
            && routing["task_id"] == json!(b.task_id)
            && routing["root_task_id"] == json!(b.root_task_id)
            && routing["snapshot_id"] == json!(b.routing_snapshot_id)
            && routing["requirement_revision"] == b.requirement_revision
            && routing["policy"]["tracker_instance_id"] == b.tracker_instance_id
            && routing["policy"]["project_id"] == json!(b.tracker_project_id)
            && routing["policy"]["routes"][role]["agent_id"] == json!(intent.agent_id),
        "routing changed"
    );
    ensure!(
        lease["lease_id"] == json!(intent.tracker_lease_id)
            && lease["version"]
                .as_i64()
                .is_some_and(|n| (intent.expected_lease_version..=9007199254740991).contains(&n))
            && lease["holder_subject"] == intent.holder_subject
            && lease["holder_subject"] != task["owner_subject"]
            && native["machine_subject"]
                .as_str()
                .is_some_and(|s| !s.trim().is_empty())
            && native["machine_subject"] != lease["holder_subject"]
            && native["machine_subject"] != task["owner_subject"]
            && ["run_id", "chat_id"].iter().all(|key| native[*key]
                .as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
                .is_some_and(|id| !id.is_nil())),
        "lease or native binding changed"
    );
    let time = |v: &Value| -> anyhow::Result<_> {
        Ok(chrono::DateTime::parse_from_rfc3339(
            v.as_str().ok_or_else(|| anyhow::anyhow!("clock absent"))?,
        )?)
    };
    let observed = time(&value["observed_at"])?;
    let expires = time(&lease["expires_at"])?;
    let heartbeat = time(&lease["heartbeat_at"])?;
    ensure!(
        time(&lease["claimed_at"])? <= heartbeat
            && heartbeat <= observed
            && observed < expires
            && (expires - heartbeat).num_milliseconds() <= 30_000,
        "lease expired"
    );
    Ok((expires - observed).to_std()?)
}

async fn tracker(
    config: &SdlcWorkspaceConfig,
    intent: &TaskWorkspaceIntent,
) -> Result<Instant, ApiError> {
    tracker_bound(config, intent, None).await
}

async fn tracker_bound(
    config: &SdlcWorkspaceConfig,
    intent: &TaskWorkspaceIntent,
    context: Option<&TaskCredentialContext>,
) -> Result<Instant, ApiError> {
    let (start, value) =
        crate::task_delivery::tracker::read_lifecycle(config, intent.binding.task_id).await?;
    if context.is_some_and(|context| !context.native_matches(&value)) {
        return Err(ApiError::conflict("Original native task run changed"));
    }
    let until = start
        + current(&value, intent).map_err(|_| {
            ApiError::conflict("Текущий Tracker task assignment или lease не совпадает")
        })?;
    if Instant::now() >= until {
        return Err(ApiError::conflict("Lease истёк во время readback"));
    }
    Ok(until)
}

async fn authorize_saved(
    conn: &mut sqlx::PgConnection,
    config: &SdlcWorkspaceConfig,
    record: &ledger::TaskPipelineJournal,
    secrets: &SecretsConfig,
) -> Result<Instant, ApiError> {
    if let Some(credential) = &record.delegated_credential {
        if record.service_account_id.is_some() || record.api_token_id.is_some() {
            return Err(ApiError::forbidden());
        }
        let stored = StoredCredential::from_value(&credential.0)?;
        return authorize_child(
            conn,
            config,
            secrets,
            record.project_id,
            &record.intent,
            &stored,
        )
        .await;
    }
    authorize_keys(
        conn,
        config,
        record.service_account_id.ok_or_else(ApiError::forbidden)?,
        record.api_token_id.ok_or_else(ApiError::forbidden)?,
        record.project_id,
        &record.intent,
    )
    .await?;
    tracker(config, &record.intent).await
}

async fn authorize_child(
    conn: &mut sqlx::PgConnection,
    config: &SdlcWorkspaceConfig,
    secrets: &SecretsConfig,
    project: Uuid,
    intent: &TaskWorkspaceIntent,
    stored: &StoredCredential,
) -> Result<Instant, ApiError> {
    let child_until = stored.verify(config, secrets, project, intent).await?;
    if !ledger::task_project_matches(conn, project, intent)
        .await
        .map_err(ApiError::internal)?
    {
        return Err(ApiError::conflict(
            "Task project/repository mapping changed",
        ));
    }
    bound_child_authority(
        child_until,
        tracker_bound(config, intent, Some(&stored.context)),
    )
    .await
}

async fn bound_child_authority<F>(child_until: Instant, tracker: F) -> Result<Instant, ApiError>
where
    F: std::future::Future<Output = Result<Instant, ApiError>>,
{
    if Instant::now() >= child_until {
        return Err(ApiError::forbidden());
    }
    let until = child_until.min(tracker.await?);
    if Instant::now() >= until {
        return Err(ApiError::forbidden());
    }
    Ok(until)
}

pub(crate) async fn admit_delegated(
    state: &AppState,
    identity: &Identity,
    project: Uuid,
    intent: TaskWorkspaceIntent,
    context: TaskCredentialContext,
) -> Result<Submission, ApiError> {
    if !state.config.runner.embedded_enabled {
        return Err(ApiError::service_unavailable(
            "Task preparation requires embedded runner",
        ));
    }
    identity.bind(&context, project, &intent)?;
    let stored = identity.store(context, &state.config.secrets)?;
    let mut tx = pool(state)?.begin().await.map_err(ApiError::internal)?;
    authorize_child(
        &mut tx,
        &state.config.sdlc_workspace,
        &state.config.secrets,
        project,
        &intent,
        &stored,
    )
    .await?;
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Submission {
        subject: identity.subject,
        token: None,
        delegated: Some(stored),
        intent,
        config: state.config.sdlc_workspace.clone(),
        secrets: state.config.secrets.clone(),
    })
}

async fn authorize_keys(
    conn: &mut sqlx::PgConnection,
    config: &SdlcWorkspaceConfig,
    subject: Uuid,
    token: Uuid,
    project: Uuid,
    intent: &TaskWorkspaceIntent,
) -> Result<(), ApiError> {
    if config.operation_subject != Some(subject) {
        return Err(ApiError::forbidden());
    }
    let valid: Option<Uuid> = sqlx::query_scalar("SELECT t.id FROM api_tokens t JOIN service_accounts s ON s.id=t.service_account_id WHERE t.id=$1 AND s.id=$2 AND t.project_id=$3 AND t.principal_type='service_account' AND s.enabled AND t.revoked_at IS NULL AND (t.expires_at IS NULL OR t.expires_at>clock_timestamp()) AND 'api:write'=ANY(t.scopes) FOR SHARE OF t,s")
        .bind(token).bind(subject).bind(project)
        .fetch_optional(&mut *conn).await.map_err(ApiError::internal)?;
    if valid.is_none() {
        return Err(ApiError::forbidden());
    }
    if !ledger::task_project_matches(conn, project, intent)
        .await
        .map_err(ApiError::internal)?
    {
        return Err(ApiError::conflict(
            "Task project/repository mapping изменён",
        ));
    }
    Ok(())
}

pub(crate) async fn admit(
    state: &AppState,
    claims: &AccessClaims,
    project: Uuid,
    intent: TaskWorkspaceIntent,
) -> Result<Submission, ApiError> {
    intent.validate().map_err(ApiError::bad_request)?;
    if !state.config.runner.embedded_enabled {
        return Err(ApiError::service_unavailable(
            "Task preparation требует embedded runner",
        ));
    }
    let mut tx = pool(state)?.begin().await.map_err(ApiError::internal)?;
    authorize(&mut tx, state, claims, project, "api:write").await?;
    if !ledger::task_project_matches(&mut tx, project, &intent)
        .await
        .map_err(ApiError::internal)?
    {
        return Err(ApiError::conflict(
            "Task project/repository mapping не совпадает",
        ));
    }
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Submission {
        subject: claims.sub,
        token: Some(claims.token_id.ok_or_else(ApiError::forbidden)?),
        delegated: None,
        intent,
        config: state.config.sdlc_workspace.clone(),
        secrets: state.config.secrets.clone(),
    })
}

pub(crate) async fn verify_submission(
    conn: &mut sqlx::PgConnection,
    project: Uuid,
    submission: &Submission,
) -> Result<Instant, ApiError> {
    if let Some(stored) = &submission.delegated {
        if submission.token.is_some() || stored.subject != submission.subject {
            return Err(ApiError::forbidden());
        }
        return authorize_child(
            conn,
            &submission.config,
            &submission.secrets,
            project,
            &submission.intent,
            stored,
        )
        .await;
    }
    authorize_keys(
        conn,
        &submission.config,
        submission.subject,
        submission.token.ok_or_else(ApiError::forbidden)?,
        project,
        &submission.intent,
    )
    .await?;
    tracker(&submission.config, &submission.intent).await
}

/// Rechecked just before actual job spawn; never returns a permit for legacy input.
pub(crate) async fn already_submitted(
    pool: &PgPool,
    pipeline: Uuid,
    attempt: Uuid,
) -> Result<bool, ApiError> {
    let mut conn = pool.acquire().await.map_err(ApiError::internal)?;
    ledger::task_attempt_submitted(&mut conn, pipeline, attempt)
        .await
        .map_err(ApiError::internal)
}

pub(crate) enum Spawn {
    Legacy,
    Permit(Instant),
    Held,
}

pub(crate) async fn before_spawn(
    pool: &PgPool,
    pipeline: Uuid,
    attempt: Uuid,
    config: &SdlcWorkspaceConfig,
    secrets: &SecretsConfig,
) -> Result<Spawn, ApiError> {
    let mut tx = pool.begin().await.map_err(ApiError::internal)?;
    let Some(record) = ledger::find_task_pipeline(&mut tx, pipeline, true)
        .await
        .map_err(ApiError::internal)?
    else {
        return Ok(Spawn::Legacy);
    };
    if ledger::task_attempt_submitted(&mut tx, pipeline, attempt)
        .await
        .map_err(ApiError::internal)?
    {
        return Ok(Spawn::Held);
    }
    let until = authorize_saved(&mut tx, config, &record, secrets).await?;
    if !ledger::claim_task_submission(&mut tx, pipeline, attempt)
        .await
        .map_err(ApiError::internal)?
    {
        return Ok(Spawn::Held);
    }
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Spawn::Permit(until))
}

/// Reauthorize central child at ordinary publication boundaries; local SAT behavior is unchanged.
pub(crate) async fn before_delegated_effect(
    pool: &PgPool,
    pipeline: Uuid,
    attempt: Uuid,
    config: &crate::runner::RuntimeRunnerConfig,
) -> Result<(), ApiError> {
    let mut tx = pool.begin().await.map_err(ApiError::internal)?;
    let Some(record) = ledger::find_task_pipeline(&mut tx, pipeline, false)
        .await
        .map_err(ApiError::internal)?
    else {
        return Ok(());
    };
    if record.delegated_credential.is_none() {
        return Ok(());
    }
    if !ledger::task_attempt_submitted(&mut tx, pipeline, attempt)
        .await
        .map_err(ApiError::internal)?
    {
        return Err(ApiError::conflict(
            "Original task submission is not confirmed",
        ));
    }
    let until = authorize_saved(&mut tx, &config.sdlc_workspace, &record, &config.secrets).await?;
    tx.commit().await.map_err(ApiError::internal)?;
    if Instant::now() >= until {
        return Err(ApiError::conflict(
            "Task authority expired before publication",
        ));
    }
    Ok(())
}

/// Verify an already submitted task attempt without claiming or replaying it.
pub(crate) async fn verify_submitted_attempt(
    pool: &PgPool,
    pipeline: Uuid,
    attempt: Uuid,
    config: &SdlcWorkspaceConfig,
    secrets: &SecretsConfig,
) -> Result<Instant, ApiError> {
    let mut tx = pool.begin().await.map_err(ApiError::internal)?;
    let record = ledger::find_task_pipeline(&mut tx, pipeline, false)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::conflict("Task pipeline отсутствует"))?;
    if !ledger::task_attempt_submitted(&mut tx, pipeline, attempt)
        .await
        .map_err(ApiError::internal)?
    {
        return Err(ApiError::conflict(
            "Исходная task submission не подтверждена",
        ));
    }
    let until = authorize_saved(&mut tx, config, &record, secrets).await?;
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(until)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn prepare(
    pool: &PgPool,
    pipeline: Uuid,
    job: Uuid,
    lease: Uuid,
    attempt: Uuid,
    generation: i64,
    config: &crate::runner::RuntimeRunnerConfig,
    root: &FsPath,
) -> Result<Option<OwnedWorkspace>, ApiError> {
    let mut tx = pool.begin().await.map_err(ApiError::internal)?;
    let Some(record) = ledger::find_task_pipeline(&mut tx, pipeline, true)
        .await
        .map_err(ApiError::internal)?
    else {
        return Ok(None);
    };
    let until = authorize_saved(&mut tx, &config.sdlc_workspace, &record, &config.secrets).await?;
    if job != record.preparation_job_id {
        return Ok(None);
    }
    let original = record.preparation_request.as_ref().map(|r| &r.0);
    let fresh = original.is_none();
    let request = match original {
        Some(request) => {
            let expected = record
                .intent
                .request(lease, attempt, generation, request.workspace_id.clone())
                .map_err(ApiError::conflict)?;
            if *request != expected {
                return Err(ApiError::conflict("Исходная preparation attempt неизменна"));
            }
            request.clone()
        }
        None => record
            .intent
            .request(
                lease,
                attempt,
                generation,
                format!("attempt-{attempt}-{generation}-{}", Uuid::new_v4().simple()),
            )
            .map_err(ApiError::conflict)?,
    };
    let workspace_id = request.workspace_id.clone();
    verify_lease(&mut tx, record.project_id, job, &request).await?;
    if fresh
        && !ledger::claim_task_preparation(&mut tx, pipeline, &request)
            .await
            .map_err(ApiError::internal)?
    {
        return Err(ApiError::conflict("Preparation уже начата"));
    }
    tx.commit().await.map_err(ApiError::internal)?;
    let storage = crate::repository_catalog::local_storage_for_project(pool, record.project_id)
        .await?
        .ok_or_else(|| ApiError::conflict("Hosted repository недоступен"))?;
    let storage = crate::git_host::validate_repo_name(&storage).map_err(ApiError::conflict)?;
    let source = preparation::PreparationSource {
        project_id: record.project_id,
        repository_id: request.repository_id,
        repository: config.git_root.join(format!("{storage}.git")),
        source_commit: request.source_commit.clone(),
    };
    let result = if fresh {
        preparation::prepare_with_deadline(root, &source, &request, Some(until)).await
    } else {
        preparation::readback(root, &source, &request).await
    };
    result.map_err(|_| ApiError::conflict("Preparation удержана; требуется original readback"))?;
    let mut tx = pool.begin().await.map_err(ApiError::internal)?;
    let until = authorize_saved(&mut tx, &config.sdlc_workspace, &record, &config.secrets).await?;
    verify_lease(&mut tx, record.project_id, job, &request).await?;
    if let Some(receipt) = find_owned_receipt(&mut tx, &record).await? {
        if receipt.request != request || receipt.request_hash != hash(&request)? {
            return Err(ApiError::conflict("Original receipt не совпадает"));
        }
    } else {
        let receipt = WorkspaceOperationReceipt {
            schema: "forge/workspace-operation-receipt/v1".into(),
            operation_id: Uuid::new_v4(),
            project_id: record.project_id,
            request_hash: hash(&request)?,
            request,
            recorded_at: Utc::now(),
            status: WorkspaceOperationStatus::Blocked,
            blockers: vec![
                WorkspaceOperationBlocker::TrackerAdmissionUnavailable,
                WorkspaceOperationBlocker::TrackerWorkspaceBindingUnavailable,
            ],
            physical_source_observed: true,
            dispatch_allowed: false,
        };
        if let Some(credential) = &record.delegated_credential {
            let stored = StoredCredential::from_value(&credential.0)?;
            ledger::append_delegated_receipt(
                &mut tx,
                stored.subject,
                &stored.context_hash,
                &receipt,
            )
            .await
            .map_err(ApiError::internal)?;
        } else {
            ledger::append_receipt(
                &mut tx,
                record.service_account_id.ok_or_else(ApiError::forbidden)?,
                &receipt,
            )
            .await
            .map_err(ApiError::internal)?;
        }
    }
    if Instant::now() >= until {
        return Err(ApiError::conflict("Lease истёк до receipt"));
    }
    sqlx::query("UPDATE job_leases SET acknowledged_at=COALESCE(acknowledged_at,clock_timestamp()) WHERE id=$1 AND runner_id IS NULL")
        .bind(lease).execute(&mut *tx).await.map_err(ApiError::internal)?;
    tx.commit().await.map_err(ApiError::internal)?;
    OwnedWorkspace::reopen(root, &workspace_id)
        .map(Some)
        .map_err(|_| ApiError::conflict("Owned workspace недоступен"))
}

async fn verify_lease(
    conn: &mut sqlx::PgConnection,
    project: Uuid,
    job: Uuid,
    request: &WorkspaceOperationRequest,
) -> Result<(), ApiError> {
    let observed = ledger::lease_observation(conn, project, request.lease_id, true)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::conflict("Lease отсутствует"))?;
    if observed.expired
        || observed.status != "active"
        || observed.attempt_id != request.attempt_id
        || observed.generation != request.workspace_generation
        || observed.current_generation != request.workspace_generation
        || observed.source_commit.as_deref() != Some(&request.source_commit)
    {
        return Err(ApiError::conflict("Source lease изменён"));
    }
    let valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM job_leases l JOIN execution_attempts a ON a.id=l.attempt_id AND a.job_id=l.job_id JOIN jobs j ON j.id=a.job_id JOIN stages s ON s.id=j.stage_id JOIN pipelines p ON p.id=s.pipeline_id WHERE l.id=$1 AND a.id=$2 AND l.generation=$3 AND j.id=$4 AND p.project_id=$5 AND p.commit_sha=$6 AND l.runner_id IS NULL AND l.runner_name='embedded' AND l.lease_status='active' AND l.lease_expires_at>clock_timestamp() AND l.cancel_requested_at IS NULL AND a.status='running' AND j.status='running' AND l.generation=(SELECT max(generation) FROM job_leases WHERE job_id=j.id))")
        .bind(request.lease_id).bind(request.attempt_id).bind(request.workspace_generation)
        .bind(job).bind(project).bind(&request.source_commit).fetch_one(conn).await.map_err(ApiError::internal)?;
    if !valid {
        return Err(ApiError::conflict("Текущий embedded lease не совпадает"));
    }
    Ok(())
}

/// Authenticated original locator: pending is null, not accepted/success; no lease token.
#[utoipa::path(get, path="/api/v1/projects/{project_id}/pipelines/{pipeline_id}/task-workspace",
    tag="SDLC workspace", params(("project_id"=Uuid, Path),("pipeline_id"=Uuid, Path)),
    responses((status=200,body=Option<WorkspaceOperationReceipt>),(status=403),(status=404)))]
pub(crate) async fn read_receipt(
    State(state): State<Arc<AppState>>,
    claims: Option<Extension<AccessClaims>>,
    child: Option<Extension<Identity>>,
    Path((project, pipeline)): Path<(Uuid, Uuid)>,
) -> ApiResult<Option<WorkspaceOperationReceipt>> {
    let mut tx = pool(&state)?.begin().await.map_err(ApiError::internal)?;
    if child.is_none() {
        let claims = claims.as_ref().ok_or_else(ApiError::forbidden)?;
        authorize(&mut tx, &state, &claims.0, project, "api:read").await?;
    }
    let record = ledger::find_task_pipeline(&mut tx, pipeline, false)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    if record.project_id != project {
        return Err(ApiError::forbidden());
    }
    match (child, claims, &record.delegated_credential) {
        (Some(Extension(identity)), None, Some(credential)) => {
            let stored = StoredCredential::from_value(&credential.0)?;
            if stored.subject != identity.subject || stored.fingerprint != identity.fingerprint() {
                return Err(ApiError::forbidden());
            }
            identity.bind(&stored.context, project, &record.intent)?;
            authorize_saved(
                &mut tx,
                &state.config.sdlc_workspace,
                &record,
                &state.config.secrets,
            )
            .await?;
        }
        (None, Some(Extension(claims)), None) => {
            if record.service_account_id != Some(claims.sub) {
                return Err(ApiError::forbidden());
            }
        }
        _ => return Err(ApiError::forbidden()),
    }
    let receipt = find_owned_receipt(&mut tx, &record).await?;
    if let Some(receipt) = &receipt {
        if record.preparation_request.as_ref().map(|r| &r.0) != Some(&receipt.request)
            || receipt.request_hash != hash(&receipt.request)?
        {
            return Err(ApiError::conflict("Original receipt locator не совпадает"));
        }
    }
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(receipt))
}

async fn find_owned_receipt(
    conn: &mut sqlx::PgConnection,
    record: &ledger::TaskPipelineJournal,
) -> Result<Option<WorkspaceOperationReceipt>, ApiError> {
    if let Some(credential) = &record.delegated_credential {
        let stored = StoredCredential::from_value(&credential.0)?;
        ledger::find_delegated_receipt(
            conn,
            record.project_id,
            stored.subject,
            &stored.context_hash,
            &record.intent.operation_key,
        )
        .await
        .map_err(ApiError::internal)
    } else {
        ledger::find_receipt(
            conn,
            record.project_id,
            record.service_account_id.ok_or_else(ApiError::forbidden)?,
            &record.intent.operation_key,
        )
        .await
        .map_err(ApiError::internal)
    }
}

#[cfg(test)]
mod tests;
