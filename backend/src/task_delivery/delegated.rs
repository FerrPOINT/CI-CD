//! Delivery authority committed by existing v1 variables_sha256, never by role alone.
use super::*;
use crate::api::ApiError;
use crate::delegated_credentials::{Identity, TaskCredentialContext, digest as canonical_hash};
use serde_json::Value;

const KIND: &str = "forge/task-delivery/v1";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeliveryVariables {
    pub forge_task_delivery: Binding,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireVariables {
    forge_task_delivery: String,
}

fn variables_value(value: &DeliveryVariables) -> Result<Value, ApiError> {
    let encoded =
        serde_json::to_string(&value.forge_task_delivery).map_err(|_| ApiError::forbidden())?;
    Ok(serde_json::json!({"forgeTaskDelivery":encoded}))
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Binding {
    pub kind: String,
    pub command: DeliveryCommand,
    pub source_pipeline_id: Uuid,
    pub development_result_sha256: String,
    pub target_policy_sha256: String,
    pub target_kind: TargetKind,
    pub artifact: Option<Artifact>,
    pub rollback_manifest_sha256: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    Static,
    Oci,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Artifact {
    pub id: Uuid,
    pub attempt_id: Uuid,
    pub sha256: String,
    pub size_bytes: i64,
    pub config_sha256: String,
    pub plan_sha256: String,
}

impl Binding {
    fn validate(&self, context: &TaskCredentialContext) -> Result<(), ApiError> {
        self.command.validate().map_err(ApiError::bad_request)?;
        let intent = &context.intent;
        let fence = self
            .command
            .task_fence
            .as_ref()
            .ok_or_else(ApiError::forbidden)?;
        let current = &intent.binding;
        let original = &self.command.original;
        if self.kind != KIND
            || intent.role != WorkspaceRole::Devops
            || self.source_pipeline_id.is_nil()
            || !digest(&self.development_result_sha256)
            || !digest(&self.target_policy_sha256)
            || current.task_id != original.task_id
            || current.root_task_id != original.root_task_id
            || current.assignment_id != fence.assignment_id
            || current.execution_id != fence.execution_id
            || current.assignment_hash != fence.assignment_hash
            || current.fencing_token != fence.fencing_token
            || intent.agent_id != fence.agent_id
            || intent.tracker_lease_id != fence.lease_id
            || intent.expected_lease_version != fence.expected_lease_version
        {
            return Err(ApiError::forbidden());
        }
        match (
            self.command.action,
            &self.artifact,
            &self.rollback_manifest_sha256,
        ) {
            (DeliveryAction::Deploy, Some(a), None)
                if Some(a.id) == self.command.artifact_id
                    && !a.attempt_id.is_nil()
                    && a.size_bytes > 0
                    && digest(&a.sha256)
                    && digest(&a.config_sha256)
                    && digest(&a.plan_sha256) =>
            {
                Ok(())
            }
            (DeliveryAction::Rollback, None, Some(hash)) if digest(hash) => Ok(()),
            _ => Err(ApiError::forbidden()),
        }
    }

    fn bind_source(
        &self,
        context: &TaskCredentialContext,
        original: &WorkspaceOperationReceipt,
    ) -> Result<(), ApiError> {
        self.validate(context)?;
        sdlc_workspace::verify_lookup(original, &self.command.original)?;
        let source = &original.request;
        let current = &context.intent;
        let a = &source.binding;
        let b = &current.binding;
        if original.project_id != context.project_id
            || source.operation_key != self.command.workspace_operation_key
            || source.role != WorkspaceRole::Developer
            || source.repository_id != current.repository_id
            || source.source_commit != current.source_commit
            || a.tracker_instance_id != b.tracker_instance_id
            || a.tracker_project_id != b.tracker_project_id
            || a.task_id != b.task_id
            || a.root_task_id != b.root_task_id
            || a.routing_snapshot_id != b.routing_snapshot_id
            || a.requirement_revision != b.requirement_revision
        {
            return Err(ApiError::forbidden());
        }
        Ok(())
    }

    fn candidate(&self, value: &CandidateEvidenceReadback) -> Result<(), ApiError> {
        let a = self.artifact.as_ref().ok_or_else(ApiError::forbidden)?;
        if !value.candidate_observed
            || value.pipeline_id != self.source_pipeline_id
            || value.config_sha256.as_ref() != Some(&a.config_sha256)
            || value.plan_sha256.as_ref() != Some(&a.plan_sha256)
            || value
                .artifacts
                .iter()
                .filter(|v| {
                    v.artifact_id == a.id
                        && v.attempt_id == a.attempt_id
                        && v.sha256 == a.sha256
                        && v.size_bytes == a.size_bytes
                })
                .count()
                != 1
        {
            return Err(ApiError::forbidden());
        }
        Ok(())
    }
}

/// Trusted server producer supplies retained evidence, never agent-selected authority.
/// Returns inputs for the EXISTING delegate request; this function issues no token.
pub fn delegation_inputs(
    mut context: TaskCredentialContext,
    variables: DeliveryVariables,
    key: &str,
    ttl: i64,
) -> Result<(TaskCredentialContext, Value, Value), ApiError> {
    variables.forge_task_delivery.validate(&context)?;
    let value = variables_value(&variables)?;
    if serde_json::to_vec(&value)
        .map_err(|_| ApiError::forbidden())?
        .len()
        > 16_384
    {
        return Err(ApiError::forbidden());
    }
    context.variables_sha256 = canonical_hash(&value)?;
    let request = crate::delegated_credentials::delegation_request(&context, key, ttl)?;
    Ok((context, value, request))
}

fn retained(variables: Value, context: &TaskCredentialContext) -> Result<Binding, ApiError> {
    if serde_json::to_vec(&variables)
        .map_err(|_| ApiError::forbidden())?
        .len()
        > 16_384
        || canonical_hash(&variables)? != context.variables_sha256
    {
        return Err(ApiError::forbidden());
    }
    let value: WireVariables =
        serde_json::from_value(variables).map_err(|_| ApiError::forbidden())?;
    let binding: Binding =
        serde_json::from_str(&value.forge_task_delivery).map_err(|_| ApiError::forbidden())?;
    binding.validate(context)?;
    Ok(binding)
}

pub(super) struct Session {
    pipeline: Uuid,
    identity: Identity,
    pub(super) binding: Binding,
    pub(super) original: WorkspaceOperationReceipt,
}

impl Session {
    fn custody(&self) -> Value {
        serde_json::json!({"schema":"forge/task-delivery-custody/v1", "subject":self.identity.subject,
            "contextHash":self.identity.grant_hash, "sourcePipelineId":self.binding.source_pipeline_id})
    }

    pub(super) fn check_journal(&self, dir: &Path) -> anyhow::Result<()> {
        let value: Value = read(&dir.join("task-child.json"))?;
        ensure!(
            value == self.custody(),
            "original delivery child context differs"
        );
        Ok(())
    }

    pub(super) fn record_journal(&self, dir: &Path) -> anyhow::Result<()> {
        immutable(&dir.join("task-child.json"), &self.custody())
    }

    async fn load(
        state: &AppState,
        conn: &mut sqlx::PgConnection,
        identity: &Identity,
        project: Uuid,
        key: &str,
    ) -> Result<Self, ApiError> {
        let ids: Vec<Uuid> = sqlx::query_scalar("SELECT pipeline_id FROM task_pipeline_intents WHERE project_id=$1 AND service_account_id IS NULL AND api_token_id IS NULL AND delegated_credential->>'context_hash'=$2 LIMIT 2")
            .bind(project).bind(&identity.grant_hash).fetch_all(&mut *conn).await.map_err(ApiError::internal)?;
        let [pipeline] = ids.as_slice() else {
            return Err(ApiError::forbidden());
        };
        let (record, binding) = current(conn, identity, *pipeline).await?;
        if binding.command.workspace_operation_key != key {
            return Err(ApiError::forbidden());
        }
        let source = ledger::find_task_pipeline(conn, binding.source_pipeline_id, false)
            .await
            .map_err(ApiError::internal)?
            .ok_or_else(ApiError::forbidden)?;
        if source.project_id != project || source.intent.role != WorkspaceRole::Developer {
            return Err(ApiError::forbidden());
        }
        let original = sdlc_workspace::task_pipeline::find_owned_receipt(conn, &source)
            .await?
            .ok_or_else(ApiError::forbidden)?;
        if source.preparation_request.as_ref().map(|v| &v.0) != Some(&original.request)
            || sha(&serde_json::to_vec(&original.request).map_err(|_| ApiError::forbidden())?)
                != original.request_hash
        {
            return Err(ApiError::forbidden());
        }
        let stored = crate::delegated_credentials::custody::current(conn, &record).await?;
        binding.bind_source(&stored.context, &original)?;
        let session = Self {
            pipeline: *pipeline,
            identity: identity.clone(),
            binding,
            original,
        };
        session.refresh(state, conn).await?;
        Ok(session)
    }

    pub(super) async fn refresh(
        &self,
        state: &AppState,
        conn: &mut sqlx::PgConnection,
    ) -> Result<tracker::Permit, ApiError> {
        let (record, binding) = current(conn, &self.identity, self.pipeline).await?;
        if canonical_hash(&binding)? != canonical_hash(&self.binding)? {
            return Err(ApiError::forbidden());
        }
        let stored = crate::delegated_credentials::custody::current(conn, &record).await?;
        binding.bind_source(&stored.context, &self.original)?;
        let until = sdlc_workspace::task_pipeline::authorize_saved(
            conn,
            &state.config.sdlc_workspace,
            &record,
            &state.config.secrets,
        )
        .await?;
        crate::repository_catalog::require_project_writable(
            state.pool.as_ref().ok_or_else(ApiError::forbidden)?,
            record.project_id,
        )
        .await?;
        if let Some(a) = &binding.artifact {
            let source = ledger::candidate_pipeline(conn, &self.original)
                .await
                .map_err(ApiError::internal)?
                .ok_or_else(ApiError::forbidden)?;
            let jobs = ledger::candidate_jobs(conn, binding.source_pipeline_id)
                .await
                .map_err(ApiError::internal)?;
            let lease = ledger::lease_observation(
                conn,
                record.project_id,
                self.original.request.lease_id,
                false,
            )
            .await
            .map_err(ApiError::internal)?
            .ok_or_else(ApiError::forbidden)?;
            if source.pipeline_id != binding.source_pipeline_id
                || source.status != "success"
                || source.finished_at.is_none()
                || source.commit_sha.as_deref()
                    != Some(self.original.request.source_commit.as_str())
                || source.config_sha256.as_deref() != Some(a.config_sha256.as_str())
                || source.plan_sha256.as_deref() != Some(a.plan_sha256.as_str())
                || jobs.is_empty()
                || jobs
                    .iter()
                    .any(|j| !j.terminal_acknowledged || !j.artifacts_complete)
                || lease.attempt_id != self.original.request.attempt_id
                || lease.generation != self.original.request.workspace_generation
                || lease.current_generation != lease.generation
                || !lease.acknowledged
                || !lease.completion_received
            {
                return Err(ApiError::forbidden());
            }
            let rows = ledger::candidate_artifacts(conn, binding.source_pipeline_id)
                .await
                .map_err(ApiError::internal)?;
            if rows
                .iter()
                .filter(|v| {
                    v.artifact_id == a.id
                        && v.attempt_id == Some(a.attempt_id)
                        && v.retained
                        && v.sha256.as_deref() == Some(a.sha256.as_str())
                        && v.size_bytes == a.size_bytes
                })
                .count()
                != 1
            {
                return Err(ApiError::forbidden());
            }
        }
        let (kind, hash, policy) = policy(state, record.project_id)?;
        if kind != binding.target_kind || hash != binding.target_policy_sha256 {
            return Err(ApiError::forbidden());
        }
        task_environment(conn, record.project_id, &policy).await?;
        let permit = tracker::read_bound(
            state,
            &policy,
            &self.original,
            binding
                .command
                .task_fence
                .as_ref()
                .ok_or_else(ApiError::forbidden)?,
            true,
            Some(&binding.development_result_sha256),
        )
        .await?
        .bounded(until);
        permit.check().map_err(|_| ApiError::forbidden())?;
        Ok(permit)
    }
}

async fn current(
    conn: &mut sqlx::PgConnection,
    identity: &Identity,
    pipeline: Uuid,
) -> Result<(ledger::TaskPipelineJournal, Binding), ApiError> {
    let record = ledger::find_task_pipeline(conn, pipeline, false)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::forbidden)?;
    if record.service_account_id.is_some()
        || record.api_token_id.is_some()
        || record.delegated_credential.is_none()
    {
        return Err(ApiError::forbidden());
    }
    let stored = crate::delegated_credentials::custody::current(conn, &record).await?;
    if stored.subject != identity.subject || stored.fingerprint != identity.fingerprint() {
        return Err(ApiError::forbidden());
    }
    identity.bind(&stored.context, record.project_id, &record.intent)?;
    let variables: Option<Value> = sqlx::query_scalar(
        "SELECT variables FROM pipelines WHERE id=$1 AND project_id=$2 AND commit_sha=$3 FOR SHARE",
    )
    .bind(pipeline)
    .bind(record.project_id)
    .bind(&record.intent.source_commit)
    .fetch_one(&mut *conn)
    .await
    .map_err(ApiError::internal)?;
    let binding = retained(variables.unwrap_or(Value::Null), &stored.context)?;
    Ok((record, binding))
}

fn policy(
    state: &AppState,
    project: Uuid,
) -> Result<(TargetKind, String, probes::TaskExecutionPolicy), ApiError> {
    if state.config.sdlc_workspace.local_delivery_kind == crate::config::LocalDeliveryKind::Oci {
        return oci::delegated_policy(state, project);
    }
    let target = configured_target(state, project, false)
        .map_err(|_| ApiError::service_unavailable("Owner delivery target unavailable"))?;
    Ok((
        TargetKind::Static,
        target.policy_sha256,
        target
            .policy
            .task_execution
            .ok_or_else(ApiError::forbidden)?,
    ))
}

pub(crate) async fn workspace(
    state: &AppState,
    identity: &Identity,
    project: Uuid,
    key: &str,
    expected: &WorkspaceOperationLookup,
) -> Result<Json<WorkspaceOperationReadback>, ApiError> {
    let mut tx = crate::api::pool(state)?
        .begin()
        .await
        .map_err(ApiError::internal)?;
    let session = Session::load(state, &mut tx, identity, project, key).await?;
    sdlc_workspace::verify_lookup(&session.original, expected)?;
    let lease =
        ledger::lease_observation(&mut tx, project, session.original.request.lease_id, false)
            .await
            .map_err(ApiError::internal)?;
    let request = &session.original.request;
    let result = WorkspaceOperationReadback {
        reconciliation_needed: lease.as_ref().is_none_or(|l| {
            l.generation != request.workspace_generation
                || l.current_generation != l.generation
                || l.attempt_id != request.attempt_id
                || !l.acknowledged
                || l.source_commit.as_deref() != Some(&request.source_commit)
                || (l.expired || l.status != "active") && !l.completion_received
        }),
        lease_status: lease.as_ref().map(|v| v.status.clone()),
        lease_expires_at: lease.as_ref().map(|v| v.expires_at),
        expired: lease.as_ref().is_none_or(|v| v.expired),
        current_generation: lease.as_ref().map(|v| v.current_generation),
        receipt: session.original.clone(),
    };
    let permit = session.refresh(state, &mut tx).await?;
    tx.commit().await.map_err(ApiError::internal)?;
    permit.check().map_err(|_| ApiError::forbidden())?;
    Ok(Json(result))
}

pub(crate) async fn candidate_read(
    state: &AppState,
    identity: &Identity,
    project: Uuid,
    key: &str,
    expected: &WorkspaceOperationLookup,
) -> Result<Json<CandidateEvidenceReadback>, ApiError> {
    let mut tx = crate::api::pool(state)?
        .begin()
        .await
        .map_err(ApiError::internal)?;
    let session = Session::load(state, &mut tx, identity, project, key).await?;
    sdlc_workspace::verify_lookup(&session.original, expected)?;
    let value =
        sdlc_workspace::candidate_evidence_on(state, &mut tx, session.original.clone()).await?;
    session.binding.candidate(&value)?;
    let permit = session.refresh(state, &mut tx).await?;
    tx.commit().await.map_err(ApiError::internal)?;
    permit.check().map_err(|_| ApiError::forbidden())?;
    Ok(Json(value))
}

pub(crate) async fn execute(
    state: Arc<AppState>,
    identity: &Identity,
    project: Uuid,
    key: &str,
    command: DeliveryCommand,
) -> Result<Json<TaskDeliveryReadback>, ApiError> {
    let mut tx = crate::api::pool(&state)?
        .begin()
        .await
        .map_err(ApiError::internal)?;
    let session = Session::load(&state, &mut tx, identity, project, key).await?;
    if canonical_hash(&command)? != canonical_hash(&session.binding.command)? {
        return Err(ApiError::forbidden());
    }
    let value = if session.binding.target_kind == TargetKind::Oci {
        TaskDeliveryReadback::Oci(oci::execute_delegated(state.clone(), &mut tx, &session).await?)
    } else {
        let target = configured_target(&state, project, false)
            .map_err(|_| ApiError::service_unavailable("Owner delivery target unavailable"))?;
        let existing = operation_dir(&target.root, &command.operation_key).exists();
        let candidate = if !existing && command.action == DeliveryAction::Deploy {
            Some(candidate_on(&state, &mut tx, &session, target.policy_sha256.clone()).await?)
        } else {
            None
        };
        if !existing
            && command.action == DeliveryAction::Rollback
            && pointer(&target.root, "confirmed.json").map_err(|_| ApiError::forbidden())?
                != session.binding.rollback_manifest_sha256
        {
            return Err(ApiError::forbidden());
        }
        let permit = session.refresh(&state, &mut tx).await?;
        TaskDeliveryReadback::Static(
            target
                .execute(
                    &command,
                    session.original.clone(),
                    candidate,
                    existing,
                    Some(permit),
                    Some(Refresh::Child {
                        state: &state,
                        session: &session,
                        connection: &mut tx,
                    }),
                )
                .await
                .map_err(|_| ApiError::conflict("Delivery held; read original key"))?,
        )
    };
    let permit = session.refresh(&state, &mut tx).await?;
    tx.commit().await.map_err(ApiError::internal)?;
    permit.check().map_err(|_| ApiError::forbidden())?;
    Ok(Json(value))
}

pub(super) async fn candidate_on(
    state: &AppState,
    conn: &mut sqlx::PgConnection,
    session: &Session,
    policy_sha: String,
) -> Result<VerifiedCandidate, ApiError> {
    let evidence =
        sdlc_workspace::candidate_evidence_on(state, conn, session.original.clone()).await?;
    session.binding.candidate(&evidence)?;
    candidate_from_evidence(state, conn, &session.binding.command, policy_sha, evidence)
        .await
        .map_err(|_| ApiError::forbidden())?
        .ok_or_else(ApiError::forbidden)
}

pub(crate) async fn readback(
    state: &AppState,
    identity: &Identity,
    project: Uuid,
    key: &str,
    delivery_key: &str,
    expected: &WorkspaceOperationLookup,
) -> Result<Json<TaskDeliveryReadback>, ApiError> {
    let mut tx = crate::api::pool(state)?
        .begin()
        .await
        .map_err(ApiError::internal)?;
    let session = Session::load(state, &mut tx, identity, project, key).await?;
    sdlc_workspace::verify_lookup(&session.original, expected)?;
    if session.binding.command.operation_key != delivery_key {
        return Err(ApiError::forbidden());
    }
    let result = if session.binding.target_kind == TargetKind::Oci {
        TaskDeliveryReadback::Oci(oci::read_delegated(state, &session)?)
    } else {
        let target = configured_target(state, project, false).map_err(|_| ApiError::forbidden())?;
        session
            .check_journal(&operation_dir(&target.root, delivery_key))
            .map_err(|_| ApiError::forbidden())?;
        TaskDeliveryReadback::Static(
            target
                .readback(&session.binding.command, &session.original)
                .map_err(|_| ApiError::conflict("Original delivery readback mismatch"))?,
        )
    };
    let permit = session.refresh(state, &mut tx).await?;
    tx.commit().await.map_err(ApiError::internal)?;
    permit.check().map_err(|_| ApiError::forbidden())?;
    Ok(Json(result))
}

#[cfg(test)]
mod tests;
