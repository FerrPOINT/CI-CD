//! Persistent Helm releases use a separate operator grant, never normal job rights.
use super::kubernetes::{Execution, HelmGrant, Scope};
use crate::domain::helm_deploy::{DeploymentObservation, DeploymentOffer};
use anyhow::{Context, ensure};
use std::path::Path;

pub fn command(
    scope: &Scope,
    offer: &DeploymentOffer,
    grant: &HelmGrant,
) -> anyhow::Result<Vec<String>> {
    offer.validate().map_err(anyhow::Error::msg)?;
    ensure!(
        offer.job_id == scope.job_id
            && offer.attempt_id == scope.attempt_id
            && offer.lease_id == scope.lease_id
            && offer.generation == scope.generation
            && offer.source_commit == scope.source_commit,
        "Offered Helm custody differs"
    );
    Ok(vec![
        "python3".into(),
        "-I".into(),
        "/usr/local/libexec/forge/helm_deploy.py".into(),
        "--offer".into(),
        serde_json::to_string(offer)?,
        "--grant".into(),
        serde_json::to_string(grant)?,
    ])
}
fn observation(
    offer: &DeploymentOffer,
    grant: &HelmGrant,
    value: &serde_json::Value,
) -> anyhow::Result<DeploymentObservation> {
    let receipt = value
        .get("receipt")
        .cloned()
        .map(serde_json::from_value)
        .transpose()?;
    let result = DeploymentObservation {
        schema: "forge/kubernetes-deployment-observation/v1".into(),
        phase: value["phase"]
            .as_str()
            .context("Helm readback phase missing")?
            .into(),
        deployment_id: offer.deployment_id,
        pipeline_id: offer.pipeline_id,
        attempt_id: offer.attempt_id,
        lease_id: offer.lease_id,
        generation: offer.generation,
        source_commit: offer.source_commit.clone(),
        namespace_uid: grant.namespace_uid,
        release_revision: receipt
            .as_ref()
            .map(|r: &crate::domain::helm_deploy::DeploymentReceipt| r.release_revision)
            .or_else(|| {
                value["releaseRevision"]
                    .as_i64()
                    .and_then(|v| i32::try_from(v).ok())
            }),
        receipt,
    };
    ensure!(
        result.validates(offer),
        "Helm readback is active, unknown or mismatched; retain custody"
    );
    Ok(result)
}
pub async fn finalize(
    execution: &Execution,
    workspace: &Path,
    pod_exit: i32,
) -> anyhow::Result<()> {
    let Some(offer) = execution.helm_offer()? else {
        return Ok(());
    };
    let grant = execution.helm_grant()?.context("Helm grant missing")?;
    let root = std::env::var("CICD_HELM_TARGET_ROOT")
        .unwrap_or_else(|_| "/run/forge-deploy-targets".into());
    let target = Path::new(&root)
        .join(offer.environment_id.to_string())
        .join("target.json");
    ensure!(
        target.is_absolute(),
        "Absolute operator target root required"
    );
    let output = tokio::process::Command::new("python3")
        .arg("-I")
        .arg("/usr/local/libexec/forge/helm_deploy.py")
        .arg("--target")
        .arg(target)
        .arg("--offer")
        .arg(serde_json::to_string(&offer)?)
        .arg("--grant")
        .arg(serde_json::to_string(&grant)?)
        .arg("--readback")
        .current_dir(workspace)
        .kill_on_drop(true)
        .output()
        .await?;
    ensure!(
        output.status.success() && output.stdout.len() <= 2 * 1024 * 1024,
        "Helm target readback unavailable; retain execution"
    );
    let value = serde_json::from_slice(&output.stdout)?;
    let result = observation(&offer, &grant, &value)?;
    ensure!(
        pod_exit != 0 || result.phase == "deployed",
        "Client claimed success without a verified release"
    );
    super::publication::retain_artifact(
        workspace,
        &offer.receipt_artifact,
        &serde_json::to_vec(&result)?,
    )?;
    Ok(())
}

pub fn completion_observation(
    root: &Path,
    attempt: uuid::Uuid,
    generation: i64,
) -> anyhow::Result<Option<DeploymentObservation>> {
    let execution = match Execution::reopen(root, attempt, generation) {
        Ok(execution) => execution,
        Err(_) => return Ok(None),
    };
    let Some(offer) = execution.helm_offer()? else {
        return Ok(None);
    };
    let workspace = root.join(execution.workspace_subpath()).canonicalize()?;
    ensure!(
        workspace.starts_with(root.canonicalize()?),
        "Retained workspace escapes root"
    );
    let path = workspace.join(&offer.receipt_artifact);
    if !path.exists() {
        return Ok(None);
    }
    let path = path.canonicalize()?;
    ensure!(
        path.starts_with(&workspace) && std::fs::metadata(&path)?.len() <= 1024 * 1024,
        "Deployment observation escapes workspace or exceeds bound"
    );
    let bytes = std::fs::read(path)?;
    ensure!(
        bytes.len() <= 1024 * 1024,
        "Deployment observation exceeds bound"
    );
    let value: DeploymentObservation = serde_json::from_slice(&bytes)?;
    ensure!(
        value.validates(&offer),
        "Retained deployment observation differs"
    );
    Ok(Some(value))
}
