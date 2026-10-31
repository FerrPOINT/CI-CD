use std::process::Command;

#[tokio::test]
async fn auto_registration_credential_is_used_without_being_written_to_process_logs() {
    use axum::{
        Json, Router,
        http::{HeaderMap, StatusCode},
        routing::post,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let credential = format!("synthetic_runner_secret_{}", uuid::Uuid::new_v4());
    let registration = format!("synthetic_registration_{}", uuid::Uuid::new_v4());
    let response = serde_json::json!({"runnerId":uuid::Uuid::new_v4(),"credential":credential});
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let authenticated_calls = Arc::new(AtomicUsize::new(0));
    let calls = authenticated_calls.clone();
    let expected = format!("Bearer {credential}");
    let authenticated = move |headers: HeaderMap| {
        let expected = expected.clone();
        let calls = calls.clone();
        async move {
            if headers
                .get("Authorization")
                .and_then(|value| value.to_str().ok())
                != Some(expected.as_str())
            {
                return StatusCode::UNAUTHORIZED;
            }
            calls.fetch_add(1, Ordering::SeqCst);
            StatusCode::NO_CONTENT
        }
    };
    let app = Router::new()
        .route(
            "/api/v1/runner/register",
            post(move || {
                let response = response.clone();
                async move { Json(response) }
            }),
        )
        .route("/api/v1/runner/heartbeat", post(authenticated.clone()))
        .route("/api/v1/runner/poll", post(authenticated));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let root = tempfile::tempdir().unwrap();
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_forge-runner"))
            .args([
                "--api-url",
                &origin,
                "--registration-token",
                &registration,
                "--once",
                "--no-checkout",
                "--work-dir",
            ])
            .arg(root.path())
            .env_remove("CICD_RUNNER_CREDENTIAL")
            .env_remove("CICD_RUNNER_MODE")
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    server.abort();
    let _ = server.await;
    assert!(
        output.status.success(),
        "runner fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let logs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(logs.contains("registered runner"));
    assert!(!logs.contains(&credential));
    assert!(!logs.contains(&registration));
    assert!(authenticated_calls.load(Ordering::SeqCst) >= 2);
}

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
