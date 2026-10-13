//! CLI real-API smoke tests.
//!
//! Requires `CICD_TEST_DATABASE_URL` and the `integration` feature. The CLI binary
//! remains HTTP-only; the test harness imports the server crate only to bind a
//! disposable Axum API against the same PostgreSQL migrations used in CI.

#![cfg(feature = "integration")]

use std::{process::Command, str::FromStr};

use serde_json::json;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use uuid::Uuid;

struct ApiServer {
    base_url: String,
    handle: tokio::task::JoinHandle<()>,
    files: tempfile::TempDir,
}

impl ApiServer {
    async fn start_with_auth_secret(pool: sqlx::PgPool, auth_secret: Option<String>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind disposable API port");
        let addr = listener.local_addr().expect("read disposable API addr");
        let files = tempfile::tempdir().unwrap();
        let git = cicd::git_host::GitConfig {
            root: files.path().join("repos"),
            ..Default::default()
        };
        let mut config = cicd::config::RuntimeConfig::test_default().with_auth_secret(auth_secret);
        config.artifacts.root = files.path().join("artifacts");
        let app = cicd::api::app_with_git_and_config(Some(pool), git, None, config).unwrap();
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
            files,
        }
    }

    async fn shutdown(self) {
        self.handle.abort();
        let _ = self.handle.await;
    }
}

struct TestDatabase {
    pool: sqlx::PgPool,
    admin: sqlx::PgPool,
    schema: String,
}

impl TestDatabase {
    async fn cleanup(self) {
        self.pool.close().await;
        sqlx::query(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .execute(&self.admin)
            .await
            .expect("drop owned CLI schema");
        self.admin.close().await;
    }
}

async fn test_pool() -> TestDatabase {
    let url = std::env::var("CICD_TEST_DATABASE_URL")
        .expect("CICD_TEST_DATABASE_URL must point at the integration PostgreSQL");
    let options = PgConnectOptions::from_str(&url).expect("parse integration PostgreSQL URL");
    assert!(
        options
            .get_database()
            .is_some_and(|name| name.starts_with("forge_test_")),
        "CLI integration tests require an explicit disposable forge_test_ database"
    );
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone())
        .await
        .expect("connect integration PostgreSQL");
    // Generated identifiers contain only a fixed prefix and UUID hex digits.
    let schema = format!("it_cli_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .expect("create owned CLI schema");
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options.options([("search_path", schema.as_str())]))
        .await
        .expect("connect owned CLI schema");
    cicd::migrations()
        .await
        .expect("load migrations")
        .run(&pool)
        .await
        .expect("run migrations");
    TestDatabase {
        pool,
        admin,
        schema,
    }
}

fn cli_json_from_env(api_url: &str, token: &str, args: &[&str]) -> serde_json::Value {
    let output = Command::new(env!("CARGO_BIN_EXE_cicd-cli"))
        .env("CICD_API_TOKEN", token)
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

fn cli_json_with_token(api_url: &str, token: &str, args: &[&str]) -> serde_json::Value {
    let output = Command::new(env!("CARGO_BIN_EXE_cicd-cli"))
        .env("CICD_API_URL", "http://127.0.0.1:1")
        .env("CICD_API_TOKEN", "bad-env-token")
        .env("CICD_OUTPUT", "table")
        .arg("--api-url")
        .arg(api_url)
        .arg("--token")
        .arg(token)
        .arg("--output")
        .arg("json")
        .arg("--timeout-seconds")
        .arg("5")
        .args(args)
        .output()
        .expect("run authenticated cicd-cli");
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
        .env("SDLC_API_TOKEN", "fixture-token")
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

fn cli_failure_with_token(api_url: &str, token: &str, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_cicd-cli"))
        .env("CICD_API_URL", api_url)
        .env("CICD_API_TOKEN", "bad-env-token")
        .env("CICD_OUTPUT", "json")
        .env("CICD_TIMEOUT_SECONDS", "5")
        .arg("--token")
        .arg(token)
        .args(args)
        .output()
        .expect("run failing authenticated cicd-cli command");
    assert!(
        !output.status.success(),
        "cicd-cli {} unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stdout.contains(token) && !stderr.contains(token),
        "CLI failure leaked bearer token\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    stderr.into_owned()
}

async fn insert_login_user(
    pool: &sqlx::PgPool,
    username: &str,
    role: &str,
    password: &str,
) -> Uuid {
    let user_id = Uuid::new_v4();
    sqlx::query("INSERT INTO users (id, username, role, enabled) VALUES ($1, $2, $3, true)")
        .bind(user_id)
        .bind(username)
        .bind(role)
        .execute(pool)
        .await
        .expect("insert test user");
    let password_hash = cicd::auth::hash_password(password).expect("hash test password");
    sqlx::query("INSERT INTO user_credentials (user_id, password_hash) VALUES ($1, $2)")
        .bind(user_id)
        .bind(password_hash)
        .execute(pool)
        .await
        .expect("insert test credential");
    user_id
}

async fn login_access_token(api_url: &str, username: &str, password: &str) -> String {
    let response = reqwest::Client::new()
        .post(format!("{api_url}/api/v1/auth/login"))
        .json(&json!({
            "username": username,
            "password": password,
        }))
        .send()
        .await
        .expect("login request");
    assert!(
        response.status().is_success(),
        "login failed with status {}: {}",
        response.status(),
        response.text().await.unwrap_or_default()
    );
    let body: serde_json::Value = response.json().await.expect("login JSON");
    body["access_token"]
        .as_str()
        .expect("access token")
        .to_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_exercises_real_http_api_and_postgres_stack() {
    let database = test_pool().await;
    let pool = database.pool.clone();
    let server = ApiServer::start_with_auth_secret(
        pool.clone(),
        Some(format!("fixture-secret-{}", Uuid::new_v4())),
    )
    .await;
    let fixture_user = format!("fixture-admin-{}", Uuid::new_v4());
    let fixture_user_id =
        insert_login_user(&pool, &fixture_user, "admin", "fixture-password").await;
    let fixture_access =
        login_access_token(&server.base_url, &fixture_user, "fixture-password").await;
    let namespace = Uuid::new_v4();
    let project_name = format!("cli-real-api-{}", namespace.simple());
    let repo_url = format!("https://example.invalid/{project_name}.git");

    cli_json_with_token(
        &server.base_url,
        &fixture_access,
        &["repository", "create", "--name", &project_name],
    );
    let checkout = tempfile::tempdir().unwrap();
    let bare = server
        .files
        .path()
        .join("repos")
        .join(format!("{project_name}.git"))
        .to_string_lossy()
        .into_owned();
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .current_dir(checkout.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?}: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "--initial-branch=main"]);
    git(&["config", "user.name", "CLI Fixture"]);
    git(&["config", "user.email", "fixture@example.invalid"]);
    std::fs::write(checkout.path().join("README.md"), "CLI fixture").unwrap();
    git(&["add", "."]);
    git(&["commit", "-m", "fixture main"]);
    git(&["push", &bare, "main"]);
    git(&["checkout", "-b", "feature"]);
    std::fs::write(checkout.path().join("change.txt"), "feature").unwrap();
    git(&["add", "."]);
    git(&["commit", "-m", "fixture feature"]);
    git(&["tag", "v1"]);
    git(&["push", &bare, "feature", "--tags"]);
    for operation in ["refs", "tree", "tags", "commits"] {
        cli_json_with_token(
            &server.base_url,
            &fixture_access,
            &[
                "repository",
                operation,
                "--repo",
                &project_name,
                "--limit",
                "10",
                "--offset",
                "0",
            ],
        );
    }
    cli_json_with_token(
        &server.base_url,
        &fixture_access,
        &[
            "repository",
            "blob",
            "--repo",
            &project_name,
            "--git-ref",
            "main",
            "--path",
            "README.md",
        ],
    );
    cli_json_with_token(
        &server.base_url,
        &fixture_access,
        &[
            "repository",
            "compare",
            "--repo",
            &project_name,
            "--from",
            "main",
            "--to",
            "feature",
        ],
    );
    let pr = cli_json_with_token(
        &server.base_url,
        &fixture_access,
        &[
            "pr",
            "create",
            "--repo",
            &project_name,
            "--title",
            "CLI change",
            "--source-branch",
            "feature",
            "--target-branch",
            "main",
            "--description",
            "fixture",
        ],
    );
    let number = pr["number"].as_u64().unwrap().to_string();
    cli_json_with_token(
        &server.base_url,
        &fixture_access,
        &[
            "pr",
            "list",
            "--repo",
            &project_name,
            "--limit",
            "10",
            "--offset",
            "0",
        ],
    );
    cli_json_with_token(
        &server.base_url,
        &fixture_access,
        &["pr", "get", "--repo", &project_name, "--number", &number],
    );
    for action in ["close", "reopen", "merge"] {
        cli_json_with_token(
            &server.base_url,
            &fixture_access,
            &["pr", action, "--repo", &project_name, "--number", &number],
        );
    }

    let project = cli_json_with_token(
        &server.base_url,
        &fixture_access,
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
        &fixture_access,
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
    let pipeline = cli_json_with_token(
        &server.base_url,
        &fixture_access,
        &[
            "pipeline",
            "run",
            "--project",
            &project_id,
            "--git-ref",
            "main",
            "--idempotency-key",
            &idempotency_key,
            "--variable",
            "CLI_FIXTURE=value",
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

    let artifact: serde_json::Value = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/jobs/{first_job_id}/artifacts",
            server.base_url
        ))
        .bearer_auth(&fixture_access)
        .header("X-Artifact-Name", "fixture.txt")
        .header("Content-Type", "text/plain")
        .body("CLI artifact fixture")
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let download = checkout.path().join("download.txt");
    cli_json_with_token(
        &server.base_url,
        &fixture_access,
        &[
            "artifact",
            "download",
            "--id",
            artifact["id"].as_str().unwrap(),
            "--output",
            download.to_str().unwrap(),
        ],
    );
    assert_eq!(std::fs::read(download).unwrap(), b"CLI artifact fixture");

    let replay = cli_json_with_token(
        &server.base_url,
        &fixture_access,
        &[
            "pipeline",
            "run",
            "--project",
            &project_id,
            "--git-ref",
            "main",
            "--idempotency-key",
            &idempotency_key,
            "--variable",
            "CLI_FIXTURE=value",
        ],
    );
    assert_eq!(replay["pipeline"]["id"], pipeline_id);

    let detail = cli_json_from_env(
        &server.base_url,
        &fixture_access,
        &["pipeline", "show", "--id", &pipeline_id],
    );
    assert_eq!(detail["pipeline"]["id"], pipeline_id);
    assert!(detail["stages"].as_array().expect("stages").len() >= 3);

    let attempts = cli_json_from_env(
        &server.base_url,
        &fixture_access,
        &["job", "attempts", "--id", &first_job_id],
    );
    assert!(
        !attempts.as_array().expect("attempt list").is_empty(),
        "pipeline run should create attempt history"
    );
    reqwest::Client::new()
        .post(format!(
            "{}/api/v1/jobs/{first_job_id}/logs",
            server.base_url
        ))
        .bearer_auth(&fixture_access)
        .json(&json!({"message":"CLI log fixture"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let attempt_id = attempts[0]["id"].as_str().unwrap();
    let logs = cli_json_with_token(
        &server.base_url,
        &fixture_access,
        &[
            "job",
            "logs-page",
            "--id",
            &first_job_id,
            "--attempt",
            attempt_id,
            "--limit",
            "10",
        ],
    );
    assert_eq!(logs["items"][0]["message"], "CLI log fixture");

    let environment = cli_json_with_token(
        &server.base_url,
        &fixture_access,
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

    let deployment = cli_json_with_token(
        &server.base_url,
        &fixture_access,
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

    let approved = cli_json_with_token(
        &server.base_url,
        &fixture_access,
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
        &fixture_access,
        &["deployment", "approvals", "--id", &deployment_id],
    );
    assert_eq!(approvals[0]["actor"], fixture_user_id.to_string());
    assert_eq!(approvals[0]["decision"], "approved");

    sqlx::query("UPDATE jobs SET manual = true WHERE id = $1")
        .bind(Uuid::parse_str(&first_job_id).unwrap())
        .execute(&pool)
        .await
        .unwrap();
    cli_json_with_token(
        &server.base_url,
        &fixture_access,
        &["job", "play", "--id", &first_job_id],
    );
    cli_json_with_token(
        &server.base_url,
        &fixture_access,
        &["job", "start", "--id", &first_job_id],
    );
    cli_json_with_token(
        &server.base_url,
        &fixture_access,
        &["job", "fail", "--id", &first_job_id],
    );
    cli_json_with_token(
        &server.base_url,
        &fixture_access,
        &["job", "retry", "--id", &first_job_id],
    );
    cli_json_with_token(
        &server.base_url,
        &fixture_access,
        &["job", "logs-page", "--id", &first_job_id, "--limit", "10"],
    );
    cli_json_with_token(
        &server.base_url,
        &fixture_access,
        &["pipeline", "cancel", "--id", &pipeline_id],
    );
    cli_json_with_token(
        &server.base_url,
        &fixture_access,
        &["pipeline", "retry", "--id", &pipeline_id],
    );
    let rejected = cli_json_with_token(
        &server.base_url,
        &fixture_access,
        &[
            "deployment",
            "create",
            "--environment",
            &environment_id,
            "--git-ref",
            "main",
        ],
    );
    cli_json_with_token(
        &server.base_url,
        &fixture_access,
        &[
            "deployment",
            "reject",
            "--id",
            rejected["id"].as_str().unwrap(),
            "--actor",
            "cli-smoke",
        ],
    );
    sqlx::query("UPDATE deployments SET status = 'success' WHERE id = $1")
        .bind(Uuid::parse_str(&deployment_id).unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let rollback = cli_json_with_token(
        &server.base_url,
        &fixture_access,
        &[
            "deployment",
            "rollback",
            "--id",
            &deployment_id,
            "--git-ref",
            "main",
        ],
    );
    assert_eq!(rollback["rollback_of_id"], deployment_id);
    assert_eq!(rollback["approval_state"], "pending");
    sqlx::query("UPDATE pipelines SET status = 'success' WHERE id = $1")
        .bind(Uuid::parse_str(&pipeline_id).unwrap())
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        cli_json_with_token(
            &server.base_url,
            &fixture_access,
            &[
                "pipeline",
                "wait",
                "--id",
                &pipeline_id,
                "--wait-timeout-seconds",
                "2"
            ]
        )["pipeline"]["status"],
        "success"
    );

    let stderr = cli_failure_with_token(
        &server.base_url,
        &fixture_access,
        &["pipeline", "show", "--id", &Uuid::new_v4().to_string()],
    );
    assert!(
        stderr.contains("404 Not Found") || stderr.contains("not found"),
        "CLI should surface non-zero API errors, got stderr:\n{stderr}"
    );

    let deletion = reqwest::Client::new()
        .delete(format!("{}/api/v1/projects/{project_id}", server.base_url))
        .bearer_auth(&fixture_access)
        .send()
        .await
        .expect("delete project with history");
    assert_eq!(deletion.status(), reqwest::StatusCode::CONFLICT);
    let denial: serde_json::Value = deletion.json().await.expect("history denial JSON");
    assert_eq!(denial["error"]["code"], "conflict");
    assert_eq!(
        denial["error"]["message"],
        "delivery_configuration_history_protected"
    );
    let refused = Command::new(env!("CARGO_BIN_EXE_cicd-cli"))
        .env("CICD_API_URL", &server.base_url)
        .env("CICD_API_TOKEN", &fixture_access)
        .args(["repository", "delete", "--name", &project_name])
        .output()
        .unwrap();
    assert!(!refused.status.success());
    let denial = String::from_utf8_lossy(&refused.stderr);
    assert!(denial.contains("409") && denial.contains("repository_history_must_be_preserved"));
    assert!(!denial.contains(&fixture_access));
    let retained_pr = cli_json_with_token(
        &server.base_url,
        &fixture_access,
        &["pr", "get", "--repo", &project_name, "--number", &number],
    );
    assert_eq!(retained_pr["id"], pr["id"]);
    assert_eq!(retained_pr["number"], pr["number"]);
    let retained: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pull_requests WHERE repository_name=$1")
            .bind(&project_name)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(retained > 0);
    assert!(
        server
            .files
            .path()
            .join("repos")
            .join(format!("{project_name}.git"))
            .is_dir()
    );
    server.shutdown().await;
    database.cleanup().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_exercises_auth_rbac_and_token_redaction_against_real_api() {
    let database = test_pool().await;
    let pool = database.pool.clone();
    let namespace = Uuid::new_v4();
    let auth_secret = format!("cli-auth-secret-{namespace}");
    let server = ApiServer::start_with_auth_secret(pool.clone(), Some(auth_secret)).await;
    let admin_username = format!("cli-admin-{}", namespace.simple());
    let admin_password = format!("AdminPass-{}", namespace.simple());
    insert_login_user(&pool, &admin_username, "admin", &admin_password).await;
    let admin_access = login_access_token(&server.base_url, &admin_username, &admin_password).await;

    let unauthenticated = cli_failure(&server.base_url, &["project", "list"]);
    assert!(
        unauthenticated.contains("401 Unauthorized") || unauthenticated.contains("unauthorized"),
        "CLI should surface missing bearer as unauthorized, got stderr:\n{unauthenticated}"
    );

    let project_name = format!("cli-auth-api-{}", namespace.simple());
    let repo_url = format!("https://example.invalid/{project_name}.git");
    let project = cli_json_with_token(
        &server.base_url,
        &admin_access,
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
    let project_id = project["id"].as_str().expect("project id").to_owned();

    let developer_username = format!("cli-developer-{}", namespace.simple());
    let developer_password = format!("DeveloperPass-{}", namespace.simple());
    let developer = cli_json_with_token(
        &server.base_url,
        &admin_access,
        &[
            "user",
            "create",
            "--username",
            &developer_username,
            "--role",
            "developer",
            "--password",
            &developer_password,
        ],
    );
    let developer_id = developer["id"].as_str().expect("developer id").to_owned();
    cli_json_with_token(
        &server.base_url,
        &admin_access,
        &[
            "member",
            "upsert",
            "--project",
            &project_id,
            "--user",
            &developer_id,
            "--role",
            "developer",
        ],
    );

    let viewer_username = format!("cli-viewer-{}", namespace.simple());
    let viewer_password = format!("ViewerPass-{}", namespace.simple());
    let viewer = cli_json_with_token(
        &server.base_url,
        &admin_access,
        &[
            "user",
            "create",
            "--username",
            &viewer_username,
            "--role",
            "viewer",
            "--password",
            &viewer_password,
        ],
    );
    let viewer_id = viewer["id"].as_str().expect("viewer id").to_owned();
    cli_json_with_token(
        &server.base_url,
        &admin_access,
        &[
            "member",
            "upsert",
            "--project",
            &project_id,
            "--user",
            &viewer_id,
            "--role",
            "viewer",
        ],
    );

    let developer_access =
        login_access_token(&server.base_url, &developer_username, &developer_password).await;
    let pipeline = cli_json_with_token(
        &server.base_url,
        &developer_access,
        &[
            "pipeline",
            "run",
            "--project",
            &project_id,
            "--git-ref",
            "main",
            "--idempotency-key",
            &Uuid::new_v4().to_string(),
        ],
    );
    assert_eq!(pipeline["pipeline"]["project_id"], project_id);

    let viewer_access =
        login_access_token(&server.base_url, &viewer_username, &viewer_password).await;
    let visible_projects = cli_json_with_token(
        &server.base_url,
        &viewer_access,
        &["project", "list", "--limit", "200", "--offset", "0"],
    );
    assert!(
        visible_projects
            .as_array()
            .expect("viewer project list")
            .iter()
            .any(|item| item["id"] == project_id),
        "viewer membership should allow project visibility: {visible_projects:#}"
    );
    let viewer_denied = cli_failure_with_token(
        &server.base_url,
        &viewer_access,
        &[
            "pipeline",
            "run",
            "--project",
            &project_id,
            "--git-ref",
            "main",
        ],
    );
    assert!(
        viewer_denied.contains("403 Forbidden") || viewer_denied.contains("forbidden"),
        "viewer should be denied write actions, got stderr:\n{viewer_denied}"
    );

    let read_only_token = cli_json_with_token(
        &server.base_url,
        &admin_access,
        &[
            "token",
            "create",
            "--name",
            "read-only-smoke",
            "--user",
            &developer_id,
            "--project",
            &project_id,
            "--scope",
            "api:read",
            "--expires-in-days",
            "7",
        ],
    );
    let read_only_pat = read_only_token["value"]
        .as_str()
        .expect("created PAT value")
        .to_owned();
    let scoped_projects = cli_json_with_token(
        &server.base_url,
        &read_only_pat,
        &["project", "list", "--limit", "200", "--offset", "0"],
    );
    assert_eq!(
        scoped_projects
            .as_array()
            .expect("scoped project list")
            .len(),
        1,
        "project-scoped PAT should only list its project"
    );
    assert_eq!(scoped_projects[0]["id"], project_id);
    let pat_denied = cli_failure_with_token(
        &server.base_url,
        &read_only_pat,
        &[
            "pipeline",
            "run",
            "--project",
            &project_id,
            "--git-ref",
            "main",
        ],
    );
    assert!(
        pat_denied.contains("403 Forbidden") || pat_denied.contains("forbidden"),
        "read-only PAT should be denied write actions, got stderr:\n{pat_denied}"
    );

    server.shutdown().await;
    database.cleanup().await;
}
