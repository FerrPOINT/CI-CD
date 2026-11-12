//! Resolve explicit privileged policy against an already verified owner candidate.
use super::probes::OwnerSelectionPolicy;
use crate::{
    api::ApiError,
    domain::sdlc_workspace::{CandidateArtifactEvidence, CandidateEvidenceReadback},
};
use serde::Serialize;
use uuid::Uuid;

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OwnerDeliverySelection {
    pub schema: &'static str,
    pub action: &'static str,
    pub source_pipeline_id: Uuid,
    pub config_sha256: String,
    pub plan_sha256: String,
    pub artifact: CandidateArtifactEvidence,
}

fn unavailable() -> ApiError {
    ApiError::service_unavailable("devops_owner_selection_unavailable")
}

fn selected_artifact<'a>(
    artifacts: &'a [CandidateArtifactEvidence],
    matching: &[Uuid],
) -> Result<&'a CandidateArtifactEvidence, ApiError> {
    if matching.len() != 1 {
        return Err(unavailable());
    }
    let mut selected = artifacts
        .iter()
        .filter(|artifact| artifact.artifact_id == matching[0]);
    let artifact = selected.next().ok_or_else(unavailable)?;
    if selected.next().is_some()
        || artifact.artifact_id.is_nil()
        || artifact.job_id.is_nil()
        || artifact.attempt_id.is_nil()
        || artifact.size_bytes <= 0
        || !crate::domain::task_delivery::digest(&artifact.sha256)
    {
        return Err(unavailable());
    }
    Ok(artifact)
}

pub(super) async fn observe_on(
    connection: &mut sqlx::PgConnection,
    policy: Option<&OwnerSelectionPolicy>,
    candidate: &CandidateEvidenceReadback,
) -> Result<Option<OwnerDeliverySelection>, ApiError> {
    let Some(policy) = policy else {
        return Ok(None);
    };
    let action = match &policy.action {
        super::probes::OwnerSelectionAction::Deploy => "deploy",
    };
    let ids: Vec<Uuid> = candidate
        .artifacts
        .iter()
        .map(|artifact| artifact.artifact_id)
        .collect();
    if ids.is_empty() || ids.len() > 32 {
        return Err(unavailable());
    }
    // Names are exact privileged selectors. Restrict them to the verified current
    // artifact IDs and original pipeline; historical retries and foreign rows cannot win.
    let matching: Vec<Uuid> = sqlx::query_scalar(
        "SELECT a.id FROM artifacts a JOIN jobs j ON j.id=a.job_id JOIN stages s ON s.id=j.stage_id \
         WHERE s.pipeline_id=$1 AND a.id=ANY($2) AND j.name=$3 AND a.name=$4 LIMIT 2",
    ).bind(candidate.pipeline_id).bind(&ids).bind(&policy.job_name).bind(&policy.artifact_name)
        .fetch_all(connection).await.map_err(|_| unavailable())?;
    let selected = selected_artifact(&candidate.artifacts, &matching)?;
    let config = candidate
        .config_sha256
        .as_ref()
        .filter(|v| crate::domain::task_delivery::digest(v))
        .ok_or_else(unavailable)?;
    let plan = candidate
        .plan_sha256
        .as_ref()
        .filter(|v| crate::domain::task_delivery::digest(v))
        .ok_or_else(unavailable)?;
    Ok(Some(OwnerDeliverySelection {
        schema: "forge/owner-delivery-selection/v1",
        action,
        source_pipeline_id: candidate.pipeline_id,
        config_sha256: config.clone(),
        plan_sha256: plan.clone(),
        artifact: CandidateArtifactEvidence {
            artifact_id: selected.artifact_id,
            job_id: selected.job_id,
            attempt_id: selected.attempt_id,
            sha256: selected.sha256.clone(),
            size_bytes: selected.size_bytes,
        },
    }))
}

#[cfg(test)]
#[path = "owner_selection_tests.rs"]
mod tests;
