//! Real local Git only; isolated per-command config, never process-global env mutation.
use super::*;

async fn init(root: &Path) -> PathBuf {
    let checkout = root.join("checkout");
    assert!(
        git()
            .args(["init", "--object-format=sha1", "--template="])
            .arg(&checkout)
            .output()
            .await
            .unwrap()
            .status
            .success()
    );
    checkout
}

fn isolated_config(global: &Path) -> Command {
    let mut command = author_git();
    command
        .env("GIT_CONFIG_GLOBAL", global)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_CONFIG")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env("GIT_CONFIG_COUNT", "0");
    command
}

async fn config(checkout: &Path, args: &[&str]) -> std::process::Output {
    git()
        .arg("-C")
        .arg(checkout)
        .arg("config")
        .args(args)
        .output()
        .await
        .unwrap()
}

#[tokio::test]
async fn fresh_checkout_without_global_identity_can_make_a_real_local_commit() {
    let temporary = tempfile::tempdir().unwrap();
    let checkout = init(temporary.path()).await;
    let global = temporary.path().join("global-config");
    fs::write(&global, b"").unwrap();
    default_author(&checkout, || isolated_config(&global))
        .await
        .unwrap();
    fs::write(checkout.join("note.txt"), b"local fixture\n").unwrap();
    assert!(
        git()
            .arg("-C")
            .arg(&checkout)
            .args(["add", "--", "note.txt"])
            .output()
            .await
            .unwrap()
            .status
            .success()
    );
    assert!(
        git()
            .arg("-C")
            .arg(&checkout)
            .args([
                "-c",
                "core.hooksPath=",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                "Local author fixture",
            ])
            .output()
            .await
            .unwrap()
            .status
            .success()
    );
    let author = git()
        .arg("-C")
        .arg(&checkout)
        .args(["log", "-1", "--format=%an|%ae|%cn|%ce"])
        .output()
        .await
        .unwrap();
    assert!(author.status.success());
    assert_eq!(
        String::from_utf8(author.stdout).unwrap().trim(),
        "Fleet Control Developer|developer@fleet-control.local|Fleet Control Developer|developer@fleet-control.local"
    );
    assert_eq!(fs::read(&global).unwrap(), b"");
}

#[tokio::test]
async fn configured_global_identity_is_not_shadowed_by_local_defaults() {
    let temporary = tempfile::tempdir().unwrap();
    let checkout = init(temporary.path()).await;
    let global = temporary.path().join("global-config");
    let contents = b"[user]\nname = Configured Developer\nemail = configured@example.test\n";
    fs::write(&global, contents).unwrap();
    let before = fs::read(checkout.join(".git/config")).unwrap();
    default_author(&checkout, || isolated_config(&global))
        .await
        .unwrap();
    assert_eq!(fs::read(checkout.join(".git/config")).unwrap(), before);
    assert_eq!(fs::read(&global).unwrap(), contents);
    for key in ["user.name", "user.email"] {
        assert_eq!(
            config(&checkout, &["--local", "--get", key])
                .await
                .status
                .code(),
            Some(1)
        );
    }
}

#[tokio::test]
async fn explicit_local_identity_is_preserved_and_only_the_absent_field_is_filled() {
    for (key, existing, other, fallback) in [
        (
            "user.name",
            "Explicit Developer",
            "user.email",
            "developer@fleet-control.local",
        ),
        (
            "user.email",
            "explicit@example.test",
            "user.name",
            "Fleet Control Developer",
        ),
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let checkout = init(temporary.path()).await;
        let global = temporary.path().join("global-config");
        fs::write(&global, b"").unwrap();
        assert!(
            config(&checkout, &["--local", key, existing])
                .await
                .status
                .success()
        );
        default_author(&checkout, || isolated_config(&global))
            .await
            .unwrap();
        let read = config(&checkout, &["--local", "--get", key]).await;
        assert!(read.status.success());
        assert_eq!(String::from_utf8(read.stdout).unwrap().trim(), existing);
        let read = config(&checkout, &["--local", "--get", other]).await;
        assert!(read.status.success());
        assert_eq!(String::from_utf8(read.stdout).unwrap().trim(), fallback);
    }
}

#[tokio::test]
async fn explicit_environment_git_identity_is_not_shadowed_by_local_defaults() {
    let temporary = tempfile::tempdir().unwrap();
    let checkout = init(temporary.path()).await;
    let global = temporary.path().join("global-config");
    fs::write(&global, b"").unwrap();
    let before = fs::read(checkout.join(".git/config")).unwrap();
    default_author(&checkout, || {
        let mut command = isolated_config(&global);
        command
            .env("GIT_CONFIG_COUNT", "2")
            .env("GIT_CONFIG_KEY_0", "user.name")
            .env("GIT_CONFIG_VALUE_0", "Explicit Environment Developer")
            .env("GIT_CONFIG_KEY_1", "user.email")
            .env("GIT_CONFIG_VALUE_1", "explicit-env@example.test");
        command
    })
    .await
    .unwrap();
    assert_eq!(fs::read(checkout.join(".git/config")).unwrap(), before);
}

#[tokio::test]
async fn existing_checkout_reuse_never_rewrites_config_or_adds_author_defaults() {
    let temporary = tempfile::tempdir().unwrap();
    let checkout = init(temporary.path()).await;
    assert!(
        git()
            .arg("-C")
            .arg(&checkout)
            .args([
                "-c",
                "user.name=Fixture Author",
                "-c",
                "user.email=fixture@example.test",
                "-c",
                "core.hooksPath=",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--allow-empty",
                "-m",
                "Existing checkout",
            ])
            .output()
            .await
            .unwrap()
            .status
            .success()
    );
    let base = commit(&checkout).await.unwrap();
    let source = b"{\"fixture\":\"retained-source\"}";
    private_file(&checkout.join(".git/forge-source.json"), source).unwrap();
    for explicit in [false, true] {
        if explicit {
            assert!(
                config(&checkout, &["--local", "user.name", "Rework Developer"])
                    .await
                    .status
                    .success()
            );
            assert!(
                config(&checkout, &["--local", "user.email", "rework@example.test"])
                    .await
                    .status
                    .success()
            );
        }
        let before = fs::read(checkout.join(".git/config")).unwrap();
        let reused = reuse_checkout(&checkout, &base, source).await.unwrap();
        assert_eq!(reused["state"], "checkout_reused");
        assert_eq!(fs::read(checkout.join(".git/config")).unwrap(), before);
    }
}
