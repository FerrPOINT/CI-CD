use axum::{
    Router,
    body::{Body, to_bytes},
    extract::State,
    http::{Request, StatusCode},
    response::Response,
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io::Write,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};
#[derive(Debug, Clone)]
struct Recorded {
    method: String,
    path: String,
    body: Vec<u8>,
    key: Option<String>,
    authorization: Option<String>,
}
struct TestState {
    responses: Mutex<VecDeque<(u16, Vec<u8>)>>,
    requests: Mutex<Vec<Recorded>>,
    delay: Duration,
}
struct Server {
    url: String,
    state: Arc<TestState>,
    handle: tokio::task::JoinHandle<()>,
}

#[tokio::test]
async fn profile_default_and_explicit_configuration_precedence() {
    let server = Server::start(vec![(200, json!([])), (200, json!([]))]).await;
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(&config, format!("default_profile = 'fixture'\n[profiles.fixture]\napi_url = '{}'\ntoken = 'profile-token'\noutput = 'table'\n",server.url)).unwrap();
    let path = config.clone();
    let first = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_cicd-cli"))
            .env("CICD_CONFIG", path)
            .env_remove("CICD_PROFILE")
            .env_remove("CICD_API_URL")
            .env_remove("CICD_API_TOKEN")
            .env_remove("CICD_OUTPUT")
            .env_remove("SDLC_API_TOKEN")
            .args(["--output", "json", "repository", "list"])
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&first.stdout).unwrap(),
        json!([])
    );
    std::fs::write(
        &config,
        "[profiles.fixture]\napi_url = 'http://127.0.0.1:1'\noutput = 'table'\n",
    )
    .unwrap();
    let url = server.url.clone();
    let second = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_cicd-cli"))
            .env("CICD_CONFIG", config)
            .env_remove("CICD_API_URL")
            .env_remove("CICD_API_TOKEN")
            .env_remove("CICD_OUTPUT")
            .args([
                "--profile",
                "fixture",
                "--api-url",
                &url,
                "--token",
                "flag-token",
                "--output",
                "json",
                "repository",
                "list",
            ])
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&second.stdout).unwrap(),
        json!([])
    );
    assert_eq!(
        server.requests()[0].authorization.as_deref(),
        Some("Bearer profile-token")
    );
    assert_eq!(
        server.requests()[1].authorization.as_deref(),
        Some("Bearer flag-token")
    );
}
impl Drop for Server {
    fn drop(&mut self) {
        self.handle.abort();
    }
}
impl Server {
    async fn start(responses: Vec<(u16, Value)>) -> Self {
        Self::bytes(
            responses
                .into_iter()
                .map(|(s, v)| (s, serde_json::to_vec(&v).unwrap()))
                .collect(),
            Duration::ZERO,
        )
        .await
    }
    async fn bytes(responses: Vec<(u16, Vec<u8>)>, delay: Duration) -> Self {
        let state = Arc::new(TestState {
            responses: Mutex::new(responses.into()),
            requests: Mutex::new(vec![]),
            delay,
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new().fallback(record).with_state(state.clone());
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self { url, state, handle }
    }
    fn requests(&self) -> Vec<Recorded> {
        self.state.requests.lock().unwrap().clone()
    }
    async fn run(&self, args: &[&str], input: Option<&str>) -> std::process::Output {
        let url = self.url.clone();
        let args = args.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let input = input.map(str::to_string);
        tokio::task::spawn_blocking(move || {
            let mut child = Command::new(env!("CARGO_BIN_EXE_cicd-cli"))
                .args(["--api-url", &url, "--token", "fixture-token"])
                .args(args)
                .env_remove("SDLC_API_TOKEN")
                .env_remove("CICD_PROFILE")
                .env_remove("WIKI_TOKEN")
                .env_remove("TASKTRACKER_TOKEN")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            if let Some(input) = input {
                child
                    .stdin
                    .take()
                    .unwrap()
                    .write_all(input.as_bytes())
                    .unwrap();
            } else {
                drop(child.stdin.take());
            }
            child.wait_with_output().unwrap()
        })
        .await
        .unwrap()
    }
}
async fn record(State(state): State<Arc<TestState>>, req: Request<Body>) -> Response {
    let method = req.method().to_string();
    let path = req.uri().to_string();
    let key = req
        .headers()
        .get("idempotency-key")
        .and_then(|h| h.to_str().ok())
        .map(str::to_string);
    let authorization = req
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .map(str::to_string);
    let body = to_bytes(req.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    state.requests.lock().unwrap().push(Recorded {
        method,
        path,
        body,
        key,
        authorization,
    });
    tokio::time::sleep(state.delay).await;
    let (status, bytes) = state.responses.lock().unwrap().pop_front().unwrap_or((
        500,
        br#"{"error":{"code":"UNEXPECTED_REQUEST","message":"unexpected request"}}"#.to_vec(),
    ));
    Response::builder()
        .status(StatusCode::from_u16(status).unwrap())
        .header("content-type", "application/json")
        .body(Body::from(bytes))
        .unwrap()
}
fn success(output: &std::process::Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    serde_json::from_slice(&output.stdout).unwrap()
}

#[tokio::test]
async fn lifecycle_variables_manual_play_and_logs_page() {
    let server = Server::start(vec![
        (200, json!({"id":"pipeline"})),
        (200, json!({"started":true})),
        (200, json!({"retried":true})),
        (200, json!({"logs":[],"next_after":10})),
        (200, json!({"canceled":true})),
        (200, json!({"retried":true})),
    ])
    .await;
    success(
        &server
            .run(
                &[
                    "pipeline",
                    "run",
                    "--project",
                    "project",
                    "--variable",
                    "MODE=test=ok",
                    "--idempotency-key",
                    "fixture-key",
                ],
                None,
            )
            .await,
    );
    success(&server.run(&["job", "play", "--id", "job"], None).await);
    success(&server.run(&["job", "retry", "--id", "job"], None).await);
    success(
        &server
            .run(
                &[
                    "job",
                    "logs-page",
                    "--id",
                    "job",
                    "--attempt",
                    "attempt",
                    "--after",
                    "10",
                    "--limit",
                    "20",
                    "--q",
                    "error message",
                ],
                None,
            )
            .await,
    );
    success(
        &server
            .run(&["pipeline", "cancel", "--id", "pipeline"], None)
            .await,
    );
    success(
        &server
            .run(&["pipeline", "retry", "--id", "pipeline"], None)
            .await,
    );
    let req = server.requests();
    assert_eq!(req[0].method, "POST");
    assert_eq!(req[0].key.as_deref(), Some("fixture-key"));
    assert_eq!(
        req[0].authorization.as_deref(),
        Some("Bearer fixture-token")
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&req[0].body).unwrap()["variables"]["MODE"],
        "test=ok"
    );
    assert_eq!(req[1].path, "/api/v1/jobs/job/start");
    assert!(
        req[3]
            .path
            .starts_with("/api/v1/jobs/job/attempts/attempt/logs/page?")
    );
    assert!(req[3].path.contains("after=10"));
    assert!(req[3].path.contains("q=error+message"));
}
#[tokio::test]
async fn repositories_pr_payload_and_actions() {
    let server = Server::start(vec![
        (200, json!([])),
        (200, json!({"name":"demo"})),
        (200, json!([])),
        (200, json!({"number":1})),
        (200, json!({"status":"closed"})),
        (200, json!({"status":"open"})),
        (200, json!({"status":"merged"})),
    ])
    .await;
    success(&server.run(&["repository", "list"], None).await);
    success(
        &server
            .run(
                &[
                    "repository",
                    "create",
                    "--name",
                    "demo",
                    "--visibility",
                    "private",
                ],
                None,
            )
            .await,
    );
    success(
        &server
            .run(
                &[
                    "repository",
                    "refs",
                    "--repo",
                    "demo",
                    "--limit",
                    "5",
                    "--offset",
                    "5",
                    "--kind",
                    "branch",
                ],
                None,
            )
            .await,
    );
    success(
        &server
            .run(
                &[
                    "pr",
                    "create",
                    "--repo",
                    "demo",
                    "--title",
                    "CLI PR",
                    "--source-branch",
                    "feature",
                    "--target-branch",
                    "main",
                    "--from-file",
                    "-",
                ],
                Some("Описание PR\nиз stdin"),
            )
            .await,
    );
    for action in ["close", "reopen", "merge"] {
        success(
            &server
                .run(&["pr", action, "--repo", "demo", "--number", "1"], None)
                .await,
        );
    }
    let req = server.requests();
    assert_eq!(
        serde_json::from_slice::<Value>(&req[3].body).unwrap()["description"],
        "Описание PR\nиз stdin"
    );
    assert_eq!(req[6].path, "/api/v1/repos/demo/pulls/1/action");
    assert_eq!(
        serde_json::from_slice::<Value>(&req[6].body).unwrap()["action"],
        "merge"
    );
}
#[tokio::test]
async fn wait_terminal_statuses_keep_output_and_deadline_does_not_cancel() {
    for (status, expected) in [("success", 0), ("failed", 3), ("canceled", 3)] {
        let server = Server::start(vec![(200, json!({"pipeline":{"status":status}}))]).await;
        let output = server
            .run(&["pipeline", "wait", "--id", "pipeline"], None)
            .await;
        assert_eq!(output.status.code(), Some(expected));
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap()["pipeline"]["status"],
            status
        );
    }
    let server = Server::bytes(
        vec![(200, br#"{"pipeline":{"status":"running"}}"#.to_vec())],
        Duration::from_secs(3),
    )
    .await;
    let start = std::time::Instant::now();
    let output = server
        .run(
            &[
                "--error-format",
                "json",
                "pipeline",
                "wait",
                "--id",
                "pipeline",
                "--wait-timeout-seconds",
                "1",
            ],
            None,
        )
        .await;
    assert_eq!(output.status.code(), Some(3));
    assert!(start.elapsed() < Duration::from_secs(2));
    assert!(output.stdout.is_empty());
    assert!(server.requests().iter().all(|r| r.method == "GET"));
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert!(error["error"]["status"].is_null());
    let server = Server::start(vec![(200, json!({"pipeline":{"status":"queued"}}))]).await;
    let start = std::time::Instant::now();
    let output = server
        .run(
            &[
                "--error-format",
                "json",
                "pipeline",
                "wait",
                "--id",
                "pipeline",
                "--wait-timeout-seconds",
                "1",
                "--poll-interval-seconds",
                "18446744073709551615",
            ],
            None,
        )
        .await;
    assert_eq!(output.status.code(), Some(3));
    assert!(start.elapsed() < Duration::from_secs(2));
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"]["code"],
        "WAIT_TIMEOUT"
    );
    assert_eq!(server.requests().len(), 1);
}
#[tokio::test]
async fn wait_polls_until_success() {
    let server = Server::start(vec![
        (200, json!({"pipeline":{"status":"queued"}})),
        (200, json!({"pipeline":{"status":"running"}})),
        (200, json!({"pipeline":{"status":"success"}})),
    ])
    .await;
    success(
        &server
            .run(
                &[
                    "pipeline",
                    "wait",
                    "--id",
                    "pipeline",
                    "--poll-interval-seconds",
                    "1",
                    "--wait-timeout-seconds",
                    "5",
                ],
                None,
            )
            .await,
    );
    assert_eq!(server.requests().len(), 3);
}
#[tokio::test]
async fn secret_stdin_redaction_and_artifact_no_clobber() {
    let server=Server::start(vec![(409,json!({"error":{"code":"CONFLICT","message":"sensitive-value fixture-token","request_id":"req-fixture"}}))]).await;
    let failed = server
        .run(
            &[
                "--error-format",
                "json",
                "secret",
                "set",
                "--project",
                "project",
                "--key",
                "SECRET",
                "--from-file",
                "-",
            ],
            Some("sensitive-value\n"),
        )
        .await;
    assert_eq!(failed.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&failed.stderr).contains("sensitive-value"));
    assert!(!String::from_utf8_lossy(&failed.stderr).contains("fixture-token"));
    assert_eq!(
        serde_json::from_slice::<Value>(&server.requests()[0].body).unwrap()["value"],
        "sensitive-value\n"
    );
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("artifact");
    std::fs::write(&out, b"old").unwrap();
    let failed = server
        .run(
            &[
                "artifact",
                "download",
                "--id",
                "artifact",
                "--output",
                out.to_str().unwrap(),
            ],
            None,
        )
        .await;
    assert!(!failed.status.success());
    assert_eq!(std::fs::read(&out).unwrap(), b"old");
    assert_eq!(server.requests().len(), 1);
}

#[tokio::test]
async fn parsing_errors_are_json_and_do_not_echo_credentials() {
    let server = Server::start(vec![]).await;
    let output = server
        .run(
            &[
                "--error-format",
                "json",
                "--token",
                "sensitive-value",
                "unknown-command",
            ],
            None,
        )
        .await;
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let value: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(value["error"]["code"], "CLI_USAGE");
    assert!(!String::from_utf8_lossy(&output.stderr).contains("sensitive-value"));
    let invalid_variable = server
        .run(
            &[
                "pipeline",
                "run",
                "--project",
                "fixture",
                "--git-ref",
                "main",
                "--variable",
                "sensitive-variable",
            ],
            None,
        )
        .await;
    assert_eq!(invalid_variable.status.code(), Some(2));
    assert!(invalid_variable.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&invalid_variable.stderr).contains("sensitive-variable"));
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn effective_transport_token_is_redacted() {
    for (token, expected) in [
        (" fixture-token ", "fixture-token"),
        (" ", "shared-fixture-token"),
    ] {
        let server = Server::start(vec![(
            403,
            json!({"error": {"code": "DENIED", "message": expected}}),
        )])
        .await;
        let url = server.url.clone();
        let output = tokio::task::spawn_blocking(move || {
            Command::new(env!("CARGO_BIN_EXE_cicd-cli"))
                .args([
                    "--api-url",
                    &url,
                    "--token",
                    token,
                    "--error-format",
                    "json",
                    "project",
                    "list",
                ])
                .env("SDLC_API_TOKEN", "shared-fixture-token")
                .env_remove("CICD_PROFILE")
                .env_remove("CICD_API_TOKEN")
                .env_remove("TASKTRACKER_TOKEN")
                .output()
                .unwrap()
        })
        .await
        .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["status"], 403);
        assert_eq!(error["error"]["message"], "[REDACTED]");
        assert_eq!(
            server.requests()[0].authorization.as_deref(),
            Some(format!("Bearer {expected}").as_str())
        );
    }
}

fn workspace_lookup() -> Value {
    json!({"requestHash":"a".repeat(64),
        "taskId":"00000000-0000-4000-8000-000000000001",
        "rootTaskId":"00000000-0000-4000-8000-000000000001",
        "assignmentId":"00000000-0000-4000-8000-000000000002",
        "executionId":"00000000-0000-4000-8000-000000000003","fencingToken":1})
}

#[tokio::test]
async fn task_workspace_commands_preserve_fence_original_keys_and_lookup() {
    let project = "00000000-0000-4000-8000-000000000004";
    let lookup = workspace_lookup();
    let fence = json!({"developmentResultId":"00000000-0000-4000-8000-000000000005",
        "assignmentId":"00000000-0000-4000-8000-000000000006",
        "executionId":"00000000-0000-4000-8000-000000000007",
        "agentId":"00000000-0000-4000-8000-000000000008",
        "leaseId":"00000000-0000-4000-8000-000000000009",
        "fencingToken":2,"expectedLeaseVersion":3,"assignmentHash":"b".repeat(64)});
    let delivery = json!({"operationKey":"deliver:original","workspaceOperationKey":"prepare:original",
        "original":lookup,"action":"deploy","artifactId":"00000000-0000-4000-8000-000000000010",
        "expectedManifestSha256":null,"taskFence":fence});
    let prepare = json!({"operationKey":"prepare:original","contractVersion":1});
    let response = json!({"receipt":{"status":"unknown","sdlcAcceptanceVerified":false},"reconciliationNeeded":true});
    let server = Server::start(vec![(200, response.clone()); 5]).await;
    let args = [
        "workspace",
        "prepare",
        "--project",
        project,
        "--from-file",
        "-",
    ];
    assert_eq!(
        success(&server.run(&args, Some(&prepare.to_string())).await),
        response
    );
    for command in [
        "readback",
        "candidate-evidence",
        "delivery",
        "delivery-readback",
    ] {
        let mut args = vec![
            "workspace",
            command,
            "--project",
            project,
            "--operation-key",
            "prepare:original",
            "--from-file",
            "-",
        ];
        if command == "delivery-readback" {
            args.extend(["--delivery-key", "deliver:original"]);
        }
        let body = if command == "delivery" {
            &delivery
        } else {
            &lookup
        };
        assert_eq!(
            success(&server.run(&args, Some(&body.to_string())).await),
            response
        );
    }
    let requests = server.requests();
    assert_eq!(requests.len(), 5);
    let root = format!("/api/v1/projects/{project}/sdlc/workspace-operations");
    assert_eq!(requests[0].path, root);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(
        serde_json::from_slice::<Value>(&requests[0].body).unwrap(),
        prepare
    );
    assert_eq!(
        requests[3].path,
        format!("{root}/prepare%3Aoriginal/delivery-operations")
    );
    assert_eq!(requests[3].method, "POST");
    assert_eq!(
        serde_json::from_slice::<Value>(&requests[3].body).unwrap(),
        delivery
    );
    for (index, suffix) in [
        (1, ""),
        (2, "/candidate-evidence"),
        (4, "/delivery-operations/deliver%3Aoriginal"),
    ] {
        let request = &requests[index];
        assert_eq!(request.method, "GET");
        assert!(request.body.is_empty());
        let url = reqwest::Url::parse(&format!("http://localhost{}", request.path)).unwrap();
        assert_eq!(url.path(), format!("{root}/prepare%3Aoriginal{suffix}"));
        let pairs = url
            .query_pairs()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(pairs.len(), 6);
        for (key, value) in lookup.as_object().unwrap() {
            let expected = value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string());
            assert_eq!(pairs.get(key.as_str()).unwrap().as_ref(), expected);
        }
    }
    assert!(
        requests
            .iter()
            .all(|r| r.key.is_none() && r.authorization.as_deref() == Some("Bearer fixture-token"))
    );
}

#[tokio::test]
async fn task_delivery_rejection_never_falls_back_or_reposts() {
    for status in [409, 503] {
        let server = Server::start(vec![(
            status,
            json!({"error":{"code":"HELD","message":"fixture-token"}}),
        )])
        .await;
        let output = server
            .run(
                &[
                    "--error-format",
                    "json",
                    "workspace",
                    "delivery",
                    "--project",
                    "00000000-0000-4000-8000-000000000004",
                    "--operation-key",
                    "prepare:original",
                    "--from-file",
                    "-",
                ],
                Some("{\"operationKey\":\"deliver:original\",\"taskFence\":null}"),
            )
            .await;
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert_eq!(server.requests().len(), 1);
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["status"], status);
        assert!(!String::from_utf8_lossy(&output.stderr).contains("fixture-token"));
    }
}

#[tokio::test]
async fn task_workspace_bad_inputs_are_private_and_make_no_http_request() {
    let server = Server::start(vec![]).await;
    let mut extra = workspace_lookup();
    extra["unexpectedPrivateField"] = json!("private-input-marker");
    for (command, body) in [
        ("delivery", "{private-input-marker".to_owned()),
        ("delivery", "null".to_owned()),
        (
            "delivery",
            json!({"private-input-marker":"x".repeat(16*1024)}).to_string(),
        ),
        ("readback", extra.to_string()),
        ("readback", "{}".to_owned()),
    ] {
        let output = server
            .run(
                &[
                    "--error-format",
                    "json",
                    "workspace",
                    command,
                    "--project",
                    "00000000-0000-4000-8000-000000000004",
                    "--operation-key",
                    "prepare:original",
                    "--from-file",
                    "-",
                ],
                Some(&body),
            )
            .await;
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("private-input-marker"));
    }
    for key in ["..", "../foreign", "foreign/key", ""] {
        let output = server
            .run(
                &[
                    "workspace",
                    "readback",
                    "--project",
                    "00000000-0000-4000-8000-000000000004",
                    "--operation-key",
                    key,
                    "--from-file",
                    "-",
                ],
                Some(&workspace_lookup().to_string()),
            )
            .await;
        assert!(!output.status.success());
    }
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn task_delivery_file_input_keeps_rollback_and_missing_fence_unchanged() {
    let server = Server::start(vec![(200, json!({"status":"unavailable"})); 2]).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("command.json");
    let mut body = json!({"operationKey":"rollback:original","workspaceOperationKey":"prepare:original",
        "original":workspace_lookup(),"action":"rollback","artifactId":null,
        "expectedManifestSha256":"a".repeat(64)});
    for fence in [None, Some(json!({"expectedLeaseVersion":4}))] {
        if let Some(fence) = fence {
            body["taskFence"] = fence;
        }
        std::fs::write(&path, body.to_string()).unwrap();
        success(
            &server
                .run(
                    &[
                        "workspace",
                        "delivery",
                        "--project",
                        "00000000-0000-4000-8000-000000000004",
                        "--operation-key",
                        "prepare:original",
                        "--from-file",
                        path.to_str().unwrap(),
                    ],
                    None,
                )
                .await,
        );
        let request = server.requests().pop().unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&request.body).unwrap(),
            body
        );
    }
    assert_eq!(server.requests().len(), 2);
}

#[tokio::test]
async fn task_pipeline_cli_preserves_supplied_intent_key_and_authenticated_locator() {
    let project = uuid::Uuid::new_v4().to_string();
    let pipeline = uuid::Uuid::new_v4().to_string();
    let key = uuid::Uuid::new_v4().to_string();
    let task = uuid::Uuid::new_v4();
    let intent = json!({"operationKey":"workspace:original","repositoryId":uuid::Uuid::new_v4(),
        "sourceCommit":"a".repeat(40),"role":"developer","agentId":uuid::Uuid::new_v4(),
        "trackerLeaseId":uuid::Uuid::new_v4(),"holderSubject":"scheduler","expectedLeaseVersion":1,
        "binding":{"trackerInstanceId":uuid::Uuid::new_v4().to_string(),
            "trackerProjectId":uuid::Uuid::new_v4(),"taskId":task,"rootTaskId":task,
            "assignmentId":uuid::Uuid::new_v4(),"executionId":uuid::Uuid::new_v4(),
            "routingSnapshotId":uuid::Uuid::new_v4(),"requirementRevision":1,"fencingToken":1,
            "assignmentHash":"b".repeat(64),"workflowTaskRef":"SDLC-7"}});
    let server = Server::start(vec![
        (200, json!({"pipeline":{"id":pipeline}})),
        (200, Value::Null),
    ])
    .await;
    let raw = intent.to_string();
    success(
        &server
            .run(
                &[
                    "--output",
                    "json",
                    "pipeline",
                    "run",
                    "--project",
                    &project,
                    "--git-ref",
                    &"a".repeat(40),
                    "--idempotency-key",
                    &key,
                    "--task-intent",
                    "-",
                ],
                Some(&raw),
            )
            .await,
    );
    assert_eq!(
        success(
            &server
                .run(
                    &[
                        "--output",
                        "json",
                        "pipeline",
                        "task-workspace",
                        "--project",
                        &project,
                        "--id",
                        &pipeline
                    ],
                    None
                )
                .await
        ),
        Value::Null
    );
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].key.as_deref(), Some(key.as_str()));
    assert_eq!(
        serde_json::from_slice::<Value>(&requests[0].body).unwrap()["task_intent"],
        intent
    );
    assert_eq!(requests[1].method, "GET");
    assert_eq!(
        requests[1].path,
        format!("/api/v1/projects/{project}/pipelines/{pipeline}/task-workspace")
    );
    assert!(
        requests
            .iter()
            .all(|r| r.authorization.as_deref() == Some("Bearer fixture-token"))
    );
}

#[tokio::test]
async fn task_pipeline_cli_no_generated_key_malformed_input_or_http_retry() {
    let server = Server::start(vec![(409, json!({"error":"held"}))]).await;
    let args = [
        "pipeline",
        "run",
        "--project",
        "00000000-0000-4000-8000-000000000001",
        "--task-intent",
        "-",
    ];
    let output = server.run(&args, Some("{}")).await;
    assert!(!output.status.success());
    assert!(server.requests().is_empty());
    for input in ["private-invalid-json", "[]", "null"] {
        let output = server
            .run(
                &[
                    "pipeline",
                    "run",
                    "--project",
                    "00000000-0000-4000-8000-000000000001",
                    "--idempotency-key",
                    "00000000-0000-4000-8000-000000000002",
                    "--task-intent",
                    "-",
                ],
                Some(input),
            )
            .await;
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("private-invalid-json"));
        assert!(server.requests().is_empty());
    }
    let output = server
        .run(
            &[
                "pipeline",
                "run",
                "--project",
                "00000000-0000-4000-8000-000000000001",
                "--idempotency-key",
                "00000000-0000-4000-8000-000000000002",
                "--task-intent",
                "-",
            ],
            Some("{}"),
        )
        .await;
    assert!(!output.status.success());
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn help_hides_token_environment_value() {
    let output = Command::new(env!("CARGO_BIN_EXE_cicd-cli"))
        .args(["--help"])
        .env("CICD_API_TOKEN", "help-only-fixture-token")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("CICD_API_TOKEN"));
    assert!(!stdout.contains("help-only-fixture-token"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("help-only-fixture-token"));
}
