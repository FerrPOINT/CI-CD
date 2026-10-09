use std::process::Command;

#[test]
fn forge_runner_exposes_protocol_flags() {
    let output = Command::new(env!("CARGO_BIN_EXE_forge-runner"))
        .arg("--help")
        .output()
        .expect("run forge-runner --help");

    assert!(
        output.status.success(),
        "forge-runner --help failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("help output is utf-8");
    for flag in [
        "--api-url",
        "--credential",
        "--registration-token",
        "--tags",
        "--total-slots",
        "--poll-interval-seconds",
        "--work-dir",
        "--once",
        "--no-checkout",
        "--keep-workspace",
        "--inspect-workspaces",
        "--reconcile-workspaces",
    ] {
        assert!(
            stdout.contains(flag),
            "forge-runner help should document {flag}"
        );
    }
}

#[test]
fn forge_runner_inspection_is_offline_and_unresolved_startup_never_polls() {
    let root = std::env::temp_dir().join(format!("forge-inspect-binary-{}", uuid::Uuid::new_v4()));
    let workspace = cicd::runner_workspace::OwnedWorkspace::create(
        &root,
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
        1,
    )
    .unwrap();
    workspace.create_empty_checkout().unwrap();
    let inspect = Command::new(env!("CARGO_BIN_EXE_forge-runner"))
        .args([
            "--api-url",
            "http://127.0.0.1:1",
            "--inspect-workspaces",
            "--work-dir",
        ])
        .arg(&root)
        .output()
        .unwrap();
    assert!(inspect.status.success());
    let inventory: serde_json::Value = serde_json::from_slice(&inspect.stdout).unwrap();
    assert_eq!(inventory.as_array().unwrap().len(), 1);
    assert_eq!(inventory[0]["terminal_status"], serde_json::Value::Null);
    assert_eq!(inventory[0]["acknowledged"], false);
    let startup = Command::new(env!("CARGO_BIN_EXE_forge-runner"))
        .args(["--api-url", "http://127.0.0.1:1", "--once", "--work-dir"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(!startup.status.success());
    assert!(
        String::from_utf8(startup.stderr)
            .unwrap()
            .contains("unresolved workspaces require owner readback")
    );
    assert!(workspace.checkout().exists());
    std::fs::remove_dir_all(&root).unwrap();
}
