//! Read-only owner facts. This module neither enrolls a target nor grants delivery authority.
use super::*;
use crate::api::ApiError;
use crate::domain::sdlc_workspace::WorkspaceTaskBinding;

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TargetEvidence {
    pub observed_at: chrono::DateTime<chrono::Utc>,
    pub target_kind: delegated::TargetKind,
    /// Digest of the full owner policy, not of the redacted projection below.
    pub policy_sha256: String,
    pub task_execution: probes::TaskExecutionPolicy,
    pub current: ManifestEvidence,
    pub confirmed: ManifestEvidence,
}

#[derive(Debug, Serialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ManifestEvidence {
    Absent,
    Present { sha256: String },
}

pub(super) fn manifest_evidence(hash: Option<String>) -> ManifestEvidence {
    match hash {
        Some(sha256) => ManifestEvidence::Present { sha256 },
        None => ManifestEvidence::Absent,
    }
}

/// Read the existing lock without creating it. Writers use the same inode's exclusive flock.
pub(super) struct ReadLock(std::fs::File);
impl ReadLock {
    pub(super) fn acquire(root: &Path) -> anyhow::Result<Self> {
        let file = files::open_read(&root.join(".lock"))?;
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            ensure!(
                unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) } == 0,
                "owner publication busy"
            );
            Ok(Self(file))
        }
        #[cfg(not(unix))]
        {
            let _ = file;
            anyhow::bail!("owner readback requires Unix file locking")
        }
    }
}
impl Drop for ReadLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
        }
    }
}

pub(super) fn operations(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let path = root.join("operations");
    plain(&path)?;
    ensure!(path.is_dir(), "owner operation inventory unavailable");
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(path)? {
        ensure!(
            entries.len() < 1024,
            "owner operation inventory exceeds bound"
        );
        let path = entry?.path();
        plain(&path)?;
        ensure!(path.is_dir(), "owner operation is not a directory");
        entries.push(path);
    }
    Ok(entries)
}

fn static_snapshot(target: &Target) -> anyhow::Result<(ManifestEvidence, ManifestEvidence)> {
    let _lock = ReadLock::acquire(&target.root)?;
    for name in ["manifests", "objects"] {
        let path = target.root.join(name);
        plain(&path)?;
        ensure!(path.is_dir(), "owner target inventory unavailable");
    }
    let mut known = Vec::new();
    for path in operations(&target.root)? {
        let intent: Intent = read(&path.join("intent.json"))?;
        ensure!(
            path == operation_dir(&target.root, &intent.receipt.command.operation_key),
            "owner operation locator mismatch"
        );
        let observed =
            target.readback(&intent.receipt.command, &intent.receipt.original_operation)?;
        ensure!(
            !observed.reconciliation_needed,
            "unknown publication holds target"
        );
        ensure!(
            path.join("result.json").is_file() || path.join("reconciled.json").is_file(),
            "publication result absent"
        );
        let latest = observed
            .reconciled_receipt
            .as_ref()
            .unwrap_or(&observed.receipt);
        known.push((latest.manifest_sha256.clone(), latest.status));
    }
    pointers(&target.root, &known, &["manifests", "objects"], |hash| {
        let manifest = target.manifest(hash)?;
        let task = target
            .policy
            .task_execution
            .as_ref()
            .context("owner task policy absent")?;
        ensure!(
            matches_binding(
                task,
                manifest.operation_receipt.request.repository_id,
                &manifest.operation_receipt.request.binding
            ),
            "foreign manifest source binding"
        );
        Ok(())
    })
}

pub(super) fn pointers(
    root: &Path,
    known: &[(Option<String>, DeliveryStatus)],
    empty_directories: &[&str],
    validate: impl Fn(&str) -> anyhow::Result<()>,
) -> anyhow::Result<(ManifestEvidence, ManifestEvidence)> {
    let current = pointer(root, "current.json")?;
    let confirmed = pointer(root, "confirmed.json")?;
    if current.is_none() || confirmed.is_none() {
        ensure!(
            current.is_none() && confirmed.is_none() && known.is_empty(),
            "missing pointer is not proven empty"
        );
        for directory in empty_directories {
            let path = root.join(directory);
            plain(&path)?;
            ensure!(
                std::fs::read_dir(path)?.next().is_none(),
                "orphan target inventory"
            );
        }
    } else {
        ensure!(
            known.iter().any(|(hash, _)| hash == &current),
            "current pointer lacks owner receipt"
        );
        ensure!(
            known
                .iter()
                .any(|(hash, status)| hash == &confirmed && *status == DeliveryStatus::Verified),
            "confirmed pointer lacks verified owner receipt"
        );
    }
    for hash in current.iter().chain(confirmed.iter()) {
        validate(hash)?;
    }
    Ok((manifest_evidence(current), manifest_evidence(confirmed)))
}

pub(super) fn matches_binding(
    policy: &probes::TaskExecutionPolicy,
    repository: Uuid,
    binding: &WorkspaceTaskBinding,
) -> bool {
    policy.environment == "nonproduction"
        && policy.repository_id == repository
        && policy.tracker_instance_id == binding.tracker_instance_id
        && policy.tracker_project_id == binding.tracker_project_id
        && !policy.environment_id.is_nil()
        && !policy.holder_subject.trim().is_empty()
}

pub(crate) async fn observe(
    state: &AppState,
    connection: &mut sqlx::PgConnection,
    project: Uuid,
    repository: Uuid,
    binding: &WorkspaceTaskBinding,
) -> Result<TargetEvidence, ApiError> {
    let (kind, policy_sha256, task_execution) =
        delegated::policy(state, project).map_err(|_| unavailable())?;
    if !matches_binding(&task_execution, repository, binding) {
        return Err(ApiError::conflict("devops_target_binding_mismatch"));
    }
    task_environment(connection, project, &task_execution).await?;
    let (current, confirmed) = match kind {
        delegated::TargetKind::Static => {
            let target = configured_target(state, project, false).map_err(|_| unavailable())?;
            if target.policy_sha256 != policy_sha256 {
                return Err(unavailable());
            }
            static_snapshot(&target).map_err(|_| unavailable())?
        }
        delegated::TargetKind::Oci => oci::owner_evidence::snapshot(state, project, &policy_sha256)
            .map_err(|_| unavailable())?,
    };
    Ok(TargetEvidence {
        observed_at: chrono::Utc::now(),
        target_kind: kind,
        policy_sha256,
        task_execution,
        current,
        confirmed,
    })
}

fn unavailable() -> ApiError {
    // Filesystem, process, policy paths and private receipt contents never cross the API boundary.
    ApiError::service_unavailable("devops_owner_evidence_unavailable")
}

#[cfg(test)]
#[path = "owner_evidence_tests.rs"]
mod tests;
