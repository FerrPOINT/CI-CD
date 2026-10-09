use super::*;
use cicd_domain::sdlc_workspace::{WorkspaceRole, WorkspaceTaskBinding};

struct Fixture {
    root: PathBuf,
    workspaces: PathBuf,
    source: PreparationSource,
    request: WorkspaceOperationRequest,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("forge-preparation-test-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let repo = root.join("source");
        fs::create_dir(&repo).unwrap();
        fixture_git(&repo, &["init", "--initial-branch=main"]);
        fixture_git(&repo, &["config", "user.name", "Preparation fixture"]);
        fixture_git(&repo, &["config", "user.email", "fixture@example.invalid"]);
        fs::write(repo.join("tracked.txt"), "pinned").unwrap();
        fixture_git(&repo, &["add", "."]);
        fixture_git(&repo, &["commit", "-m", "pinned"]);
        let pin = fixture_git(&repo, &["rev-parse", "HEAD"]);
        fs::write(repo.join("tracked.txt"), "new head").unwrap();
        fixture_git(&repo, &["commit", "-am", "new head"]);
        fixture_git(&root, &["clone", "--bare", "source", "owner.git"]);
        let attempt_id = Uuid::new_v4();
        let repository_id = Uuid::new_v4();
        let request = WorkspaceOperationRequest {
            contract_version: 1,
            operation_key: "prepared-fixture".into(),
            binding: WorkspaceTaskBinding {
                tracker_instance_id: "tracker-fixture".into(),
                tracker_project_id: Uuid::new_v4(),
                task_id: Uuid::new_v4(),
                root_task_id: Uuid::new_v4(),
                assignment_id: Uuid::new_v4(),
                execution_id: Uuid::new_v4(),
                routing_snapshot_id: Uuid::new_v4(),
                requirement_revision: 1,
                fencing_token: 42,
                assignment_hash: "a".repeat(64),
                workflow_task_ref: "SDLC-7".into(),
            },
            repository_id,
            source_commit: pin.clone(),
            lease_id: Uuid::new_v4(),
            attempt_id,
            workspace_generation: 3,
            workspace_id: format!("attempt-{attempt_id}-3-{}", Uuid::new_v4().simple()),
            role: WorkspaceRole::Analyst,
            access: WorkspaceAccess::ReadOnly,
        };
        Self {
            workspaces: root.join("workspaces"),
            source: PreparationSource {
                project_id: Uuid::new_v4(),
                repository_id,
                repository: root.join("owner.git"),
                source_commit: pin,
            },
            root,
            request,
        }
    }

    fn directory(&self) -> PathBuf {
        self.workspaces.join(&self.request.workspace_id)
    }

    fn checkout(&self) -> PathBuf {
        self.directory().join("workspace")
    }

    fn pin_current_source(&mut self) {
        let repo = self.root.join("source");
        fixture_git(&repo, &["add", "."]);
        fixture_git(&repo, &["commit", "-m", "physical fixture"]);
        fixture_git(
            &repo,
            &[
                "push",
                self.source.repository.to_str().unwrap(),
                "HEAD:main",
            ],
        );
        self.request.source_commit = fixture_git(&repo, &["rev-parse", "HEAD"]);
        self.source.source_commit = self.request.source_commit.clone();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn fixture_git(cwd: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(output.status.success(), "Git fixture failed");
    String::from_utf8(output.stdout).unwrap().trim().into()
}

#[tokio::test]
async fn allocated_identity_exact_pin_replay_and_restart_readback() {
    let f = Fixture::new();
    let first = prepare(&f.workspaces, &f.source, &f.request).await.unwrap();
    assert_eq!(first.workspace_id, f.request.workspace_id);
    assert_eq!(first.project_id, f.source.project_id);
    assert_eq!(first.repository_id, f.request.repository_id);
    assert_eq!(first.source_commit, f.request.source_commit);
    assert_eq!(
        fs::read_to_string(f.checkout().join("tracked.txt")).unwrap(),
        "pinned"
    );
    assert_eq!(
        fixture_git(&f.checkout(), &["rev-parse", "--abbrev-ref", "HEAD"]),
        "HEAD"
    );
    assert_eq!(
        prepare(&f.workspaces, &f.source, &f.request).await.unwrap(),
        first
    );
    assert_eq!(
        readback(&f.workspaces, &f.source, &f.request)
            .await
            .unwrap(),
        first
    );
    assert_eq!(OwnedWorkspace::inventory(&f.workspaces).unwrap().len(), 1);
    assert!(!f.directory().join(ACTIVE).exists());
    assert!(!f.directory().join(super::super::COMPLETION).exists());
}

#[tokio::test]
async fn invalid_owner_source_and_write_access_fail_before_directory_creation() {
    let mut f = Fixture::new();
    let original_repository = f.source.repository_id;
    f.source.repository_id = Uuid::new_v4();
    assert!(prepare(&f.workspaces, &f.source, &f.request).await.is_err());
    f.source.repository_id = original_repository;
    let original_project = f.source.project_id;
    f.source.project_id = Uuid::nil();
    assert!(prepare(&f.workspaces, &f.source, &f.request).await.is_err());
    f.source.project_id = original_project;
    for pin in ["main".into(), "A".repeat(40), "f".repeat(40)] {
        f.source.source_commit = pin.clone();
        f.request.source_commit = pin;
        assert!(prepare(&f.workspaces, &f.source, &f.request).await.is_err());
    }
    f.request.source_commit = fixture_git(&f.source.repository, &["rev-parse", "HEAD"]);
    f.source.source_commit = f.request.source_commit.clone();
    f.request.role = WorkspaceRole::Developer;
    f.request.access = WorkspaceAccess::ReadWrite;
    assert!(prepare(&f.workspaces, &f.source, &f.request).await.is_err());
    assert!(!f.workspaces.exists());
}

#[tokio::test]
async fn changed_task_assignment_source_project_and_lease_never_replace_intent() {
    let mut f = Fixture::new();
    let first = prepare(&f.workspaces, &f.source, &f.request).await.unwrap();
    let original = fs::read(f.directory().join(INTENT)).unwrap();
    for field in [
        "task",
        "root",
        "assignment",
        "execution",
        "snapshot",
        "revision",
        "fence",
        "hash",
        "lease",
        "generation",
        "operation",
        "role",
        "workflow",
    ] {
        let mut changed = f.request.clone();
        match field {
            "task" => changed.binding.task_id = Uuid::new_v4(),
            "root" => changed.binding.root_task_id = Uuid::new_v4(),
            "assignment" => changed.binding.assignment_id = Uuid::new_v4(),
            "execution" => changed.binding.execution_id = Uuid::new_v4(),
            "snapshot" => changed.binding.routing_snapshot_id = Uuid::new_v4(),
            "revision" => changed.binding.requirement_revision += 1,
            "fence" => changed.binding.fencing_token += 1,
            "hash" => changed.binding.assignment_hash = "b".repeat(64),
            "lease" => changed.lease_id = Uuid::new_v4(),
            "generation" => changed.workspace_generation += 1,
            "operation" => changed.operation_key = "different".into(),
            "role" => changed.role = WorkspaceRole::Reviewer,
            "workflow" => changed.binding.workflow_task_ref = "SDLC-8".into(),
            _ => unreachable!(),
        }
        assert!(
            prepare(&f.workspaces, &f.source, &changed).await.is_err(),
            "{field}"
        );
        assert!(
            readback(&f.workspaces, &f.source, &changed).await.is_err(),
            "{field}"
        );
        assert_eq!(fs::read(f.directory().join(INTENT)).unwrap(), original);
    }
    let project = f.source.project_id;
    f.source.project_id = Uuid::new_v4();
    assert!(prepare(&f.workspaces, &f.source, &f.request).await.is_err());
    f.source.project_id = project;
    assert_eq!(
        readback(&f.workspaces, &f.source, &f.request)
            .await
            .unwrap(),
        first
    );
}

#[tokio::test]
async fn concurrent_same_identity_has_one_directory_and_no_duplicate_effect() {
    let f = Fixture::new();
    let (a, b) = tokio::join!(
        prepare(&f.workspaces, &f.source, &f.request),
        prepare(&f.workspaces, &f.source, &f.request)
    );
    assert!(a.is_ok() || b.is_ok());
    assert_eq!(fs::read_dir(&f.workspaces).unwrap().count(), 1);
    assert!(readback(&f.workspaces, &f.source, &f.request).await.is_ok());
}

#[tokio::test]
async fn frozen_source_pin_and_repository_cannot_be_replaced_on_replay() {
    let mut f = Fixture::new();
    prepare(&f.workspaces, &f.source, &f.request).await.unwrap();
    let original = fs::read(f.directory().join(INTENT)).unwrap();
    let pin = f.request.source_commit.clone();
    let changed = fixture_git(&f.source.repository, &["rev-parse", "HEAD"]);
    assert_ne!(pin, changed);
    f.request.source_commit = changed.clone();
    f.source.source_commit = changed;
    assert!(prepare(&f.workspaces, &f.source, &f.request).await.is_err());
    f.request.source_commit = pin.clone();
    f.source.source_commit = pin;
    fixture_git(&f.root, &["clone", "--bare", "source", "other.git"]);
    f.source.repository = f.root.join("other.git");
    assert!(prepare(&f.workspaces, &f.source, &f.request).await.is_err());
    assert!(
        readback(&f.workspaces, &f.source, &f.request)
            .await
            .is_err()
    );
    assert_eq!(fs::read(f.directory().join(INTENT)).unwrap(), original);
    assert_eq!(
        fs::read_to_string(f.checkout().join("tracked.txt")).unwrap(),
        "pinned"
    );
}

#[tokio::test]
async fn full_sha_must_be_commit_object_and_source_must_be_bare() {
    let mut f = Fixture::new();
    let repository = f.source.repository.clone();
    f.source.repository = f.root.join("source");
    assert!(prepare(&f.workspaces, &f.source, &f.request).await.is_err());
    f.source.repository = repository;
    let tree = fixture_git(&f.source.repository, &["rev-parse", "HEAD^{tree}"]);
    f.source.source_commit = tree.clone();
    f.request.source_commit = tree;
    assert!(prepare(&f.workspaces, &f.source, &f.request).await.is_err());
    assert!(!f.workspaces.exists());
}

#[tokio::test]
async fn idle_checkout_can_finalize_lost_journal_but_unknown_active_cannot() {
    let f = Fixture::new();
    let first = prepare(&f.workspaces, &f.source, &f.request).await.unwrap();
    fs::remove_file(f.directory().join(PREPARED)).unwrap();
    assert!(
        readback(&f.workspaces, &f.source, &f.request)
            .await
            .is_err()
    );
    assert!(!f.directory().join(PREPARED).exists());
    assert_eq!(
        prepare(&f.workspaces, &f.source, &f.request).await.unwrap(),
        first
    );
    fs::remove_file(f.directory().join(PREPARED)).unwrap();
    fs::copy(f.directory().join(INTENT), f.directory().join(ACTIVE)).unwrap();
    assert!(prepare(&f.workspaces, &f.source, &f.request).await.is_err());
    assert!(
        readback(&f.workspaces, &f.source, &f.request)
            .await
            .is_err()
    );
    assert!(f.directory().join(ACTIVE).exists());
    assert!(!f.directory().join(PREPARED).exists());
}

#[tokio::test]
async fn missing_or_partial_checkout_is_retained_without_clone_retry() {
    let f = Fixture::new();
    let owned = OwnedWorkspace::create_identified(
        &f.workspaces,
        &f.request.workspace_id,
        f.request.attempt_id,
        f.request.lease_id,
        f.request.workspace_generation,
    )
    .unwrap();
    write_new(
        &owned.directory,
        INTENT,
        &validate(&f.source, &f.request).unwrap(),
    )
    .unwrap();
    assert!(prepare(&f.workspaces, &f.source, &f.request).await.is_err());
    assert!(!f.checkout().exists());
    owned.create_empty_checkout().unwrap();
    fs::write(f.checkout().join("partial"), "retained").unwrap();
    assert!(prepare(&f.workspaces, &f.source, &f.request).await.is_err());
    assert_eq!(
        fs::read_to_string(f.checkout().join("partial")).unwrap(),
        "retained"
    );
    assert!(!f.directory().join(PREPARED).exists());
}

#[tokio::test]
async fn torn_journal_dirty_or_changed_origin_fail_closed_without_repair() {
    let f = Fixture::new();
    prepare(&f.workspaces, &f.source, &f.request).await.unwrap();
    fs::write(f.directory().join(PREPARED), "{").unwrap();
    assert!(prepare(&f.workspaces, &f.source, &f.request).await.is_err());
    assert_eq!(fs::read(f.directory().join(PREPARED)).unwrap(), b"{");
    fs::copy(f.directory().join(INTENT), f.directory().join(PREPARED)).unwrap();
    fs::write(f.checkout().join("tracked.txt"), "dirty").unwrap();
    assert!(prepare(&f.workspaces, &f.source, &f.request).await.is_err());
    assert_eq!(
        fs::read_to_string(f.checkout().join("tracked.txt")).unwrap(),
        "dirty"
    );
    fs::write(f.checkout().join("tracked.txt"), "pinned").unwrap();
    fixture_git(
        &f.checkout(),
        &[
            "remote",
            "set-url",
            "origin",
            "https://example.invalid/foreign.git",
        ],
    );
    assert!(prepare(&f.workspaces, &f.source, &f.request).await.is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_paths_markers_and_foreign_checkout_are_never_followed() {
    let f = Fixture::new();
    fs::create_dir(&f.workspaces).unwrap();
    std::os::unix::fs::symlink(&f.source.repository, f.directory()).unwrap();
    assert!(prepare(&f.workspaces, &f.source, &f.request).await.is_err());
    fs::remove_file(f.directory()).unwrap();
    prepare(&f.workspaces, &f.source, &f.request).await.unwrap();
    fs::remove_file(f.directory().join(PREPARED)).unwrap();
    std::os::unix::fs::symlink(f.directory().join(INTENT), f.directory().join(PREPARED)).unwrap();
    assert!(prepare(&f.workspaces, &f.source, &f.request).await.is_err());
    assert!(
        readback(&f.workspaces, &f.source, &f.request)
            .await
            .is_err()
    );
    fs::remove_file(f.directory().join(PREPARED)).unwrap();
    fs::remove_dir_all(f.checkout()).unwrap();
    std::os::unix::fs::symlink(&f.source.repository, f.checkout()).unwrap();
    assert!(prepare(&f.workspaces, &f.source, &f.request).await.is_err());
    assert!(f.source.repository.join("HEAD").is_file());
}

#[tokio::test]
async fn legacy_or_executed_workspace_cannot_be_enrolled_by_preparation() {
    let f = Fixture::new();
    let owned = OwnedWorkspace::create_identified(
        &f.workspaces,
        &f.request.workspace_id,
        f.request.attempt_id,
        f.request.lease_id,
        f.request.workspace_generation,
    )
    .unwrap();
    assert!(prepare(&f.workspaces, &f.source, &f.request).await.is_err());
    assert!(!f.directory().join(INTENT).exists());
    write_new(
        &owned.directory,
        INTENT,
        &validate(&f.source, &f.request).unwrap(),
    )
    .unwrap();
    owned.record_completion("success").unwrap();
    assert!(prepare(&f.workspaces, &f.source, &f.request).await.is_err());
    assert!(!f.checkout().exists());
}

#[tokio::test]
async fn local_git_filters_helpers_includes_and_upload_hooks_are_rejected() {
    let f = Fixture::new();
    for key in [
        "uploadpack.packObjectsHook",
        "core.hooksPath",
        "include.path",
    ] {
        fixture_git(
            &f.source.repository,
            &["config", key, "unsupported-fixture"],
        );
        assert!(
            prepare(&f.workspaces, &f.source, &f.request).await.is_err(),
            "{key}"
        );
        fixture_git(&f.source.repository, &["config", "--unset", key]);
        assert!(!f.workspaces.exists());
    }
    prepare(&f.workspaces, &f.source, &f.request).await.unwrap();
    for key in [
        "filter.fixture.clean",
        "filter.fixture.process",
        "credential.helper",
        "include.path",
    ] {
        fixture_git(&f.checkout(), &["config", key, "unsupported-fixture"]);
        assert!(
            prepare(&f.workspaces, &f.source, &f.request).await.is_err(),
            "{key}"
        );
        assert!(
            readback(&f.workspaces, &f.source, &f.request)
                .await
                .is_err(),
            "{key}"
        );
        fixture_git(&f.checkout(), &["config", "--unset", key]);
    }
    assert!(readback(&f.workspaces, &f.source, &f.request).await.is_ok());
}

async fn assert_physical_rejection_without_repair(f: &Fixture) {
    let index_path = f.checkout().join(".git/index");
    let index = fs::read(&index_path).unwrap();
    let intent = fs::read(f.directory().join(INTENT)).unwrap();
    let final_journal = fs::read(f.directory().join(PREPARED)).unwrap();
    let dirty = fs::read(f.checkout().join("tracked.txt")).unwrap();
    assert!(prepare(&f.workspaces, &f.source, &f.request).await.is_err());
    assert!(
        readback(&f.workspaces, &f.source, &f.request)
            .await
            .is_err()
    );
    assert_eq!(fs::read(&index_path).unwrap(), index);
    assert_eq!(
        fs::read(f.directory().join(PREPARED)).unwrap(),
        final_journal
    );
    fs::remove_file(f.directory().join(PREPARED)).unwrap();
    assert!(prepare(&f.workspaces, &f.source, &f.request).await.is_err());
    assert!(!f.directory().join(PREPARED).exists());
    assert!(!f.directory().join(ACTIVE).exists());
    assert_eq!(fs::read(&index_path).unwrap(), index);
    assert_eq!(fs::read(f.directory().join(INTENT)).unwrap(), intent);
    assert_eq!(fs::read(f.checkout().join("tracked.txt")).unwrap(), dirty);
}

#[tokio::test]
async fn assume_unchanged_dirty_replay_readback_and_lost_journal_fail_without_index_repair() {
    let f = Fixture::new();
    prepare(&f.workspaces, &f.source, &f.request).await.unwrap();
    fixture_git(
        &f.checkout(),
        &["update-index", "--assume-unchanged", "tracked.txt"],
    );
    fs::write(f.checkout().join("tracked.txt"), "forged").unwrap();
    assert_eq!(fixture_git(&f.checkout(), &["status", "--porcelain"]), "");
    assert_physical_rejection_without_repair(&f).await;
    assert!(fixture_git(&f.checkout(), &["ls-files", "-v"]).starts_with("h "));
}

#[tokio::test]
async fn skip_worktree_dirty_replay_readback_and_lost_journal_fail_without_index_repair() {
    let f = Fixture::new();
    prepare(&f.workspaces, &f.source, &f.request).await.unwrap();
    fixture_git(
        &f.checkout(),
        &["update-index", "--skip-worktree", "tracked.txt"],
    );
    fs::write(f.checkout().join("tracked.txt"), "forged").unwrap();
    assert_eq!(fixture_git(&f.checkout(), &["status", "--porcelain"]), "");
    assert_physical_rejection_without_repair(&f).await;
    assert!(fixture_git(&f.checkout(), &["ls-files", "-v"]).starts_with("S "));
}

#[tokio::test]
async fn unsupported_index_flags_are_rejected_even_with_unchanged_physical_bytes() {
    for flag in ["--assume-unchanged", "--skip-worktree"] {
        let f = Fixture::new();
        prepare(&f.workspaces, &f.source, &f.request).await.unwrap();
        fixture_git(&f.checkout(), &["update-index", flag, "tracked.txt"]);
        assert_physical_rejection_without_repair(&f).await;
    }
}

#[cfg(unix)]
#[tokio::test]
async fn same_size_restored_mtime_and_spoofed_stat_cache_cannot_hide_physical_bytes() {
    use sha1::{Digest, Sha1};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let f = Fixture::new();
    prepare(&f.workspaces, &f.source, &f.request).await.unwrap();
    fixture_git(&f.checkout(), &["update-index", "--index-version=2"]);
    let file_path = f.checkout().join("tracked.txt");
    let before = fs::metadata(&file_path).unwrap();
    fs::write(&file_path, "forged").unwrap();
    fs::File::options()
        .write(true)
        .open(&file_path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(before.modified().unwrap()))
        .unwrap();
    fs::set_permissions(&file_path, fs::Permissions::from_mode(0o600)).unwrap();
    let meta = fs::metadata(&file_path).unwrap();
    assert_eq!(meta.len(), before.len());
    assert_eq!(meta.modified().unwrap(), before.modified().unwrap());
    assert_eq!(meta.mode() & 0o777, 0o600);
    let index_mode = if meta.mode() & 0o100 != 0 {
        0o100755
    } else {
        0o100644
    };
    // An adversarial index can claim current stat fields while retaining the pinned blob OID.
    let index_path = f.checkout().join(".git/index");
    let mut index = fs::read(&index_path).unwrap();
    assert_eq!(&index[..12], b"DIRC\0\0\0\x02\0\0\0\x01");
    let stats = [
        meta.ctime() as u32,
        meta.ctime_nsec() as u32,
        meta.mtime() as u32,
        meta.mtime_nsec() as u32,
        meta.dev() as u32,
        meta.ino() as u32,
        index_mode,
        meta.uid(),
        meta.gid(),
        meta.len() as u32,
    ];
    for (slot, value) in index[12..52].chunks_exact_mut(4).zip(stats) {
        slot.copy_from_slice(&value.to_be_bytes());
    }
    let end = index.len() - 20;
    let checksum = Sha1::digest(&index[..end]);
    index[end..].copy_from_slice(&checksum);
    fs::write(&index_path, index).unwrap();
    // Avoid Git's racy-stat fallback without slowing this fixture with a sleep.
    fs::File::options()
        .write(true)
        .open(&index_path)
        .unwrap()
        .set_times(
            fs::FileTimes::new().set_modified(before.modified().unwrap() + Duration::from_secs(10)),
        )
        .unwrap();
    assert_eq!(fixture_git(&f.checkout(), &["status", "--porcelain"]), "");
    assert!(
        readback(&f.workspaces, &f.source, &f.request)
            .await
            .unwrap_err()
            .to_string()
            .contains("physical checkout differs from pinned tree")
    );
    assert_physical_rejection_without_repair(&f).await;
}

#[cfg(unix)]
#[tokio::test]
async fn physical_executable_mode_is_checked_even_when_git_filemode_is_disabled() {
    use std::os::unix::fs::PermissionsExt;
    let mut f = Fixture::new();
    let source_file = f.root.join("source/tracked.txt");
    fs::set_permissions(source_file, fs::Permissions::from_mode(0o755)).unwrap();
    f.pin_current_source();
    prepare(&f.workspaces, &f.source, &f.request).await.unwrap();
    fixture_git(&f.checkout(), &["config", "core.filemode", "false"]);
    fs::set_permissions(
        f.checkout().join("tracked.txt"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert_eq!(fixture_git(&f.checkout(), &["status", "--porcelain"]), "");
    assert_physical_rejection_without_repair(&f).await;
}

#[cfg(unix)]
#[tokio::test]
async fn group_execute_cannot_substitute_git_owner_execute_mode() {
    use std::os::unix::fs::PermissionsExt;
    let mut f = Fixture::new();
    fs::set_permissions(
        f.root.join("source/tracked.txt"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    f.pin_current_source();
    prepare(&f.workspaces, &f.source, &f.request).await.unwrap();
    fixture_git(&f.checkout(), &["config", "core.filemode", "false"]);
    fs::set_permissions(
        f.checkout().join("tracked.txt"),
        fs::Permissions::from_mode(0o654),
    )
    .unwrap();
    assert_eq!(fixture_git(&f.checkout(), &["status", "--porcelain"]), "");
    assert_physical_rejection_without_repair(&f).await;
}

#[cfg(unix)]
#[tokio::test]
async fn physical_symlink_target_and_type_are_verified_without_following_target() {
    let mut f = Fixture::new();
    let source_link = f.root.join("source/link");
    std::os::unix::fs::symlink("tracked.txt", &source_link).unwrap();
    f.pin_current_source();
    let original = prepare(&f.workspaces, &f.source, &f.request).await.unwrap();
    assert_eq!(
        readback(&f.workspaces, &f.source, &f.request)
            .await
            .unwrap(),
        original
    );
    let link = f.checkout().join("link");
    fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink("foreign.txt", &link).unwrap();
    assert!(prepare(&f.workspaces, &f.source, &f.request).await.is_err());
    assert!(
        readback(&f.workspaces, &f.source, &f.request)
            .await
            .is_err()
    );
    assert_eq!(fs::read_link(&link).unwrap(), PathBuf::from("foreign.txt"));
    fs::remove_file(&link).unwrap();
    fs::write(&link, "tracked.txt").unwrap();
    assert_physical_rejection_without_repair(&f).await;
}

#[tokio::test]
async fn ignored_files_and_empty_untracked_directories_are_not_pinned_contents() {
    let mut f = Fixture::new();
    fs::write(f.root.join("source/.gitignore"), "hidden\n").unwrap();
    f.pin_current_source();
    prepare(&f.workspaces, &f.source, &f.request).await.unwrap();
    fs::write(f.checkout().join("hidden"), "unexpected").unwrap();
    assert_eq!(fixture_git(&f.checkout(), &["status", "--porcelain"]), "");
    assert!(
        readback(&f.workspaces, &f.source, &f.request)
            .await
            .is_err()
    );
    fs::remove_file(f.checkout().join("hidden")).unwrap();
    fs::create_dir(f.checkout().join("empty-extra")).unwrap();
    assert_physical_rejection_without_repair(&f).await;
    assert!(f.checkout().join("empty-extra").is_dir());
}

#[tokio::test]
async fn moderate_nested_repository_inventory_and_clean_recovery_do_not_use_metadata_4k_cap() {
    let mut f = Fixture::new();
    let nested = f.root.join("source/nested/files");
    fs::create_dir_all(&nested).unwrap();
    for n in 0..1000 {
        fs::write(
            nested.join(format!("tracked-file-{n:04}.txt")),
            format!("blob {n}\n"),
        )
        .unwrap();
    }
    f.pin_current_source();
    let first = prepare(&f.workspaces, &f.source, &f.request).await.unwrap();
    assert!(fixture_git(&f.checkout(), &["ls-files", "--stage", "-v"]).len() > 4096);
    let index_path = f.checkout().join(".git/index");
    let index = fs::read(&index_path).unwrap();
    assert_eq!(
        prepare(&f.workspaces, &f.source, &f.request).await.unwrap(),
        first
    );
    assert_eq!(
        readback(&f.workspaces, &f.source, &f.request)
            .await
            .unwrap(),
        first
    );
    fs::remove_file(f.directory().join(PREPARED)).unwrap();
    assert_eq!(
        prepare(&f.workspaces, &f.source, &f.request).await.unwrap(),
        first
    );
    assert_eq!(fs::read(index_path).unwrap(), index);
}
