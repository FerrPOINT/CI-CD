    use super::*;
    use cicd::runner_workspace::OwnedWorkspace;

    #[test]
    fn only_exact_closed_owner_readback_authorizes_unsubmitted_cleanup() {
        let record = cicd::runner_workspace::RetainedWorkspace {
            workspace_id: "owned".into(),
            attempt_id: Uuid::new_v4(),
            lease_id: Uuid::new_v4(),
            generation: 7,
            terminal_status: None,
            acknowledged: false,
        };
        let mut receipt = LeaseReceipt {
            protocol_version: 1,
            lease_id: record.lease_id,
            attempt_id: record.attempt_id,
            fencing_token: record.generation,
            lease_status: "canceled".into(),
            terminal_status: Some("canceled".into()),
            completed_at: Some(Utc::now()),
            terminal_acknowledged: false,
        };
        validate_unsubmitted_owner_readback(&record, &receipt).unwrap();
        // Keep the real false flag: cancellation of unissued work is not a Pod completion ACK.
        assert!(!receipt.terminal_acknowledged);
        receipt.lease_status = "active".into();
        assert!(validate_unsubmitted_owner_readback(&record, &receipt).is_err());
        receipt.lease_status = "expired".into();
        receipt.terminal_status = Some("success".into());
        assert!(validate_unsubmitted_owner_readback(&record, &receipt).is_err());
        receipt.terminal_status = Some("failed".into());
        receipt.fencing_token += 1;
        assert!(validate_unsubmitted_owner_readback(&record, &receipt).is_err());
        receipt.fencing_token = record.generation;
        receipt.attempt_id = Uuid::new_v4();
        assert!(validate_unsubmitted_owner_readback(&record, &receipt).is_err());
    }

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
                image_publish: None,
                helm_deploy: None,
                id: Uuid::new_v4(),
                job_id: Uuid::new_v4(),
                executor: "shell".into(),
                image: "test".into(),
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
    async fn failed_kubernetes_outputs_skip_only_missing_and_upload_existing() {
        use std::sync::{Arc, Mutex};
        let uploads = Arc::new(Mutex::new(Vec::new()));
        let observed = uploads.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new().fallback(move |request: axum::extract::Request| {
            let observed = observed.clone();
            async move {
                if request.uri().path().ends_with("/artifacts") {
                    let name = request.headers()["X-Artifact-Path"]
                        .to_str()
                        .unwrap()
                        .to_owned();
                    let bytes = axum::body::to_bytes(request.into_body(), 1024)
                        .await
                        .unwrap();
                    observed.lock().unwrap().push((name, bytes.to_vec()));
                }
                axum::http::StatusCode::OK
            }
        });
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let root = std::env::temp_dir().join(format!("forge-failed-output-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("output")).unwrap();
        std::fs::write(root.join("output/present.txt"), b"actual output").unwrap();
        let mut offer = test_offer("git missing", 10);
        offer.attempt.artifacts = vec!["output/missing.txt".into(), "output/present.txt".into()];
        let context = LogContext {
            client: reqwest::Client::new(),
            base,
            credential: "fixture".into(),
            lease_id: offer.lease_id,
            lease_token: offer.lease_token.clone(),
            fencing_token: offer.fencing_token,
            attempt_id: offer.attempt.id,
            masks: vec![],
        };
        upload_terminal_artifacts(&context, &offer, &root, true)
            .await
            .unwrap();
        assert_eq!(
            *uploads.lock().unwrap(),
            vec![("output/present.txt".into(), b"actual output".to_vec())]
        );
        uploads.lock().unwrap().clear();
        // Existing success/shell contract still rejects the missing required output.
        assert!(
            upload_declared_artifacts(&context, &offer, &root)
                .await
                .is_err()
        );
        assert!(uploads.lock().unwrap().is_empty());
        server.abort();
        let _ = server.await;
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn failed_artifact_missing_policy_keeps_symlink_and_path_safety() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!("forge-failed-path-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("workspace")).unwrap();
        std::fs::create_dir_all(root.join("outside")).unwrap();
        let workspace = std::fs::canonicalize(root.join("workspace")).unwrap();
        symlink(root.join("outside"), workspace.join("escape")).unwrap();
        symlink(workspace.join("not-created"), workspace.join("dangling")).unwrap();
        assert!(
            terminal_artifact_path(&workspace, "output/missing", true)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            terminal_artifact_path(&workspace, "escape/missing", true)
                .await
                .is_err()
        );
        assert!(
            terminal_artifact_path(&workspace, "dangling/missing", true)
                .await
                .is_err()
        );
        assert!(
            terminal_artifact_path(&workspace, "../outside/missing", true)
                .await
                .is_err()
        );
        assert!(
            terminal_artifact_path(&workspace, "/outside/missing", true)
                .await
                .is_err()
        );
        assert!(
            terminal_artifact_path(&workspace, "output/missing", false)
                .await
                .is_err()
        );
        std::fs::remove_dir_all(root).unwrap();
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
                .post(|axum::Json(body):axum::Json<serde_json::Value>| async move {
                    axum::Json(json!({"protocolVersion":1,"accepted":body["lines"].as_array().unwrap().len(),"nextAfter":0}))
                }),
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
}

async fn execute_kubernetes(
    cli: &Cli,
    offer: &LeaseOffer,
    log: Option<LogContext>,
    secrets: &[(String, String)],
    workspace: cicd::runner_workspace::OwnedWorkspace,
) -> anyhow::Result<(ExecutionResult, cicd::runner_workspace::OwnedWorkspace)> {
    use cicd::execution::kubernetes::{Config, Execution, Scope};
    if offer.attempt.executor != "kubernetes" {
        bail!("Kubernetes runner refuses shell executor");
    }
    if workspace_root(cli) != Path::new("/workspaces") {
        bail!("Kubernetes controller requires the /workspaces claim mount");
    }
    let source = offer
        .attempt
        .commit_sha
        .clone()
        .context("Kubernetes execution requires exact source pin")?;
    let config = Config::from_env()?;
    let scope = Scope {
        job_id: offer.attempt.job_id,
        attempt_id: offer.attempt.id,
        lease_id: offer.lease_id,
        generation: offer.fencing_token,
        source_commit: source,
    };
    let (image, command) = if let Some(deploy) = &offer.attempt.helm_deploy {
        if offer.attempt.image_publish.is_some()
            || offer.attempt.image != "forge:runner-helm-deploy"
            || offer.attempt.commands != ["forge:helm-deploy"]
            || !secrets.is_empty()
        {
            bail!("Typed Helm deployment contains arbitrary execution");
        }
        let grant = config
            .helm_targets
            .iter()
            .find(|grant| grant.environment_id == deploy.environment_id)
            .context("Environment has no operator deployment grant")?;
        (
            grant.client_image.clone(),
            cicd::execution::helm::command(&scope, deploy, grant)?,
        )
    } else if let Some(spec) = &offer.attempt.image_publish {
        if offer.attempt.image != "forge:runner-image-publish"
            || offer.attempt.commands != ["forge:image-publish"]
            || !secrets.is_empty()
        {
            bail!("Typed image publication contains arbitrary execution");
        }
        let host = config
            .host_build
            .as_ref()
            .context("Host image publication disabled")?;
        (
            host.client_image.clone(),
            cicd::execution::publication::command(&scope, spec)?,
        )
    } else {
        (
            cicd::execution::kubernetes::resolve_image(&offer.attempt.image).await?,
            kubernetes_command_vector(&offer.attempt.commands),
        )
    };
    let mut execution = Execution::prepare(
        config,
        &workspace_root(cli),
        scope,
        workspace.checkout(),
        &image,
        &command,
        secrets,
        offer.attempt.timeout_seconds as u64,
    )?;
    if let Some(spec) = &offer.attempt.image_publish {
        execution.host_build_template(spec)?;
    }
    if let Some(deploy) = &offer.attempt.helm_deploy {
        execution.helm_template(deploy)?;
    }
    execution.save_external_recovery(
        &offer.lease_token,
        &offer.attempt.artifacts,
        &offer.attempt.secrets,
    )?;
    if let Err(error) = execution.start().await {
        if execution.close_not_submitted().await.unwrap_or(false) {
            return Ok((
                ExecutionResult {
                    outcome: "failed",
                    exit_code: None,
                    diagnostic: Some(truncate_diagnostic(&format!(
                        "Kubernetes request not submitted: {error:#}"
                    ))),
                },
                workspace,
            ));
        }
        return Err(UnconfirmedExecution.into());
    }
    let deadline =
        std::time::Instant::now() + Duration::from_secs(offer.attempt.timeout_seconds as u64);
    let mut timed_out = false;
    let terminal = loop {
        if let Some(terminal) = execution
            .observe()
            .await
            .map_err(|_| UnconfirmedExecution)?
        {
            break terminal;
        }
        let cancel = match &log {
            Some(context) => poll_lease_cancel_requested(context)
                .await
                .map_err(|_| UnconfirmedExecution)?,
            None => false,
        };
        timed_out |= std::time::Instant::now() >= deadline;
        if cancel || timed_out {
            cicd::execution::publication::cancel(&execution)
                .await
                .map_err(|_| UnconfirmedExecution)?;
            if let Some(context) = &log {
                execution
                    .cancel_with_logs(|line| {
                        let context = context.clone();
                        async move {
                            append_log_lines(
                                &context,
                                vec![RunnerLogLine {
                                    stream: "stdout",
                                    message: protocol_log_message(&mask_secrets(
                                        &line,
                                        &context.masks,
                                    )),
                                }],
                            )
                            .await
                        }
                    })
                    .await
                    .map_err(|_| UnconfirmedExecution)?;
            } else {
                execution.cancel().await.map_err(|_| UnconfirmedExecution)?;
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    };
    cicd::execution::publication::finalize(&execution, workspace.checkout(), terminal.exit_code)
        .await
        .map_err(|_| UnconfirmedExecution)?;
    cicd::execution::helm::finalize(&execution, workspace.checkout(), terminal.exit_code)
        .await
        .map_err(|_| UnconfirmedExecution)?;
    if let Some(context) = &log {
        if !execution.cancelled() {
            forward_kubernetes_logs(&execution, context)
                .await
                .map_err(|error| {
                    anyhow::Error::new(UnconfirmedExecution)
                        .context(format!("terminal Pod log forwarding incomplete: {error:#}"))
                })?;
            upload_terminal_artifacts(
                context,
                offer,
                workspace.checkout(),
                terminal.exit_code != 0,
            )
            .await
            .map_err(|_| UnconfirmedExecution)?;
        }
    }
    if execution.cancelled()
        && offer.attempt.helm_deploy.as_ref().is_some_and(|deploy| {
            workspace
                .checkout()
                .join(&deploy.receipt_artifact)
                .is_file()
        })
    {
        if let Some(context) = &log {
            upload_declared_artifacts(context, offer, workspace.checkout())
                .await
                .map_err(|_| UnconfirmedExecution)?;
        }
    }
    let result = ExecutionResult {
        outcome: if timed_out {
            "failed"
        } else if execution.cancelled() {
            "canceled"
        } else if terminal.exit_code == 0 {
            "success"
        } else {
            "failed"
        },
        exit_code: Some(terminal.exit_code),
        diagnostic: None,
    };
    Ok((result, workspace))
}

fn kubernetes_command_vector(commands: &[String]) -> Vec<String> {
    let mut args = vec![
        "sh".into(),
        "-c".into(),
        "for command do sh -lc \"$command\" || exit $?; done".into(),
        "forge-job".into(),
    ];
    args.extend(commands.iter().cloned());
    args
}

async fn unsubmitted_owner_readback(
    client: &reqwest::Client,
    base: &str,
    credential: &str,
    record: &cicd::runner_workspace::RetainedWorkspace,
) -> anyhow::Result<LeaseReceipt> {
    let response = client
        .get(format!(
            "{base}/api/v1/runner/leases/{}/receipt",
            record.lease_id
        ))
        .bearer_auth(credential)
        .send()
        .await?;
    let value: LeaseReceipt = response_json(response).await?;
    validate_unsubmitted_owner_readback(record, &value)?;
    Ok(value)
}
fn validate_unsubmitted_owner_readback(
    record: &cicd::runner_workspace::RetainedWorkspace,
    value: &LeaseReceipt,
) -> anyhow::Result<()> {
    ensure!(
        value.protocol_version == PROTOCOL_VERSION
            && value.lease_id == record.lease_id
            && value.attempt_id == record.attempt_id
            && value.fencing_token == record.generation
            && value.completed_at.is_some()
            && matches!(
                value.lease_status.as_str(),
                "expired" | "completed" | "canceled"
            )
            && matches!(
                (
                    value.lease_status.as_str(),
                    value.terminal_status.as_deref()
                ),
                ("expired", Some("failed" | "canceled"))
                    | ("completed", Some("failed"))
                    | ("canceled", Some("canceled"))
            ),
        "known-unsubmitted work lacks its exact closed owner receipt"
    );
    Ok(())
}
async fn finish_unsubmitted_workspace(
    workspace: cicd::runner_workspace::OwnedWorkspace,
    directory: &Path,
    receipt: &LeaseReceipt,
    keep: bool,
) -> anyhow::Result<()> {
    let outcome = receipt
        .terminal_status
        .as_deref()
        .context("closed outcome missing")?;
    // This records actual server state including terminalAcknowledged:false.
    // It is owner readback for NO execution, not a fabricated completion/Pod ACK.
    cicd::execution::kubernetes::write_not_submitted_readback(
        directory,
        &serde_json::to_vec(receipt)?,
    )?;
    workspace
        .record_completion(outcome)
        .or_else(|_| workspace.record_recovered_termination(outcome))?;
    workspace.acknowledge_completion(outcome)?;
    if !keep {
        workspace.cleanup_after_ack().await?;
    }
    Ok(())
}
async fn recover_kubernetes(
    client: &reqwest::Client,
    base: &str,
    credential: &str,
    root: &Path,
    record: &cicd::runner_workspace::RetainedWorkspace,
    keep_workspace: bool,
) -> anyhow::Result<()> {
    use cicd::execution::kubernetes::Execution;
    let workspace = cicd::runner_workspace::OwnedWorkspace::reopen(root, &record.workspace_id)?;
    workspace.verify_identity(record.attempt_id, record.lease_id, record.generation)?;
    if let Some(directory) = cicd::execution::kubernetes::close_unprepared(
        &cicd::execution::kubernetes::Config::from_env()?,
        root,
        record.attempt_id,
        record.lease_id,
        record.generation,
        workspace.checkout(),
    )
    .await?
    {
        let receipt = unsubmitted_owner_readback(client, base, credential, record).await?;
        return finish_unsubmitted_workspace(workspace, &directory, &receipt, keep_workspace).await;
    }
    let execution = Execution::reopen(root, record.attempt_id, record.generation)?;
    let scope = execution.scope();
    if scope.lease_id != record.lease_id {
        bail!("retained lease identity mismatch")
    }
    if execution.close_not_submitted().await? {
        if let Ok(receipt) = unsubmitted_owner_readback(client, base, credential, record).await {
            let directory = root.join(".kubernetes-control").join(format!(
                "forge-{}-g{}",
                record.attempt_id.simple(),
                record.generation
            ));
            return finish_unsubmitted_workspace(workspace, &directory, &receipt, keep_workspace)
                .await;
        }
    }
    let (token, artifacts, secrets) = execution.external_recovery()?;
    let offer = LeaseOffer {
        lease_id: record.lease_id,
        lease_token: token,
        fencing_token: record.generation,
        attempt: AttemptSpec {
            image_publish: None,
            helm_deploy: None,
            id: record.attempt_id,
            job_id: scope.job_id,
            job_key: "recovery".into(),
            executor: "kubernetes".into(),
            image: execution.image().into(),
            git_ref: String::new(),
            commit_sha: Some(scope.source_commit.clone()),
            commands: Vec::new(),
            artifacts,
            secrets,
            timeout_seconds: 86400,
            workspace: WorkspaceSpec {
                checkout: false,
                checkout_url: None,
            },
        },
    };
    let workspace = cicd::runner_workspace::OwnedWorkspace::reopen(root, &record.workspace_id)?;
    workspace.verify_identity(record.attempt_id, record.lease_id, record.generation)?;
    if execution.close_not_submitted().await? {
        let outcome = match record.terminal_status.as_deref() {
            None | Some("failed") => "failed",
            Some("canceled") => "canceled",
            _ => anyhow::bail!("unsubmitted work cannot claim success"),
        };
        return complete_executed_attempt(
            client,
            base,
            credential,
            &offer,
            (
                ExecutionResult {
                    outcome,
                    exit_code: None,
                    diagnostic: Some(
                        "Kubernetes request was never submitted; retained owner recovery".into(),
                    ),
                },
                workspace,
            ),
            keep_workspace,
        )
        .await;
    }
    let mut terminal = execution.reconcile().await?;
    let mut renew_at = std::time::Instant::now();
    let mut revoked = false;
    let mut context = LogContext {
        client: client.clone(),
        base: base.into(),
        credential: credential.into(),
        lease_id: offer.lease_id,
        lease_token: offer.lease_token.clone(),
        fencing_token: offer.fencing_token,
        attempt_id: offer.attempt.id,
        masks: execution.recovery_masks(&offer.attempt.secrets).await?,
    };
    while terminal.is_none() {
        if std::time::Instant::now() >= renew_at {
            let response = client
                .post(format!(
                    "{base}/api/v1/runner/leases/{}/renew",
                    offer.lease_id
                ))
                .bearer_auth(credential)
                .json(&LeaseMutation {
                    protocol_version: PROTOCOL_VERSION,
                    lease_token: &offer.lease_token,
                    fencing_token: offer.fencing_token,
                })
                .send()
                .await;
            revoked |= !response.is_ok_and(|r| r.status().is_success());
            renew_at = std::time::Instant::now() + Duration::from_secs(40);
        }
        let cancel = match poll_lease_cancel_requested(&context).await {
            Ok(cancel) => cancel,
            Err(_) => {
                revoked = true;
                true
            }
        };
        if cancel || revoked {
            cicd::execution::publication::cancel(&execution).await?;
            execution.cancel().await?;
        }
        terminal = execution.observe().await?;
        if terminal.is_none() {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    let terminal = terminal.context("terminal missing")?;
    let cancel_requested = match poll_lease_cancel_requested(&context).await {
        Ok(cancel) => cancel,
        Err(_) => {
            revoked = true;
            false
        }
    };
    // Read the original server projection; expiry alone is not an accepted ACK.
    let response = client
        .get(format!(
            "{base}/api/v1/runner/leases/{}/receipt",
            offer.lease_id
        ))
        .bearer_auth(credential)
        .send()
        .await?;
    let receipt: LeaseReceipt = response_json(response).await?;
    let outcome = if receipt.terminal_acknowledged {
        receipt
            .terminal_status
            .as_deref()
            .context("terminal status missing")?
    } else if matches!(
        receipt.terminal_status.as_deref(),
        Some("failed" | "canceled")
    ) {
        revoked = true;
        receipt.terminal_status.as_deref().unwrap()
    } else if cancel_requested {
        "canceled"
    } else if let Some(intended) = record.terminal_status.as_deref() {
        intended
    } else if execution.cancelled() {
        "canceled"
    } else if terminal.exit_code == 0 {
        "success"
    } else {
        "failed"
    };
    if receipt.terminal_acknowledged {
        workspace
            .record_completion(outcome)
            .or_else(|_| workspace.record_recovered_termination(outcome))?;
        workspace.acknowledge_completion(outcome)?;
        execution.acknowledge().await?;
        if !keep_workspace {
            workspace.cleanup_after_ack().await?;
        }
        execution.finish_workspace_cleanup()?;
        return Ok(());
    }
    cicd::execution::publication::finalize(&execution, workspace.checkout(), terminal.exit_code)
        .await
        .map_err(|_| UnconfirmedExecution)?;
    cicd::execution::helm::finalize(&execution, workspace.checkout(), terminal.exit_code)
        .await
        .map_err(|_| UnconfirmedExecution)?;
    if !revoked && outcome != "canceled" {
        forward_kubernetes_logs(&execution, &context).await?;
        if outcome != "canceled" {
            upload_terminal_artifacts(
                &context,
                &offer,
                workspace.checkout(),
                outcome == "failed" && terminal.exit_code != 0,
            )
            .await?;
        }
    }
    context.masks.clear();
    complete_executed_attempt(
        client,
        base,
        credential,
        &offer,
        (
            ExecutionResult {
                outcome: match outcome {
                    "success" => "success",
                    "canceled" => "canceled",
                    _ => "failed",
                },
                exit_code: Some(terminal.exit_code),
                diagnostic: Some(
                    "reconciled original Kubernetes execution; no commands replayed".into(),
                ),
            },
            workspace,
        ),
        keep_workspace,
    )
    .await
}

#[cfg(all(test, unix))]
mod kubernetes_command_tests {
    use super::*;
    #[tokio::test]
    async fn first_failed_command_stops_and_successful_commands_keep_independent_shells() {
        let args = kubernetes_command_vector(&["false".into(), "echo should-not-run".into()]);
        let result = Command::new(&args[0])
            .args(&args[1..])
            .output()
            .await
            .unwrap();
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
        let args = kubernetes_command_vector(&[
            "export LOCAL_ONLY=secret; echo one".into(),
            "echo ${LOCAL_ONLY-unset}".into(),
        ]);
        let result = Command::new(&args[0])
            .args(&args[1..])
            .output()
            .await
            .unwrap();
        assert!(result.status.success());
        assert_eq!(String::from_utf8(result.stdout).unwrap(), "one\nunset\n");
    }
}

#[cfg(test)]
mod runner_log_forwarding_tests {
    use super::*;
    use axum::{Json, response::IntoResponse};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    fn context(base: String) -> LogContext {
        LogContext {
            client: reqwest::Client::builder()
                .retry(reqwest::retry::never())
                .build()
                .unwrap(),
            base,
            credential: "fixture".into(),
            lease_id: Uuid::new_v4(),
            lease_token: "fixture-lease".into(),
            fencing_token: 1,
            attempt_id: Uuid::new_v4(),
            masks: vec!["fixture-secret".into()],
        }
    }

    #[tokio::test]
    async fn terminal_log_snapshot_batches_preserve_order_masking_and_final_tail() {
        let requests = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
        let received = requests.clone();
        let sequence = Arc::new(AtomicUsize::new(0));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new().fallback(move |Json(body): Json<serde_json::Value>| {
            let received = received.clone();
            let sequence = sequence.clone();
            async move {
                let count = body["lines"].as_array().unwrap().len();
                let next = sequence.fetch_add(count, Ordering::SeqCst) + count;
                received.lock().unwrap().push(body);
                Json(json!({"protocolVersion":1,"accepted":count,"nextAfter":next}))
            }
        });
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let context = context(base);
        let mut batch = RunnerLogBatch::default();
        // The original failure exhausted the 1200-request bucket after 1159
        // one-line appends. This larger real-sized snapshot needs only 69.
        for index in 0..6805 {
            batch
                .push(&context, format!("line-{index}:fixture-secret\r"))
                .await
                .unwrap();
        }
        assert_eq!(
            batch.lines.len(),
            5,
            "the final partial batch is not acknowledged yet"
        );
        batch.flush(&context).await.unwrap();
        assert!(batch.lines.is_empty());
        {
            let bodies = requests.lock().unwrap();
            assert_eq!(bodies.len(), 69);
            let lines: Vec<_> = bodies
                .iter()
                .flat_map(|body| body["lines"].as_array().unwrap())
                .collect();
            assert_eq!(lines.len(), 6805);
            for (index, line) in lines.iter().enumerate() {
                assert_eq!(line["message"], format!("line-{index}:***\\r"));
                assert_eq!(line["stream"], "stdout");
            }
            assert!(
                bodies
                    .iter()
                    .all(|body| body["lines"].as_array().unwrap().len() <= 100)
            );
        }
        server.abort();
        let _ = server.await;
    }

    #[tokio::test]
    async fn terminal_log_batches_bound_serialized_bytes_including_json_escaping() {
        let bodies = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
        let received = bodies.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new().fallback(move |Json(body): Json<serde_json::Value>| {
            let received = received.clone();
            async move {
                let count = body["lines"].as_array().unwrap().len();
                received.lock().unwrap().push(body);
                Json(json!({"protocolVersion":1,"accepted":count,"nextAfter":100}))
            }
        });
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let context = context(base);
        let mut batch = RunnerLogBatch::default();
        for _ in 0..12 {
            batch.push(&context, "\u{0001}".repeat(8192)).await.unwrap();
        }
        batch.flush(&context).await.unwrap();
        {
            let bodies = bodies.lock().unwrap();
            assert!(
                bodies.len() > 1,
                "byte bound must flush before reaching 100 lines"
            );
            assert_eq!(
                bodies
                    .iter()
                    .map(|b| b["lines"].as_array().unwrap().len())
                    .sum::<usize>(),
                12
            );
            assert!(
                bodies
                    .iter()
                    .all(|b| serde_json::to_vec(b).unwrap().len() < LOG_APPEND_MAX_BYTES)
            );
            assert!(
                bodies
                    .iter()
                    .all(|b| serde_json::to_vec(&b["lines"]).unwrap().len()
                        <= LOG_BATCH_MAX_BYTES + 2)
            );
        }
        server.abort();
        let _ = server.await;
    }

    #[tokio::test]
    async fn log_append_retries_only_rejected_429_with_the_same_batch() {
        let bodies = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
        let received = bodies.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new().fallback(move |Json(body): Json<serde_json::Value>| {
            let received = received.clone();
            async move {
                let mut bodies = received.lock().unwrap();
                let first = bodies.is_empty();
                let count = body["lines"].as_array().unwrap().len();
                bodies.push(body);
                if first {
                    (
                        axum::http::StatusCode::TOO_MANY_REQUESTS,
                        [("retry-after", "0")],
                    )
                        .into_response()
                } else {
                    Json(json!({"protocolVersion":1,"accepted":count,"nextAfter":7}))
                        .into_response()
                }
            }
        });
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let context = context(base);
        let mut batch = RunnerLogBatch::default();
        batch
            .push(&context, "retained ordered line".into())
            .await
            .unwrap();
        batch.flush(&context).await.unwrap();
        {
            let bodies = bodies.lock().unwrap();
            assert_eq!(bodies.len(), 2);
            assert_eq!(bodies[0], bodies[1]);
            assert!(batch.lines.is_empty());
        }
        server.abort();
        let _ = server.await;
    }

    #[tokio::test]
    async fn rejected_or_uncertain_log_append_retains_batch_without_unbounded_retry() {
        for mode in ["limited", "live-limited", "partial", "server"] {
            let requests = Arc::new(AtomicUsize::new(0));
            let received = requests.clone();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let app = axum::Router::new().fallback(move || {
                let received = received.clone();
                async move {
                    received.fetch_add(1, Ordering::SeqCst);
                    match mode {
                        "limited" | "live-limited" => (
                            axum::http::StatusCode::TOO_MANY_REQUESTS,
                            [("retry-after", "0")],
                        )
                            .into_response(),
                        "partial" => Json(json!({"protocolVersion":1,"accepted":0,"nextAfter":2}))
                            .into_response(),
                        _ => axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                    }
                }
            });
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let context = context(base);
            let mut batch = RunnerLogBatch::default();
            batch.push(&context, "unacknowledged".into()).await.unwrap();
            if mode == "live-limited" {
                assert!(append_log_lines(&context, &batch.lines).await.is_err());
            } else {
                assert!(batch.flush(&context).await.is_err());
            }
            assert_eq!(
                batch.lines.len(),
                1,
                "incomplete logging cannot be acknowledged"
            );
            assert_eq!(
                requests.load(Ordering::SeqCst),
                if mode == "limited" { 3 } else { 1 }
            );
            server.abort();
            let _ = server.await;
        }
    }

    #[test]
    fn log_rate_limit_delay_honors_seconds_http_date_and_window_default() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-27T15:44:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let seconds = reqwest::header::HeaderValue::from_static("7");
        assert_eq!(log_retry_delay(Some(&seconds), now), Duration::from_secs(7));
        let date = reqwest::header::HeaderValue::from_static("Tue, 27 Oct 2026 15:44:09 GMT");
        assert_eq!(log_retry_delay(Some(&date), now), Duration::from_secs(9));
        let past = reqwest::header::HeaderValue::from_static("Tue, 27 Oct 2026 15:43:00 GMT");
        assert_eq!(log_retry_delay(Some(&past), now), Duration::ZERO);
        assert_eq!(log_retry_delay(None, now), Duration::from_secs(60));
        let invalid = reqwest::header::HeaderValue::from_static("invalid");
        assert_eq!(
            log_retry_delay(Some(&invalid), now),
            Duration::from_secs(60)
        );
    }
