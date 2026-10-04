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
