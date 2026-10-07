use super::{ApiError, ApiResult, AppState, pool};
use crate::{auth::AccessClaims, domain::sdlc_workspace::*, store::sdlc_workspace as ledger};
use axum::{
    Extension, Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    routing::{get, post},
};
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/api/v1/projects/{project_id}/sdlc/workspace-operations",
            post(prepare_workspace_operation),
        )
        .route(
            "/api/v1/projects/{project_id}/sdlc/workspace-operations/{operation_key}",
            get(get_workspace_operation),
        )
        .route(
            "/api/v1/projects/{project_id}/sdlc/workspace-operations/{operation_key}/candidate-evidence",
            get(get_candidate_evidence),
        )
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(axum::middleware::map_response(|mut response: axum::response::Response| async move {
            response.headers_mut().insert("cache-control", axum::http::HeaderValue::from_static("no-store"));
            response
        }))
}

pub(super) async fn authorize(
    conn: &mut sqlx::PgConnection,
    state: &AppState,
    claims: &AccessClaims,
    project: Uuid,
    scope: &str,
) -> Result<(), ApiError> {
    let subject = state
        .config
        .sdlc_workspace
        .operation_subject
        .ok_or_else(|| {
            ApiError::service_unavailable("SDLC workspace machine subject is not configured")
        })?;
    if claims.sub != subject
        || claims.role != "service_account"
        || claims.service_account_name.is_none()
        || claims.token_project_id != Some(project)
        || claims.sid.is_some()
    {
        return Err(ApiError::forbidden());
    }
    // Re-read and lock credential state in the operation transaction; no human/admin fallback.
    let valid = sqlx::query_scalar::<_, Uuid>("SELECT t.id FROM api_tokens t JOIN service_accounts sa ON sa.id=t.service_account_id WHERE t.id=$1 AND sa.id=$2 AND t.project_id=$3 AND t.principal_type='service_account' AND sa.enabled AND t.revoked_at IS NULL AND (t.expires_at IS NULL OR t.expires_at>clock_timestamp()) AND $4=ANY(t.scopes) FOR SHARE OF t,sa")
        .bind(claims.token_id).bind(subject).bind(project).bind(scope)
        .fetch_optional(conn).await.map_err(ApiError::internal)?;
    if valid.is_none() {
        return Err(ApiError::forbidden());
    }
    Ok(())
}

fn verify_lookup(
    receipt: &WorkspaceOperationReceipt,
    expected: &WorkspaceOperationLookup,
) -> Result<(), ApiError> {
    let b = &receipt.request.binding;
    if expected.request_hash != receipt.request_hash
        || expected.task_id != b.task_id
        || expected.root_task_id != b.root_task_id
        || expected.assignment_id != b.assignment_id
        || expected.execution_id != b.execution_id
        || expected.fencing_token != b.fencing_token
    {
        return Err(ApiError::conflict(
            "original operation binding or hash mismatch",
        ));
    }
    Ok(())
}

#[utoipa::path(post, path="/api/v1/projects/{project_id}/sdlc/workspace-operations",
    params(("project_id"=Uuid, Path, description="Forge project UUID")),
    request_body=WorkspaceOperationRequest,
    responses((status=200, description="Immutable blocked operation receipt; no workspace admission", body=WorkspaceOperationReceipt),
        (status=400, description="Malformed binding or access"), (status=403, description="Machine/project/scope denied"),
        (status=401, description="Missing, expired or revoked credential"), (status=404, description="Owner project missing"),
        (status=409, description="Changed original key or stale local source/lease"),
        (status=413, description="Request exceeds 16 KiB"), (status=422, description="Unknown, null or malformed typed fields"),
        (status=503, description="Owner configuration unavailable")), tag="SDLC workspace")]
pub async fn prepare_workspace_operation(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<AccessClaims>,
    Path(project): Path<Uuid>,
    Json(request): Json<WorkspaceOperationRequest>,
) -> ApiResult<WorkspaceOperationReceipt> {
    request.validate().map_err(ApiError::bad_request)?;
    let hash = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&request)
                .map_err(|e| ApiError::internal(sqlx::Error::Encode(Box::new(e))))?
        )
    );
    let mut tx = pool(&state)?.begin().await.map_err(ApiError::internal)?;
    authorize(&mut tx, &state, &claims, project, "api:write").await?;
    // Serializes original-key inserts within one owner project. No job is queued or claimed.
    let exists = sqlx::query_scalar::<_, Uuid>("SELECT id FROM projects WHERE id=$1 FOR UPDATE")
        .bind(project)
        .fetch_optional(&mut *tx)
        .await
        .map_err(ApiError::internal)?;
    if exists.is_none() {
        return Err(ApiError::not_found_named("project"));
    }
    if let Some(receipt) =
        ledger::find_receipt(&mut tx, project, claims.sub, &request.operation_key)
            .await
            .map_err(ApiError::internal)?
    {
        if receipt.request != request || receipt.request_hash != hash {
            return Err(ApiError::conflict(
                "operation key already has different immutable input",
            ));
        }
        tx.commit().await.map_err(ApiError::internal)?;
        return Ok(Json(receipt));
    }
    let claimed_key = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM sdlc_workspace_operations WHERE project_id=$1 AND operation_key=$2)")
        .bind(project).bind(&request.operation_key).fetch_one(&mut *tx).await.map_err(ApiError::internal)?;
    if claimed_key {
        return Err(ApiError::conflict(
            "operation key belongs to another owner subject",
        ));
    }
    let lease = ledger::lease_observation(&mut tx, project, request.lease_id, true)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::conflict("lease does not belong to this project"))?;
    if lease.attempt_id != request.attempt_id
        || lease.generation != request.workspace_generation
        || lease.current_generation != request.workspace_generation
        || lease.status != "active"
        || lease.expired
        || !lease.acknowledged
        || lease.source_commit.as_deref() != Some(&request.source_commit)
    {
        return Err(ApiError::conflict(
            "stale lease, generation, attempt or source pin",
        ));
    }
    let repository =
        sqlx::query_scalar::<_, String>("SELECT name FROM repositories WHERE id=$1 FOR SHARE")
            .bind(request.repository_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(ApiError::internal)?
            .ok_or_else(|| ApiError::conflict("unknown pinned repository"))?;
    let repository =
        crate::git_host::validate_repo_name(&repository).map_err(ApiError::bad_request)?;
    let patterns = crate::git_host::repository_url_like_patterns(&repository);
    let linked = sqlx::query_scalar::<_, bool>("SELECT repository_url ILIKE $2 ESCAPE '\\' OR repository_url ILIKE $3 ESCAPE '\\' OR repository_url ILIKE $4 ESCAPE '\\' FROM projects WHERE id=$1")
        .bind(project).bind(patterns.path).bind(patterns.scp).bind(patterns.exact)
        .fetch_one(&mut *tx).await.map_err(ApiError::internal)?;
    if !linked {
        return Err(ApiError::conflict(
            "repository is not linked to owner project",
        ));
    }
    let mut blockers = vec![
        WorkspaceOperationBlocker::TrackerAdmissionUnavailable,
        WorkspaceOperationBlocker::TrackerWorkspaceBindingUnavailable,
    ];
    let mut observed = false;
    if let Some(root) = &state.config.sdlc_workspace.observation_root {
        let owned = crate::runner_workspace::OwnedWorkspace::reopen(root, &request.workspace_id)
            .map_err(|_| {
                ApiError::conflict("owned physical workspace unavailable or mismatched")
            })?;
        owned
            .verify_identity(
                request.attempt_id,
                request.lease_id,
                request.workspace_generation,
            )
            .map_err(|_| ApiError::conflict("physical workspace identity mismatch"))?;
        let source = state.git.root.join(format!("{repository}.git"));
        tokio::time::timeout(
            Duration::from_secs(3),
            owned.observe_pinned_source(&source, &request.source_commit),
        )
        .await
        .map_err(|_| {
            ApiError::service_unavailable("bounded physical source observation timed out")
        })?
        .map_err(|error| {
            if error.is::<crate::runner_workspace::SourceObservationTimeout>() {
                ApiError::service_unavailable("bounded physical source observation timed out")
            } else {
                ApiError::conflict("physical repository/source pin or clean state mismatch")
            }
        })?;
        observed = true;
    } else {
        blockers.push(WorkspaceOperationBlocker::PhysicalWorkspaceObservationUnavailable);
    }
    let fresh = ledger::lease_observation(&mut tx, project, request.lease_id, false)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::conflict("lease unavailable"))?;
    if fresh.expired
        || fresh.status != "active"
        || fresh.current_generation != request.workspace_generation
    {
        return Err(ApiError::conflict(
            "lease expired or superseded during observation",
        ));
    }
    // Tracker reservation is preparation only. It cannot authorize this binding or an effect.
    let receipt = WorkspaceOperationReceipt {
        schema: "forge/workspace-operation-receipt/v1".into(),
        operation_id: Uuid::new_v4(),
        project_id: project,
        request_hash: hash,
        request,
        recorded_at: Utc::now(),
        status: WorkspaceOperationStatus::Blocked,
        blockers,
        physical_source_observed: observed,
        dispatch_allowed: false,
    };
    ledger::append_receipt(&mut tx, claims.sub, &receipt)
        .await
        .map_err(ApiError::internal)?;
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(receipt))
}

#[utoipa::path(get, path="/api/v1/projects/{project_id}/sdlc/workspace-operations/{operation_key}",
    params(("project_id"=Uuid, Path), ("operation_key"=String, Path),
        ("requestHash"=String, Query), ("taskId"=Uuid, Query), ("rootTaskId"=Uuid, Query),
        ("assignmentId"=Uuid, Query), ("executionId"=Uuid, Query), ("fencingToken"=i64, Query)),
    responses((status=200, description="Original receipt plus fresh lease observation; never renewal", body=WorkspaceOperationReadback),
        (status=403, description="Machine/project/read scope denied"), (status=404, description="No receipt owned by this subject"),
        (status=400, description="Missing or malformed original lookup identity"), (status=401, description="Missing, expired or revoked credential"),
        (status=409, description="Original binding/hash mismatch"), (status=503, description="Owner configuration unavailable")), tag="SDLC workspace")]
pub async fn get_workspace_operation(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<AccessClaims>,
    Path((project, key)): Path<(Uuid, String)>,
    Query(expected): Query<WorkspaceOperationLookup>,
) -> ApiResult<WorkspaceOperationReadback> {
    let mut tx = pool(&state)?.begin().await.map_err(ApiError::internal)?;
    authorize(&mut tx, &state, &claims, project, "api:read").await?;
    let receipt = ledger::find_receipt(&mut tx, project, claims.sub, &key)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found_named("workspace operation"))?;
    verify_lookup(&receipt, &expected)?;
    let observation = ledger::lease_observation(&mut tx, project, receipt.request.lease_id, false)
        .await
        .map_err(ApiError::internal)?;
    let current_generation = observation.as_ref().map(|l| l.current_generation);
    let expired = observation.as_ref().is_none_or(|l| l.expired);
    let reconciliation_needed = observation.as_ref().is_none_or(|l| {
        l.generation != receipt.request.workspace_generation
            || l.current_generation != l.generation
            || l.attempt_id != receipt.request.attempt_id
            || !l.acknowledged
            || l.source_commit.as_deref() != Some(&receipt.request.source_commit)
            || (l.expired || l.status != "active") && !l.completion_received
    });
    let result = WorkspaceOperationReadback {
        receipt,
        lease_status: observation.as_ref().map(|l| l.status.clone()),
        lease_expires_at: observation.map(|l| l.expires_at),
        expired,
        current_generation,
        reconciliation_needed,
    };
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(result))
}

#[utoipa::path(get, path="/api/v1/projects/{project_id}/sdlc/workspace-operations/{operation_key}/candidate-evidence",
    params(("project_id"=Uuid, Path), ("operation_key"=String, Path),
        ("requestHash"=String, Query), ("taskId"=Uuid, Query), ("rootTaskId"=Uuid, Query),
        ("assignmentId"=Uuid, Query), ("executionId"=Uuid, Query), ("fencingToken"=i64, Query)),
    responses((status=200, description="Fresh bounded owner evidence; task delivery remains blocked", body=CandidateEvidenceReadback),
        (status=400), (status=401), (status=403), (status=404), (status=409), (status=503)), tag="SDLC workspace")]
pub async fn get_candidate_evidence(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<AccessClaims>,
    Path((project, key)): Path<(Uuid, String)>,
    Query(expected): Query<WorkspaceOperationLookup>,
) -> ApiResult<CandidateEvidenceReadback> {
    let mut tx = pool(&state)?.begin().await.map_err(ApiError::internal)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *tx)
        .await
        .map_err(ApiError::internal)?;
    authorize(&mut tx, &state, &claims, project, "api:read").await?;
    let receipt = ledger::find_receipt(&mut tx, project, claims.sub, &key)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found_named("workspace operation"))?;
    verify_lookup(&receipt, &expected)?;
    let pipeline = ledger::candidate_pipeline(&mut tx, &receipt)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::conflict("original pipeline identity unavailable"))?;
    let lease = ledger::lease_observation(&mut tx, project, receipt.request.lease_id, false)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::conflict("original lease identity unavailable"))?;
    if lease.attempt_id != receipt.request.attempt_id
        || lease.generation != receipt.request.workspace_generation
        || lease.current_generation != lease.generation
        || pipeline.commit_sha.as_deref() != Some(&receipt.request.source_commit)
    {
        return Err(ApiError::conflict(
            "original candidate attempt, generation or SHA superseded",
        ));
    }
    let mut blockers = vec![
        "tracker_admission_unavailable".into(),
        "tracker_workspace_binding_unavailable".into(),
        "candidate_write_authority_unavailable".into(),
        "served_deployment_identity_unavailable".into(),
        "health_evidence_unavailable".into(),
        "acceptance_evidence_unavailable".into(),
        "confirmed_rollback_version_unavailable".into(),
    ];
    let mut candidate_observed = true;
    let observed_at = Utc::now();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let repository = crate::git_host::validate_repo_name(&pipeline.repository_name)
        .map_err(ApiError::bad_request)?;
    let repository = state.git.root.join(format!("{repository}.git"));
    let config = tokio::time::timeout(
        Duration::from_secs(3),
        crate::runner_workspace::preparation::repository_pipeline_config(
            &repository,
            &receipt.request.source_commit,
        ),
    )
    .await;
    let config_valid = pipeline.config_source.as_deref() == Some("repository")
        && pipeline.resolved_commit_sha.as_deref() == Some(&receipt.request.source_commit)
        && config.as_ref().is_ok_and(|result| {
            result.as_ref().is_ok_and(|bytes| {
                pipeline
                    .raw_config
                    .as_ref()
                    .is_some_and(|raw| bytes == raw.as_bytes())
                    && pipeline
                        .config_sha256
                        .as_ref()
                        .is_some_and(|hash| *hash == format!("{:x}", Sha256::digest(bytes)))
            })
        })
        && pipeline
            .plan
            .as_ref()
            .and_then(|plan| serde_json::to_vec(plan).ok())
            .is_some_and(|bytes| {
                pipeline
                    .plan_sha256
                    .as_ref()
                    .is_some_and(|hash| *hash == format!("{:x}", Sha256::digest(bytes)))
            });
    if !config_valid {
        candidate_observed = false;
        blockers.push("exact_repository_pipeline_config_unverified".into());
    }
    let jobs = ledger::candidate_jobs(&mut tx, pipeline.pipeline_id)
        .await
        .map_err(ApiError::internal)?;
    if pipeline.status != "success"
        || pipeline.finished_at.is_none()
        || jobs.is_empty()
        || jobs.len() > 1000
        || jobs
            .iter()
            .any(|j| !j.terminal_acknowledged || !j.artifacts_complete)
    {
        candidate_observed = false;
        blockers.push("pipeline_gates_or_completion_ack_unverified".into());
    }
    let rows = ledger::candidate_artifacts(&mut tx, pipeline.pipeline_id)
        .await
        .map_err(ApiError::internal)?;
    let mut artifacts = Vec::new();
    let mut artifacts_valid = !rows.is_empty() && rows.len() <= 32;
    let mut total = 0i64;
    for row in rows.iter().take(32) {
        let Some(attempt) = row.attempt_id else {
            artifacts_valid = false;
            continue;
        };
        let Some(hash) = &row.sha256 else {
            artifacts_valid = false;
            continue;
        };
        total = total.saturating_add(row.size_bytes.max(0));
        if !row.retained
            || std::time::Instant::now() >= deadline
            || total > 128 * 1024 * 1024
            || !jobs.iter().any(|job| {
                job.job_id == row.job_id
                    && job.attempt_id == Some(attempt)
                    && job.terminal_acknowledged
            })
            || !tokio::time::timeout(
                deadline.saturating_duration_since(std::time::Instant::now()),
                crate::candidate_evidence::verify_artifact(
                    &state.config.artifacts.root,
                    std::path::Path::new(&row.storage_path),
                    hash,
                    row.size_bytes,
                ),
            )
            .await
            .is_ok_and(|result| result.is_ok())
        {
            artifacts_valid = false;
            continue;
        }
        artifacts.push(CandidateArtifactEvidence {
            artifact_id: row.artifact_id,
            job_id: row.job_id,
            attempt_id: attempt,
            sha256: hash.clone(),
            size_bytes: row.size_bytes,
        });
    }
    if !artifacts_valid {
        candidate_observed = false;
        blockers.push("attempt_artifact_bytes_unverified".into());
    }
    tx.commit().await.map_err(ApiError::internal)?;
    let result = CandidateEvidenceReadback {
        schema: "forge/candidate-evidence-readback/v1".into(),
        source_commit: receipt.request.source_commit.clone(),
        operation_receipt: receipt,
        pipeline_id: pipeline.pipeline_id,
        observed_at,
        config_sha256: pipeline.config_sha256,
        plan_sha256: pipeline.plan_sha256,
        candidate_observed,
        artifacts,
        blockers,
        dispatch_allowed: false,
        deployment_verified: false,
        acceptance_verified: false,
    };
    Ok(Json(result))
}
