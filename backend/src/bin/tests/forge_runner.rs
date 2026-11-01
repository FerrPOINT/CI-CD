#[cfg(test)]
async fn run_shell_command(
    command: &str,
    cwd: &Path,
    timeout_seconds: i32,
    log_context: Option<&LogContext>,
    secret_env: &[(String, String)],
) -> anyhow::Result<CommandOutcome> {
    let mut processes = AttemptProcesses::new()?;
    let result = run_shell_command_in_scope(
        command,
        cwd,
        timeout_seconds,
        log_context,
        secret_env,
        &mut processes,
    )
    .await;
    processes.finish().await?;
    result
}

use super::*;
use cicd::runner_workspace::OwnedWorkspace;

fn test_cli(work_dir: PathBuf) -> Cli {
    Cli {
        api_url: "http://127.0.0.1:22801".to_string(),
        name: "test-runner".to_string(),
        credential: Some("credential".to_string()),
        registration_token: None,
        tags: vec!["linux".to_string()],
        total_slots: 1,
        work_dir: Some(work_dir),
        poll_interval_seconds: 1,
        once: true,
        no_checkout: true,
        keep_workspace: false,
        inspect_workspaces: false,
        reconcile_workspaces: false,
    }
}

fn test_offer(command: &str, timeout_seconds: i32) -> LeaseOffer {
    LeaseOffer {
        lease_id: Uuid::new_v4(),
        lease_token: "lease-token".to_string(),
        fencing_token: 1,
        attempt: AttemptSpec {
            id: Uuid::new_v4(),
            job_key: "test".to_string(),
            git_ref: "main".to_string(),
            commit_sha: None,
            commands: vec![command.to_string()],
            secrets: Vec::new(),
            artifacts: Vec::new(),
            timeout_seconds,
            workspace: WorkspaceSpec {
                checkout: false,
                checkout_url: None,
            },
        },
    }
}

#[tokio::test]
async fn unconfirmed_execution_never_submits_terminal_completion() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let requests = Arc::new(AtomicUsize::new(0));
    let count = requests.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = axum::Router::new().fallback(move || {
        count.fetch_add(1, Ordering::SeqCst);
        async { axum::http::StatusCode::OK }
    });
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let root = std::env::temp_dir().join(format!("forge-unconfirmed-{}", Uuid::new_v4()));
    let offer = test_offer("exit 0", 5);
    let workspace =
        OwnedWorkspace::create(&root, offer.attempt.id, offer.lease_id, offer.fencing_token)
            .unwrap();
    drop(workspace);
    let cli = test_cli(root.clone());
    let error = complete_attempt_result(
        &reqwest::Client::new(),
        &base,
        "fixture",
        &cli,
        &offer,
        Err(anyhow::Error::new(UnconfirmedExecution).context("child wait failed")),
    )
    .await
    .unwrap_err();
    assert!(error.is::<UnconfirmedExecution>());
    assert_eq!(requests.load(Ordering::SeqCst), 0);
    let inventory = OwnedWorkspace::inventory(&root).unwrap();
    assert_eq!(inventory.len(), 1);
    assert!(inventory[0].terminal_status.is_none());
    assert!(!inventory[0].acknowledged);
    assert!(ensure_no_unresolved_workspaces(&root).is_err());
    server.abort();
    let _ = server.await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn completion_ack_controls_workspace_cleanup() {
    for (body, acknowledged) in [
        (
            json!({"protocolVersion": 1, "accepted": true, "terminalStatus": "success"}),
            true,
        ),
        (
            json!({"protocolVersion": 1, "accepted": false, "terminalStatus": "success"}),
            false,
        ),
        (
            json!({"protocolVersion": 1, "accepted": true, "terminalStatus": "failed"}),
            false,
        ),
        (
            json!({"protocolVersion": 2, "accepted": true, "terminalStatus": "success"}),
            false,
        ),
        (json!({}), false),
    ] {
        let work_dir = std::env::temp_dir().join(format!("forge-ack-test-{}", Uuid::new_v4()));
        let cli = test_cli(work_dir.clone());
        let offer = test_offer("exit 0", 5);
        let executed = execute_attempt(&cli, &offer, None, &[]).await.unwrap();
        let checkout = executed.1.checkout().to_path_buf();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new().route(
            "/{*path}",
            axum::routing::post(move || async move { axum::Json(body) }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let completion = complete_executed_attempt(
            &reqwest::Client::new(),
            &base,
            "test-credential",
            &offer,
            executed,
            false,
        )
        .await;
        assert_eq!(completion.is_ok(), acknowledged);
        assert_eq!(checkout.exists(), !acknowledged);
        server.abort();
        let _ = server.await;
        std::fs::remove_dir_all(work_dir).unwrap();
    }
}

#[tokio::test]
async fn pinned_attempt_cannot_skip_checkout() {
    let work_dir = std::env::temp_dir().join(format!("forge-pin-test-{}", Uuid::new_v4()));
    let cli = test_cli(work_dir.clone());
    let mut offer = test_offer("exit 0", 5);
    offer.attempt.commit_sha = Some("a".repeat(40));
    assert!(execute_attempt(&cli, &offer, None, &[]).await.is_err());
    std::fs::remove_dir_all(work_dir).unwrap();
}

#[tokio::test]
async fn lost_completion_response_retains_workspace() {
    use tokio::io::AsyncReadExt;
    let work_dir = std::env::temp_dir().join(format!("forge-lost-ack-test-{}", Uuid::new_v4()));
    let cli = test_cli(work_dir.clone());
    let offer = test_offer("exit 0", 5);
    let executed = execute_attempt(&cli, &offer, None, &[]).await.unwrap();
    let checkout = executed.1.checkout().to_path_buf();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buffer = [0_u8; 4096];
        assert!(socket.read(&mut buffer).await.unwrap() > 0);
        // Request may have been accepted; connection loss is not a terminal ACK.
    });
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    assert!(
        complete_executed_attempt(&client, &base, "test-credential", &offer, executed, false)
            .await
            .is_err()
    );
    server.await.unwrap();
    assert!(checkout.exists());
    let inventory = cicd::runner_workspace::OwnedWorkspace::inventory(&work_dir).unwrap();
    assert_eq!(inventory[0].terminal_status.as_deref(), Some("success"));
    assert!(!inventory[0].acknowledged);
    assert!(ensure_no_unresolved_workspaces(&work_dir).is_err());
    std::fs::remove_dir_all(work_dir).unwrap();
}

fn receipt_body(offer: &LeaseOffer) -> serde_json::Value {
    json!({
        "protocolVersion": 1, "leaseId": offer.lease_id, "attemptId": offer.attempt.id,
        "fencingToken": offer.fencing_token, "leaseStatus": "completed",
        "terminalStatus": "success", "completedAt": Utc::now(), "terminalAcknowledged": true
    })
}

#[tokio::test]
async fn unknown_completion_uses_readback_without_resending_post() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let root = std::env::temp_dir().join(format!("forge-readback-{}", Uuid::new_v4()));
    let cli = test_cli(root.clone());
    let offer = test_offer("exit 0", 5);
    let body = receipt_body(&offer);
    let posts = Arc::new(AtomicUsize::new(0));
    let count = posts.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = axum::Router::new().route(
        "/{*path}",
        axum::routing::post(move || {
            count.fetch_add(1, Ordering::SeqCst);
            async { axum::http::StatusCode::SERVICE_UNAVAILABLE }
        })
        .get(move || async { axum::Json(body) }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let executed = execute_attempt(&cli, &offer, None, &[]).await.unwrap();
    complete_executed_attempt(
        &reqwest::Client::new(),
        &base,
        "fixture",
        &offer,
        executed,
        true,
    )
    .await
    .unwrap();
    assert_eq!(posts.load(Ordering::SeqCst), 1);
    assert!(cicd::runner_workspace::OwnedWorkspace::inventory(&root).unwrap()[0].acknowledged);
    ensure_no_unresolved_workspaces(&root).unwrap();
    reconcile_workspaces(&reqwest::Client::new(), &base, "fixture", &root, false)
        .await
        .unwrap();
    assert!(
        cicd::runner_workspace::OwnedWorkspace::inventory(&root)
            .unwrap()
            .is_empty()
    );
    assert_eq!(posts.load(Ordering::SeqCst), 1);
    server.abort();
    let _ = server.await;
    std::fs::remove_dir(root).unwrap();
}

#[tokio::test]
async fn restart_reconciliation_requires_exact_owner_receipt() {
    let root = std::env::temp_dir().join(format!("forge-reconcile-{}", Uuid::new_v4()));
    let cli = test_cli(root.clone());
    let offer = test_offer("exit 0", 5);
    let (_, workspace) = execute_attempt(&cli, &offer, None, &[]).await.unwrap();
    workspace.record_completion("success").unwrap();
    let path = workspace.checkout().to_path_buf();
    drop(workspace);
    for (field, value) in [
        ("protocolVersion", json!(2)),
        ("leaseId", json!(Uuid::new_v4())),
        ("attemptId", json!(Uuid::new_v4())),
        ("fencingToken", json!(2)),
        ("leaseStatus", json!("expired")),
        ("terminalStatus", json!("failed")),
        ("terminalAcknowledged", json!(false)),
        ("completedAt", json!(null)),
    ] {
        let mut body = receipt_body(&offer);
        body[field] = value;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new().route(
            "/{*path}",
            axum::routing::get(move || async { axum::Json(body) }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        assert!(
            reconcile_workspaces(&reqwest::Client::new(), &base, "fixture", &root, false)
                .await
                .is_err()
        );
        assert!(path.exists());
        assert!(
            !cicd::runner_workspace::OwnedWorkspace::inventory(&root).unwrap()[0].acknowledged
        );
        server.abort();
        let _ = server.await;
    }
    let body = receipt_body(&offer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = axum::Router::new().route(
        "/{*path}",
        axum::routing::get(move || async { axum::Json(body) }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    reconcile_workspaces(&reqwest::Client::new(), &base, "fixture", &root, false)
        .await
        .unwrap();
    assert!(!path.exists());
    server.abort();
    let _ = server.await;
    std::fs::remove_dir(root).unwrap();
}

#[tokio::test]
async fn unfinished_execution_is_never_reconciled_by_expiry_or_remote_success() {
    let root = std::env::temp_dir().join(format!("forge-unfinished-{}", Uuid::new_v4()));
    let offer = test_offer("exit 0", 5);
    let workspace = prepare_workspace(&test_cli(root.clone()), &offer)
        .await
        .unwrap();
    assert!(ensure_no_unresolved_workspaces(&root).is_err());
    assert!(
        reconcile_workspaces(
            &reqwest::Client::new(),
            "http://127.0.0.1:1",
            "fixture",
            &root,
            false
        )
        .await
        .is_err()
    );
    assert!(workspace.checkout().exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn api_base_accepts_root_or_v1_url() {
    assert_eq!(
        normalize_api_base("http://127.0.0.1:22801"),
        "http://127.0.0.1:22801"
    );
    assert_eq!(
        normalize_api_base("http://127.0.0.1:22801/api/v1/"),
        "http://127.0.0.1:22801"
    );
}

#[test]
fn diagnostic_is_bounded() {
    let input = "x".repeat(5000);
    assert_eq!(truncate_diagnostic(&input).len(), 4096);
}

#[test]
fn protocol_log_message_escapes_internal_line_breaks() {
    assert_eq!(protocol_log_message("one\rtwo\nthree"), "one\\rtwo\\nthree");
}

#[test]
fn runner_long_poll_wait_is_capped_and_empty_sleep_preserves_interval() {
    let mut cli = test_cli(std::env::temp_dir());
    cli.poll_interval_seconds = 5;
    assert_eq!(runner_poll_wait_seconds(&cli), 5);
    assert_eq!(
        empty_poll_sleep_duration(&cli, Duration::from_secs(2)),
        Some(Duration::from_secs(3))
    );
    assert_eq!(
        empty_poll_sleep_duration(&cli, Duration::from_secs(5)),
        None
    );

    cli.poll_interval_seconds = 300;
    assert_eq!(runner_poll_wait_seconds(&cli), 30);
    assert_eq!(
        empty_poll_sleep_duration(&cli, Duration::from_secs(30)),
        Some(Duration::from_secs(270))
    );
}

#[test]
fn active_heartbeat_busy_slots_are_capped_by_runner_capacity() {
    let leases = vec![Uuid::new_v4(), Uuid::new_v4()];

    assert_eq!(active_busy_slots(4, &leases), 2);
    assert_eq!(active_busy_slots(1, &leases), 1);
    assert_eq!(active_busy_slots(0, &leases), 0);
}

#[test]
fn masks_secret_values_before_log_upload() {
    assert_eq!(
        mask_secrets(
            "deploy token=super-secret",
            &["super-secret".to_string(), "".to_string()]
        ),
        "deploy token=***"
    );
}

#[test]
fn declared_artifact_name_preserves_relative_context() {
    assert_eq!(
        cicd::domain::artifact_names::artifact_name_from_declared_path(
            "target/release/app.tar.gz"
        ),
        "target__release__app.tar.gz"
    );
}

#[tokio::test]
async fn declared_artifact_path_cannot_escape_workspace() {
    let work_dir =
        std::env::temp_dir().join(format!("forge-runner-test-{}", Uuid::new_v4().simple()));
    let workspace = work_dir.join("workspace");
    std::fs::create_dir_all(&workspace).expect("create workspace");
    std::fs::write(workspace.join("report.txt"), b"ok").expect("write artifact");

    let workspace = tokio::fs::canonicalize(&workspace)
        .await
        .expect("canonical workspace");
    assert!(
        resolve_workspace_artifact(&workspace, "report.txt")
            .await
            .is_ok()
    );
    assert!(
        resolve_workspace_artifact(&workspace, "../outside.txt")
            .await
            .is_err()
    );

    let _ = std::fs::remove_dir_all(work_dir);
}

#[tokio::test]
async fn failed_command_reports_process_exit_code() {
    let work_dir =
        std::env::temp_dir().join(format!("forge-runner-test-{}", Uuid::new_v4().simple()));
    let cli = test_cli(work_dir.clone());
    let offer = test_offer("exit 7", 5);

    let (result, workspace) = execute_attempt(&cli, &offer, None, &[]).await.unwrap();

    assert_eq!(result.outcome, "failed");
    assert_eq!(result.exit_code, Some(7));
    assert!(
        workspace.checkout().exists(),
        "no owner acknowledgement yet"
    );
    workspace.record_completion(result.outcome).unwrap();
    workspace.acknowledge_completion(result.outcome).unwrap();
    workspace.cleanup_after_ack().await.unwrap();
    let _ = std::fs::remove_dir_all(work_dir);
}

#[tokio::test]
async fn secret_env_is_available_to_commands() {
    let work_dir =
        std::env::temp_dir().join(format!("forge-runner-test-{}", Uuid::new_v4().simple()));
    let cli = test_cli(work_dir.clone());
    let command = if cfg!(windows) {
        "if \"%DEPLOY_TOKEN%\"==\"super-secret\" (exit /b 0) else (exit /b 9)"
    } else {
        "test \"$DEPLOY_TOKEN\" = \"super-secret\""
    };
    let mut offer = test_offer(command, 5);
    offer.attempt.secrets = vec!["DEPLOY_TOKEN".to_string()];

    let (result, workspace) = execute_attempt(
        &cli,
        &offer,
        None,
        &[("DEPLOY_TOKEN".to_string(), "super-secret".to_string())],
    )
    .await
    .unwrap();

    assert_eq!(result.outcome, "success");
    assert!(
        workspace.checkout().exists(),
        "no owner acknowledgement yet"
    );
    workspace.record_completion(result.outcome).unwrap();
    workspace.acknowledge_completion(result.outcome).unwrap();
    workspace.cleanup_after_ack().await.unwrap();
    let _ = std::fs::remove_dir_all(work_dir);
}

#[tokio::test]
async fn timeout_failure_has_no_success_exit_code() {
    let work_dir =
        std::env::temp_dir().join(format!("forge-runner-test-{}", Uuid::new_v4().simple()));
    let cli = test_cli(work_dir.clone());
    let command = if cfg!(windows) {
        "ping -n 3 127.0.0.1 > nul"
    } else {
        "sleep 2"
    };
    let offer = test_offer(command, 1);

    let (result, workspace) = execute_attempt(&cli, &offer, None, &[]).await.unwrap();

    assert_eq!(result.outcome, "failed");
    assert_eq!(result.exit_code, None);
    assert!(
        workspace.checkout().exists(),
        "no owner acknowledgement yet"
    );
    workspace.record_completion(result.outcome).unwrap();
    workspace.acknowledge_completion(result.outcome).unwrap();
    workspace.cleanup_after_ack().await.unwrap();
    assert!(
        result
            .diagnostic
            .as_deref()
            .is_some_and(|value| value.contains("timed out"))
    );
    let _ = std::fs::remove_dir_all(work_dir);
}
#[cfg(target_os = "linux")]
async fn descendant_termination_case(mode: &str) {
    let root = std::env::temp_dir().join(format!("forge-process-tree-{}", Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = axum::Router::new().route(
        "/{*path}",
        axum::routing::get(|| async { axum::Json(json!({"cancelRequested": true})) })
            .post(|| async { axum::http::StatusCode::OK }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let context = LogContext {
        client: reqwest::Client::new(),
        base,
        credential: "test-credential".into(),
        lease_id: Uuid::new_v4(),
        lease_token: "test-lease".into(),
        fencing_token: 1,
        attempt_id: Uuid::new_v4(),
        masks: vec![],
    };
    let command = if mode == "normal" {
        "sleep 20 & echo $! > child.pid; printf 'normal-output\\n'"
    } else {
        "sleep 20 & echo $! > child.pid; wait"
    };
    let timeout = if mode == "timeout" { 1 } else { 30 };
    let started = tokio::time::Instant::now();
    let result = run_shell_command(command, &root, timeout, Some(&context), &[]).await;
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "descendant held the log pipe"
    );
    match mode {
        "timeout" => assert!(result.unwrap_err().to_string().contains("timed out")),
        "normal" => {
            assert!(matches!(result.unwrap(), CommandOutcome::Exited(s) if s.success()))
        }
        _ => assert!(matches!(result.unwrap(), CommandOutcome::Canceled)),
    }
    let pid: i32 = std::fs::read_to_string(root.join("child.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(
        unsafe { libc::kill(pid, 0) },
        -1,
        "descendant remains alive or unreaped"
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
    server.abort();
    let _ = server.await;
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn timeout_terminates_descendants_with_open_log_pipes() {
    descendant_termination_case("timeout").await;
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn cancellation_terminates_descendants_before_terminal_completion() {
    descendant_termination_case("cancel").await;
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn normal_exit_reaps_leftover_children_and_preserves_success() {
    descendant_termination_case("normal").await;
}
#[cfg(target_os = "linux")]
#[tokio::test]
async fn background_service_survives_between_commands_and_stops_at_attempt_end() {
    let root = std::env::temp_dir().join(format!("forge-background-{}", Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let mut processes = AttemptProcesses::new().unwrap();
    for command in [
        "sleep 20 > background.log 2>&1 & echo $! > child.pid",
        "kill -0 $(cat child.pid); printf 'next-step-ok\n'",
    ] {
        let result = run_shell_command_in_scope(command, &root, 10, None, &[], &mut processes)
            .await
            .unwrap();
        assert!(matches!(result, CommandOutcome::Exited(s) if s.success()));
    }
    let pid: i32 = std::fs::read_to_string(root.join("child.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(unsafe { libc::kill(pid, 0) }, 0);
    processes.finish().await.unwrap();
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
    std::fs::remove_dir_all(root).unwrap();
}
