//! CLI real-API smoke tests.
//!
//! Requires `CICD_TEST_DATABASE_URL` and the `integration` feature. The CLI binary
//! remains HTTP-only; the test harness imports the server crate only to bind a
//! disposable Axum API against the same PostgreSQL migrations used in CI.

#![cfg(feature = "integration")]

use std::process::Command;

use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

struct ApiServer {
    base_url: String,
    handle: tokio::task::JoinHandle<()>,
}

impl ApiServer {
    async fn start(pool: sqlx::PgPool) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind disposable API port");
        let addr = listener.local_addr().expect("read disposable API addr");
        let app = cicd::api::app_with_auth_secret(Some(pool), None);
        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve disposable API");
        });
        // Give the spawned server task one scheduler tick before the CLI
        // process tries to connect on slower CI workers.
        tokio::task::yield_now().await;
        Self {
            base_url: format!("http://{addr}"),
            handle,
        }
    }

    async fn shutdown(self) {
        self.handle.abort();
        let _ = self.handle.await;
    }
}

async fn test_pool() -> sqlx::PgPool {
    let url = std::env::var("CICD_TEST_DATABASE_URL")
        .expect("CICD_TEST_DATABASE_URL must point at the integration PostgreSQL");
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .expect("connect integration PostgreSQL");
    cicd::migrations()
        .await
        .expect("load migrations")
        .run(&pool)
        .await
        .expect("run migrations");
    pool
}

fn cli_json_with_flags(api_url: &str, args: &[&str]) -> serde_json::Value {
    let output = Command::new(env!("CARGO_BIN_EXE_cicd-cli"))
        .env("CICD_API_URL", "http://127.0.0.1:1")
        .env("CICD_OUTPUT", "table")
        .arg("--api-url")
        .arg(api_url)
        .arg("--output")
        .arg("json")
        .arg("--timeout-seconds")
        .arg("5")
        .args(args)
        .output()
        .expect("run cicd-cli");
    assert!(
        output.status.success(),
        "cicd-cli {} failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("CLI JSON output")
}

fn cli_json_from_env(api_url: &str, args: &[&str]) -> serde_json::Value {
    let output = Command::new(env!("CARGO_BIN_EXE_cicd-cli"))
        .env("CICD_API_URL", api_url)
        .env("CICD_OUTPUT", "json")
        .env("CICD_TIMEOUT_SECONDS", "5")
        .args(args)
        .output()
        .expect("run cicd-cli with env config");
    assert!(
        output.status.success(),
        "cicd-cli {} failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("CLI JSON output")
}

fn cli_failure(api_url: &str, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_cicd-cli"))
        .env("CICD_API_URL", api_url)
        .env("CICD_OUTPUT", "json")
        .env("CICD_TIMEOUT_SECONDS", "5")
        .args(args)
        .output()
        .expect("run failing cicd-cli command");
    assert!(
        !output.status.success(),
        "cicd-cli {} unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_exercises_real_http_api_and_postgres_stack() {
    let pool = test_pool().await;
    let server = ApiServer::start(pool.clone()).await;
    let namespace = Uuid::new_v4();
    let project_name = format!("cli-real-api-{}", namespace.simple());
    let repo_url = format!("https://example.invalid/{project_name}.git");

    let project = cli_json_with_flags(
        &server.base_url,
        &[
            "project",
            "create",
            "--name",
            &project_name,
            "--repository-url",
            &repo_url,
            "--branch",
            "main",
        ],
    );
    assert_eq!(project["name"], project_name);
    assert_eq!(project["repository_url"], repo_url);
    let project_id = project["id"].as_str().expect("project id").to_owned();

    let projects = cli_json_from_env(
        &server.base_url,
        &["project", "list", "--limit", "200", "--offset", "0"],
    );
    assert!(
        projects
            .as_array()
            .expect("project list")
            .iter()
            .any(|item| item["id"] == project_id),
        "created project should be visible through env-configured CLI list: {projects:#}"
    );

    let idempotency_key = Uuid::new_v4().to_string();
    let pipeline = cli_json_with_flags(
        &server.base_url,
        &[
            "pipeline",
            "run",
            "--project",
            &project_id,
            "--git-ref",
            "main",
            "--idempotency-key",
            &idempotency_key,
        ],
    );
    assert_eq!(pipeline["pipeline"]["project_id"], project_id);
    assert_eq!(pipeline["pipeline"]["git_ref"], "main");
    assert_eq!(pipeline["pipeline"]["status"], "queued");
    assert!(pipeline["plan"]["plan_sha256"].as_str().is_some());
    let pipeline_id = pipeline["pipeline"]["id"]
        .as_str()
        .expect("pipeline id")
        .to_owned();
    let first_job_id = pipeline["stages"][0]["jobs"][0]["id"]
        .as_str()
        .expect("first job id")
        .to_owned();

    let replay = cli_json_with_flags(
        &server.base_url,
        &[
            "pipeline",
            "run",
            "--project",
            &project_id,
            "--git-ref",
            "main",
            "--idempotency-key",
            &idempotency_key,
        ],
    );
    assert_eq!(replay["pipeline"]["id"], pipeline_id);

    let detail = cli_json_from_env(
        &server.base_url,
        &["pipeline", "show", "--id", &pipeline_id],
    );
    assert_eq!(detail["pipeline"]["id"], pipeline_id);
    assert!(detail["stages"].as_array().expect("stages").len() >= 3);

    let attempts = cli_json_from_env(
        &server.base_url,
        &["job", "attempts", "--id", &first_job_id],
    );
    assert!(
        attempts.as_array().expect("attempt list").len() >= 1,
        "pipeline run should create attempt history"
    );

    let environment = cli_json_with_flags(
        &server.base_url,
        &[
            "environment",
            "create",
            "--project",
            &project_id,
            "--name",
            "production",
            "--protected",
            "--required-approvals",
            "1",
        ],
    );
    assert_eq!(environment["protected"], true);
    assert_eq!(environment["required_approvals"], 1);
    let environment_id = environment["id"]
        .as_str()
        .expect("environment id")
        .to_owned();

    let deployment = cli_json_with_flags(
        &server.base_url,
        &[
            "deployment",
            "create",
            "--environment",
            &environment_id,
            "--git-ref",
            "main",
        ],
    );
    assert_eq!(deployment["approval_required"], true);
    assert_eq!(deployment["approval_state"], "pending");
    assert!(deployment["pipeline_id"].is_null());
    let deployment_id = deployment["id"].as_str().expect("deployment id").to_owned();

    let approved = cli_json_with_flags(
        &server.base_url,
        &[
            "deployment",
            "approve",
            "--id",
            &deployment_id,
            "--actor",
            "cli-smoke",
            "--comment",
            "real API gate",
        ],
    );
    assert_eq!(approved["approval_state"], "approved");
    assert_eq!(approved["approval_count"], 1);
    assert!(approved["pipeline_id"].as_str().is_some());

    let approvals = cli_json_from_env(
        &server.base_url,
        &["deployment", "approvals", "--id", &deployment_id],
    );
    assert_eq!(approvals[0]["actor"], "cli-smoke");
    assert_eq!(approvals[0]["decision"], "approved");

    let stderr = cli_failure(
        &server.base_url,
        &["pipeline", "show", "--id", &Uuid::new_v4().to_string()],
    );
    assert!(
        stderr.contains("404 Not Found") || stderr.contains("not found"),
        "CLI should surface non-zero API errors, got stderr:\n{stderr}"
    );

    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(Uuid::parse_str(&project_id).expect("parse project id"))
        .execute(&pool)
        .await
        .expect("cleanup project");
    server.shutdown().await;
}
