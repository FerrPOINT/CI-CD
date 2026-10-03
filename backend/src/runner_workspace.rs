use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, bail, ensure};
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use uuid::Uuid;

const MARKER: &str = ".forge-attempt.json";

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Owner {
    schema: String,
    attempt_id: Uuid,
    lease_id: Uuid,
    generation: i64,
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
        ensure!(generation > 0, "invalid workspace lease generation");
        let root = if root.is_absolute() {
            root.to_path_buf()
        } else {
            std::env::current_dir()?.join(root)
        };
        check_ancestors(&root)?;
        fs::create_dir_all(&root).context("create runner workspace root")?;
        check_ancestors(&root)?;
        let root = root.canonicalize()?;
        let directory = root.join(format!(
            "attempt-{attempt_id}-{generation}-{}",
            Uuid::new_v4().simple()
        ));
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

    /// Caller must first obtain a durable terminal acknowledgement from the owner.
    pub async fn cleanup_after_ack(self) -> anyhow::Result<()> {
        tokio::task::spawn_blocking(move || {
            self.verify_owner()?;
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
        second.cleanup_after_ack().await.unwrap();
        assert!(first.checkout().join("old.txt").exists());
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
}
