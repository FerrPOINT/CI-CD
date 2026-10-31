//! Typed environment deployments reuse the existing history and artifact store.
use crate::{
    api::ApiError,
    config::ArtifactsConfig,
    domain::{
        helm_deploy::{DeploymentOffer, HelmDeploySpec, KubernetesTarget},
        image_publish::ImagePublishSpec,
        published_image::PublishedImage,
    },
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Transaction};
use std::{collections::BTreeMap, fs};
use uuid::Uuid;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn prepare(
    tx: &mut Transaction<'_, Postgres>,
    artifacts: &ArtifactsConfig,
    pipeline_id: Uuid,
    job_id: Uuid,
    job_key: &str,
    attempt_id: Uuid,
    lease_id: Uuid,
    generation: i64,
    commit: &str,
    plan: &Value,
    spec: &HelmDeploySpec,
) -> Result<Option<DeploymentOffer>, ApiError> {
    use crate::domain::helm_deploy::{DeploymentObservation, ReleaseMetadata, RollbackRelease};
    spec.validate().map_err(ApiError::conflict)?;
    // The instance row is the common delivery/lifecycle fence. Admission is
    // checked before Helm can apply replicas from an ordinary manifest.
    let workspace: Option<(Uuid,String,String,String)> = sqlx::query_as(
        "SELECT id,desired_runtime,runtime,environment_hash FROM development_workspaces WHERE environment_id=$1 FOR UPDATE",
    ).bind(spec.environment_id).fetch_optional(&mut **tx).await.map_err(ApiError::internal)?;
    let bound_environment_hash = workspace.as_ref().map(|row| row.3.clone());
    if let Some((workspace, desired, observed, _)) = workspace {
        if desired != "running" || observed != "running" {
            return Err(ApiError::conflict("workspace_must_be_running_for_delivery"));
        }
        let blocked: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM development_workspace_operations WHERE workspace_id=$1
             AND state IN ('queued','running','unknown') AND (pipeline_id IS NULL OR pipeline_id<>$2))",
        ).bind(workspace).bind(pipeline_id).fetch_one(&mut **tx).await.map_err(ApiError::internal)?;
        if blocked {
            return Err(ApiError::conflict("workspace_operation_blocks_delivery"));
        }
        let owner_matches: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM development_workspace_sessions s JOIN pipeline_human_owners p
             ON p.owner_subject=s.owner_subject WHERE s.workspace_id=$1 AND s.ended_at IS NULL AND p.pipeline_id=$2)",
        ).bind(workspace).bind(pipeline_id).fetch_one(&mut **tx).await.map_err(ApiError::internal)?;
        if !owner_matches {
            return Err(ApiError::conflict(
                "workspace_delivery_requires_session_owner",
            ));
        }
    }

    let (target, project_id, protected, approvals): (Option<Value>, Uuid, bool, i32) =
        sqlx::query_as(
            "SELECT e.kubernetes_target,e.project_id,e.protected,e.required_approvals
         FROM environments e JOIN pipelines p ON p.project_id=e.project_id
         WHERE e.id=$1 AND p.id=$2 FOR UPDATE OF e",
        )
        .bind(spec.environment_id)
        .bind(pipeline_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::conflict("Environment belongs to another CI project"))?;
    if let Some(expected) = bound_environment_hash {
        let observed = cicd_infra::development_workspace::request_hash(
            target
                .as_ref()
                .ok_or_else(|| ApiError::conflict("workspace_environment_target_required"))?,
        )
        .map_err(crate::development_workspaces::error)?;
        if expected != observed {
            return Err(ApiError::conflict("workspace_environment_target_drift"));
        }
    }
    let target: KubernetesTarget = serde_json::from_value(
        target.ok_or_else(|| ApiError::conflict("Environment has no Kubernetes target"))?,
    )
    .map_err(|_| ApiError::conflict("Invalid Kubernetes environment"))?;
    target.validate().map_err(ApiError::conflict)?;
    let requested:Option<(Uuid,Option<Uuid>,Option<Uuid>)>=sqlx::query_as(
        "SELECT d.id,d.rollback_of_id,d.execution_attempt_id FROM deployments d
         JOIN pipelines p ON p.id=$1 AND p.project_id=$3
         WHERE d.id::text=p.variables->>'deployment_id' AND d.environment_id=$2
         AND (d.pipeline_id IS NULL OR d.pipeline_id=p.id)
         AND EXISTS(SELECT 1 FROM pipeline_triggers t WHERE t.pipeline_id=p.id
             AND t.project_id=p.project_id AND t.source IN ('deployment-approval','deployment-rollback'))
         FOR UPDATE OF d")
        .bind(pipeline_id).bind(spec.environment_id).bind(project_id)
        .fetch_optional(&mut **tx).await.map_err(ApiError::internal)?;
    if protected && approvals > 0 {
        let id = requested
            .as_ref()
            .map(|row| row.0)
            .ok_or_else(|| ApiError::conflict("Protected deployment has no approved operation"))?;
        let approved: bool = sqlx::query_scalar(
            "SELECT count(*) FILTER(WHERE decision='approved') >= $2
             AND count(*) FILTER(WHERE decision='rejected')=0
             FROM deployment_approvals WHERE deployment_id=$1",
        )
        .bind(id)
        .bind(approvals as i64)
        .fetch_one(&mut **tx)
        .await
        .map_err(ApiError::internal)?;
        if !approved {
            return Err(ApiError::conflict("Deployment approval is incomplete"));
        }
    }
    let active: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM deployments WHERE environment_id=$1
         AND runtime_phase IN ('prepared','running','held'))",
    )
    .bind(spec.environment_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(ApiError::internal)?;
    if active {
        return Err(ApiError::conflict(
            "Environment already has an unresolved deployment",
        ));
    }
    let pipeline_order: i64 =
        sqlx::query_scalar("SELECT delivery_order FROM pipelines WHERE id=$1")
            .bind(pipeline_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(ApiError::internal)?;
    let (latest_order, current_revision): (i64, i32) = sqlx::query_as(
        "SELECT COALESCE(max((runtime_input->>'pipelineOrder')::bigint),0),
                COALESCE(max((runtime_receipt->>'releaseRevision')::integer),0)
         FROM deployments WHERE environment_id=$1 AND runtime_phase='finished'
         AND runtime_receipt->>'phase' IN ('deployed','failed')",
    )
    .bind(spec.environment_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(ApiError::internal)?;
    if pipeline_order < latest_order {
        return Ok(None);
    }
    let rollback_source = requested.as_ref().and_then(|row| row.1);
    let (images, rollback) = if let Some(source) = rollback_source {
        let (input, observed): (Value, Value) = sqlx::query_as(
            "SELECT runtime_input,runtime_receipt FROM deployments
             WHERE id=$1 AND environment_id=$2 AND status='success' AND runtime_phase='finished'",
        )
        .bind(source)
        .bind(spec.environment_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::conflict("Qualified rollback snapshot missing"))?;
        let previous: DeploymentOffer = serde_json::from_value(input)
            .map_err(|_| ApiError::conflict("Invalid rollback input snapshot"))?;
        let observed: DeploymentObservation = serde_json::from_value(observed)
            .map_err(|_| ApiError::conflict("Invalid rollback live observation"))?;
        previous.validate().map_err(ApiError::conflict)?;
        if !observed.validates(&previous)
            || observed.phase != "deployed"
            || previous.source_commit != commit
            || previous.target.cluster != target.cluster
            || previous.target.namespace != target.namespace
            || previous.target.release != target.release
            || previous.images.keys().ne(spec.images.keys())
        {
            return Err(ApiError::conflict(
                "Rollback must retain the qualified source, target and image set",
            ));
        }
        let revision = observed
            .release_revision
            .ok_or_else(|| ApiError::conflict("Rollback revision missing"))?;
        (
            previous.images.clone(),
            Some(RollbackRelease {
                revision,
                expected_current_revision: current_revision,
                metadata: ReleaseMetadata::installed(&previous),
            }),
        )
    } else {
        let mut images = BTreeMap::new();
        for (value_path, binding) in &spec.images {
            let entry = plan["jobs"]
                .as_array()
                .and_then(|entries| {
                    entries
                        .iter()
                        .find(|entry| entry["key"].as_str() == Some(&binding.job))
                })
                .ok_or_else(|| {
                    ApiError::conflict("Publication producer absent from immutable plan")
                })?;
            let build: ImagePublishSpec = serde_json::from_value(entry["image_publish"].clone())
                .map_err(|_| ApiError::conflict("Deployment image is not a typed publication"))?;
            build.validate().map_err(ApiError::conflict)?;
            let current = plan["jobs"]
                .as_array()
                .and_then(|entries| {
                    entries
                        .iter()
                        .find(|entry| entry["key"].as_str() == Some(job_key))
                })
                .ok_or_else(|| ApiError::conflict("Deployment job absent from immutable plan"))?;
            if binding.artifact != build.published_artifact
                || !current["needs"].as_array().is_some_and(|needs| {
                    needs
                        .iter()
                        .any(|v| v.as_str() == Some(binding.job.as_str()))
                })
            {
                return Err(ApiError::conflict(
                    "Deployment image binding differs from its required producer",
                ));
            }
            let (producer_job, producer_attempt): (Uuid, Uuid) = sqlx::query_as(
                "SELECT j.id,a.id FROM jobs j JOIN stages s ON s.id=j.stage_id
             JOIN execution_attempts a ON a.job_id=j.id
             WHERE s.pipeline_id=$1 AND j.name=$2 AND j.status='success'
             AND a.status='success' ORDER BY a.attempt_no DESC LIMIT 1",
            )
            .bind(pipeline_id)
            .bind(&binding.job)
            .fetch_optional(&mut **tx)
            .await
            .map_err(ApiError::internal)?
            .ok_or_else(|| ApiError::conflict("Publication producer is not successful"))?;
            let published_bytes = artifact(
                tx,
                artifacts,
                producer_job,
                producer_attempt,
                &binding.artifact,
            )
            .await?;
            let descriptor = artifact(
                tx,
                artifacts,
                producer_job,
                producer_attempt,
                &build.descriptor_artifact,
            )
            .await?;
            let published: PublishedImage = serde_json::from_slice(&published_bytes)
                .map_err(|_| ApiError::conflict("Invalid publication artifact"))?;
            let hash = format!("{:x}", Sha256::digest(&descriptor));
            if !published.binds(commit, &hash)
                || published.image_ref.split_once('@').map(|v| v.0)
                    != Some(build.repository.as_str())
            {
                return Err(ApiError::conflict(
                    "Published image does not bind this pipeline commit and descriptor",
                ));
            }
            images.insert(value_path.clone(), published);
        }

        (images, None)
    };
    let id = requested
        .as_ref()
        .filter(|row| row.2.is_none())
        .map(|row| row.0)
        .unwrap_or_else(Uuid::new_v4);
    let offer = DeploymentOffer {
        environment_id: spec.environment_id,
        deployment_id: id,
        pipeline_id,
        job_id,
        attempt_id,
        lease_id,
        generation,
        source_commit: commit.into(),
        pipeline_order,
        target,
        images,
        receipt_artifact: spec.receipt_artifact.clone(),
        rollback,
    };
    offer.validate().map_err(ApiError::conflict)?;
    let input = serde_json::to_value(&offer)
        .map_err(|_| ApiError::conflict("Invalid deployment input serialization"))?;
    if requested
        .as_ref()
        .is_some_and(|row| row.0 == id && row.2.is_none())
    {
        sqlx::query(
            "UPDATE deployments SET pipeline_id=$2,status='running',execution_attempt_id=$3,
            runtime_phase='prepared',runtime_input=$4 WHERE id=$1",
        )
        .bind(id)
        .bind(pipeline_id)
        .bind(attempt_id)
        .bind(input)
        .execute(&mut **tx)
        .await
        .map_err(ApiError::internal)?;
    } else {
        sqlx::query("INSERT INTO deployments (id,environment_id,pipeline_id,git_ref,status,
            execution_attempt_id,runtime_phase,runtime_input,rollback_of_id)
            SELECT $1,$2,p.id,p.git_ref,'running',$3,'prepared',$4,$6 FROM pipelines p WHERE p.id=$5")
            .bind(id).bind(spec.environment_id).bind(attempt_id).bind(input).bind(pipeline_id)
            .bind(rollback_source).execute(&mut **tx).await.map_err(ApiError::internal)?;
    }
    Ok(Some(offer))
}

async fn artifact(
    tx: &mut Transaction<'_, Postgres>,
    config: &ArtifactsConfig,
    job: Uuid,
    attempt: Uuid,
    name: &str,
) -> Result<Vec<u8>, ApiError> {
    let (path, hash, size): (String, Option<String>, i64) = sqlx::query_as(
        "SELECT storage_path,sha256,size_bytes FROM artifacts
         WHERE job_id=$1 AND attempt_id=$2 AND name=$3 AND purged_at IS NULL
         AND expires_at>now() ORDER BY created_at DESC,id DESC LIMIT 1",
    )
    .bind(job)
    .bind(attempt)
    .bind(crate::domain::artifact_names::artifact_name_from_declared_path(name))
    .fetch_optional(&mut **tx)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError::conflict("Required deployment artifact missing"))?;
    if !(1..=1024 * 1024).contains(&size) {
        return Err(ApiError::conflict("Deployment artifact exceeds bound"));
    }
    let path = crate::platform::contained_artifact_path(config, &path)?;
    let metadata =
        fs::metadata(&path).map_err(|_| ApiError::conflict("Deployment artifact unavailable"))?;
    if metadata.len() != size as u64 {
        return Err(ApiError::conflict("Deployment artifact size differs"));
    }
    let bytes =
        fs::read(path).map_err(|_| ApiError::conflict("Deployment artifact unavailable"))?;
    if hash.as_deref() != Some(format!("{:x}", Sha256::digest(&bytes)).as_str()) {
        return Err(ApiError::conflict("Deployment artifact checksum differs"));
    }
    Ok(bytes)
}

/// Only authenticated completion calls this. Lease expiry alone never releases
/// a target whose application effects have not been read back.
pub(crate) async fn finish(
    tx: &mut Transaction<'_, Postgres>,
    attempt: Uuid,
    lease: Uuid,
    generation: i64,
    outcome: &str,
    observation: Option<&crate::domain::helm_deploy::DeploymentObservation>,
) -> Result<(), ApiError> {
    let row: Option<(Uuid, Uuid, Value)> = sqlx::query_as(
        "SELECT id,environment_id,runtime_input FROM deployments
         WHERE execution_attempt_id=$1 FOR UPDATE",
    )
    .bind(attempt)
    .fetch_optional(&mut **tx)
    .await
    .map_err(ApiError::internal)?;
    let Some((id, environment, input)) = row else {
        if observation.is_some() {
            return Err(ApiError::conflict("Observation has no deployment owner"));
        }
        return Ok(());
    };
    let offer: DeploymentOffer = serde_json::from_value(input)
        .map_err(|_| ApiError::conflict("Retained deployment input invalid"))?;
    offer.validate().map_err(ApiError::conflict)?;
    if offer.attempt_id != attempt
        || offer.lease_id != lease
        || offer.generation != generation
        || observation.is_some_and(|value| !value.validates(&offer))
        || observation.is_none()
        || (outcome == "success" && observation.is_none_or(|value| value.phase != "deployed"))
    {
        return Err(ApiError::conflict(
            "Deployment completion differs from its execution custody or live receipt",
        ));
    }
    let serialized = observation
        .map(serde_json::to_value)
        .transpose()
        .map_err(|_| ApiError::conflict("Invalid deployment observation serialization"))?;
    sqlx::query("UPDATE deployments SET runtime_phase='finished',runtime_receipt=COALESCE($2,runtime_receipt),
        status=$3 WHERE id=$1").bind(id).bind(serialized)
        .bind(if outcome=="success"{"success"}else{"failed"})
        .execute(&mut **tx).await.map_err(ApiError::internal)?;
    if let Some(value) = observation {
        if matches!(value.phase.as_str(), "deployed" | "failed") {
            sqlx::query("UPDATE environments SET status=$2 WHERE id=$1")
                .bind(environment)
                .bind(if value.phase == "deployed" {
                    "available"
                } else {
                    "degraded"
                })
                .execute(&mut **tx)
                .await
                .map_err(ApiError::internal)?;
        }
    }
    Ok(())
}
