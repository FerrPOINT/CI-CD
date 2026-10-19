use super::*;
use crate::task_delivery::owner_evidence::{
    ManifestEvidence, ReadLock, matches_binding, operations, pointers,
};

pub(in crate::task_delivery) fn selection_policy(
    state: &AppState,
    project: Uuid,
    policy_sha256: &str,
) -> anyhow::Result<Option<crate::task_delivery::owner_selection::Policy>> {
    let owner = task_owner(state, project)?;
    ensure!(owner.policy_hash == policy_sha256, "owner policy changed");
    Ok(owner.policy.checks.delivery_selection)
}

pub(in crate::task_delivery) fn snapshot(
    state: &AppState,
    project: Uuid,
    policy_sha256: &str,
) -> anyhow::Result<(ManifestEvidence, ManifestEvidence)> {
    let owner = task_owner(state, project)?;
    ensure!(owner.policy_hash == policy_sha256, "owner policy changed");
    let _lock = ReadLock::acquire(&owner.root)?;
    let mut known = Vec::new();
    for path in operations(&owner.root)? {
        let intent: Intent = read(&path.join("intent.json"))?;
        ensure!(
            path == operation_dir(&owner.root, &intent.receipt.command.operation_key),
            "OCI operation locator mismatch"
        );
        let observed =
            owner.readback(&intent.receipt.command, &intent.receipt.original_operation)?;
        ensure!(
            !observed.reconciliation_needed,
            "unknown OCI publication holds target"
        );
        ensure!(
            path.join("result.json").is_file() || path.join("reconciled.json").is_file(),
            "OCI publication result absent"
        );
        let latest = observed
            .reconciled_receipt
            .as_ref()
            .unwrap_or(&observed.receipt);
        known.push((latest.manifest_sha256.clone(), latest.status));
    }
    pointers(&owner.root, &known, &["manifests"], |hash| {
        let manifest = owner.manifest(hash)?;
        let task = owner
            .policy
            .checks
            .task_execution
            .as_ref()
            .context("owner task policy absent")?;
        ensure!(
            matches_binding(
                task,
                manifest.candidate.operation_receipt.request.repository_id,
                &manifest.candidate.operation_receipt.request.binding
            ),
            "foreign OCI manifest source binding"
        );
        manifest
            .candidate
            .operation_receipt
            .request
            .validate()
            .map_err(anyhow::Error::msg)?;
        ensure!(
            manifest.candidate.operation_receipt.request_hash
                == sha(&serde_json::to_vec(
                    &manifest.candidate.operation_receipt.request
                )?),
            "OCI manifest original request hash mismatch"
        );
        Ok(())
    })
}
