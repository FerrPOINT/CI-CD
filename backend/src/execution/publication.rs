//! Readback of a remote build is independent from termination of its client Pod.
use super::kubernetes::{Execution, Scope};
use crate::domain::{image_publish::ImagePublishSpec, published_image::PublishedImage};
use anyhow::{Context, ensure};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{fs, io::Write, path::Path};

pub async fn available() -> bool {
    let mut command = tokio::process::Command::new("python3");
    command
        .arg("/usr/local/libexec/forge/host_image_build.py")
        .arg("--health")
        .kill_on_drop(true);
    let output = tokio::time::timeout(std::time::Duration::from_secs(12), command.output()).await;
    match output {
        Ok(Ok(output)) if output.status.success() && output.stdout.len() <= 4096 => {
            serde_json::from_slice::<serde_json::Value>(&output.stdout).is_ok_and(|value| {
                value["schema"] == "forge/build-host-health/v1"
                    && value["healthy"] == true
                    && value["admissionAvailable"] == true
            })
        }
        _ => false,
    }
}
pub fn command(scope: &Scope, spec: &ImagePublishSpec) -> anyhow::Result<Vec<String>> {
    spec.validate().map_err(anyhow::Error::msg)?;
    Ok(vec![
        "python3".into(),
        "-I".into(),
        "/usr/local/libexec/forge/host_image_build.py".into(),
        "--request".into(),
        serde_json::to_string(&serde_json::json!({
            "jobId": scope.job_id, "attemptId": scope.attempt_id, "leaseId": scope.lease_id,
            "generation": scope.generation, "sourceCommit": scope.source_commit, "spec": spec,
        }))?,
    ])
}

pub async fn cancel(execution: &Execution) -> anyhow::Result<()> {
    if execution.host_build_spec()?.is_none() {
        return Ok(());
    }
    let scope = execution.scope();
    let output = tokio::process::Command::new("python3")
        .arg("-I")
        .arg("/usr/local/libexec/forge/host_image_build.py")
        .arg("--cancel")
        .arg(scope.attempt_id.to_string())
        .arg(scope.generation.to_string())
        .kill_on_drop(true)
        .output()
        .await?;
    ensure!(
        output.status.success() && output.stdout.len() <= 2 * 1024 * 1024,
        "Worker cancellation unavailable; retain execution"
    );
    let value: Value = serde_json::from_slice(&output.stdout)?;
    ensure!(
        value["schema"] == "forge/host-image-build-status/v1",
        "Invalid worker cancellation readback"
    );
    if value["phase"] != "absent" {
        // Active state is expected immediately after SIGTERM; the final readback
        // below still must prove termination before lease completion.
        ensure!(
            value["jobId"] == serde_json::json!(scope.job_id)
                && value["attemptId"] == serde_json::json!(scope.attempt_id)
                && value["leaseId"] == serde_json::json!(scope.lease_id)
                && value["generation"] == scope.generation
                && value["sourceCommit"] == scope.source_commit,
            "Worker cancellation custody differs"
        );
    }
    Ok(())
}

/// Returns only after a root-owned worker receipt proves completion or no effect.
/// Unreachable/running/unknown host state retains the existing lease and workspace.
pub async fn finalize(
    execution: &Execution,
    workspace: &Path,
    pod_exit: i32,
) -> anyhow::Result<()> {
    let Some(spec) = execution.host_build_spec()? else {
        return Ok(());
    };
    let scope = execution.scope();
    let receipt = wait_for_terminal_receipt(
        || read_worker_status(execution),
        scope,
        execution.cancelled(),
        std::time::Duration::from_secs(120),
    )
    .await?;
    match validate_receipt(&receipt, scope)? {
        HostTerminal::Active => anyhow::bail!("Worker is still active; retain execution"),
        HostTerminal::NotStarted | HostTerminal::Failed => {
            if validate_receipt(&receipt, scope)? == HostTerminal::NotStarted {
                // Fence even an as-yet unrecorded SSH request before closing custody.
                cancel(execution).await?;
            }
            ensure!(
                pod_exit != 0,
                "Client claimed success without a completed worker build"
            );
        }
        HostTerminal::Complete => {
            let result = &receipt["result"];
            let (descriptor, published) = validated_artifacts(result, scope, &spec)?;
            retain_artifact(workspace, &spec.descriptor_artifact, &descriptor)?;
            retain_artifact(workspace, &spec.published_artifact, &published)?;
        }
    }
    Ok(())
}

async fn read_worker_status(execution: &Execution) -> anyhow::Result<Value> {
    let scope = execution.scope();
    let output = tokio::process::Command::new("python3")
        .arg("-I")
        .arg("/usr/local/libexec/forge/host_image_build.py")
        .arg("--status")
        .arg(scope.attempt_id.to_string())
        .arg(scope.generation.to_string())
        .kill_on_drop(true)
        .output()
        .await?;
    ensure!(
        output.status.success() && output.stdout.len() <= 2 * 1024 * 1024,
        "Worker build readback unavailable; retain execution"
    );
    Ok(serde_json::from_slice(&output.stdout)?)
}

/// A canceled client can finish before the rootless worker confirms solve closure.
async fn wait_for_terminal_receipt<F, Fut>(
    mut read: F,
    scope: &Scope,
    canceled: bool,
    closing_timeout: std::time::Duration,
) -> anyhow::Result<Value>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<Value>>,
{
    let deadline = tokio::time::Instant::now() + closing_timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        ensure!(
            !remaining.is_zero(),
            "Worker stopping deadline exceeded; retain execution"
        );
        // The helper owns its SSH child and bounds each readback to 45 seconds.
        // Do not kill the helper ahead of its child/key cleanup.
        let receipt = read().await?;
        match validate_receipt(&receipt, scope)? {
            HostTerminal::Active => {
                ensure!(
                    canceled,
                    "Noncanceled client ended before its worker; retain execution"
                );
                ensure!(
                    tokio::time::Instant::now() < deadline,
                    "Worker stopping deadline exceeded; retain execution"
                );
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
            _ => return Ok(receipt),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum HostTerminal {
    Active,
    NotStarted,
    Failed,
    Complete,
}

fn validate_receipt(value: &Value, scope: &Scope) -> anyhow::Result<HostTerminal> {
    ensure!(
        value["schema"] == "forge/host-image-build-status/v1",
        "Unknown host build receipt"
    );
    if value["phase"] == "absent" {
        return Ok(HostTerminal::NotStarted);
    }
    ensure!(
        value["jobId"] == serde_json::json!(scope.job_id)
            && value["attemptId"] == serde_json::json!(scope.attempt_id)
            && value["leaseId"] == serde_json::json!(scope.lease_id)
            && value["generation"] == scope.generation
            && value["sourceCommit"] == scope.source_commit,
        "Host build custody differs"
    );
    match value["phase"].as_str() {
        Some("running" | "receiving") if value["ownerAlive"] == true => Ok(HostTerminal::Active),
        Some("complete") => Ok(HostTerminal::Complete),
        Some("failed") => Ok(HostTerminal::Failed),
        Some("receiving") if value["ownerAlive"] == false => Ok(HostTerminal::NotStarted),
        _ => anyhow::bail!("Worker build has unknown or active effects; retain custody"),
    }
}

fn validated_artifacts(
    value: &Value,
    scope: &Scope,
    spec: &ImagePublishSpec,
) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    ensure!(
        value["schema"] == "forge/host-image-build-result/v1"
            && value["sourceCommit"] == scope.source_commit,
        "Wrong build result identity"
    );
    let descriptor = value["descriptorBytes"]
        .as_str()
        .context("Original OCI metadata missing")?
        .as_bytes();
    ensure!(
        descriptor.len() <= 1024 * 1024,
        "Build metadata exceeds bound"
    );
    let original: Value = serde_json::from_slice(descriptor)?;
    let hash = format!("{:x}", Sha256::digest(descriptor));
    let published: PublishedImage = serde_json::from_value(value["publishedImage"].clone())?;
    ensure!(
        published.binds(&scope.source_commit, &hash)
            && value["descriptorSha256"] == hash
            && original == value["descriptor"]
            && original["containerimage.digest"].as_str().is_some_and(
                |digest| published.image_ref == format!("{}@{digest}", spec.repository)
            ),
        "Publication does not bind the original OCI metadata and commit"
    );
    Ok((descriptor.to_vec(), serde_json::to_vec(&published)?))
}

pub(crate) fn retain_artifact(root: &Path, path: &str, bytes: &[u8]) -> anyhow::Result<()> {
    ensure!(
        crate::domain::image_build::valid_relative_path(path, false),
        "Unsafe artifact path"
    );
    let root = root.canonicalize()?;
    let target = root.join(path);
    let parent = target.parent().context("Artifact parent missing")?;
    let mut existing = parent;
    while !existing.exists() {
        existing = existing.parent().context("Artifact ancestor missing")?;
    }
    ensure!(
        existing.canonicalize()?.starts_with(&root),
        "Artifact parent escapes workspace"
    );
    fs::create_dir_all(parent)?;
    ensure!(
        parent.canonicalize()?.starts_with(&root),
        "Artifact parent changed"
    );
    if let Ok(metadata) = fs::symlink_metadata(&target) {
        ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink() && fs::read(&target)? == bytes,
            "Existing artifact has different contents"
        );
        return Ok(());
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut stream = options.open(target)?;
    stream.write_all(bytes)?;
    stream.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    struct TestDirectory(std::path::PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("forge-publication-{}", Uuid::new_v4()));
            std::fs::create_dir(&root).unwrap();
            Self(root)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn scope() -> Scope {
        Scope {
            job_id: Uuid::new_v4(),
            attempt_id: Uuid::new_v4(),
            lease_id: Uuid::new_v4(),
            generation: 4,
            source_commit: "a".repeat(40),
        }
    }
    fn active_receipt(scope: &Scope) -> Value {
        serde_json::json!({"schema":"forge/host-image-build-status/v1",
            "phase":"running","ownerAlive":true,"jobId":scope.job_id,
            "attemptId":scope.attempt_id,"leaseId":scope.lease_id,
            "generation":scope.generation,"sourceCommit":scope.source_commit})
    }

    #[tokio::test]
    async fn canceled_worker_closure_is_polled_without_repeating_execution() {
        let scope = scope();
        let mut reads = 0;
        let receipt = wait_for_terminal_receipt(
            || {
                reads += 1;
                let mut value = active_receipt(&scope);
                if reads == 2 {
                    value["phase"] = serde_json::json!("failed");
                    value["ownerAlive"] = serde_json::json!(false);
                }
                std::future::ready(Ok(value))
            },
            &scope,
            true,
            std::time::Duration::from_secs(3),
        )
        .await
        .unwrap();
        assert_eq!(reads, 2);
        assert_eq!(receipt["phase"], "failed");
    }

    #[tokio::test]
    async fn canceled_worker_unknown_owner_and_expired_closing_deadline_stay_held() {
        let scope = scope();
        let mut unknown = active_receipt(&scope);
        unknown["ownerAlive"] = serde_json::json!(false);
        assert!(
            wait_for_terminal_receipt(
                || std::future::ready(Ok(unknown.clone())),
                &scope,
                true,
                std::time::Duration::from_secs(1),
            )
            .await
            .is_err()
        );
        assert!(
            wait_for_terminal_receipt(
                || std::future::ready(Ok(active_receipt(&scope))),
                &scope,
                true,
                std::time::Duration::ZERO,
            )
            .await
            .is_err()
        );
        assert!(
            wait_for_terminal_receipt(
                || std::future::ready(Ok(active_receipt(&scope))),
                &scope,
                false,
                std::time::Duration::from_secs(1),
            )
            .await
            .is_err()
        );
        let mut foreign = active_receipt(&scope);
        foreign["generation"] = serde_json::json!(scope.generation + 1);
        assert!(
            wait_for_terminal_receipt(
                || std::future::ready(Ok(foreign.clone())),
                &scope,
                true,
                std::time::Duration::from_secs(1),
            )
            .await
            .is_err()
        );
    }

    #[test]
    fn remote_readback_keeps_exact_lease_attempt_and_generation() {
        let s = scope();
        let mut r = serde_json::json!({"schema":"forge/host-image-build-status/v1",
            "phase":"complete","jobId":s.job_id,"attemptId":s.attempt_id,
            "leaseId":s.lease_id,"generation":s.generation,"sourceCommit":s.source_commit});
        assert_eq!(validate_receipt(&r, &s).unwrap(), HostTerminal::Complete);
        r["generation"] = serde_json::json!(5);
        assert!(validate_receipt(&r, &s).is_err());
        r["generation"] = serde_json::json!(4);
        r["phase"] = serde_json::json!("running");
        r["ownerAlive"] = serde_json::json!(false);
        assert!(validate_receipt(&r, &s).is_err());
        r["phase"] = serde_json::json!("receiving");
        assert_eq!(validate_receipt(&r, &s).unwrap(), HostTerminal::NotStarted);
    }
    #[test]
    fn artifact_write_is_idempotent_and_never_overwrites_unknown_bytes() {
        let root = TestDirectory::new();
        retain_artifact(root.path(), "out/image.json", b"verified").unwrap();
        retain_artifact(root.path(), "out/image.json", b"verified").unwrap();
        assert!(retain_artifact(root.path(), "out/image.json", b"different").is_err());
        assert!(retain_artifact(root.path(), "../other", b"bad").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/tmp", root.path().join("escape")).unwrap();
            assert!(retain_artifact(root.path(), "escape/file", b"bad").is_err());
        }
    }
}
