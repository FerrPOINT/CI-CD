use super::*;

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
        artifact_name_from_declared_path("target/release/app.tar.gz"),
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

    let result = execute_attempt(&cli, &offer, None, &[]).await.unwrap();

    assert_eq!(result.outcome, "failed");
    assert_eq!(result.exit_code, Some(7));
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

    let result = execute_attempt(
        &cli,
        &offer,
        None,
        &[("DEPLOY_TOKEN".to_string(), "super-secret".to_string())],
    )
    .await
    .unwrap();

    assert_eq!(result.outcome, "success");
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

    let result = execute_attempt(&cli, &offer, None, &[]).await.unwrap();

    assert_eq!(result.outcome, "failed");
    assert_eq!(result.exit_code, None);
    assert!(
        result
            .diagnostic
            .as_deref()
            .is_some_and(|value| value.contains("timed out"))
    );
    let _ = std::fs::remove_dir_all(work_dir);
}
