use super::*;
use axum::http::StatusCode;
use std::{fs, process::Command};

struct Fixture {
    root: PathBuf,
    bare: PathBuf,
    initial: String,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("forge-merge-fence-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let source = root.join("source");
        fs::create_dir(&source).unwrap();
        let bare = root.join("owner.git");
        let git = |args: &[&str]| Self::command(&source, args);
        git(&["init", "--quiet", "--initial-branch=main"]);
        fs::write(source.join("base"), "base\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "--quiet", "-m", "base"]);
        let initial = git(&["rev-parse", "HEAD"]);
        git(&["switch", "--quiet", "-c", "task"]);
        fs::write(source.join("feature"), "task\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "--quiet", "-m", "task"]);
        git(&["switch", "--quiet", "main"]);
        fs::write(source.join("main-data"), "main\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "--quiet", "-m", "main data"]);
        for tag in ["main", "task", "tag-only"] {
            git(&["tag", tag, &initial]);
        }
        git(&["clone", "--quiet", "--bare", ".", bare.to_str().unwrap()]);
        Self {
            root,
            bare,
            initial,
        }
    }

    fn command(path: &std::path::Path, args: &[&str]) -> String {
        let result = Command::new("git")
            .current_dir(path)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        String::from_utf8(result.stdout).unwrap().trim().to_string()
    }

    fn git(&self, args: &[&str]) -> String {
        Self::command(&self.bare, args)
    }

    fn merged(&self, source: &BranchSnapshot, target: &BranchSnapshot) -> String {
        let tree = self.git(&["merge-tree", "--write-tree", "-z", &target.oid, &source.oid]);
        let tree = tree.split('\0').next().unwrap();
        self.git(&[
            "commit-tree",
            tree,
            "-p",
            &target.oid,
            "-p",
            &source.oid,
            "-m",
            "candidate",
        ])
    }

    fn advanced(&self, parent: &str) -> String {
        let tree = self.git(&["rev-parse", &format!("{parent}^{{tree}}")]);
        self.git(&[
            "commit-tree",
            &tree,
            "-p",
            parent,
            "-m",
            "concurrent advance",
        ])
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[tokio::test]
async fn merge_snapshots_ignore_tags_and_preserve_both_branch_histories() {
    let f = Fixture::new();
    let target = branch_snapshot(&f.bare, "main").await.unwrap();
    let source = branch_snapshot(&f.bare, "refs/heads/task").await.unwrap();
    assert_ne!(target.oid, f.initial);
    assert_ne!(source.oid, f.initial);
    let merged = f.merged(&source, &target);
    publish_merge(&f.bare, &source, &target, &merged)
        .await
        .unwrap();
    assert_eq!(f.git(&["show", "refs/heads/main:main-data"]), "main");
    assert_eq!(f.git(&["show", "refs/heads/main:feature"]), "task");
    assert_eq!(
        f.git(&["rev-list", "--parents", "-n", "1", "refs/heads/main"]),
        format!("{merged} {} {}", target.oid, source.oid)
    );
    for tag in ["main", "task"] {
        assert_eq!(
            f.git(&["rev-parse", &format!("refs/tags/{tag}")]),
            f.initial
        );
    }
    assert_eq!(f.git(&["rev-parse", "refs/heads/task"]), source.oid);
}

#[tokio::test]
async fn stale_target_cannot_overwrite_another_writer() {
    let f = Fixture::new();
    let target = branch_snapshot(&f.bare, "main").await.unwrap();
    let source = branch_snapshot(&f.bare, "task").await.unwrap();
    let merged = f.merged(&source, &target);
    let advanced = f.advanced(&target.oid);
    f.git(&["update-ref", &target.reference, &advanced]);
    assert_eq!(
        publish_merge(&f.bare, &source, &target, &merged)
            .await
            .unwrap_err()
            .status,
        StatusCode::CONFLICT
    );
    assert_eq!(f.git(&["rev-parse", &target.reference]), advanced);
    assert_eq!(f.git(&["rev-parse", &source.reference]), source.oid);
}

#[tokio::test]
async fn stale_source_cannot_publish_an_outdated_verified_commit() {
    let f = Fixture::new();
    let target = branch_snapshot(&f.bare, "main").await.unwrap();
    let source = branch_snapshot(&f.bare, "task").await.unwrap();
    let merged = f.merged(&source, &target);
    let advanced = f.advanced(&source.oid);
    f.git(&["update-ref", &source.reference, &advanced]);
    assert_eq!(
        publish_merge(&f.bare, &source, &target, &merged)
            .await
            .unwrap_err()
            .status,
        StatusCode::CONFLICT
    );
    assert_eq!(f.git(&["rev-parse", &target.reference]), target.oid);
    assert_eq!(f.git(&["rev-parse", &source.reference]), advanced);
}

#[tokio::test]
async fn merge_requires_a_branch_not_a_tag_or_revision_expression() {
    let f = Fixture::new();
    for invalid in [
        "HEAD",
        "refs/tags/main",
        "task^",
        "main\nupdate refs/heads/task",
    ] {
        assert_eq!(
            branch_snapshot(&f.bare, invalid).await.unwrap_err().status,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        branch_snapshot(&f.bare, "tag-only")
            .await
            .unwrap_err()
            .status,
        StatusCode::CONFLICT
    );
}
