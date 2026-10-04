use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, bail, ensure};
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use uuid::Uuid;

pub mod preparation;

const MARKER: &str = ".forge-attempt.json";
const COMPLETION: &str = ".forge-completion.json";
const ACK: &str = ".forge-completion-ack.json";
const MAX_INVENTORY_ENTRIES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Owner {
    schema: String,
    attempt_id: Uuid,
    lease_id: Uuid,
    generation: i64,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Completion {
    schema: String,
    owner: Owner,
    terminal_status: String,
}

/// Local recovery metadata only; a fresh server readback authorizes cleanup.
#[derive(Debug, Serialize)]
pub struct RetainedWorkspace {
    pub workspace_id: String,
    pub attempt_id: Uuid,
    pub lease_id: Uuid,
    pub generation: i64,
    pub terminal_status: Option<String>,
    pub acknowledged: bool,
}

/// A physical attempt directory, not an SDLC workspace receipt or a sandbox.
pub struct OwnedWorkspace {
    root: PathBuf,
    directory: PathBuf,
    checkout: PathBuf,
    owner: Owner,
}

impl OwnedWorkspace {
    pub fn create(
        root: &Path,
        attempt_id: Uuid,
        lease_id: Uuid,
        generation: i64,
    ) -> anyhow::Result<Self> {
        let workspace_id = format!(
            "attempt-{attempt_id}-{generation}-{}",
            Uuid::new_v4().simple()
        );
        Self::create_identified(root, &workspace_id, attempt_id, lease_id, generation)
    }

    fn create_identified(
        root: &Path,
        workspace_id: &str,
        attempt_id: Uuid,
        lease_id: Uuid,
        generation: i64,
    ) -> anyhow::Result<Self> {
        ensure!(generation > 0, "invalid workspace lease generation");
        ensure!(
            !attempt_id.is_nil() && !lease_id.is_nil(),
            "invalid workspace lease identity"
        );
        let root = if root.is_absolute() {
            root.to_path_buf()
        } else {
            std::env::current_dir()?.join(root)
        };
        check_ancestors(&root)?;
        fs::create_dir_all(&root).context("create runner workspace root")?;
        check_ancestors(&root)?;
        let root = root.canonicalize()?;
        let prefix = format!("attempt-{attempt_id}-{generation}-");
        let nonce = workspace_id.strip_prefix(&prefix).unwrap_or("");
        ensure!(
            Uuid::parse_str(nonce).is_ok_and(|id| !id.is_nil() && id.simple().to_string() == nonce),
            "workspace identity must match attempt and generation"
        );
        let directory = root.join(workspace_id);
        fs::create_dir(&directory).context("create fresh attempt directory")?;
        let owner = Owner {
            schema: "forge/attempt-workspace/v1".to_owned(),
            attempt_id,
            lease_id,
            generation,
        };
        let mut marker = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(directory.join(MARKER))?;
        marker.write_all(&serde_json::to_vec(&owner)?)?;
        marker.sync_all()?;
        #[cfg(unix)]
        {
            fs::File::open(&directory)?.sync_all()?;
            fs::File::open(&root)?.sync_all()?;
        }
        let checkout = directory.join("workspace");
        Ok(Self {
            root,
            directory,
            checkout,
            owner,
        })
    }

    pub fn checkout(&self) -> &Path {
        &self.checkout
    }

    pub fn verify_identity(
        &self,
        attempt_id: Uuid,
        lease_id: Uuid,
        generation: i64,
    ) -> anyhow::Result<()> {
        ensure!(
            self.owner.attempt_id == attempt_id
                && self.owner.lease_id == lease_id
                && self.owner.generation == generation,
            "workspace readback identity changed"
        );
        self.verify_owner()
    }

    pub fn record_completion(&self, terminal_status: &str) -> anyhow::Result<()> {
        ensure!(
            matches!(terminal_status, "success" | "failed" | "canceled"),
            "invalid workspace completion outcome"
        );
        self.verify_owner()?;
        self.write_record(
            COMPLETION,
            &Completion {
                schema: "forge/attempt-completion/v1".to_owned(),
                owner: self.owner.clone(),
                terminal_status: terminal_status.to_owned(),
            },
        )
    }

    /// Call only after an exact terminal ACK or authenticated owner readback.
    pub fn acknowledge_completion(&self, terminal_status: &str) -> anyhow::Result<()> {
        self.verify_owner()?;
        let completion = self.read_record(COMPLETION)?;
        ensure!(
            completion.terminal_status == terminal_status,
            "workspace terminal acknowledgement mismatch"
        );
        self.write_record(ACK, &completion)
    }

    fn read_record(&self, name: &str) -> anyhow::Result<Completion> {
        let completion: Completion = read_guarded_json(&self.directory.join(name))?;
        ensure!(
            completion.schema == "forge/attempt-completion/v1"
                && completion.owner == self.owner
                && matches!(
                    completion.terminal_status.as_str(),
                    "success" | "failed" | "canceled"
                ),
            "invalid workspace completion record"
        );
        Ok(completion)
    }

    fn write_record(&self, name: &str, record: &Completion) -> anyhow::Result<()> {
        let path = self.directory.join(name);
        match OpenOptions::new().create_new(true).write(true).open(&path) {
            Ok(mut file) => {
                file.write_all(&serde_json::to_vec(record)?)?;
                file.sync_all()?;
                #[cfg(unix)]
                fs::File::open(&self.directory)?.sync_all()?;
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                ensure!(
                    self.read_record(name)? == *record,
                    "workspace completion conflict"
                );
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn reopen(root: &Path, workspace_id: &str) -> anyhow::Result<Self> {
        ensure!(
            workspace_id.starts_with("attempt-")
                && !workspace_id.contains(['/', '\\'])
                && workspace_id.len() <= 128,
            "invalid workspace identity"
        );
        check_ancestors(root)?;
        let root = root.canonicalize()?;
        let directory = root.join(workspace_id);
        check_ancestors(&directory)?;
        let owner: Owner = read_guarded_json(&directory.join(MARKER))?;
        let prefix = format!("attempt-{}-{}-", owner.attempt_id, owner.generation);
        let suffix = workspace_id
            .strip_prefix(&prefix)
            .context("workspace identity mismatch")?;
        let nonce = Uuid::parse_str(suffix).context("invalid workspace nonce")?;
        ensure!(
            owner.schema == "forge/attempt-workspace/v1"
                && !owner.attempt_id.is_nil()
                && !owner.lease_id.is_nil()
                && owner.generation > 0
                && nonce.simple().to_string() == suffix,
            "invalid workspace ownership"
        );
        let checkout = directory.join("workspace");
        let workspace = Self {
            root,
            directory,
            checkout,
            owner,
        };
        workspace.verify_owner()?;
        Ok(workspace)
    }

    pub fn inventory(root: &Path) -> anyhow::Result<Vec<RetainedWorkspace>> {
        let root = if root.is_absolute() {
            root.to_path_buf()
        } else {
            std::env::current_dir()?.join(root)
        };
        check_ancestors(&root)?;
        if !root.exists() {
            return Ok(Vec::new());
        }
        let mut inventory = Vec::new();
        for (index, entry) in fs::read_dir(&root)?.enumerate() {
            ensure!(
                index < MAX_INVENTORY_ENTRIES,
                "workspace inventory exceeds safety limit"
            );
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                bail!("invalid workspace directory name");
            };
            if !name.starts_with("attempt-") {
                continue;
            }
            let workspace = Self::reopen(&root, name)?;
            let terminal_status = match fs::symlink_metadata(workspace.directory.join(COMPLETION)) {
                Ok(_) => Some(workspace.read_record(COMPLETION)?.terminal_status),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.into()),
            };
            let acknowledged = match fs::symlink_metadata(workspace.directory.join(ACK)) {
                Ok(_) => {
                    let ack = workspace.read_record(ACK)?;
                    ensure!(
                        Some(&ack.terminal_status) == terminal_status.as_ref(),
                        "orphan workspace acknowledgement"
                    );
                    true
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                Err(error) => return Err(error.into()),
            };
            inventory.push(RetainedWorkspace {
                workspace_id: name.to_owned(),
                attempt_id: workspace.owner.attempt_id,
                lease_id: workspace.owner.lease_id,
                generation: workspace.owner.generation,
                terminal_status,
                acknowledged,
            });
        }
        inventory.sort_by(|a, b| a.workspace_id.cmp(&b.workspace_id));
        Ok(inventory)
    }

    pub fn create_empty_checkout(&self) -> anyhow::Result<()> {
        self.verify_owner()?;
        fs::create_dir(&self.checkout).context("create empty attempt checkout")
    }

    /// Legacy unpinned jobs remain supported, but cannot prove an SDLC pin.
    pub async fn clone_checkout(
        &self,
        repository: &str,
        commit_sha: Option<&str>,
        git_ref: &str,
    ) -> anyhow::Result<()> {
        self.verify_owner()?;
        if let Some(sha) = commit_sha {
            ensure!(
                sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "checkout requires a full commit SHA"
            );
        }
        let mut clone = self.git(&self.directory);
        clone.args(["clone", "--quiet", "--no-checkout", "--no-hardlinks"]);
        if commit_sha.is_none() && !git_ref.is_empty() {
            clone.args(["--branch", git_ref]);
        }
        clone.args(["--", repository, "workspace"]);
        run_git(clone, "clone failed; workspace retained for reconciliation").await?;
        let target = commit_sha.unwrap_or("HEAD");
        let mut checkout = self.git(&self.checkout);
        checkout.args(["checkout", "--quiet", "--detach", target, "--"]);
        run_git(
            checkout,
            "checkout failed; workspace retained for reconciliation",
        )
        .await?;
        if let Some(sha) = commit_sha {
            let mut head = self.git(&self.checkout);
            head.args(["rev-parse", "--verify", "HEAD^{commit}"]);
            let head = run_git(head, "commit readback failed").await?;
            ensure!(
                String::from_utf8(head)?.trim().eq_ignore_ascii_case(sha),
                "checkout commit differs from the assignment pin"
            );
        }
        let mut status = self.git(&self.checkout);
        status.args(["status", "--porcelain=v1", "--untracked-files=all"]);
        ensure!(
            run_git(status, "checkout status readback failed")
                .await?
                .is_empty(),
            "initial attempt checkout is not clean"
        );
        self.verify_owner()?;
        Ok(())
    }

    /// Read-only physical observation, never an SDLC admission or resource receipt.
    pub async fn observe_pinned_source(&self, repository: &Path, sha: &str) -> anyhow::Result<()> {
        self.verify_owner()?;
        check_ancestors(&self.checkout.join(".git"))?;
        check_ancestors(repository)?;
        let expected = repository.canonicalize()?;
        let origin = self
            .bounded_git_read(&["config", "--get", "remote.origin.url"])
            .await?;
        let origin = String::from_utf8(origin)?;
        ensure!(
            Path::new(origin.trim()).is_absolute(),
            "origin must be owner-local"
        );
        check_ancestors(Path::new(origin.trim()))?;
        ensure!(
            Path::new(origin.trim()).canonicalize()? == expected,
            "repository pin mismatch"
        );
        let head = self
            .bounded_git_read(&["rev-parse", "--verify", "HEAD^{commit}"])
            .await?;
        ensure!(
            String::from_utf8(head)?.trim() == sha,
            "source pin mismatch"
        );
        let reference = self
            .bounded_git_read(&["rev-parse", "--abbrev-ref", "HEAD"])
            .await?;
        ensure!(
            String::from_utf8(reference)?.trim() == "HEAD",
            "source must remain detached"
        );
        ensure!(
            self.bounded_git_read(&["status", "--porcelain=v1", "--untracked-files=all"])
                .await?
                .is_empty(),
            "workspace is not clean"
        );
        self.verify_owner()?;
        Ok(())
    }

    async fn bounded_git_read(&self, args: &[&str]) -> anyhow::Result<Vec<u8>> {
        use tokio::io::AsyncReadExt;
        let mut command = self.git(&self.checkout);
        let mut child = command
            .args(args)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let mut bytes = Vec::new();
        child
            .stdout
            .take()
            .context("missing git stdout")?
            .take(4097)
            .read_to_end(&mut bytes)
            .await?;
        ensure!(bytes.len() <= 4096, "git observation exceeds bound");
        ensure!(child.wait().await?.success(), "git observation failed");
        Ok(bytes)
    }

    /// Caller must first obtain a durable terminal acknowledgement from the owner.
    pub async fn cleanup_after_ack(self) -> anyhow::Result<()> {
        tokio::task::spawn_blocking(move || {
            self.verify_owner()?;
            ensure!(
                self.read_record(COMPLETION)? == self.read_record(ACK)?,
                "workspace terminal acknowledgement missing"
            );
            fs::remove_dir_all(&self.directory).context("remove acknowledged attempt workspace")
        })
        .await
        .context("join workspace cleanup")?
    }

    fn verify_owner(&self) -> anyhow::Result<()> {
        check_ancestors(&self.directory)?;
        ensure!(
            self.directory.parent() == Some(self.root.as_path())
                && self.directory.canonicalize()? == self.directory,
            "attempt workspace escaped its configured root"
        );
        let marker_path = self.directory.join(MARKER);
        let metadata = fs::symlink_metadata(&marker_path)?;
        ensure!(
            metadata.is_file() && !is_link(&metadata) && metadata.len() <= 4096,
            "invalid workspace ownership marker"
        );
        let actual: Owner = serde_json::from_slice(&fs::read(marker_path)?)?;
        ensure!(actual == self.owner, "workspace ownership marker mismatch");
        if self.checkout.exists() || fs::symlink_metadata(&self.checkout).is_ok() {
            let metadata = fs::symlink_metadata(&self.checkout)?;
            ensure!(
                metadata.is_dir() && !is_link(&metadata),
                "checkout is not an owned directory"
            );
        }
        Ok(())
    }

    fn git(&self, cwd: &Path) -> Command {
        let mut command = Command::new("git");
        command
            .args(["--no-replace-objects", "-c", "core.fsmonitor=false", "-c"])
            .arg(format!(
                "core.hooksPath={}",
                self.directory.join("disabled-hooks").display()
            ))
            .current_dir(cwd)
            .stdin(Stdio::null())
            .env("GIT_TERMINAL_PROMPT", "0");
        for name in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        ] {
            command.env_remove(name);
        }
        command
    }
}

fn read_guarded_json<T: serde::de::DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && !is_link(&metadata) && metadata.len() <= 4096,
        "invalid workspace metadata file"
    );
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(4097).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 4096,
        "workspace metadata exceeds safety limit"
    );
    serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("invalid workspace metadata"))
}

async fn run_git(mut command: Command, diagnostic: &str) -> anyhow::Result<Vec<u8>> {
    // Never include transport output: URLs and helpers may contain credentials.
    command.stderr(Stdio::null());
    let output = command
        .output()
        .await
        .context("start checkout Git operation")?;
    if !output.status.success() {
        bail!("{diagnostic}");
    }
    Ok(output.stdout)
}

fn check_ancestors(path: &Path) -> anyhow::Result<()> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => ensure!(
                metadata.is_dir() && !is_link(&metadata),
                "workspace path contains a link or non-directory"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace(root: &Path) -> OwnedWorkspace {
        OwnedWorkspace::create(root, Uuid::new_v4(), Uuid::new_v4(), 1).unwrap()
    }

    fn root() -> PathBuf {
        std::env::temp_dir().join(format!("forge-owned-workspace-test-{}", Uuid::new_v4()))
    }

    fn acknowledge_fixture(workspace: &OwnedWorkspace) {
        workspace.record_completion("success").unwrap();
        workspace.acknowledge_completion("success").unwrap();
    }

    #[tokio::test]
    async fn fresh_attempts_do_not_reuse_or_delete_existing_directories() {
        let root = root();
        let first = workspace(&root);
        first.create_empty_checkout().unwrap();
        fs::write(first.checkout().join("old.txt"), "previous run").unwrap();
        let second = workspace(&root);
        second.create_empty_checkout().unwrap();
        assert_ne!(first.checkout(), second.checkout());
        assert!(!second.checkout().join("old.txt").exists());
        acknowledge_fixture(&second);
        second.cleanup_after_ack().await.unwrap();
        assert!(first.checkout().join("old.txt").exists());
        acknowledge_fixture(&first);
        first.cleanup_after_ack().await.unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[tokio::test]
    async fn foreign_marker_prevents_cleanup() {
        let root = root();
        let owned = workspace(&root);
        fs::write(owned.directory.join(MARKER), "{}").unwrap();
        let directory = owned.directory.clone();
        assert!(owned.cleanup_after_ack().await.is_err());
        assert!(directory.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn linked_root_and_replaced_checkout_fail_closed() {
        let root = root();
        fs::create_dir(&root).unwrap();
        let target = root.join("target");
        fs::create_dir(&target).unwrap();
        let link = root.join("linked");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(OwnedWorkspace::create(&link, Uuid::new_v4(), Uuid::new_v4(), 1).is_err());
        let owned = workspace(&root);
        std::os::unix::fs::symlink(&target, owned.checkout()).unwrap();
        assert!(owned.cleanup_after_ack().await.is_err());
        assert!(target.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn clone_reads_exact_old_commit_not_current_branch_head() {
        let root = root();
        fs::create_dir(&root).unwrap();
        let repository = root.join("repository");
        fs::create_dir(&repository).unwrap();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .args(args)
                .current_dir(&repository)
                .output()
                .unwrap();
            assert!(output.status.success(), "Git fixture failed");
            String::from_utf8(output.stdout).unwrap().trim().to_owned()
        };
        git(&["init", "--initial-branch=main"]);
        git(&["config", "user.name", "Workspace test"]);
        git(&["config", "user.email", "workspace@example.invalid"]);
        fs::write(repository.join("tracked.txt"), "first").unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "first"]);
        let pin = git(&["rev-parse", "HEAD"]);
        fs::write(repository.join("tracked.txt"), "second").unwrap();
        git(&["commit", "-am", "second"]);
        let owned = workspace(&root);
        owned
            .clone_checkout(repository.to_str().unwrap(), Some(&pin), "main")
            .await
            .unwrap();
        assert_eq!(
            fs::read_to_string(owned.checkout().join("tracked.txt")).unwrap(),
            "first"
        );
        acknowledge_fixture(&owned);
        owned.cleanup_after_ack().await.unwrap();
        let invalid = workspace(&root);
        assert!(
            invalid
                .clone_checkout(repository.to_str().unwrap(), Some("main"), "main")
                .await
                .is_err()
        );
        assert!(!invalid.checkout().exists());
        let missing = workspace(&root);
        assert!(
            missing
                .clone_checkout(repository.to_str().unwrap(), Some(&"f".repeat(40)), "main")
                .await
                .is_err()
        );
        assert!(missing.directory.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn completion_journal_survives_reopen_and_never_overwrites_conflicts() {
        let root = root();
        let owned = workspace(&root);
        owned.create_empty_checkout().unwrap();
        let inventory = OwnedWorkspace::inventory(&root).unwrap();
        assert_eq!(inventory.len(), 1);
        assert_eq!(inventory[0].terminal_status, None);
        assert!(!inventory[0].acknowledged);
        owned.record_completion("failed").unwrap();
        owned.record_completion("failed").unwrap();
        assert!(owned.record_completion("success").is_err());
        assert!(owned.acknowledge_completion("success").is_err());
        let record = OwnedWorkspace::inventory(&root).unwrap().pop().unwrap();
        assert_eq!(record.terminal_status.as_deref(), Some("failed"));
        let reopened = OwnedWorkspace::reopen(&root, &record.workspace_id).unwrap();
        assert!(reopened.cleanup_after_ack().await.is_err());
        let reopened = OwnedWorkspace::reopen(&root, &record.workspace_id).unwrap();
        reopened.acknowledge_completion("failed").unwrap();
        assert!(OwnedWorkspace::inventory(&root).unwrap()[0].acknowledged);
        reopened.cleanup_after_ack().await.unwrap();
        assert!(OwnedWorkspace::inventory(&root).unwrap().is_empty());
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn inventory_denies_torn_or_foreign_records_and_path_substitution() {
        let root = root();
        let owned = workspace(&root);
        owned.record_completion("success").unwrap();
        let path = owned.directory.join(COMPLETION);
        let original = fs::read(&path).unwrap();
        fs::write(&path, b"{").unwrap();
        assert!(OwnedWorkspace::inventory(&root).is_err());
        assert!(owned.record_completion("success").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"{");
        fs::write(&path, original).unwrap();
        assert!(OwnedWorkspace::reopen(&root, "../outside").is_err());
        fs::rename(&owned.directory, root.join("attempt-forged-name")).unwrap();
        assert!(OwnedWorkspace::inventory(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reopened_workspace_must_still_match_the_receipt_identity() {
        let root = root();
        let owned = workspace(&root);
        let record = OwnedWorkspace::inventory(&root).unwrap().pop().unwrap();
        let mut changed = owned.owner.clone();
        changed.lease_id = Uuid::new_v4();
        fs::write(
            owned.directory.join(MARKER),
            serde_json::to_vec(&changed).unwrap(),
        )
        .unwrap();
        let reopened = OwnedWorkspace::reopen(&root, &record.workspace_id).unwrap();
        assert!(
            reopened
                .verify_identity(record.attempt_id, record.lease_id, record.generation)
                .is_err()
        );
        assert!(owned.directory.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn completion_metadata_links_are_not_read_or_overwritten() {
        let root = root();
        let owned = workspace(&root);
        let outside = root.join("foreign.json");
        fs::write(&outside, "secret fixture").unwrap();
        std::os::unix::fs::symlink(&outside, owned.directory.join(COMPLETION)).unwrap();
        assert!(owned.record_completion("failed").is_err());
        assert!(OwnedWorkspace::inventory(&root).is_err());
        assert_eq!(fs::read_to_string(&outside).unwrap(), "secret fixture");
        fs::remove_dir_all(root).unwrap();
    }
}
