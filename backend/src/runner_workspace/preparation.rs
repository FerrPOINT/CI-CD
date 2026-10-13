//! Owner-local IO only. No HTTP route, admission, credentials, job commands or trusted receipt.
use super::{OwnedWorkspace, check_ancestors, read_guarded_json};
use anyhow::{Context, ensure};
use cicd_domain::sdlc_workspace::{WorkspaceAccess, WorkspaceOperationRequest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{io::AsyncReadExt, process::Command};
use uuid::Uuid;

const INTENT: &str = ".forge-preparation.json";
const PREPARED: &str = ".forge-prepared.json";
const ACTIVE: &str = ".forge-preparation-active.json";

mod physical;

/// Owner-local bare Git bytes at an exact source pin; no mutable ref or fallback.
pub(crate) async fn repository_pipeline_config(
    repository: &Path,
    sha: &str,
) -> anyhow::Result<Vec<u8>> {
    ensure!(
        sha.len() == 40
            && sha
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "full source SHA required"
    );
    check_ancestors(repository)?;
    ensure!(
        repository.canonicalize()? == repository,
        "non-canonical repository path"
    );
    verify_repository(repository, sha).await?;
    git_bounded(
        repository,
        &["show", &format!("{sha}:.forge-ci.yml")],
        256 * 1024,
    )
    .await
}

/// Must come from the future owner-authorized source binding, never a request URL/path.
pub struct PreparationSource {
    pub project_id: Uuid,
    pub repository_id: Uuid,
    pub repository: PathBuf,
    pub source_commit: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    schema: String,
    project_id: Uuid,
    repository: PathBuf,
    request: WorkspaceOperationRequest,
}

/// Historical local preparation journal, not base-sdlc/workspace-receipt/v1.
#[derive(Debug, PartialEq, Eq)]
pub struct PreparationObservation {
    pub workspace_id: String,
    pub project_id: Uuid,
    pub repository_id: Uuid,
    pub source_commit: String,
    pub intent_sha256: String,
}

/// The caller must persist the exact workspace ID and revalidate live lease/fences separately.
/// A duplicate only observes the retained effect; missing/partial checkout never triggers clone.
pub async fn prepare(
    root: &Path,
    source: &PreparationSource,
    request: &WorkspaceOperationRequest,
) -> anyhow::Result<PreparationObservation> {
    let intent = validate(source, request)?;
    verify_source(&intent).await?;
    let directory = root.join(&request.workspace_id);
    let fresh = match fs::symlink_metadata(&directory) {
        Ok(_) => false,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
        Err(e) => return Err(e.into()),
    };
    let owned = if fresh {
        OwnedWorkspace::create_identified(
            root,
            &request.workspace_id,
            request.attempt_id,
            request.lease_id,
            request.workspace_generation,
        )?
    } else {
        OwnedWorkspace::reopen(root, &request.workspace_id)?
    };
    owned.verify_identity(
        request.attempt_id,
        request.lease_id,
        request.workspace_generation,
    )?;
    if fresh {
        write_new(&owned.directory, INTENT, &intent)?;
        write_new(&owned.directory, ACTIVE, &intent)?;
        let repository = intent
            .repository
            .to_str()
            .context("repository path is not UTF-8")?;
        git(
            &owned.directory,
            &[
                "clone",
                "--quiet",
                "--no-checkout",
                "--no-local",
                "--no-hardlinks",
                "--",
                repository,
                "workspace",
            ],
        )
        .await?;
        git(
            owned.checkout(),
            &[
                "checkout",
                "--quiet",
                "--detach",
                &request.source_commit,
                "--",
            ],
        )
        .await?;
    } else {
        require_intent(&owned, &intent)?;
        require_idle(&owned)?;
    }
    observe(&owned, &intent).await?;
    if fresh {
        // Only this invocation awaited its own Git children. Restart/expiry never clears ACTIVE.
        fs::remove_file(owned.directory.join(ACTIVE))?;
        #[cfg(unix)]
        fs::File::open(&owned.directory)?.sync_all()?;
    }
    // Finalization after a lost journal write is readback-only, never checkout or clone.
    match fs::symlink_metadata(owned.directory.join(PREPARED)) {
        Ok(_) => require_prepared(&owned, &intent)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            write_new(&owned.directory, PREPARED, &intent)?;
        }
        Err(e) => return Err(e.into()),
    }
    observation(&intent)
}

/// Does not create/finalize journals or renew leases. The source and filesystem are re-observed.
pub async fn readback(
    root: &Path,
    source: &PreparationSource,
    request: &WorkspaceOperationRequest,
) -> anyhow::Result<PreparationObservation> {
    let intent = validate(source, request)?;
    verify_source(&intent).await?;
    let owned = OwnedWorkspace::reopen(root, &request.workspace_id)?;
    owned.verify_identity(
        request.attempt_id,
        request.lease_id,
        request.workspace_generation,
    )?;
    require_intent(&owned, &intent)?;
    require_idle(&owned)?;
    require_prepared(&owned, &intent)?;
    observe(&owned, &intent).await?;
    observation(&intent)
}

fn validate(
    source: &PreparationSource,
    request: &WorkspaceOperationRequest,
) -> anyhow::Result<Intent> {
    request.validate().map_err(anyhow::Error::msg)?;
    ensure!(
        request.access == WorkspaceAccess::ReadOnly,
        "preparation grants no write access"
    );
    ensure!(
        !source.project_id.is_nil()
            && source.repository_id == request.repository_id
            && source.source_commit == request.source_commit
            && source.repository.is_absolute(),
        "owner source identity mismatch"
    );
    check_ancestors(&source.repository)?;
    ensure!(
        source.repository.canonicalize()? == source.repository,
        "source path must be canonical"
    );
    let intent = Intent {
        schema: "forge/local-workspace-preparation/v1".into(),
        project_id: source.project_id,
        repository: source.repository.clone(),
        request: request.clone(),
    };
    ensure!(
        serde_json::to_vec(&intent)?.len() <= 4096,
        "preparation intent exceeds journal bound"
    );
    Ok(intent)
}

async fn verify_source(intent: &Intent) -> anyhow::Result<()> {
    verify_repository(&intent.repository, &intent.request.source_commit).await
}

async fn verify_repository(repository: &Path, sha: &str) -> anyhow::Result<()> {
    ensure!(
        sha.len() == 40
            && sha
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "source must be full lowercase SHA"
    );
    check_ancestors(repository)?;
    verify_git_config(repository, &repository.join("config")).await?;
    ensure!(
        git(repository, &["rev-parse", "--is-bare-repository"]).await? == b"true\n",
        "owner source must be bare"
    );
    let commit = format!("{sha}^{{commit}}");
    let actual = git(repository, &["rev-parse", "--verify", &commit]).await?;
    ensure!(
        actual == format!("{sha}\n").as_bytes(),
        "source pin must identify an exact commit"
    );
    Ok(())
}

fn require_intent(owned: &OwnedWorkspace, intent: &Intent) -> anyhow::Result<()> {
    ensure!(
        read_guarded_json::<Intent>(&owned.directory.join(INTENT))? == *intent,
        "preparation intent conflict; reconciliation required"
    );
    ensure!(
        fs::symlink_metadata(owned.directory.join(super::COMPLETION))
            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
            && fs::symlink_metadata(owned.directory.join(super::ACK))
                .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
        "executed/acknowledged workspace cannot be a preparation"
    );
    Ok(())
}

fn require_prepared(owned: &OwnedWorkspace, intent: &Intent) -> anyhow::Result<()> {
    ensure!(
        read_guarded_json::<Intent>(&owned.directory.join(PREPARED))? == *intent,
        "preparation journal conflict; reconciliation required"
    );
    Ok(())
}

fn require_idle(owned: &OwnedWorkspace) -> anyhow::Result<()> {
    ensure!(
        fs::symlink_metadata(owned.directory.join(ACTIVE))
            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
        "preparation owner may still be active; reconciliation required"
    );
    Ok(())
}

async fn observe(owned: &OwnedWorkspace, intent: &Intent) -> anyhow::Result<()> {
    owned.verify_owner()?;
    require_intent(owned, intent)?;
    observe_checkout(
        owned.checkout(),
        &intent.repository,
        &intent.request.source_commit,
        Instant::now() + Duration::from_secs(30),
    )
    .await?;
    owned.verify_owner()?;
    Ok(())
}

pub(super) async fn observe_checkout(
    checkout: &Path,
    repository: &Path,
    sha: &str,
    deadline: Instant,
) -> anyhow::Result<()> {
    verify_repository(repository, sha).await?;
    let dotgit = checkout.join(".git");
    check_ancestors(&dotgit)?;
    ensure!(dotgit.is_dir(), "partial checkout; reconciliation required");
    verify_git_config(checkout, &dotgit.join("config")).await?;
    let origin = git(checkout, &["config", "--get", "remote.origin.url"]).await?;
    ensure!(
        origin
            == format!(
                "{}\n",
                repository
                    .to_str()
                    .context("repository path is not UTF-8")?
            )
            .as_bytes(),
        "checkout origin mismatch"
    );
    let head = git(checkout, &["rev-parse", "--verify", "HEAD^{commit}"]).await?;
    ensure!(
        head == format!("{sha}\n").as_bytes(),
        "checkout source mismatch"
    );
    ensure!(
        git(checkout, &["rev-parse", "--abbrev-ref", "HEAD"]).await? == b"HEAD\n",
        "checkout must be detached"
    );
    physical::verify(checkout, repository, sha, deadline).await
}

fn observation(intent: &Intent) -> anyhow::Result<PreparationObservation> {
    Ok(PreparationObservation {
        workspace_id: intent.request.workspace_id.clone(),
        project_id: intent.project_id,
        repository_id: intent.request.repository_id,
        source_commit: intent.request.source_commit.clone(),
        intent_sha256: format!("{:x}", Sha256::digest(serde_json::to_vec(intent)?)),
    })
}

async fn verify_git_config(cwd: &Path, path: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && !super::is_link(&metadata) && metadata.len() <= 4096,
        "invalid local Git configuration"
    );
    let config = git(
        cwd,
        &[
            "config",
            "--file",
            path.to_str().context("Git config path is not UTF-8")?,
            "--no-includes",
            "--null",
            "--list",
        ],
    )
    .await?;
    let config = std::str::from_utf8(&config)?;
    for entry in config.split_terminator('\0') {
        let (key, value) = entry.split_once('\n').context("invalid Git config entry")?;
        let allowed = match key {
            "core.repositoryformatversion" => value == "0",
            "core.filemode" | "core.bare" | "core.logallrefupdates" => {
                matches!(value, "true" | "false")
            }
            "remote.origin.url" | "remote.origin.fetch" => true,
            key if key.starts_with("branch.") => {
                key.ends_with(".remote") || key.ends_with(".merge")
            }
            _ => false,
        };
        ensure!(
            allowed,
            "unsupported local Git configuration; no hooks/helpers/filters/includes are permitted"
        );
    }
    Ok(())
}

fn write_new(directory: &Path, name: &str, intent: &Intent) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec(intent)?;
    ensure!(
        bytes.len() <= 4096,
        "preparation intent exceeds journal bound"
    );
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(directory.join(name))?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    #[cfg(unix)]
    fs::File::open(directory)?.sync_all()?;
    Ok(())
}

async fn git(cwd: &Path, args: &[&str]) -> anyhow::Result<Vec<u8>> {
    git_bounded(cwd, args, 4096).await
}

async fn git_bounded(cwd: &Path, args: &[&str], bound: usize) -> anyhow::Result<Vec<u8>> {
    let mut command = Command::new("git");
    // No ambient Git configuration, credentials, hooks, templates, filters or remote transports.
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default());
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", root);
    }
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", null)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .args([
            "--no-replace-objects",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "credential.helper=",
            "-c",
            "init.templateDir=",
            "-c",
            "protocol.allow=never",
            "-c",
            "protocol.file.allow=always",
            "-c",
        ])
        .arg(format!("core.hooksPath={null}"))
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn().context("start owner-local Git operation")?;
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        let mut bytes = Vec::new();
        child
            .stdout
            .take()
            .context("missing Git stdout")?
            .take(bound as u64 + 1)
            .read_to_end(&mut bytes)
            .await?;
        ensure!(bytes.len() <= bound, "Git readback exceeds bound");
        ensure!(
            child.wait().await?.success(),
            "Git preparation/readback failed; reconciliation required"
        );
        Ok(bytes)
    })
    .await;
    match result {
        Ok(Ok(bytes)) => Ok(bytes),
        other => {
            // Retain all files. A timeout/error is never evidence that a future retry is safe.
            let _ = tokio::time::timeout(Duration::from_secs(10), child.kill()).await;
            other.context("Git preparation timed out; reconciliation required")?
        }
    }
}

#[cfg(test)]
mod tests;
