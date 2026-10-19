//! Optional privileged owner policy, not a command supplied by the task caller.
use crate::domain::{
    sdlc_workspace::{CandidateArtifactEvidence, CandidateEvidenceReadback},
    task_delivery::digest,
};
use anyhow::ensure;
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Policy {
    pub version: u8,
    pub action: Action,
    pub stage_name: String,
    pub job_name: String,
    /// Exact stored artifact name, not an inferred path or file extension.
    pub artifact_name: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = OwnerDeliverySelectionAction)]
pub enum Action {
    Deploy,
}

impl Policy {
    pub(super) fn validate(&self) -> anyhow::Result<()> {
        ensure!(self.version == 1, "unknown owner selection version");
        for value in [&self.stage_name, &self.job_name, &self.artifact_name] {
            ensure!(
                !value.is_empty()
                    && value.len() <= 128
                    && value.trim() == value
                    && value.bytes().all(|b| (0x21..=0x7e).contains(&b)),
                "invalid exact owner selection name"
            );
        }
        ensure!(
            !self.artifact_name.contains(['/', '\\']),
            "owner selection requires a stored artifact name"
        );
        Ok(())
    }
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = OwnerDeliverySelection)]
pub struct Selection {
    pub schema: &'static str,
    pub action: Action,
    pub source_pipeline_id: Uuid,
    pub config_sha256: String,
    pub plan_sha256: String,
    pub artifact: CandidateArtifactEvidence,
}

fn selected(candidate: &CandidateEvidenceReadback, ids: &[Uuid]) -> anyhow::Result<Selection> {
    ensure!(
        candidate.candidate_observed
            && !candidate.dispatch_allowed
            && !candidate.deployment_verified
            && !candidate.acceptance_verified
            && !candidate.pipeline_id.is_nil()
            && ids.len() == 1,
        "owner selection is missing or ambiguous"
    );
    let (Some(config), Some(plan)) = (
        candidate.config_sha256.as_ref().filter(|v| digest(v)),
        candidate.plan_sha256.as_ref().filter(|v| digest(v)),
    ) else {
        anyhow::bail!("selected candidate digest unavailable")
    };
    let matches: Vec<_> = candidate
        .artifacts
        .iter()
        .filter(|a| a.artifact_id == ids[0])
        .collect();
    ensure!(matches.len() == 1, "selected candidate changed");
    let artifact = matches[0];
    ensure!(
        !artifact.artifact_id.is_nil()
            && !artifact.job_id.is_nil()
            && !artifact.attempt_id.is_nil()
            && artifact.size_bytes > 0
            && digest(&artifact.sha256),
        "selected artifact unavailable"
    );
    Ok(Selection {
        schema: "forge/owner-delivery-selection/v1",
        action: Action::Deploy,
        source_pipeline_id: candidate.pipeline_id,
        config_sha256: config.clone(),
        plan_sha256: plan.clone(),
        artifact: artifact.clone(),
    })
}

pub(super) async fn resolve(
    connection: &mut PgConnection,
    policy: Option<&Policy>,
    candidate: &CandidateEvidenceReadback,
) -> anyhow::Result<Option<Selection>> {
    let Some(policy) = policy else {
        return Ok(None);
    };
    policy.validate()?;
    // Same source pipeline and latest attempt as the existing candidate observer. Colliding
    // declared path encodings, duplicate job names and duplicate artifacts never choose a first.
    let rows: Vec<(Uuid, Vec<String>)> = sqlx::query_as(
        "SELECT ar.id,j.artifact_paths FROM artifacts ar JOIN jobs j ON j.id=ar.job_id \
         JOIN stages s ON s.id=j.stage_id \
         WHERE s.pipeline_id=$1 AND s.name=$2 AND j.name=$3 AND ar.name=$4 \
         AND ar.attempt_id=(SELECT a.id FROM execution_attempts a WHERE a.job_id=j.id ORDER BY a.attempt_no DESC LIMIT 1) \
         AND ar.purged_at IS NULL AND ar.expires_at>clock_timestamp() \
         ORDER BY ar.id LIMIT 2",
    ).bind(candidate.pipeline_id).bind(&policy.stage_name).bind(&policy.job_name)
        .bind(&policy.artifact_name).fetch_all(connection).await?;
    ensure!(rows.len() == 1, "owner selection is missing or ambiguous");
    let (artifact, paths) = &rows[0];
    let names = crate::domain::artifact_names::declared_artifact_names(paths)
        .map_err(anyhow::Error::msg)?;
    ensure!(
        names
            .iter()
            .filter(|name| *name == &policy.artifact_name)
            .count()
            == 1,
        "owner selection is not an exact declared artifact"
    );
    Ok(Some(selected(candidate, &[*artifact])?))
}

#[cfg(test)]
#[path = "owner_selection_tests.rs"]
mod tests;
