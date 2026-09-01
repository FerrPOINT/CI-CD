//! Real-DB integration tests (TEST_PLAN §real-DB level).
//!
//! Requires a running test-compose PostgreSQL:
//!   docker compose -f backend/docker-compose.test.yml up -d postgres-test
//!   CICD_TEST_DATABASE_URL=postgres://forge_owner:...@postgres-test:5432/forge_test_cicd
//! Most rows use an isolated schema-unique UUID namespace. The scheduler and
//! runner dispatch paths intentionally scan global due/queued work, so the CI
//! integration gate runs this file with one test thread.

#![cfg(feature = "integration")]

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use base64::Engine;
use chrono::{Duration, Timelike, Utc};
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;
use uuid::Uuid;

type CanceledExternalLeaseState = (
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<chrono::DateTime<chrono::Utc>>,
    Option<chrono::DateTime<chrono::Utc>>,
);

async fn test_pool() -> sqlx::PgPool {
    let url = std::env::var("CICD_TEST_DATABASE_URL")
        .expect("CICD_TEST_DATABASE_URL must point at the test-compose PostgreSQL");
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .expect("connect to test database");
    // Apply migrations (idempotent; also validates the migration chain).
    cicd::migrations().run(&pool).await.expect("run migrations");
    pool
}

#[tokio::test]
async fn migrations_apply_and_reapply_idempotently() {
    let pool = test_pool().await;
    // Second run must be a no-op.
    cicd::migrations()
        .run(&pool)
        .await
        .expect("re-run migrations idempotently");
}

#[tokio::test]
async fn readiness_reports_database_and_migrations() {
    let pool = test_pool().await;
    let app = cicd::api::app(Some(pool));
    let response = app
        .oneshot(
            Request::get("/api/v1/readiness")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["status"], "ready");
    assert_eq!(body["service"], "cicd");
    assert_eq!(body["database"], "ok");
    assert_eq!(body["migrations"]["status"], "ok");
    assert!(
        body["migrations"]["latest_required_version"]
            .as_i64()
            .unwrap()
            >= 16
    );
    assert_eq!(
        body["migrations"]["latest_applied_version"],
        body["migrations"]["latest_required_version"]
    );
    assert_eq!(
        body["migrations"]["pending_versions"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        body["migrations"]["checksum_mismatches"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

#[tokio::test]
async fn job_log_append_serializes_concurrent_attempt_writes() {
    let pool = test_pool().await;
    let namespace = Uuid::new_v4();
    let project_id = Uuid::new_v4();
    let pipeline_id = Uuid::new_v4();
    let stage_id = Uuid::new_v4();
    let job_id = Uuid::new_v4();
    let attempt_id = Uuid::new_v4();

    sqlx::query("INSERT INTO projects (id, name, repository_url) VALUES ($1, $2, $3)")
        .bind(project_id)
        .bind(format!("it-log-race-{}", namespace.simple()))
        .bind("https://example.invalid/log-race.git")
        .execute(&pool)
        .await
        .expect("insert project");
    sqlx::query(
        "INSERT INTO pipelines (id, project_id, git_ref, status) \
         VALUES ($1, $2, 'main', 'running')",
    )
    .bind(pipeline_id)
    .bind(project_id)
    .execute(&pool)
    .await
    .expect("insert pipeline");
    sqlx::query(
        "INSERT INTO stages (id, pipeline_id, name, position, status) \
         VALUES ($1, $2, 'build', 0, 'running')",
    )
    .bind(stage_id)
    .bind(pipeline_id)
    .execute(&pool)
    .await
    .expect("insert stage");
    sqlx::query(
        "INSERT INTO jobs (id, stage_id, name, image, command, position, status, started_at) \
         VALUES ($1, $2, 'compile', 'alpine:3.21', 'echo test', 0, 'running', now())",
    )
    .bind(job_id)
    .bind(stage_id)
    .execute(&pool)
    .await
    .expect("insert job");
    sqlx::query(
        "INSERT INTO execution_attempts (id, job_id, attempt_no, status, trigger, started_at) \
         VALUES ($1, $2, 1, 'running', 'race-test', now())",
    )
    .bind(attempt_id)
    .bind(job_id)
    .execute(&pool)
    .await
    .expect("insert attempt");

    let first_pool = pool.clone();
    let second_pool = pool.clone();
    let first = tokio::spawn(async move {
        cicd::store::append_job_log(&first_pool, job_id, attempt_id, "stdout").await
    });
    let second = tokio::spawn(async move {
        cicd::store::append_job_log(&second_pool, job_id, attempt_id, "stderr").await
    });
    let mut records = [
        first.await.expect("first task join").expect("first append"),
        second
            .await
            .expect("second task join")
            .expect("second append"),
    ];
    records.sort_by_key(|record| record.sequence);

    assert_eq!(records[0].sequence, 1);
    assert_eq!(records[1].sequence, 2);
    let sequences: Vec<i32> =
        sqlx::query_scalar("SELECT sequence FROM job_logs WHERE attempt_id = $1 ORDER BY sequence")
            .bind(attempt_id)
            .fetch_all(&pool)
            .await
            .expect("fetch log sequences");
    assert_eq!(sequences, vec![1, 2]);

    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project_id)
        .execute(&pool)
        .await
        .expect("cleanup project");
}

#[tokio::test]
async fn project_crud_roundtrip() {
    let pool = test_pool().await;
    let id = Uuid::new_v4();
    let name = format!("it-project-{}", id.simple());

    sqlx::query("INSERT INTO projects (id, name, repository_url) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(&name)
        .bind("https://example.invalid/repo.git")
        .execute(&pool)
        .await
        .expect("insert project");

    let (got_name,): (String,) = sqlx::query_as("SELECT name FROM projects WHERE id = $1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("select project");
    assert_eq!(got_name, name);

    let deleted = sqlx::query_scalar::<_, Uuid>("DELETE FROM projects WHERE id = $1 RETURNING id")
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("delete project");
    assert_eq!(deleted, id);
}

#[tokio::test]
async fn auth_tables_and_sessions_work() {
    let pool = test_pool().await;
    let user_id = Uuid::new_v4();
    let username = format!("it-user-{}", user_id.simple());

    sqlx::query("INSERT INTO users (id, username, role) VALUES ($1, $2, 'admin')")
        .bind(user_id)
        .bind(&username)
        .execute(&pool)
        .await
        .expect("insert user");

    let hash = cicd::auth::hash_password("IntegrationPass1!").expect("hash password");
    sqlx::query("INSERT INTO user_credentials (user_id, password_hash) VALUES ($1, $2)")
        .bind(user_id)
        .bind(&hash)
        .execute(&pool)
        .await
        .expect("insert credential");
    assert!(cicd::auth::verify_password(&hash, "IntegrationPass1!"));

    // Session lifecycle: create -> valid -> rotate revokes old.
    let refresh = cicd::auth::new_refresh_token();
    let stored = cicd::auth::hash_token(&refresh);
    let session_id = cicd::auth::create_session(&pool, user_id, &stored)
        .await
        .expect("create session");
    let su = cicd::auth::session_user(&pool, &stored)
        .await
        .expect("session user");
    assert_eq!(su.user_id, user_id);
    assert!(
        cicd::auth::access_session_user(&pool, session_id, user_id)
            .await
            .is_ok()
    );

    let (_uid, new_session_id, new_token) = cicd::auth::rotate_session(&pool, &stored)
        .await
        .expect("rotate session");
    assert!(cicd::auth::session_user(&pool, &stored).await.is_err());
    assert!(
        cicd::auth::access_session_user(&pool, session_id, user_id)
            .await
            .is_err()
    );
    let new_stored = cicd::auth::hash_token(&new_token);
    assert!(cicd::auth::session_user(&pool, &new_stored).await.is_ok());
    assert!(
        cicd::auth::access_session_user(&pool, new_session_id, user_id)
            .await
            .is_ok()
    );

    let (_uid, second_session_id, second_token) = cicd::auth::rotate_session(&pool, &new_stored)
        .await
        .expect("rotate session a second time");
    assert!(cicd::auth::session_user(&pool, &new_stored).await.is_err());
    let second_stored = cicd::auth::hash_token(&second_token);
    assert!(
        cicd::auth::session_user(&pool, &second_stored)
            .await
            .is_ok()
    );
    assert!(
        cicd::auth::access_session_user(&pool, second_session_id, user_id)
            .await
            .is_ok()
    );

    let revoked = cicd::auth::revoke_session(&pool, &second_stored)
        .await
        .expect("revoke session");
    assert_eq!(revoked, Some(user_id));
    assert!(
        cicd::auth::session_user(&pool, &second_stored)
            .await
            .is_err()
    );
    assert!(
        cicd::auth::access_session_user(&pool, second_session_id, user_id)
            .await
            .is_err()
    );
    let revoked_again = cicd::auth::revoke_session(&pool, &second_stored)
        .await
        .expect("revoke session idempotently");
    assert_eq!(revoked_again, None);

    let disabled_refresh = cicd::auth::new_refresh_token();
    let disabled_stored = cicd::auth::hash_token(&disabled_refresh);
    cicd::auth::create_session(&pool, user_id, &disabled_stored)
        .await
        .expect("create live session before disabling user");

    // Disabled user cannot authenticate even with a live session.
    sqlx::query("UPDATE users SET enabled = false WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("disable user");
    assert!(
        cicd::auth::session_user(&pool, &disabled_stored)
            .await
            .is_err()
    );

    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("cleanup user");
}

#[tokio::test]
async fn auth_logout_endpoint_revokes_refresh_session() {
    let pool = test_pool().await;
    let user_id = Uuid::new_v4();
    let username = format!("it-logout-user-{}", user_id.simple());
    let refresh = cicd::auth::new_refresh_token();
    let stored = cicd::auth::hash_token(&refresh);

    sqlx::query("INSERT INTO users (id, username, role) VALUES ($1, $2, 'admin')")
        .bind(user_id)
        .bind(&username)
        .execute(&pool)
        .await
        .expect("insert user");
    cicd::auth::create_session(&pool, user_id, &stored)
        .await
        .expect("create session");

    let app = cicd::api::app(Some(pool.clone()));
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/auth/logout")
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"refresh_token":"{refresh}"}}"#)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["revoked"], true);
    assert!(cicd::auth::session_user(&pool, &stored).await.is_err());

    let replay = app
        .oneshot(
            Request::post("/api/v1/auth/logout")
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"refresh_token":"{refresh}"}}"#)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(replay.status(), StatusCode::OK);
    let body = response_json(replay).await;
    assert_eq!(body["revoked"], false);

    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("cleanup user");
}

#[tokio::test]
async fn access_token_is_bound_to_active_session() {
    let pool = test_pool().await;
    let user_id = Uuid::new_v4();
    let username = format!("it-session-bound-user-{}", user_id.simple());
    let password = "IntegrationPass1!";
    let password_hash = cicd::auth::hash_password(password).expect("hash password");

    sqlx::query("INSERT INTO users (id, username, role) VALUES ($1, $2, 'admin')")
        .bind(user_id)
        .bind(&username)
        .execute(&pool)
        .await
        .expect("insert user");
    sqlx::query("INSERT INTO user_credentials (user_id, password_hash) VALUES ($1, $2)")
        .bind(user_id)
        .bind(&password_hash)
        .execute(&pool)
        .await
        .expect("insert credential");

    let app = cicd::api::app_with_auth_secret(
        Some(pool.clone()),
        Some(format!("integration-secret-{user_id}")),
    );
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"username":"{username}","password":"{password}"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    let access_token = body["access_token"]
        .as_str()
        .expect("access token")
        .to_owned();
    let refresh_token = body["refresh_token"]
        .as_str()
        .expect("refresh token")
        .to_owned();

    let response = app
        .clone()
        .oneshot(
            Request::get("/api/v1/users")
                .header("authorization", format!("Bearer {access_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    sqlx::query("UPDATE users SET role = 'viewer' WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("downgrade user role");
    let response = app
        .clone()
        .oneshot(
            Request::get("/api/v1/users")
                .header("authorization", format!("Bearer {access_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/auth/logout")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"refresh_token":"{refresh_token}"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .oneshot(
            Request::get("/api/v1/users")
                .header("authorization", format!("Bearer {access_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("cleanup user");
}

#[tokio::test]
async fn scoped_api_tokens_limit_project_routes_and_soft_revoke() {
    let pool = test_pool().await;
    let namespace = Uuid::new_v4();
    let admin_id = Uuid::new_v4();
    let username = format!("it-token-admin-{}", admin_id.simple());
    let password = "IntegrationPass1!";
    let project_a = Uuid::new_v4();
    let project_b = Uuid::new_v4();
    let repo_a = format!("it-token-a-{}", namespace.simple());
    let repo_b = format!("it-token-b-{}", namespace.simple());

    sqlx::query("INSERT INTO users (id, username, role) VALUES ($1, $2, 'admin')")
        .bind(admin_id)
        .bind(&username)
        .execute(&pool)
        .await
        .expect("insert token admin");
    sqlx::query("INSERT INTO user_credentials (user_id, password_hash) VALUES ($1, $2)")
        .bind(admin_id)
        .bind(cicd::auth::hash_password(password).expect("hash password"))
        .execute(&pool)
        .await
        .expect("insert token admin credential");
    sqlx::query(
        "INSERT INTO projects (id, name, repository_url) VALUES \
         ($1, $2, $3), ($4, $5, $6)",
    )
    .bind(project_a)
    .bind(format!("it-token-project-a-{}", namespace.simple()))
    .bind(format!("http://127.0.0.1:22802/git/{repo_a}.git"))
    .bind(project_b)
    .bind(format!("it-token-project-b-{}", namespace.simple()))
    .bind(format!("http://127.0.0.1:22802/git/{repo_b}.git"))
    .execute(&pool)
    .await
    .expect("insert scoped token projects");

    let app = cicd::api::app_with_auth_secret(
        Some(pool.clone()),
        Some(format!("scoped-token-secret-{namespace}")),
    );
    let admin_access = login_access_token(app.clone(), &username, password).await;

    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/api-tokens")
                .header("authorization", format!("Bearer {admin_access}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"missing-project"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/api-tokens")
                .header("authorization", format!("Bearer {admin_access}"))
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"name":"project-a-read","project_id":"{project_a}","scopes":["api:read"],"expires_in_days":7}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let created = response_json(response).await;
    assert_eq!(created["project_id"], project_a.to_string());
    assert_eq!(created["scopes"][0], "api:read");
    assert!(created["expires_at"].is_string());
    assert!(created["revoked_at"].is_null());
    let token_id = created["id"].as_str().expect("token id").to_string();
    let pat = created["value"].as_str().expect("token value").to_string();

    let response = app
        .clone()
        .oneshot(
            Request::get("/api/v1/projects")
                .header("authorization", format!("Bearer {pat}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let projects = response_json(response).await;
    assert_eq!(projects.as_array().expect("projects array").len(), 1);
    assert_eq!(projects[0]["id"], project_a.to_string());

    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/projects/{project_a}"))
                .header("authorization", format!("Bearer {pat}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/projects/{project_b}"))
                .header("authorization", format!("Bearer {pat}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = app
        .clone()
        .oneshot(
            Request::patch(format!("/api/v1/projects/{project_a}"))
                .header("authorization", format!("Bearer {pat}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"default_branch":"release"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/repos/{repo_b}/refs"))
                .header("authorization", format!("Bearer {pat}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = app
        .clone()
        .oneshot(
            Request::delete(format!("/api/v1/api-tokens/{token_id}"))
                .header("authorization", format!("Bearer {admin_access}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .oneshot(
            Request::get(format!("/api/v1/projects/{project_a}"))
                .header("authorization", format!("Bearer {pat}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(admin_id)
        .execute(&pool)
        .await
        .expect("cleanup token admin");
    sqlx::query("DELETE FROM projects WHERE id = ANY($1)")
        .bind([project_a, project_b])
        .execute(&pool)
        .await
        .expect("cleanup scoped token projects");
}

#[tokio::test]
async fn external_runner_protocol_claims_acknowledges_renews_and_completes_job() {
    let pool = test_pool().await;
    sqlx::query("DELETE FROM projects WHERE name LIKE 'it-runner-protocol-%'")
        .execute(&pool)
        .await
        .expect("cleanup stale runner protocol projects");

    let namespace = Uuid::new_v4();
    let registration_token = format!("registration-{}", namespace.simple());
    let project_id = Uuid::new_v4();
    let pipeline_id = Uuid::new_v4();
    let stage_id = Uuid::new_v4();
    let job_id = Uuid::new_v4();
    let attempt_id = Uuid::new_v4();
    let project_name = format!("it-runner-protocol-{}", namespace.simple());
    let previous_secrets_key = std::env::var("CICD_SECRETS_KEY").ok();
    let previous_artifacts_dir = std::env::var("CICD_ARTIFACTS_DIR").ok();
    let secrets_key = base64::engine::general_purpose::STANDARD.encode([7_u8; 32]);
    let artifact_root = std::env::temp_dir().join(format!("forge-runner-artifacts-{namespace}"));
    std::fs::create_dir_all(&artifact_root).expect("create runner artifact root");

    // SAFETY: only the runner protocol integration test reads this process env.
    unsafe {
        std::env::set_var("CICD_RUNNER_REGISTRATION_TOKEN", &registration_token);
        std::env::set_var("CICD_SECRETS_KEY", secrets_key);
        std::env::set_var("CICD_ARTIFACTS_DIR", &artifact_root);
    }

    let app = cicd::api::app_with_auth_secret(
        Some(pool.clone()),
        Some(format!("runner-protocol-secret-{namespace}")),
    );

    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/runner/register")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::json!({
                    "protocolVersion": 1,
                    "registrationToken": "wrong-token",
                    "name": format!("runner-wrong-{}", namespace.simple()),
                    "tags": ["linux", "docker"],
                    "capabilities": {"executorKinds": ["docker"], "os": "linux", "arch": "amd64"}
                }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/runner/register")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::json!({
                    "protocolVersion": 1,
                    "registrationToken": registration_token,
                    "name": format!("runner-{}", namespace.simple()),
                    "tags": ["linux", "docker"],
                    "capabilities": {"executorKinds": ["docker"], "os": "linux", "arch": "amd64"}
                }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let registered = response_json(response).await;
    let runner_id = Uuid::parse_str(registered["runnerId"].as_str().unwrap()).unwrap();
    let credential = registered["credential"].as_str().unwrap().to_owned();
    assert!(credential.starts_with("cicd_runner_"));
    assert_eq!(registered["protocolVersion"], 1);
    assert_eq!(registered["pollWaitMaxSeconds"], 30);

    let (stored_hash, token_hint): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT credential_hash, token_hint FROM runners WHERE id = $1")
            .bind(runner_id)
            .fetch_one(&pool)
            .await
            .expect("fetch runner credential metadata");
    assert!(stored_hash.is_some());
    assert_ne!(stored_hash.as_deref(), Some(credential.as_str()));
    assert!(
        token_hint
            .as_deref()
            .is_some_and(|hint| hint.contains("..."))
    );

    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/runner/heartbeat")
                .header("authorization", format!("Bearer {credential}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::json!({
                    "protocolVersion": 1,
                    "status": "online",
                    "capacity": {"totalSlots": 2, "busySlots": 0},
                    "tags": ["linux", "docker"],
                    "capabilities": {"executorKinds": ["docker"], "os": "linux", "arch": "amd64"},
                    "activeLeaseIds": []
                }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let (runner_status, draining, total_slots, busy_slots): (
        String,
        bool,
        Option<i32>,
        Option<i32>,
    ) = sqlx::query_as(
        "SELECT status, draining, capacity_total_slots, capacity_busy_slots FROM runners WHERE id = $1",
    )
    .bind(runner_id)
    .fetch_one(&pool)
    .await
    .expect("fetch runner heartbeat state");
    assert_eq!(runner_status, "online");
    assert!(!draining);
    assert_eq!(total_slots, Some(2));
    assert_eq!(busy_slots, Some(0));

    sqlx::query("INSERT INTO projects (id, name, repository_url) VALUES ($1, $2, $3)")
        .bind(project_id)
        .bind(&project_name)
        .bind("https://example.invalid/external-runner.git")
        .execute(&pool)
        .await
        .expect("insert project");
    let secret_app = cicd::api::app(Some(pool.clone()));
    for (key, value) in [
        ("DEPLOY_TOKEN", "super-secret-token"),
        ("OTHER_SECRET", "do-not-release"),
    ] {
        let response = secret_app
            .clone()
            .oneshot(
                Request::post(format!("/api/v1/projects/{project_id}/secrets"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "key": key,
                            "value": value
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    sqlx::query(
        "INSERT INTO pipelines (id, project_id, git_ref, status, created_at) \
         VALUES ($1, $2, 'main', 'queued', '2000-01-01T00:00:00Z')",
    )
    .bind(pipeline_id)
    .bind(project_id)
    .execute(&pool)
    .await
    .expect("insert pipeline");
    sqlx::query(
        "INSERT INTO stages (id, pipeline_id, name, position, status) \
         VALUES ($1, $2, 'build', 0, 'queued')",
    )
    .bind(stage_id)
    .bind(pipeline_id)
    .execute(&pool)
    .await
    .expect("insert stage");
    sqlx::query(
        "INSERT INTO jobs (id, stage_id, name, image, command, required_tags, required_secrets, artifact_paths, position, status, timeout_seconds) \
         VALUES ($1, $2, 'compile', 'alpine:3.21', 'echo ok', ARRAY['docker','linux'], ARRAY['DEPLOY_TOKEN'], ARRAY['target/release/app.tar.gz'], 0, 'queued', 30)",
    )
    .bind(job_id)
    .bind(stage_id)
    .execute(&pool)
    .await
    .expect("insert job");
    sqlx::query(
        "INSERT INTO execution_attempts (id, job_id, attempt_no, status, trigger) \
         VALUES ($1, $2, 1, 'queued', 'initial')",
    )
    .bind(attempt_id)
    .bind(job_id)
    .execute(&pool)
    .await
    .expect("insert attempt");
    sqlx::query(
        "INSERT INTO job_queue (id, job_id, attempt_id, pipeline_id, stage_id, state, priority, required_tags) \
         VALUES ($1, $2, $3, $4, $5, 'queued', 100, ARRAY['docker','linux'])",
    )
    .bind(Uuid::new_v4())
    .bind(job_id)
    .bind(attempt_id)
    .bind(pipeline_id)
    .bind(stage_id)
    .execute(&pool)
    .await
    .expect("insert protocol queue row");

    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/runner/work:poll")
                .header("authorization", format!("Bearer {credential}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "protocolVersion": 1,
                        "capacity": {"freeSlots": 1},
                        "tags": ["linux"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let queue_still_waiting: String =
        sqlx::query_scalar("SELECT state FROM job_queue WHERE attempt_id = $1")
            .bind(attempt_id)
            .fetch_one(&pool)
            .await
            .expect("fetch queue state after incompatible poll");
    assert_eq!(queue_still_waiting, "queued");

    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/runner/work:poll")
                .header("authorization", format!("Bearer {credential}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "protocolVersion": 1,
                        "capacity": {"freeSlots": 1},
                        "tags": ["linux", "docker"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let queue_still_waiting: String =
        sqlx::query_scalar("SELECT state FROM job_queue WHERE attempt_id = $1")
            .bind(attempt_id)
            .fetch_one(&pool)
            .await
            .expect("fetch queue state after incompatible executor poll");
    assert_eq!(queue_still_waiting, "queued");

    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/runner/heartbeat")
                .header("authorization", format!("Bearer {credential}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "protocolVersion": 1,
                        "status": "online",
                        "capacity": {"totalSlots": 2, "busySlots": 0},
                        "tags": ["linux", "docker"],
                        "capabilities": {"executorKinds": ["shell"], "os": "linux", "arch": "amd64"},
                        "activeLeaseIds": []
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/runner/work:poll")
                .header("authorization", format!("Bearer {credential}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "protocolVersion": 1,
                        "capacity": {"freeSlots": 1},
                        "tags": ["linux", "docker"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let offer = response_json(response).await;
    assert_eq!(offer["protocolVersion"], 1);
    assert_eq!(offer["fencingToken"], 1);
    assert_eq!(offer["attempt"]["id"], attempt_id.to_string());
    assert_eq!(offer["attempt"]["jobId"], job_id.to_string());
    assert_eq!(offer["attempt"]["jobKey"], "compile");
    assert_eq!(offer["attempt"]["executor"], "shell");
    assert_eq!(offer["attempt"]["commands"][0], "echo ok");
    assert_eq!(
        offer["attempt"]["secrets"],
        serde_json::json!(["DEPLOY_TOKEN"])
    );
    assert_eq!(
        offer["attempt"]["artifacts"],
        serde_json::json!(["target/release/app.tar.gz"])
    );
    assert_eq!(offer["attempt"]["timeoutSeconds"], 30);
    assert_eq!(offer["attempt"]["workspace"]["checkout"], true);
    assert_eq!(
        offer["attempt"]["workspace"]["checkoutUrl"],
        "https://example.invalid/external-runner.git"
    );
    let lease_id = Uuid::parse_str(offer["leaseId"].as_str().unwrap()).unwrap();
    let lease_token = offer["leaseToken"].as_str().unwrap().to_owned();
    let runner_artifact_body = b"runner artifact bytes".to_vec();

    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/runner/leases/{lease_id}/secrets:resolve"))
                .header("authorization", format!("Bearer {credential}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "protocolVersion": 1,
                        "leaseToken": lease_token,
                        "fencingToken": 1,
                        "secretNames": ["DEPLOY_TOKEN"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);

    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/runner/leases/{lease_id}/complete"))
                .header("authorization", format!("Bearer {credential}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "protocolVersion": 1,
                        "leaseToken": lease_token,
                        "fencingToken": 1,
                        "attemptId": attempt_id,
                        "outcome": "success",
                        "finishedAt": chrono::Utc::now()
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);

    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/runner/leases/{lease_id}/artifacts"))
                .header("authorization", format!("Bearer {credential}"))
                .header("x-runner-protocol-version", "1")
                .header("x-lease-token", &lease_token)
                .header("x-fencing-token", "1")
                .header("x-attempt-id", attempt_id.to_string())
                .header("x-artifact-path", "target/release/app.tar.gz")
                .header("x-artifact-name", "runner-report.txt")
                .header("content-type", "text/plain")
                .body(Body::from(runner_artifact_body.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);

    let (
        job_status,
        attempt_status,
        pipeline_status,
        lease_runner,
        lease_hash_present,
        queue_state,
        queue_lease_matches,
    ): (String, String, String, Option<Uuid>, bool, String, bool) = sqlx::query_as(
        "SELECT j.status, a.status, p.status, l.runner_id, l.lease_token_hash IS NOT NULL, \
                q.state, q.lease_id = l.id \
         FROM jobs j \
         JOIN execution_attempts a ON a.job_id = j.id \
         JOIN job_leases l ON l.attempt_id = a.id \
         JOIN job_queue q ON q.attempt_id = a.id \
         JOIN stages s ON s.id = j.stage_id \
         JOIN pipelines p ON p.id = s.pipeline_id \
         WHERE j.id = $1",
    )
    .bind(job_id)
    .fetch_one(&pool)
    .await
    .expect("fetch claimed state");
    assert_eq!(job_status, "running");
    assert_eq!(attempt_status, "running");
    assert_eq!(pipeline_status, "running");
    assert_eq!(lease_runner, Some(runner_id));
    assert!(lease_hash_present);
    assert_eq!(queue_state, "leased");
    assert!(queue_lease_matches);

    for endpoint in ["ack", "renew"] {
        let response = app
            .clone()
            .oneshot(
                Request::post(format!("/api/v1/runner/leases/{lease_id}/{endpoint}"))
                    .header("authorization", format!("Bearer {credential}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "protocolVersion": 1,
                            "leaseToken": lease_token,
                            "fencingToken": 1
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["protocolVersion"], 1);
        assert_eq!(body["cancelRequested"], false);
    }

    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/runner/leases/{lease_id}/control"))
                .header("authorization", format!("Bearer {credential}"))
                .header("x-runner-protocol-version", "1")
                .header("x-lease-token", &lease_token)
                .header("x-fencing-token", "1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let control = response_json(response).await;
    assert_eq!(control["protocolVersion"], 1);
    assert_eq!(control["cancelRequested"], false);

    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/runner/leases/{lease_id}/secrets:resolve"))
                .header("authorization", format!("Bearer {credential}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "protocolVersion": 1,
                        "leaseToken": lease_token,
                        "fencingToken": 1,
                        "secretNames": ["OTHER_SECRET"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/runner/leases/{lease_id}/secrets:resolve"))
                .header("authorization", format!("Bearer {credential}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "protocolVersion": 1,
                        "leaseToken": lease_token,
                        "fencingToken": 1,
                        "secretNames": [" DEPLOY_TOKEN ", "DEPLOY_TOKEN"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    let resolved_secrets = response_json(response).await;
    assert_eq!(resolved_secrets["protocolVersion"], 1);
    assert!(resolved_secrets["expiresAt"].as_str().is_some());
    assert_eq!(
        resolved_secrets["items"],
        serde_json::json!([{
            "name": "DEPLOY_TOKEN",
            "injection": "env",
            "value": "super-secret-token"
        }])
    );

    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/runner/leases/{lease_id}/logs"))
                .header("authorization", format!("Bearer {credential}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "protocolVersion": 1,
                        "leaseToken": lease_token,
                        "fencingToken": 1,
                        "attemptId": attempt_id,
                        "lines": [
                            {"stream": "stdout", "message": "compile started\n"},
                            {"stream": "stderr", "message": "warning: cached dependency"}
                        ]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let appended_logs = response_json(response).await;
    assert_eq!(appended_logs["accepted"], 2);
    assert_eq!(appended_logs["nextAfter"], 2);

    let log_messages: Vec<String> =
        sqlx::query_scalar("SELECT message FROM job_logs WHERE attempt_id = $1 ORDER BY sequence")
            .bind(attempt_id)
            .fetch_all(&pool)
            .await
            .expect("fetch runner protocol logs");
    assert_eq!(
        log_messages,
        vec![
            "[stdout] compile started".to_string(),
            "[stderr] warning: cached dependency".to_string()
        ]
    );

    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/runner/leases/{lease_id}/artifacts"))
                .header("authorization", format!("Bearer {credential}"))
                .header("x-runner-protocol-version", "1")
                .header("x-lease-token", &lease_token)
                .header("x-fencing-token", "1")
                .header("x-attempt-id", attempt_id.to_string())
                .header("x-artifact-path", "reports/undeclared.txt")
                .header("x-artifact-name", "runner-report.txt")
                .header("content-type", "text/plain")
                .body(Body::from(runner_artifact_body.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/runner/leases/{lease_id}/artifacts"))
                .header("authorization", format!("Bearer {credential}"))
                .header("x-runner-protocol-version", "1")
                .header("x-lease-token", &lease_token)
                .header("x-fencing-token", "1")
                .header("x-attempt-id", attempt_id.to_string())
                .header("x-artifact-path", "target/release/app.tar.gz")
                .header("x-artifact-name", "runner-report.txt")
                .header("content-type", "text/plain")
                .body(Body::from(runner_artifact_body.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let uploaded_artifact = response_json(response).await;
    let artifact_id = Uuid::parse_str(uploaded_artifact["id"].as_str().unwrap()).unwrap();
    assert_eq!(uploaded_artifact["job_id"], job_id.to_string());
    assert_eq!(uploaded_artifact["attempt_id"], attempt_id.to_string());
    assert_eq!(uploaded_artifact["name"], "runner-report.txt");
    assert_eq!(uploaded_artifact["content_type"], "text/plain");
    assert_eq!(
        uploaded_artifact["size_bytes"],
        runner_artifact_body.len() as i64
    );
    assert_eq!(
        uploaded_artifact["sha256"],
        format!("{:x}", Sha256::digest(&runner_artifact_body))
    );

    let response = secret_app
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/artifacts/{artifact_id}/download"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read runner artifact body");
    assert_eq!(bytes.as_ref(), runner_artifact_body.as_slice());

    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/runner/leases/{lease_id}/renew"))
                .header("authorization", format!("Bearer {credential}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "protocolVersion": 1,
                        "leaseToken": lease_token,
                        "fencingToken": 2
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);

    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/runner/leases/{lease_id}/complete"))
                .header("authorization", format!("Bearer {credential}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "protocolVersion": 1,
                        "leaseToken": lease_token,
                        "fencingToken": 1,
                        "attemptId": attempt_id,
                        "outcome": "success",
                        "exitCode": 0,
                        "finishedAt": Utc::now()
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let completed = response_json(response).await;
    assert_eq!(completed["accepted"], true);
    assert_eq!(completed["terminalStatus"], "success");

    let (
        job_status,
        attempt_status,
        lease_status,
        terminal_status,
        pipeline_status,
        queue_state,
        queue_completed_at,
    ): (
        String,
        String,
        String,
        Option<String>,
        String,
        String,
        Option<chrono::DateTime<chrono::Utc>>,
    ) = sqlx::query_as(
        "SELECT j.status, a.status, l.lease_status, l.terminal_status, p.status, \
                q.state, q.completed_at \
         FROM jobs j \
         JOIN execution_attempts a ON a.job_id = j.id \
         JOIN job_leases l ON l.attempt_id = a.id \
         JOIN job_queue q ON q.attempt_id = a.id \
         JOIN stages s ON s.id = j.stage_id \
         JOIN pipelines p ON p.id = s.pipeline_id \
         WHERE j.id = $1",
    )
    .bind(job_id)
    .fetch_one(&pool)
    .await
    .expect("fetch completed state");
    assert_eq!(job_status, "success");
    assert_eq!(attempt_status, "success");
    assert_eq!(lease_status, "completed");
    assert_eq!(terminal_status.as_deref(), Some("success"));
    assert_eq!(pipeline_status, "success");
    assert_eq!(queue_state, "completed");
    assert!(queue_completed_at.is_some());

    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/runner/leases/{lease_id}/logs"))
                .header("authorization", format!("Bearer {credential}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "protocolVersion": 1,
                        "leaseToken": lease_token,
                        "fencingToken": 1,
                        "attemptId": attempt_id,
                        "lines": [{"stream": "stdout", "message": "too late"}]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);

    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project_id)
        .execute(&pool)
        .await
        .expect("cleanup protocol project");
    sqlx::query("DELETE FROM runners WHERE id = $1")
        .bind(runner_id)
        .execute(&pool)
        .await
        .expect("cleanup protocol runner");
    unsafe {
        std::env::remove_var("CICD_RUNNER_REGISTRATION_TOKEN");
        match previous_secrets_key {
            Some(value) => std::env::set_var("CICD_SECRETS_KEY", value),
            None => std::env::remove_var("CICD_SECRETS_KEY"),
        }
        match previous_artifacts_dir {
            Some(value) => std::env::set_var("CICD_ARTIFACTS_DIR", value),
            None => std::env::remove_var("CICD_ARTIFACTS_DIR"),
        }
    }
    let _ = std::fs::remove_dir_all(&artifact_root);
}

#[tokio::test]
async fn external_runner_long_poll_wakes_when_work_is_enqueued() {
    let pool = test_pool().await;
    let namespace = Uuid::new_v4();
    let runner_id = Uuid::new_v4();
    let credential = format!("credential-{}", namespace.simple());
    let project_id = Uuid::new_v4();
    let pipeline_id = Uuid::new_v4();
    let stage_id = Uuid::new_v4();
    let job_id = Uuid::new_v4();
    let attempt_id = Uuid::new_v4();

    sqlx::query(
        "INSERT INTO runners \
         (id, name, tags, status, last_seen_at, credential_hash, credential_expires_at, capabilities, heartbeat_payload) \
         VALUES ($1, $2, ARRAY['linux'], 'online', now(), $3, now() + interval '1 day', $4, '{}'::jsonb)",
    )
    .bind(runner_id)
    .bind(format!("runner-long-poll-{}", namespace.simple()))
    .bind(cicd::auth::hash_token(&credential))
    .bind(serde_json::json!({"executorKinds": ["shell"]}))
    .execute(&pool)
    .await
    .expect("insert runner");

    sqlx::query("INSERT INTO projects (id, name, repository_url) VALUES ($1, $2, $3)")
        .bind(project_id)
        .bind(format!("it-runner-long-poll-{}", namespace.simple()))
        .bind("https://example.invalid/long-poll.git")
        .execute(&pool)
        .await
        .expect("insert project");
    sqlx::query(
        "INSERT INTO pipelines (id, project_id, git_ref, status) VALUES ($1, $2, 'main', 'queued')",
    )
    .bind(pipeline_id)
    .bind(project_id)
    .execute(&pool)
    .await
    .expect("insert pipeline");
    sqlx::query(
        "INSERT INTO stages (id, pipeline_id, name, position, status) \
         VALUES ($1, $2, 'build', 0, 'queued')",
    )
    .bind(stage_id)
    .bind(pipeline_id)
    .execute(&pool)
    .await
    .expect("insert stage");
    sqlx::query(
        "INSERT INTO jobs \
         (id, stage_id, name, image, command, required_tags, required_secrets, artifact_paths, position, status, timeout_seconds, manual) \
         VALUES ($1, $2, 'compile', 'alpine:3.21', 'echo ok', ARRAY['linux'], ARRAY[]::text[], ARRAY[]::text[], 0, 'queued', 30, true)",
    )
    .bind(job_id)
    .bind(stage_id)
    .execute(&pool)
    .await
    .expect("insert initially manual job");
    sqlx::query(
        "INSERT INTO execution_attempts (id, job_id, attempt_no, status, trigger) \
         VALUES ($1, $2, 1, 'queued', 'initial')",
    )
    .bind(attempt_id)
    .bind(job_id)
    .execute(&pool)
    .await
    .expect("insert attempt");

    let app = cicd::api::app_with_auth_secret(
        Some(pool.clone()),
        Some(format!("runner-long-poll-secret-{namespace}")),
    );
    let poll_app = app.clone();
    let poll_credential = credential.clone();
    let mut poll = tokio::spawn(async move {
        poll_app
            .oneshot(
                Request::post("/api/v1/runner/work:poll")
                    .header("authorization", format!("Bearer {poll_credential}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "protocolVersion": 1,
                            "capacity": {"freeSlots": 1},
                            "waitSeconds": 5,
                            "tags": ["linux"]
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap()
    });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(
        !poll.is_finished(),
        "poll returned before work was enqueued"
    );

    sqlx::query("UPDATE jobs SET manual = false WHERE id = $1")
        .bind(job_id)
        .execute(&pool)
        .await
        .expect("release manual gate");
    let enqueued = cicd::store::enqueue_job_attempt(&pool, job_id, attempt_id)
        .await
        .expect("enqueue released job");
    assert_eq!(enqueued, 1);

    let response = tokio::time::timeout(std::time::Duration::from_secs(2), &mut poll)
        .await
        .expect("long poll should wake after enqueue")
        .expect("poll task should complete");
    assert_eq!(response.status(), StatusCode::OK);
    let offer = response_json(response).await;
    assert_eq!(offer["attempt"]["id"], attempt_id.to_string());
    assert_eq!(offer["attempt"]["jobId"], job_id.to_string());

    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project_id)
        .execute(&pool)
        .await
        .expect("cleanup long poll project");
    sqlx::query("DELETE FROM runners WHERE id = $1")
        .bind(runner_id)
        .execute(&pool)
        .await
        .expect("cleanup long poll runner");
}

#[tokio::test]
async fn git_smart_http_uses_project_membership_when_auth_enabled() {
    let pool = test_pool().await;
    let namespace = Uuid::new_v4();
    let private_repo = format!("it_private_{}", namespace.simple());
    let lookalike_repo = private_repo.replace('_', "x");
    let public_repo = format!("it-public-{}", namespace.simple());
    let root = std::env::temp_dir().join(format!("forge-git-auth-{namespace}"));
    tokio::fs::create_dir_all(&root)
        .await
        .expect("create git root");
    for repo in [&private_repo, &public_repo] {
        let status = tokio::process::Command::new("git")
            .args(["init", "--bare", "--quiet"])
            .arg(root.join(format!("{repo}.git")))
            .status()
            .await
            .expect("init bare repo");
        assert!(status.success());
    }

    sqlx::query(
        "INSERT INTO repositories (id, name, visibility) VALUES \
         ($1, $2, 'private'), ($3, $4, 'public')",
    )
    .bind(Uuid::new_v4())
    .bind(&private_repo)
    .bind(Uuid::new_v4())
    .bind(&public_repo)
    .execute(&pool)
    .await
    .expect("insert repositories");
    let project_id = Uuid::new_v4();
    sqlx::query("INSERT INTO projects (id, name, repository_url) VALUES ($1, $2, $3)")
        .bind(project_id)
        .bind(format!("it-git-project-{}", namespace.simple()))
        .bind(format!("http://127.0.0.1:22802/git/{private_repo}.git"))
        .execute(&pool)
        .await
        .expect("insert project");
    let lookalike_project_id = Uuid::new_v4();
    sqlx::query("INSERT INTO projects (id, name, repository_url) VALUES ($1, $2, $3)")
        .bind(lookalike_project_id)
        .bind(format!("it-git-lookalike-{}", namespace.simple()))
        .bind(format!("http://127.0.0.1:22802/git/{lookalike_repo}.git"))
        .execute(&pool)
        .await
        .expect("insert lookalike project");

    let developer_id = Uuid::new_v4();
    let viewer_id = Uuid::new_v4();
    let outsider_id = Uuid::new_v4();
    let password = "IntegrationPass1!";
    for (user_id, username, role, membership_project_id) in [
        (
            developer_id,
            format!("it-git-dev-{}", developer_id.simple()),
            "developer",
            project_id,
        ),
        (
            viewer_id,
            format!("it-git-viewer-{}", viewer_id.simple()),
            "viewer",
            project_id,
        ),
        (
            outsider_id,
            format!("it-git-outsider-{}", outsider_id.simple()),
            "developer",
            lookalike_project_id,
        ),
    ] {
        sqlx::query("INSERT INTO users (id, username, role) VALUES ($1, $2, $3)")
            .bind(user_id)
            .bind(&username)
            .bind(role)
            .execute(&pool)
            .await
            .expect("insert git user");
        sqlx::query("INSERT INTO user_credentials (user_id, password_hash) VALUES ($1, $2)")
            .bind(user_id)
            .bind(cicd::auth::hash_password(password).expect("hash password"))
            .execute(&pool)
            .await
            .expect("insert git credential");
        sqlx::query(
            "INSERT INTO project_memberships (project_id, user_id, role) VALUES ($1, $2, $3)",
        )
        .bind(membership_project_id)
        .bind(user_id)
        .bind(role)
        .execute(&pool)
        .await
        .expect("insert git membership");
    }

    let app = cicd::api::app_with_git_and_auth_secret(
        Some(pool.clone()),
        cicd::git_host::GitConfig {
            root: root.clone(),
            token: None,
            internal_token: None,
        },
        Some(format!("git-auth-secret-{namespace}")),
    );
    let developer_access = login_access_token(
        app.clone(),
        &format!("it-git-dev-{}", developer_id.simple()),
        password,
    )
    .await;
    let viewer_access = login_access_token(
        app.clone(),
        &format!("it-git-viewer-{}", viewer_id.simple()),
        password,
    )
    .await;
    let outsider_access = login_access_token(
        app.clone(),
        &format!("it-git-outsider-{}", outsider_id.simple()),
        password,
    )
    .await;
    let read_only_pat = format!(
        "cicd_{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    );
    sqlx::query(
        "INSERT INTO api_tokens \
         (id, name, token_hash, token_hint, user_id, project_id, scopes, expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, now() + interval '1 day')",
    )
    .bind(Uuid::new_v4())
    .bind("git-read-only")
    .bind(cicd::auth::hash_token(&read_only_pat))
    .bind("cicd_pat...test")
    .bind(developer_id)
    .bind(project_id)
    .bind(vec!["git:read".to_string()])
    .execute(&pool)
    .await
    .expect("insert read-only PAT");
    let viewer_basic =
        base64::engine::general_purpose::STANDARD.encode(format!("git:{viewer_access}"));
    let developer_basic =
        base64::engine::general_purpose::STANDARD.encode(format!("git:{developer_access}"));

    let response = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/git/{private_repo}.git/info/refs?service=git-upload-pack"
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/git/{public_repo}.git/info/refs?service=git-upload-pack"
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/git/{public_repo}.git/info/refs?service=git-receive-pack"
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/git/{private_repo}.git/info/refs?service=git-upload-pack"
            ))
            .header("authorization", format!("Basic {viewer_basic}"))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/git/{private_repo}.git/info/refs?service=git-upload-pack"
            ))
            .header("authorization", format!("Bearer {viewer_access}"))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/git/{private_repo}.git/info/refs?service=git-receive-pack"
            ))
            .header("authorization", format!("Bearer {outsider_access}"))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/git/{private_repo}.git/info/refs?service=git-receive-pack"
            ))
            .header("authorization", format!("Basic {viewer_basic}"))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/git/{private_repo}.git/info/refs?service=git-upload-pack"
            ))
            .header("authorization", format!("Bearer {read_only_pat}"))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/git/{private_repo}.git/info/refs?service=git-receive-pack"
            ))
            .header("authorization", format!("Bearer {read_only_pat}"))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = app
        .oneshot(
            Request::get(format!(
                "/git/{private_repo}.git/info/refs?service=git-receive-pack"
            ))
            .header("authorization", format!("Basic {developer_basic}"))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let legacy_token = format!("legacy-git-token-{namespace}");
    let legacy_app = cicd::api::app_with_git_and_auth_secret(
        Some(pool.clone()),
        cicd::git_host::GitConfig {
            root: root.clone(),
            token: Some(legacy_token.clone()),
            internal_token: None,
        },
        Some(format!("git-auth-secret-{namespace}")),
    );
    let response = legacy_app
        .oneshot(
            Request::get(format!(
                "/git/{private_repo}.git/info/refs?service=git-receive-pack"
            ))
            .header("x-git-token", legacy_token)
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let trusted_app = cicd::api::app_with_git_and_auth_secret(
        Some(pool.clone()),
        cicd::git_host::GitConfig {
            root: root.clone(),
            token: None,
            internal_token: None,
        },
        None,
    );
    let response = trusted_app
        .oneshot(
            Request::get("/git/missing-repo.git/info/refs?service=git-upload-pack")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    sqlx::query("DELETE FROM users WHERE id = ANY($1)")
        .bind([developer_id, viewer_id, outsider_id])
        .execute(&pool)
        .await
        .expect("cleanup git users");
    sqlx::query("DELETE FROM projects WHERE id = ANY($1)")
        .bind([project_id, lookalike_project_id])
        .execute(&pool)
        .await
        .expect("cleanup git projects");
    sqlx::query("DELETE FROM repositories WHERE name = ANY($1)")
        .bind(&[private_repo, public_repo])
        .execute(&pool)
        .await
        .expect("cleanup git repositories");
    let _ = tokio::fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn pipeline_trigger_replays_same_idempotency_key() {
    let pool = test_pool().await;
    let project_id = Uuid::new_v4();
    let name = format!("it-idempotency-{}", project_id.simple());

    sqlx::query("INSERT INTO projects (id, name, repository_url) VALUES ($1, $2, $3)")
        .bind(project_id)
        .bind(&name)
        .bind("https://example.invalid/repo.git")
        .execute(&pool)
        .await
        .expect("insert project");

    let app = cicd::api::app(Some(pool.clone()));
    let key = Uuid::new_v4().to_string();
    let body = r#"{"git_ref":"main","variables":{"deploy_env":"staging"}}"#;
    let first = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/projects/{project_id}/pipelines"))
                .header("content-type", "application/json")
                .header("Idempotency-Key", &key)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    assert!(first.headers().get("idempotency-replayed").is_none());
    let first_body = response_json(first).await;
    let pipeline_id = first_body["pipeline"]["id"]
        .as_str()
        .expect("pipeline id")
        .to_owned();
    let plan = &first_body["plan"];
    assert_eq!(plan["config_source"], "legacy_template");
    assert_eq!(plan["parser_version"], "forge-legacy-linear/1");
    assert_eq!(plan["git_ref"], "main");
    assert_eq!(plan["plan"]["format"], "legacy-linear");
    assert_eq!(plan["plan"]["stages"].as_array().unwrap().len(), 3);
    assert_eq!(plan["plan"]["dependencies"].as_array().unwrap().len(), 2);
    assert_eq!(plan["plan_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(plan["config_sha256"].as_str().unwrap().len(), 64);

    let replay = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/projects/{project_id}/pipelines"))
                .header("content-type", "application/json")
                .header("Idempotency-Key", &key)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(
        replay.headers().get("idempotency-replayed").unwrap(),
        "true"
    );
    let replay_body = response_json(replay).await;
    assert_eq!(replay_body["pipeline"]["id"], pipeline_id);
    assert_eq!(
        replay_body["plan"]["plan_sha256"],
        first_body["plan"]["plan_sha256"]
    );

    let conflict = app
        .oneshot(
            Request::post(format!("/api/v1/projects/{project_id}/pipelines"))
                .header("content-type", "application/json")
                .header("Idempotency-Key", &key)
                .body(Body::from(r#"{"git_ref":"release"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(conflict.status(), StatusCode::CONFLICT);

    let pipeline_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pipelines WHERE project_id = $1")
            .bind(project_id)
            .fetch_one(&pool)
            .await
            .expect("count pipelines");
    assert_eq!(pipeline_count, 1);

    let trigger_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pipeline_triggers WHERE project_id = $1")
            .bind(project_id)
            .fetch_one(&pool)
            .await
            .expect("count trigger records");
    assert_eq!(trigger_count, 1);

    let pipeline_uuid = Uuid::parse_str(&pipeline_id).expect("pipeline uuid");
    let plan_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pipeline_plans WHERE pipeline_id = $1")
            .bind(pipeline_uuid)
            .fetch_one(&pool)
            .await
            .expect("count pipeline plans");
    assert_eq!(plan_count, 1);

    let (job_count, queue_count): (i64, i64) = sqlx::query_as(
        "SELECT count(DISTINCT j.id), count(DISTINCT q.id) \
         FROM jobs j \
         JOIN stages s ON s.id = j.stage_id \
         LEFT JOIN job_queue q ON q.job_id = j.id AND q.state = 'queued' \
         WHERE s.pipeline_id = $1",
    )
    .bind(pipeline_uuid)
    .fetch_one(&pool)
    .await
    .expect("count triggered job queue rows");
    assert_eq!(queue_count, job_count);

    let mut immutability_tx = pool.begin().await.expect("begin immutability check");
    sqlx::query("SET LOCAL statement_timeout = '2s'")
        .execute(&mut *immutability_tx)
        .await
        .expect("set statement timeout");
    let update_result = sqlx::query(
        "UPDATE pipeline_plans SET plan_sha256 = repeat('0', 64) WHERE pipeline_id = $1",
    )
    .bind(pipeline_uuid)
    .execute(&mut *immutability_tx)
    .await;
    assert!(update_result.is_err(), "pipeline plan must be immutable");
    immutability_tx
        .rollback()
        .await
        .expect("rollback immutability check");

    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project_id)
        .execute(&pool)
        .await
        .expect("cleanup project");
}

#[tokio::test]
async fn pipeline_trigger_reads_v1_dag_config_from_bare_repository() {
    let pool = test_pool().await;
    let namespace = Uuid::new_v4();
    let repo_name = format!("it_v1_dag_{}", namespace.simple());
    let root = std::env::temp_dir().join(format!("forge-v1-dag-{namespace}"));
    let bare_repo = root.join(format!("{repo_name}.git"));
    let worktree = root.join("worktree");
    tokio::fs::create_dir_all(&root)
        .await
        .expect("create git root");

    run_git(&["init", "--bare", "--quiet"], Some(&bare_repo)).await;
    run_git(&["init", "--quiet"], Some(&worktree)).await;
    run_git(
        &[
            "-C",
            worktree.to_str().unwrap(),
            "config",
            "user.email",
            "ci@example.invalid",
        ],
        None,
    )
    .await;
    run_git(
        &[
            "-C",
            worktree.to_str().unwrap(),
            "config",
            "user.name",
            "Forge CI",
        ],
        None,
    )
    .await;
    tokio::fs::write(
        worktree.join(".forge-ci.yml"),
        r#"
version: 1
defaults:
  image: alpine:3.21
  tags: [linux, docker]
jobs:
  build:
    commands: ["cargo build --release"]
  lint:
    commands: ["cargo fmt --check"]
    allow_failure: true
  test:
    needs: [build, lint]
    image: rust:1.86
    timeout: 45m
    tags: [linux]
    secrets: [DEPLOY_TOKEN]
    artifacts:
      paths: [target/release/app.tar.gz, reports/junit.xml]
    commands:
      - cargo test
      - cargo clippy --all-targets
"#,
    )
    .await
    .expect("write v1 pipeline config");
    run_git(
        &["-C", worktree.to_str().unwrap(), "add", ".forge-ci.yml"],
        None,
    )
    .await;
    run_git(
        &[
            "-C",
            worktree.to_str().unwrap(),
            "commit",
            "--quiet",
            "-m",
            "initial",
        ],
        None,
    )
    .await;
    run_git(
        &["-C", worktree.to_str().unwrap(), "branch", "-M", "main"],
        None,
    )
    .await;
    run_git(
        &[
            "-C",
            worktree.to_str().unwrap(),
            "remote",
            "add",
            "origin",
            bare_repo.to_str().unwrap(),
        ],
        None,
    )
    .await;
    run_git(
        &[
            "-C",
            worktree.to_str().unwrap(),
            "push",
            "--quiet",
            "origin",
            "main",
        ],
        None,
    )
    .await;

    // SAFETY: trigger config lookup currently reads CICD_GIT_ROOT directly.
    // This test uses a unique repo name/root; other trigger tests still fall
    // back to the legacy template when their repo is absent under this root.
    unsafe {
        std::env::set_var("CICD_GIT_ROOT", &root);
    }

    let project_id = Uuid::new_v4();
    sqlx::query("INSERT INTO projects (id, name, repository_url) VALUES ($1, $2, $3)")
        .bind(project_id)
        .bind(format!("it-v1-dag-{}", namespace.simple()))
        .bind(format!("http://127.0.0.1/git/{repo_name}.git"))
        .execute(&pool)
        .await
        .expect("insert project");

    let app = cicd::api::app(Some(pool.clone()));
    let response = app
        .oneshot(
            Request::post(format!("/api/v1/projects/{project_id}/pipelines"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"git_ref":"main"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    let pipeline_id = Uuid::parse_str(body["pipeline"]["id"].as_str().unwrap()).unwrap();

    let plan = &body["plan"];
    assert_eq!(plan["config_source"], "repository");
    assert_eq!(plan["parser_version"], "forge-dsl/1.0.0");
    assert_eq!(plan["git_ref"], "main");
    assert_eq!(plan["resolved_commit_sha"].as_str().unwrap().len(), 40);
    assert_eq!(plan["plan"]["format"], "v1-dag");
    assert_eq!(plan["plan"]["version"], 1);
    assert_eq!(plan["plan"]["jobs"].as_array().unwrap().len(), 3);
    assert_eq!(plan["plan"]["dependencies"].as_array().unwrap().len(), 2);
    assert_eq!(plan["plan"]["dependencies"][0]["from"], "build");
    assert_eq!(plan["plan"]["dependencies"][0]["to"], "test");
    assert_eq!(plan["plan"]["dependencies"][1]["from"], "lint");
    assert_eq!(plan["plan"]["dependencies"][1]["to"], "test");
    assert_eq!(plan["plan_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(plan["config_sha256"].as_str().unwrap().len(), 64);

    let stages = body["stages"].as_array().unwrap();
    assert_eq!(stages.len(), 2);
    assert_eq!(stages[0]["name"], "dag-0");
    assert_eq!(stages[1]["name"], "dag-1");
    assert_eq!(
        stages[0]["jobs"][0]["required_tags"],
        serde_json::json!(["docker", "linux"])
    );
    assert_eq!(
        stages[1]["jobs"][0]["required_tags"],
        serde_json::json!(["linux"])
    );
    assert_eq!(
        stages[1]["jobs"][0]["required_secrets"],
        serde_json::json!(["DEPLOY_TOKEN"])
    );
    assert_eq!(
        stages[1]["jobs"][0]["artifact_paths"],
        serde_json::json!(["reports/junit.xml", "target/release/app.tar.gz"])
    );
    let planned_test_job = plan["plan"]["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|job| job["key"].as_str() == Some("test"))
        .expect("test job in plan snapshot");
    assert_eq!(
        planned_test_job["artifact_paths"],
        serde_json::json!(["reports/junit.xml", "target/release/app.tar.gz"])
    );
    assert_eq!(
        stages[0]["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|job| job["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["build", "lint"]
    );
    assert_eq!(stages[1]["jobs"][0]["name"], "test");

    let persisted_jobs = sqlx::query_as::<_, (String, Option<i32>, bool, String, Vec<String>, Vec<String>, Vec<String>, Vec<String>)>(
        "SELECT j.name, j.timeout_seconds, j.allow_failure, j.command, j.required_tags, j.required_secrets, j.artifact_paths, q.required_tags \
         FROM jobs j JOIN stages s ON s.id = j.stage_id \
         JOIN job_queue q ON q.job_id = j.id \
         WHERE s.pipeline_id = $1 ORDER BY s.position, j.position",
    )
    .bind(pipeline_id)
    .fetch_all(&pool)
    .await
    .expect("select persisted v1 jobs");
    assert_eq!(persisted_jobs[0].0, "build");
    assert_eq!(persisted_jobs[0].1, None);
    assert!(!persisted_jobs[0].2);
    assert_eq!(
        persisted_jobs[0].4,
        vec!["docker".to_string(), "linux".to_string()]
    );
    assert_eq!(persisted_jobs[0].5, Vec::<String>::new());
    assert_eq!(persisted_jobs[0].6, Vec::<String>::new());
    assert_eq!(
        persisted_jobs[0].7,
        vec!["docker".to_string(), "linux".to_string()]
    );
    assert_eq!(persisted_jobs[1].0, "lint");
    assert!(persisted_jobs[1].2);
    assert_eq!(
        persisted_jobs[1].4,
        vec!["docker".to_string(), "linux".to_string()]
    );
    assert_eq!(persisted_jobs[2].0, "test");
    assert_eq!(persisted_jobs[2].1, Some(2700));
    assert_eq!(persisted_jobs[2].4, vec!["linux".to_string()]);
    assert_eq!(persisted_jobs[2].5, vec!["DEPLOY_TOKEN".to_string()]);
    assert_eq!(
        persisted_jobs[2].6,
        vec![
            "reports/junit.xml".to_string(),
            "target/release/app.tar.gz".to_string()
        ]
    );
    assert_eq!(persisted_jobs[2].7, vec!["linux".to_string()]);
    assert_eq!(
        persisted_jobs[2].3,
        "set -e\ncargo test\ncargo clippy --all-targets"
    );

    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project_id)
        .execute(&pool)
        .await
        .expect("cleanup v1 project");
    let _ = tokio::fs::remove_dir_all(&root).await;
}

async fn run_git(args: &[&str], path_arg: Option<&std::path::Path>) {
    let mut command = tokio::process::Command::new("git");
    command.args(args);
    if let Some(path) = path_arg {
        command.arg(path);
    }
    let output = command.output().await.expect("run git command");
    assert!(
        output.status.success(),
        "git {:?} failed: {}{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn artifact_download_rejects_storage_paths_outside_artifact_root() {
    let pool = test_pool().await;
    let project_id = Uuid::new_v4();
    let pipeline_id = Uuid::new_v4();
    let stage_id = Uuid::new_v4();
    let job_id = Uuid::new_v4();
    let attempt_id = Uuid::new_v4();
    let artifact_id = Uuid::new_v4();
    let project_name = format!("it-artifact-containment-{}", project_id.simple());
    let artifact_root = std::env::temp_dir().join(format!("forge-artifacts-{}", project_id));
    let inside_path = artifact_root.join(format!("{artifact_id}.bin"));
    let outside_path = std::env::temp_dir().join(format!("forge-outside-{artifact_id}.txt"));

    std::fs::create_dir_all(&artifact_root).expect("create artifact root");
    std::fs::write(&inside_path, b"inside artifact").expect("write inside artifact");
    std::fs::write(&outside_path, b"outside artifact root").expect("write outside artifact");
    // SAFETY: this integration test is the only artifact-route test and uses
    // a unique directory; other integration tests do not read this variable.
    unsafe {
        std::env::set_var("CICD_ARTIFACTS_DIR", &artifact_root);
    }

    sqlx::query("INSERT INTO projects (id, name, repository_url) VALUES ($1, $2, $3)")
        .bind(project_id)
        .bind(&project_name)
        .bind("https://example.invalid/repo.git")
        .execute(&pool)
        .await
        .expect("insert project");
    sqlx::query(
        "INSERT INTO pipelines (id, project_id, git_ref, status) VALUES ($1, $2, 'main', 'queued')",
    )
    .bind(pipeline_id)
    .bind(project_id)
    .execute(&pool)
    .await
    .expect("insert pipeline");
    sqlx::query(
        "INSERT INTO stages (id, pipeline_id, name, position, status) VALUES ($1, $2, 'build', 0, 'queued')",
    )
    .bind(stage_id)
    .bind(pipeline_id)
    .execute(&pool)
    .await
    .expect("insert stage");
    sqlx::query(
        "INSERT INTO jobs (id, stage_id, name, image, command, position, status) VALUES \
         ($1, $2, 'compile', 'alpine:3.21', 'echo ok', 0, 'queued')",
    )
    .bind(job_id)
    .bind(stage_id)
    .execute(&pool)
    .await
    .expect("insert job");
    sqlx::query(
        "INSERT INTO execution_attempts (id, job_id, attempt_no, status, trigger) \
         VALUES ($1, $2, 1, 'queued', 'initial')",
    )
    .bind(attempt_id)
    .bind(job_id)
    .execute(&pool)
    .await
    .expect("insert attempt");

    let app = cicd::api::app(Some(pool.clone()));
    let uploaded_body = b"uploaded artifact bytes".to_vec();
    let uploaded_sha256 = format!("{:x}", Sha256::digest(&uploaded_body));
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/jobs/{job_id}/artifacts"))
                .header("x-artifact-name", "build.log")
                .header("content-type", "text/plain")
                .body(Body::from(uploaded_body.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let uploaded = response_json(response).await;
    assert_eq!(uploaded["sha256"].as_str(), Some(uploaded_sha256.as_str()));
    assert_eq!(
        uploaded["size_bytes"].as_i64(),
        Some(uploaded_body.len() as i64)
    );
    let uploaded_id = Uuid::parse_str(uploaded["id"].as_str().unwrap()).unwrap();
    let uploaded_path: String =
        sqlx::query_scalar("SELECT storage_path FROM artifacts WHERE id = $1")
            .bind(uploaded_id)
            .fetch_one(&pool)
            .await
            .expect("uploaded artifact storage path");
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/artifacts/{uploaded_id}/download"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read uploaded artifact body");
    assert_eq!(bytes.as_ref(), uploaded_body.as_slice());

    std::fs::write(uploaded_path, b"corrupt artifact").expect("corrupt uploaded artifact");
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/artifacts/{uploaded_id}/download"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);

    let inside_sha256 = format!("{:x}", Sha256::digest(b"inside artifact"));
    sqlx::query(
        "INSERT INTO artifacts \
         (id, job_id, attempt_id, name, storage_path, content_type, sha256, size_bytes) \
         VALUES ($1, $2, $3, 'report.txt', $4, 'text/plain', $5, 15)",
    )
    .bind(artifact_id)
    .bind(job_id)
    .bind(attempt_id)
    .bind(inside_path.to_string_lossy().as_ref())
    .bind(inside_sha256)
    .execute(&pool)
    .await
    .expect("insert artifact");

    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/artifacts/{artifact_id}/download"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read artifact body");
    assert_eq!(bytes.as_ref(), b"inside artifact");

    sqlx::query("UPDATE artifacts SET storage_path = $2 WHERE id = $1")
        .bind(artifact_id)
        .bind(outside_path.to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .expect("forge artifact metadata");
    let response = app
        .oneshot(
            Request::get(format!("/api/v1/artifacts/{artifact_id}/download"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project_id)
        .execute(&pool)
        .await
        .expect("cleanup project");
    let _ = std::fs::remove_dir_all(&artifact_root);
    let _ = std::fs::remove_file(&outside_path);
}

#[tokio::test]
async fn project_memberships_enforce_project_roles_and_cascade() {
    let pool = test_pool().await;
    let project_id = Uuid::new_v4();
    let maintainer_id = Uuid::new_v4();
    let viewer_id = Uuid::new_v4();
    let project_name = format!("it-rbac-{}", project_id.simple());
    let maintainer_name = format!("it-maintainer-{}", maintainer_id.simple());
    let viewer_name = format!("it-viewer-{}", viewer_id.simple());

    sqlx::query("INSERT INTO projects (id, name, repository_url) VALUES ($1, $2, $3)")
        .bind(project_id)
        .bind(&project_name)
        .bind("https://example.invalid/repo.git")
        .execute(&pool)
        .await
        .expect("insert project");
    sqlx::query(
        "INSERT INTO users (id, username, role) VALUES \
         ($1, $2, 'maintainer'), ($3, $4, 'viewer')",
    )
    .bind(maintainer_id)
    .bind(&maintainer_name)
    .bind(viewer_id)
    .bind(&viewer_name)
    .execute(&pool)
    .await
    .expect("insert users");

    sqlx::query(
        "INSERT INTO project_memberships (project_id, user_id, role) VALUES \
         ($1, $2, 'maintainer'), ($1, $3, 'viewer')",
    )
    .bind(project_id)
    .bind(maintainer_id)
    .bind(viewer_id)
    .execute(&pool)
    .await
    .expect("insert memberships");

    let invalid_role = sqlx::query(
        "INSERT INTO project_memberships (project_id, user_id, role) VALUES ($1, $2, 'admin')",
    )
    .bind(project_id)
    .bind(maintainer_id)
    .execute(&pool)
    .await;
    assert!(invalid_role.is_err(), "admin is not a project role");

    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(viewer_id)
        .execute(&pool)
        .await
        .expect("delete viewer");
    let viewer_memberships: i64 =
        sqlx::query_scalar("SELECT count(*) FROM project_memberships WHERE user_id = $1")
            .bind(viewer_id)
            .fetch_one(&pool)
            .await
            .expect("count viewer memberships");
    assert_eq!(viewer_memberships, 0);

    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project_id)
        .execute(&pool)
        .await
        .expect("delete project");
    let remaining_memberships: i64 =
        sqlx::query_scalar("SELECT count(*) FROM project_memberships WHERE project_id = $1")
            .bind(project_id)
            .fetch_one(&pool)
            .await
            .expect("count project memberships");
    assert_eq!(remaining_memberships, 0);
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(maintainer_id)
        .execute(&pool)
        .await
        .expect("cleanup maintainer");
}

#[tokio::test]
async fn job_retry_preserves_attempt_logs_and_appends_to_new_attempt() {
    let pool = test_pool().await;
    let project_id = Uuid::new_v4();
    let pipeline_id = Uuid::new_v4();
    let stage_id = Uuid::new_v4();
    let job_id = Uuid::new_v4();
    let first_attempt_id = Uuid::new_v4();
    let project_name = format!("it-attempts-{}", project_id.simple());

    sqlx::query("INSERT INTO projects (id, name, repository_url) VALUES ($1, $2, $3)")
        .bind(project_id)
        .bind(&project_name)
        .bind("https://example.invalid/repo.git")
        .execute(&pool)
        .await
        .expect("insert project");
    sqlx::query(
        "INSERT INTO pipelines (id, project_id, git_ref, status, finished_at) \
         VALUES ($1, $2, 'main', 'failed', now())",
    )
    .bind(pipeline_id)
    .bind(project_id)
    .execute(&pool)
    .await
    .expect("insert pipeline");
    sqlx::query(
        "INSERT INTO stages (id, pipeline_id, name, position, status) \
         VALUES ($1, $2, 'build', 0, 'failed')",
    )
    .bind(stage_id)
    .bind(pipeline_id)
    .execute(&pool)
    .await
    .expect("insert stage");
    sqlx::query(
        "INSERT INTO jobs (id, stage_id, name, image, command, position, status, finished_at) \
         VALUES ($1, $2, 'compile', 'alpine:3.21', 'echo old', 0, 'failed', now())",
    )
    .bind(job_id)
    .bind(stage_id)
    .execute(&pool)
    .await
    .expect("insert job");
    sqlx::query(
        "INSERT INTO execution_attempts (id, job_id, attempt_no, status, trigger, finished_at) \
         VALUES ($1, $2, 1, 'failed', 'initial', now())",
    )
    .bind(first_attempt_id)
    .bind(job_id)
    .execute(&pool)
    .await
    .expect("insert first attempt");
    sqlx::query(
        "INSERT INTO job_logs (job_id, attempt_id, sequence, message) VALUES ($1, $2, 1, 'old failure')",
    )
    .bind(job_id)
    .bind(first_attempt_id)
    .execute(&pool)
    .await
    .expect("insert old log");

    let app = cicd::api::app(Some(pool.clone()));
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/jobs/{job_id}/retry"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let attempts: Vec<(Uuid, i32, String)> = sqlx::query_as(
        "SELECT id, attempt_no, status FROM execution_attempts WHERE job_id = $1 ORDER BY attempt_no",
    )
    .bind(job_id)
    .fetch_all(&pool)
    .await
    .expect("select attempts");
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[0], (first_attempt_id, 1, "failed".to_owned()));
    assert_eq!(attempts[1].1, 2);
    assert_eq!(attempts[1].2, "queued");
    let second_attempt_id = attempts[1].0;
    let retry_queue_state: String =
        sqlx::query_scalar("SELECT state FROM job_queue WHERE attempt_id = $1")
            .bind(second_attempt_id)
            .fetch_one(&pool)
            .await
            .expect("select retry queue state");
    assert_eq!(retry_queue_state, "queued");

    let response = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/api/v1/jobs/{job_id}/attempts/{first_attempt_id}/logs"
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let old_logs = response_json(response).await;
    assert_eq!(old_logs.as_array().unwrap().len(), 1);
    assert_eq!(old_logs[0]["message"], "old failure");

    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/jobs/{job_id}/logs"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response_json(response).await.as_array().unwrap().is_empty());

    let response = app
        .oneshot(
            Request::post(format!("/api/v1/jobs/{job_id}/logs"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"message":"new attempt log"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let new_log = response_json(response).await;
    assert_eq!(new_log["attempt_id"], second_attempt_id.to_string());
    assert_eq!(new_log["sequence"], 1);

    let counts: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT attempt_id, count(*) FROM job_logs WHERE job_id = $1 GROUP BY attempt_id ORDER BY attempt_id",
    )
    .bind(job_id)
    .fetch_all(&pool)
    .await
    .expect("count logs");
    assert_eq!(counts.len(), 2);
    assert!(
        counts
            .iter()
            .any(|(id, count)| *id == first_attempt_id && *count == 1)
    );
    assert!(
        counts
            .iter()
            .any(|(id, count)| *id == second_attempt_id && *count == 1)
    );

    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project_id)
        .execute(&pool)
        .await
        .expect("cleanup project");
}

#[tokio::test]
async fn manual_job_start_materializes_queue_row() {
    let pool = test_pool().await;
    let project_id = Uuid::new_v4();
    let pipeline_id = Uuid::new_v4();
    let stage_id = Uuid::new_v4();
    let job_id = Uuid::new_v4();
    let attempt_id = Uuid::new_v4();
    let project_name = format!("it-manual-queue-{}", project_id.simple());

    sqlx::query("INSERT INTO projects (id, name, repository_url) VALUES ($1, $2, $3)")
        .bind(project_id)
        .bind(&project_name)
        .bind("https://example.invalid/manual-queue.git")
        .execute(&pool)
        .await
        .expect("insert project");
    sqlx::query(
        "INSERT INTO pipelines (id, project_id, git_ref, status) \
         VALUES ($1, $2, 'main', 'queued')",
    )
    .bind(pipeline_id)
    .bind(project_id)
    .execute(&pool)
    .await
    .expect("insert pipeline");
    sqlx::query(
        "INSERT INTO stages (id, pipeline_id, name, position, status) \
         VALUES ($1, $2, 'deploy', 0, 'queued')",
    )
    .bind(stage_id)
    .bind(pipeline_id)
    .execute(&pool)
    .await
    .expect("insert stage");
    sqlx::query(
        "INSERT INTO jobs (id, stage_id, name, image, command, position, status, manual) \
         VALUES ($1, $2, 'deploy-prod', 'alpine:3.21', 'echo deploy', 0, 'queued', true)",
    )
    .bind(job_id)
    .bind(stage_id)
    .execute(&pool)
    .await
    .expect("insert manual job");
    sqlx::query(
        "INSERT INTO execution_attempts (id, job_id, attempt_no, status, trigger) \
         VALUES ($1, $2, 1, 'queued', 'initial')",
    )
    .bind(attempt_id)
    .bind(job_id)
    .execute(&pool)
    .await
    .expect("insert manual attempt");

    let initially_queued: i64 =
        sqlx::query_scalar("SELECT count(*) FROM job_queue WHERE attempt_id = $1")
            .bind(attempt_id)
            .fetch_one(&pool)
            .await
            .expect("count initial queue rows");
    assert_eq!(initially_queued, 0);

    let app = cicd::api::app(Some(pool.clone()));
    let response = app
        .oneshot(
            Request::post(format!("/api/v1/jobs/{job_id}/start"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let (manual, queue_state, queue_completed_at): (
        bool,
        String,
        Option<chrono::DateTime<chrono::Utc>>,
    ) = sqlx::query_as(
        "SELECT j.manual, q.state, q.completed_at \
         FROM jobs j \
         JOIN job_queue q ON q.job_id = j.id \
         WHERE j.id = $1 AND q.attempt_id = $2",
    )
    .bind(job_id)
    .bind(attempt_id)
    .fetch_one(&pool)
    .await
    .expect("fetch manual queue state");
    assert!(!manual);
    assert_eq!(queue_state, "queued");
    assert!(queue_completed_at.is_none());

    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project_id)
        .execute(&pool)
        .await
        .expect("cleanup project");
}

#[tokio::test]
async fn job_log_page_is_bounded_and_searchable() {
    let pool = test_pool().await;
    let project_id = Uuid::new_v4();
    let pipeline_id = Uuid::new_v4();
    let stage_id = Uuid::new_v4();
    let job_id = Uuid::new_v4();
    let attempt_id = Uuid::new_v4();
    let other_attempt_id = Uuid::new_v4();
    let project_name = format!("it-log-page-{}", project_id.simple());

    sqlx::query("INSERT INTO projects (id, name, repository_url) VALUES ($1, $2, $3)")
        .bind(project_id)
        .bind(&project_name)
        .bind("https://example.invalid/log-page.git")
        .execute(&pool)
        .await
        .expect("insert project");
    sqlx::query(
        "INSERT INTO pipelines (id, project_id, git_ref, status) VALUES ($1, $2, 'main', 'running')",
    )
    .bind(pipeline_id)
    .bind(project_id)
    .execute(&pool)
    .await
    .expect("insert pipeline");
    sqlx::query(
        "INSERT INTO stages (id, pipeline_id, name, position, status) \
         VALUES ($1, $2, 'build', 0, 'running')",
    )
    .bind(stage_id)
    .bind(pipeline_id)
    .execute(&pool)
    .await
    .expect("insert stage");
    sqlx::query(
        "INSERT INTO jobs (id, stage_id, name, image, command, position, status) \
         VALUES ($1, $2, 'compile', 'alpine:3.21', 'echo test', 0, 'running')",
    )
    .bind(job_id)
    .bind(stage_id)
    .execute(&pool)
    .await
    .expect("insert job");
    sqlx::query(
        "INSERT INTO execution_attempts (id, job_id, attempt_no, status, trigger, started_at) \
         VALUES ($1, $2, 1, 'running', 'initial', now())",
    )
    .bind(attempt_id)
    .bind(job_id)
    .execute(&pool)
    .await
    .expect("insert attempt");
    for (sequence, message) in [
        (1, "compile started"),
        (2, "unit error: expected status"),
        (3, "compile finished"),
    ] {
        sqlx::query(
            "INSERT INTO job_logs (job_id, attempt_id, sequence, message) VALUES ($1, $2, $3, $4)",
        )
        .bind(job_id)
        .bind(attempt_id)
        .bind(sequence)
        .bind(message)
        .execute(&pool)
        .await
        .expect("insert log");
    }

    let app = cicd::api::app(Some(pool.clone()));
    let response = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/api/v1/jobs/{job_id}/attempts/{attempt_id}/logs/page?limit=2"
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let first_page = response_json(response).await;
    assert_eq!(first_page["items"].as_array().unwrap().len(), 2);
    assert_eq!(first_page["items"][0]["sequence"], 1);
    assert_eq!(first_page["items"][1]["sequence"], 2);
    assert_eq!(first_page["next_after"], 2);

    let response = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/api/v1/jobs/{job_id}/attempts/{attempt_id}/logs/page?limit=2&after=2"
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let second_page = response_json(response).await;
    assert_eq!(second_page["items"].as_array().unwrap().len(), 1);
    assert_eq!(second_page["items"][0]["sequence"], 3);
    assert!(second_page["next_after"].is_null());

    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/jobs/{job_id}/logs/page?limit=10&q=ERROR"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let search_page = response_json(response).await;
    assert_eq!(search_page["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        search_page["items"][0]["message"],
        "unit error: expected status"
    );

    let response = app
        .oneshot(
            Request::get(format!(
                "/api/v1/jobs/{job_id}/attempts/{other_attempt_id}/logs/page?limit=2"
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project_id)
        .execute(&pool)
        .await
        .expect("cleanup project");
}

#[tokio::test]
async fn cancel_pipeline_marks_open_attempts_canceled() {
    let pool = test_pool().await;
    let project_id = Uuid::new_v4();
    let pipeline_id = Uuid::new_v4();
    let stage_id = Uuid::new_v4();
    let running_job_id = Uuid::new_v4();
    let queued_job_id = Uuid::new_v4();
    let running_attempt_id = Uuid::new_v4();
    let queued_attempt_id = Uuid::new_v4();
    let running_lease_id = Uuid::new_v4();
    let project_name = format!("it-cancel-attempts-{}", project_id.simple());

    sqlx::query("INSERT INTO projects (id, name, repository_url) VALUES ($1, $2, $3)")
        .bind(project_id)
        .bind(&project_name)
        .bind("https://example.invalid/repo.git")
        .execute(&pool)
        .await
        .expect("insert project");
    sqlx::query(
        "INSERT INTO pipelines (id, project_id, git_ref, status, started_at) \
         VALUES ($1, $2, 'main', 'running', now())",
    )
    .bind(pipeline_id)
    .bind(project_id)
    .execute(&pool)
    .await
    .expect("insert pipeline");
    sqlx::query(
        "INSERT INTO stages (id, pipeline_id, name, position, status) \
         VALUES ($1, $2, 'build', 0, 'running')",
    )
    .bind(stage_id)
    .bind(pipeline_id)
    .execute(&pool)
    .await
    .expect("insert stage");
    sqlx::query(
        "INSERT INTO jobs (id, stage_id, name, image, command, position, status, started_at) \
         VALUES ($1, $2, 'compile', 'alpine:3.21', 'sleep 30', 0, 'running', now())",
    )
    .bind(running_job_id)
    .bind(stage_id)
    .execute(&pool)
    .await
    .expect("insert running job");
    sqlx::query(
        "INSERT INTO jobs (id, stage_id, name, image, command, position, status) \
         VALUES ($1, $2, 'lint', 'alpine:3.21', 'echo lint', 1, 'queued')",
    )
    .bind(queued_job_id)
    .bind(stage_id)
    .execute(&pool)
    .await
    .expect("insert queued job");
    sqlx::query(
        "INSERT INTO execution_attempts (id, job_id, attempt_no, status, trigger, started_at) \
         VALUES ($1, $2, 1, 'running', 'initial', now())",
    )
    .bind(running_attempt_id)
    .bind(running_job_id)
    .execute(&pool)
    .await
    .expect("insert running attempt");
    sqlx::query(
        "INSERT INTO job_leases \
         (id, job_id, attempt_id, runner_name, lease_status, generation, lease_expires_at) \
         VALUES ($1, $2, $3, 'embedded', 'active', 1, now() + interval '10 minutes')",
    )
    .bind(running_lease_id)
    .bind(running_job_id)
    .bind(running_attempt_id)
    .execute(&pool)
    .await
    .expect("insert active lease");
    let mut queued_tx = pool.begin().await.expect("begin queued queue setup");
    sqlx::query(
        "INSERT INTO execution_attempts (id, job_id, attempt_no, status, trigger) \
         VALUES ($1, $2, 1, 'queued', 'initial')",
    )
    .bind(queued_attempt_id)
    .bind(queued_job_id)
    .execute(&mut *queued_tx)
    .await
    .expect("insert queued attempt");
    sqlx::query(
        "INSERT INTO job_queue \
         (id, job_id, attempt_id, pipeline_id, stage_id, state, leased_at, lease_id) \
         VALUES ($1, $2, $3, $4, $5, 'leased', now(), $6)",
    )
    .bind(Uuid::new_v4())
    .bind(running_job_id)
    .bind(running_attempt_id)
    .bind(pipeline_id)
    .bind(stage_id)
    .bind(running_lease_id)
    .execute(&pool)
    .await
    .expect("insert leased queue row");
    sqlx::query(
        "INSERT INTO job_queue (id, job_id, attempt_id, pipeline_id, stage_id, state, not_before) \
         VALUES ($1, $2, $3, $4, $5, 'queued', now() + interval '1 day')",
    )
    .bind(Uuid::new_v4())
    .bind(queued_job_id)
    .bind(queued_attempt_id)
    .bind(pipeline_id)
    .bind(stage_id)
    .execute(&mut *queued_tx)
    .await
    .expect("insert queued queue row");
    queued_tx.commit().await.expect("commit queued queue setup");

    let app = cicd::api::app(Some(pool.clone()));
    let response = app
        .oneshot(
            Request::post(format!("/api/v1/pipelines/{pipeline_id}/cancel"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let open_attempts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM execution_attempts \
         WHERE id IN ($1, $2) AND status IN ('queued','running')",
    )
    .bind(running_attempt_id)
    .bind(queued_attempt_id)
    .fetch_one(&pool)
    .await
    .expect("count open attempts");
    assert_eq!(open_attempts, 0);

    let canceled_attempts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM execution_attempts \
         WHERE id IN ($1, $2) AND status = 'canceled' AND finished_at IS NOT NULL",
    )
    .bind(running_attempt_id)
    .bind(queued_attempt_id)
    .fetch_one(&pool)
    .await
    .expect("count canceled attempts");
    assert_eq!(canceled_attempts, 2);

    let canceled_jobs: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM jobs WHERE id IN ($1, $2) AND status = 'canceled'",
    )
    .bind(running_job_id)
    .bind(queued_job_id)
    .fetch_one(&pool)
    .await
    .expect("count canceled jobs");
    assert_eq!(canceled_jobs, 2);

    let (lease_status, terminal_status, completed_at): (
        String,
        Option<String>,
        Option<chrono::DateTime<chrono::Utc>>,
    ) = sqlx::query_as(
        "SELECT lease_status, terminal_status, completed_at FROM job_leases WHERE id = $1",
    )
    .bind(running_lease_id)
    .fetch_one(&pool)
    .await
    .expect("fetch canceled lease");
    assert_eq!(lease_status, "canceled");
    assert_eq!(terminal_status.as_deref(), Some("canceled"));
    assert!(completed_at.is_some());

    let canceled_queue_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) \
         FROM job_queue \
         WHERE attempt_id IN ($1, $2) AND state = 'canceled' AND completed_at IS NOT NULL",
    )
    .bind(running_attempt_id)
    .bind(queued_attempt_id)
    .fetch_one(&pool)
    .await
    .expect("count canceled queue rows");
    assert_eq!(canceled_queue_rows, 2);

    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project_id)
        .execute(&pool)
        .await
        .expect("cleanup project");
}

#[tokio::test]
async fn cancel_pipeline_signals_external_runner_until_confirmed() {
    let pool = test_pool().await;
    let namespace = Uuid::new_v4();
    let project_id = Uuid::new_v4();
    let pipeline_id = Uuid::new_v4();
    let stage_id = Uuid::new_v4();
    let job_id = Uuid::new_v4();
    let attempt_id = Uuid::new_v4();
    let runner_id = Uuid::new_v4();
    let lease_id = Uuid::new_v4();
    let project_name = format!("it-external-cancel-{}", namespace.simple());
    let runner_credential = format!("cicd_runner_{}", namespace.simple());
    let lease_token = format!("lease-{}", namespace.simple());

    sqlx::query("INSERT INTO projects (id, name, repository_url) VALUES ($1, $2, $3)")
        .bind(project_id)
        .bind(&project_name)
        .bind("https://example.invalid/repo.git")
        .execute(&pool)
        .await
        .expect("insert project");
    sqlx::query(
        "INSERT INTO pipelines (id, project_id, git_ref, status, started_at) \
         VALUES ($1, $2, 'main', 'running', now())",
    )
    .bind(pipeline_id)
    .bind(project_id)
    .execute(&pool)
    .await
    .expect("insert pipeline");
    sqlx::query(
        "INSERT INTO stages (id, pipeline_id, name, position, status) \
         VALUES ($1, $2, 'build', 0, 'running')",
    )
    .bind(stage_id)
    .bind(pipeline_id)
    .execute(&pool)
    .await
    .expect("insert stage");
    sqlx::query(
        "INSERT INTO jobs (id, stage_id, name, image, command, position, status, started_at) \
         VALUES ($1, $2, 'compile', 'alpine:3.21', 'sleep 30', 0, 'running', now())",
    )
    .bind(job_id)
    .bind(stage_id)
    .execute(&pool)
    .await
    .expect("insert job");
    sqlx::query(
        "INSERT INTO execution_attempts (id, job_id, attempt_no, status, trigger, started_at) \
         VALUES ($1, $2, 1, 'running', 'runner', now())",
    )
    .bind(attempt_id)
    .bind(job_id)
    .execute(&pool)
    .await
    .expect("insert attempt");
    sqlx::query(
        "INSERT INTO runners \
         (id, name, tags, status, last_seen_at, credential_hash, token_hint, credential_expires_at, capabilities) \
         VALUES ($1, $2, ARRAY['linux'], 'online', now(), $3, 'cancel-test', now() + interval '1 day', '{}'::jsonb)",
    )
    .bind(runner_id)
    .bind(format!("runner-{}", namespace.simple()))
    .bind(cicd::auth::hash_token(&runner_credential))
    .execute(&pool)
    .await
    .expect("insert runner");
    sqlx::query(
        "INSERT INTO job_leases \
         (id, job_id, attempt_id, runner_id, runner_name, lease_status, generation, lease_expires_at, \
          lease_token_hash, ack_deadline, acknowledged_at, runner_protocol_version) \
         VALUES ($1, $2, $3, $4, 'external', 'active', 1, now() + interval '10 minutes', \
                 $5, now() + interval '30 seconds', now(), 1)",
    )
    .bind(lease_id)
    .bind(job_id)
    .bind(attempt_id)
    .bind(runner_id)
    .bind(cicd::auth::hash_token(&lease_token))
    .execute(&pool)
    .await
    .expect("insert external lease");
    sqlx::query(
        "INSERT INTO job_queue \
         (id, job_id, attempt_id, pipeline_id, stage_id, state, leased_at, lease_id) \
         VALUES ($1, $2, $3, $4, $5, 'leased', now(), $6)",
    )
    .bind(Uuid::new_v4())
    .bind(job_id)
    .bind(attempt_id)
    .bind(pipeline_id)
    .bind(stage_id)
    .bind(lease_id)
    .execute(&pool)
    .await
    .expect("insert leased queue row");

    let app = cicd::api::app(Some(pool.clone()));
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/pipelines/{pipeline_id}/cancel"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let (
        job_status,
        attempt_status,
        pipeline_status,
        queue_state,
        lease_status,
        terminal_status,
        lease_completed_at,
        cancel_requested_at,
    ): CanceledExternalLeaseState = sqlx::query_as(
        "SELECT j.status, a.status, p.status, q.state, l.lease_status, l.terminal_status, \
                l.completed_at, l.cancel_requested_at \
         FROM jobs j \
         JOIN execution_attempts a ON a.job_id = j.id \
         JOIN job_leases l ON l.attempt_id = a.id \
         JOIN job_queue q ON q.attempt_id = a.id \
         JOIN stages s ON s.id = j.stage_id \
         JOIN pipelines p ON p.id = s.pipeline_id \
         WHERE j.id = $1",
    )
    .bind(job_id)
    .fetch_one(&pool)
    .await
    .expect("fetch canceled external lease state");
    assert_eq!(job_status, "canceled");
    assert_eq!(attempt_status, "canceled");
    assert_eq!(pipeline_status, "canceled");
    assert_eq!(queue_state, "canceled");
    assert_eq!(lease_status, "active");
    assert!(terminal_status.is_none());
    assert!(lease_completed_at.is_none());
    assert!(cancel_requested_at.is_some());

    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/runner/leases/{lease_id}/control"))
                .header("authorization", format!("Bearer {runner_credential}"))
                .header("x-runner-protocol-version", "1")
                .header("x-lease-token", &lease_token)
                .header("x-fencing-token", "1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let control = response_json(response).await;
    assert_eq!(control["protocolVersion"], 1);
    assert_eq!(control["cancelRequested"], true);

    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/runner/leases/{lease_id}/complete"))
                .header("authorization", format!("Bearer {runner_credential}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "protocolVersion": 1,
                        "leaseToken": lease_token,
                        "fencingToken": 1,
                        "attemptId": attempt_id,
                        "outcome": "success",
                        "exitCode": 0,
                        "finishedAt": Utc::now()
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);

    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/runner/leases/{lease_id}/complete"))
                .header("authorization", format!("Bearer {runner_credential}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "protocolVersion": 1,
                        "leaseToken": lease_token,
                        "fencingToken": 1,
                        "attemptId": attempt_id,
                        "outcome": "canceled",
                        "finishedAt": Utc::now(),
                        "diagnostic": "runner cancellation requested"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let completed = response_json(response).await;
    assert_eq!(completed["terminalStatus"], "canceled");

    let (lease_status, terminal_status, completed_at): (
        String,
        Option<String>,
        Option<chrono::DateTime<chrono::Utc>>,
    ) = sqlx::query_as(
        "SELECT lease_status, terminal_status, completed_at FROM job_leases WHERE id = $1",
    )
    .bind(lease_id)
    .fetch_one(&pool)
    .await
    .expect("fetch confirmed canceled lease");
    assert_eq!(lease_status, "canceled");
    assert_eq!(terminal_status.as_deref(), Some("canceled"));
    assert!(completed_at.is_some());

    let stale_lease_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO job_leases \
         (id, job_id, attempt_id, runner_id, runner_name, lease_status, generation, lease_expires_at, \
          lease_token_hash, ack_deadline, acknowledged_at, runner_protocol_version) \
         VALUES ($1, $2, $3, $4, 'external', 'active', 2, now() + interval '10 minutes', \
                 $5, now() + interval '30 seconds', now(), 1)",
    )
    .bind(stale_lease_id)
    .bind(job_id)
    .bind(attempt_id)
    .bind(runner_id)
    .bind(cicd::auth::hash_token(&format!("stale-{lease_token}")))
    .execute(&pool)
    .await
    .expect("insert stale active external lease");

    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/pipelines/{pipeline_id}/retry"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let active_leases: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM job_leases WHERE job_id = $1 AND lease_status = 'active'",
    )
    .bind(job_id)
    .fetch_one(&pool)
    .await
    .expect("count active leases after retry");
    assert_eq!(active_leases, 0);
    let (retried_job_status, latest_attempt_status, queue_state): (String, String, String) =
        sqlx::query_as(
            "SELECT j.status, a.status, q.state \
             FROM jobs j \
             JOIN LATERAL ( \
                 SELECT id, status \
                 FROM execution_attempts \
                 WHERE job_id = j.id \
                 ORDER BY attempt_no DESC \
                 LIMIT 1 \
             ) a ON TRUE \
             JOIN job_queue q ON q.attempt_id = a.id \
             WHERE j.id = $1",
        )
        .bind(job_id)
        .fetch_one(&pool)
        .await
        .expect("fetch retried job state");
    assert_eq!(retried_job_status, "queued");
    assert_eq!(latest_attempt_status, "queued");
    assert_eq!(queue_state, "queued");

    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project_id)
        .execute(&pool)
        .await
        .expect("cleanup project");
}

#[tokio::test]
async fn embedded_runner_closes_lease_when_prepare_fails() {
    let pool = test_pool().await;
    let project_id = Uuid::new_v4();
    let pipeline_id = Uuid::new_v4();
    let stage_id = Uuid::new_v4();
    let job_id = Uuid::new_v4();
    let attempt_id = Uuid::new_v4();
    let project_name = format!("it-runner-lease-{}", project_id.simple());
    let missing_repo = std::env::temp_dir().join(format!("forge-missing-{}.git", project_id));

    sqlx::query("INSERT INTO projects (id, name, repository_url) VALUES ($1, $2, $3)")
        .bind(project_id)
        .bind(&project_name)
        .bind(missing_repo.to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .expect("insert project");
    sqlx::query(
        "INSERT INTO pipelines (id, project_id, git_ref, status) VALUES ($1, $2, 'main', 'queued')",
    )
    .bind(pipeline_id)
    .bind(project_id)
    .execute(&pool)
    .await
    .expect("insert pipeline");
    sqlx::query(
        "INSERT INTO stages (id, pipeline_id, name, position, status) \
         VALUES ($1, $2, 'build', 0, 'queued')",
    )
    .bind(stage_id)
    .bind(pipeline_id)
    .execute(&pool)
    .await
    .expect("insert stage");
    sqlx::query(
        "INSERT INTO jobs (id, stage_id, name, image, command, position, status, timeout_seconds) \
         VALUES ($1, $2, 'compile', 'alpine:3.21', 'echo ok', 0, 'queued', 5)",
    )
    .bind(job_id)
    .bind(stage_id)
    .execute(&pool)
    .await
    .expect("insert job");
    sqlx::query(
        "INSERT INTO execution_attempts (id, job_id, attempt_no, status, trigger) \
         VALUES ($1, $2, 1, 'queued', 'initial')",
    )
    .bind(attempt_id)
    .bind(job_id)
    .execute(&pool)
    .await
    .expect("insert attempt");

    cicd::runner::run_job(pool.clone(), job_id, cicd::runner::RunningJobs::default()).await;

    let (job_status, job_finished_at): (String, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as("SELECT status, finished_at FROM jobs WHERE id = $1")
            .bind(job_id)
            .fetch_one(&pool)
            .await
            .expect("fetch job");
    assert_eq!(job_status, "failed");
    assert!(job_finished_at.is_some());

    let (attempt_status, attempt_finished_at, attempt_error): (
        String,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT status, finished_at, error_tail FROM execution_attempts WHERE id = $1",
    )
    .bind(attempt_id)
    .fetch_one(&pool)
    .await
    .expect("fetch attempt");
    assert_eq!(attempt_status, "failed");
    assert!(attempt_finished_at.is_some());
    assert!(
        attempt_error
            .as_deref()
            .is_some_and(|error| error.contains("runner: internal failure"))
    );

    let (lease_status, terminal_status, completed_at, lease_error, generation): (
        String,
        Option<String>,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<String>,
        i64,
    ) = sqlx::query_as(
        "SELECT lease_status, terminal_status, completed_at, error_tail, generation \
         FROM job_leases WHERE job_id = $1 AND attempt_id = $2",
    )
    .bind(job_id)
    .bind(attempt_id)
    .fetch_one(&pool)
    .await
    .expect("fetch lease");
    assert_eq!(lease_status, "completed");
    assert_eq!(terminal_status.as_deref(), Some("failed"));
    assert!(completed_at.is_some());
    assert_eq!(generation, 1);
    assert!(
        lease_error
            .as_deref()
            .is_some_and(|error| error.contains("runner: internal failure"))
    );

    let active_leases: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM job_leases WHERE job_id = $1 AND lease_status = 'active'",
    )
    .bind(job_id)
    .fetch_one(&pool)
    .await
    .expect("count active leases");
    assert_eq!(active_leases, 0);

    let (queue_state, queue_completed_at): (String, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as("SELECT state, completed_at FROM job_queue WHERE attempt_id = $1")
            .bind(attempt_id)
            .fetch_one(&pool)
            .await
            .expect("fetch embedded runner queue row");
    assert_eq!(queue_state, "completed");
    assert!(queue_completed_at.is_some());

    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project_id)
        .execute(&pool)
        .await
        .expect("cleanup project");
}

#[tokio::test]
async fn expired_job_lease_is_reconciled_to_failed_attempt() {
    let pool = test_pool().await;
    let project_id = Uuid::new_v4();
    let pipeline_id = Uuid::new_v4();
    let stage_id = Uuid::new_v4();
    let job_id = Uuid::new_v4();
    let attempt_id = Uuid::new_v4();
    let lease_id = Uuid::new_v4();
    let project_name = format!("it-lease-expiry-{}", project_id.simple());

    sqlx::query("INSERT INTO projects (id, name, repository_url) VALUES ($1, $2, $3)")
        .bind(project_id)
        .bind(&project_name)
        .bind("https://example.invalid/lease-expiry.git")
        .execute(&pool)
        .await
        .expect("insert project");
    sqlx::query(
        "INSERT INTO pipelines (id, project_id, git_ref, status, started_at) \
         VALUES ($1, $2, 'main', 'running', now())",
    )
    .bind(pipeline_id)
    .bind(project_id)
    .execute(&pool)
    .await
    .expect("insert pipeline");
    sqlx::query(
        "INSERT INTO stages (id, pipeline_id, name, position, status) \
         VALUES ($1, $2, 'build', 0, 'running')",
    )
    .bind(stage_id)
    .bind(pipeline_id)
    .execute(&pool)
    .await
    .expect("insert stage");
    sqlx::query(
        "INSERT INTO jobs (id, stage_id, name, image, command, position, status, started_at) \
         VALUES ($1, $2, 'compile', 'alpine:3.21', 'sleep 30', 0, 'running', now())",
    )
    .bind(job_id)
    .bind(stage_id)
    .execute(&pool)
    .await
    .expect("insert job");
    sqlx::query(
        "INSERT INTO execution_attempts (id, job_id, attempt_no, status, trigger, started_at) \
         VALUES ($1, $2, 1, 'running', 'runner', now())",
    )
    .bind(attempt_id)
    .bind(job_id)
    .execute(&pool)
    .await
    .expect("insert attempt");
    sqlx::query(
        "INSERT INTO job_leases \
         (id, job_id, attempt_id, runner_name, lease_status, generation, lease_expires_at) \
         VALUES ($1, $2, $3, 'embedded', 'active', 1, now() - interval '1 minute')",
    )
    .bind(lease_id)
    .bind(job_id)
    .bind(attempt_id)
    .execute(&pool)
    .await
    .expect("insert expired lease");
    sqlx::query(
        "INSERT INTO job_queue \
         (id, job_id, attempt_id, pipeline_id, stage_id, state, leased_at, lease_id) \
         VALUES ($1, $2, $3, $4, $5, 'leased', now() - interval '2 minutes', $6)",
    )
    .bind(Uuid::new_v4())
    .bind(job_id)
    .bind(attempt_id)
    .bind(pipeline_id)
    .bind(stage_id)
    .bind(lease_id)
    .execute(&pool)
    .await
    .expect("insert expired queue row");

    let reconciled = cicd::runner::reconcile_expired_leases(&pool)
        .await
        .expect("reconcile expired leases");
    assert_eq!(reconciled, 1);

    let (
        job_status,
        attempt_status,
        lease_status,
        terminal_status,
        pipeline_status,
        queue_state,
        queue_completed_at,
    ): (
        String,
        String,
        String,
        Option<String>,
        String,
        String,
        Option<chrono::DateTime<chrono::Utc>>,
    ) = sqlx::query_as(
        "SELECT j.status, a.status, l.lease_status, l.terminal_status, p.status, \
                q.state, q.completed_at \
         FROM jobs j \
         JOIN execution_attempts a ON a.job_id = j.id \
         JOIN job_leases l ON l.attempt_id = a.id \
         JOIN job_queue q ON q.attempt_id = a.id \
         JOIN stages s ON s.id = j.stage_id \
         JOIN pipelines p ON p.id = s.pipeline_id \
         WHERE j.id = $1",
    )
    .bind(job_id)
    .fetch_one(&pool)
    .await
    .expect("fetch reconciled state");
    assert_eq!(job_status, "failed");
    assert_eq!(attempt_status, "failed");
    assert_eq!(lease_status, "expired");
    assert_eq!(terminal_status.as_deref(), Some("failed"));
    assert_eq!(pipeline_status, "failed");
    assert_eq!(queue_state, "completed");
    assert!(queue_completed_at.is_some());

    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project_id)
        .execute(&pool)
        .await
        .expect("cleanup project");
}

#[tokio::test]
async fn list_attempts_returns_not_found_for_unknown_job() {
    let pool = test_pool().await;
    let app = cicd::api::app(Some(pool));
    let response = app
        .oneshot(
            Request::get(format!("/api/v1/jobs/{}/attempts", Uuid::new_v4()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cron_schedule_materializes_unique_fire_slots() {
    let pool = test_pool().await;
    let project_id = Uuid::new_v4();
    let project_name = format!("it-schedule-{}", project_id.simple());
    let due_slot = (Utc::now() - Duration::minutes(1))
        .with_second(0)
        .and_then(|value| value.with_nanosecond(0))
        .unwrap();
    let cron = due_slot.format("%M %H * * *").to_string();

    sqlx::query("INSERT INTO projects (id, name, repository_url) VALUES ($1, $2, $3)")
        .bind(project_id)
        .bind(&project_name)
        .bind("https://example.invalid/schedule.git")
        .execute(&pool)
        .await
        .expect("insert project");

    let app = cicd::api::app(Some(pool.clone()));
    let response = app
        .oneshot(
            Request::post(format!("/api/v1/projects/{project_id}/schedules"))
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"cron":"{cron}","git_ref":"main","enabled":true}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let schedule = response_json(response).await;
    assert!(schedule["next_fire_at"].as_str().is_some());
    assert!(schedule["last_fired_at"].is_null());
    assert!(schedule["last_fire_error"].is_null());
    let schedule_id = Uuid::parse_str(schedule["id"].as_str().unwrap()).unwrap();

    let _ = cicd::outbox::fire_due_schedules(&pool).await;
    let (pipeline_count_before_due,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM pipelines WHERE project_id = $1")
            .bind(project_id)
            .fetch_one(&pool)
            .await
            .expect("count project pipelines before due");
    assert_eq!(pipeline_count_before_due, 0);

    sqlx::query("UPDATE schedules SET next_fire_at = $2 WHERE id = $1")
        .bind(schedule_id)
        .bind(due_slot)
        .execute(&pool)
        .await
        .expect("force due schedule slot");

    let _ = cicd::outbox::fire_due_schedules(&pool).await;

    let (fire_count,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM schedule_fires WHERE schedule_id = $1")
            .bind(schedule_id)
            .fetch_one(&pool)
            .await
            .expect("count schedule fires");
    assert_eq!(fire_count, 1);

    let (scheduled_for, pipeline_id, status): (chrono::DateTime<Utc>, Uuid, String) =
        sqlx::query_as(
            "SELECT scheduled_for, pipeline_id, status FROM schedule_fires WHERE schedule_id = $1",
        )
        .bind(schedule_id)
        .fetch_one(&pool)
        .await
        .expect("fetch schedule fire");
    assert_eq!(scheduled_for, due_slot);
    assert_eq!(status, "triggered");

    let (trigger_count,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM pipeline_triggers \
         WHERE project_id = $1 AND source = 'schedule' AND pipeline_id = $2",
    )
    .bind(project_id)
    .bind(pipeline_id)
    .fetch_one(&pool)
    .await
    .expect("count schedule pipeline trigger");
    assert_eq!(trigger_count, 1);

    let (last_fired_at, next_fire_at): (chrono::DateTime<Utc>, chrono::DateTime<Utc>) =
        sqlx::query_as("SELECT last_fired_at, next_fire_at FROM schedules WHERE id = $1")
            .bind(schedule_id)
            .fetch_one(&pool)
            .await
            .expect("fetch updated schedule");
    assert_eq!(last_fired_at, due_slot);
    assert!(next_fire_at > due_slot);

    let _ = cicd::outbox::fire_due_schedules(&pool).await;

    let (pipeline_count,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM pipelines WHERE project_id = $1")
            .bind(project_id)
            .fetch_one(&pool)
            .await
            .expect("count project pipelines");
    assert_eq!(pipeline_count, 1);

    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project_id)
        .execute(&pool)
        .await
        .expect("cleanup project");
}

#[tokio::test]
async fn in_app_notification_events_are_fanned_out_and_delivered() {
    let pool = test_pool().await;
    let project_id = Uuid::new_v4();
    let pipeline_id = Uuid::new_v4();
    let enabled_notification_id = Uuid::new_v4();
    let unsupported_notification_id = Uuid::new_v4();
    let project_name = format!("it-notifications-{}", project_id.simple());

    sqlx::query("INSERT INTO projects (id, name, repository_url) VALUES ($1, $2, $3)")
        .bind(project_id)
        .bind(&project_name)
        .bind("https://example.invalid/notifications.git")
        .execute(&pool)
        .await
        .expect("insert project");
    sqlx::query(
        "INSERT INTO pipelines (id, project_id, git_ref, status) VALUES ($1, $2, 'main', 'failed')",
    )
    .bind(pipeline_id)
    .bind(project_id)
    .execute(&pool)
    .await
    .expect("insert pipeline");
    sqlx::query(
        "INSERT INTO notification_configs (id, project_id, channel, target, enabled) \
         VALUES ($1, $2, 'in_app', 'dashboard', true), ($3, $2, 'slack', 'https://hooks.invalid/test', true)",
    )
    .bind(enabled_notification_id)
    .bind(project_id)
    .bind(unsupported_notification_id)
    .execute(&pool)
    .await
    .expect("insert notification configs");

    let event_id = cicd::outbox::emit_pipeline_event(
        &pool,
        project_id,
        pipeline_id,
        "pipeline.failed",
        "failed",
    )
    .await
    .expect("emit pipeline event");

    let notification_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_messages \
         WHERE event_id = $1 AND channel = 'notification'",
    )
    .bind(event_id)
    .fetch_one(&pool)
    .await
    .expect("count notification rows");
    assert_eq!(notification_rows, 1);

    let delivered = cicd::outbox::deliver_due(&pool, &reqwest::Client::new()).await;
    assert!(delivered >= 1);

    let app = cicd::api::app(Some(pool.clone()));
    let response = app
        .oneshot(
            Request::get(format!(
                "/api/v1/projects/{project_id}/notification-events?limit=10"
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let events = response_json(response).await;
    assert_eq!(events.as_array().unwrap().len(), 1);
    assert_eq!(events[0]["event_id"], event_id.to_string());
    assert_eq!(events[0]["channel"], "in_app");
    assert_eq!(events[0]["target"], "dashboard");
    assert_eq!(events[0]["event_type"], "pipeline.failed");
    assert_eq!(events[0]["pipeline_id"], pipeline_id.to_string());
    assert_eq!(events[0]["status"], "failed");
    assert!(events[0]["message"].as_str().unwrap().contains("failed"));
    assert!(events[0]["delivered_at"].is_string());
    assert!(events[0]["last_error"].is_null());

    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project_id)
        .execute(&pool)
        .await
        .expect("cleanup project");
}

#[tokio::test]
async fn exhausted_outbox_message_is_not_retried() {
    let pool = test_pool().await;
    let event_id = Uuid::new_v4();
    let aggregate_id = Uuid::new_v4();
    let message_id = Uuid::new_v4();

    sqlx::query(
        "INSERT INTO domain_events (id, event_type, aggregate_type, aggregate_id, payload) \
         VALUES ($1, 'pipeline.failed', 'pipeline', $2, '{}'::jsonb)",
    )
    .bind(event_id)
    .bind(aggregate_id)
    .execute(&pool)
    .await
    .expect("insert domain event");
    sqlx::query(
        "INSERT INTO outbox_messages \
            (id, event_id, subscription_id, channel, destination, payload, attempts, next_attempt_at, last_error) \
         VALUES ($1, $2, 'notification:test', 'notification', $3, $4, $5, now() - interval '1 hour', 'already failed')",
    )
    .bind(message_id)
    .bind(event_id)
    .bind(cicd::outbox::notification_destination(aggregate_id))
    .bind(serde_json::json!({
        "channel": "slack",
        "event": "pipeline.failed",
        "pipeline_id": aggregate_id,
        "status": "failed",
    }))
    .bind(cicd::outbox::MAX_ATTEMPTS)
    .execute(&pool)
    .await
    .expect("insert exhausted outbox message");

    let _ = cicd::outbox::deliver_due(&pool, &reqwest::Client::new()).await;

    let (attempts, last_error): (i32, Option<String>) =
        sqlx::query_as("SELECT attempts, last_error FROM outbox_messages WHERE id = $1")
            .bind(message_id)
            .fetch_one(&pool)
            .await
            .expect("fetch exhausted outbox message");
    assert_eq!(attempts, cicd::outbox::MAX_ATTEMPTS);
    assert_eq!(last_error.as_deref(), Some("already failed"));

    sqlx::query("DELETE FROM domain_events WHERE id = $1")
        .bind(event_id)
        .execute(&pool)
        .await
        .expect("cleanup domain event");
}

#[tokio::test]
async fn failed_outbox_delivery_records_attempt_and_can_be_requeued() {
    let pool = test_pool().await;
    let project_id = Uuid::new_v4();
    let pipeline_id = Uuid::new_v4();
    let event_id = Uuid::new_v4();
    let message_id = Uuid::new_v4();
    let project_name = format!("it-outbox-history-{}", project_id.simple());

    sqlx::query("INSERT INTO projects (id, name, repository_url) VALUES ($1, $2, $3)")
        .bind(project_id)
        .bind(&project_name)
        .bind("https://example.invalid/outbox-history.git")
        .execute(&pool)
        .await
        .expect("insert project");
    sqlx::query(
        "INSERT INTO pipelines (id, project_id, git_ref, status) VALUES ($1, $2, 'main', 'failed')",
    )
    .bind(pipeline_id)
    .bind(project_id)
    .execute(&pool)
    .await
    .expect("insert pipeline");
    sqlx::query(
        "INSERT INTO domain_events (id, event_type, aggregate_type, aggregate_id, payload) \
         VALUES ($1, 'pipeline.failed', 'pipeline', $2, $3)",
    )
    .bind(event_id)
    .bind(pipeline_id)
    .bind(serde_json::json!({ "project_id": project_id, "status": "failed" }))
    .execute(&pool)
    .await
    .expect("insert domain event");
    sqlx::query(
        "INSERT INTO outbox_messages \
            (id, event_id, project_id, subscription_id, channel, destination, payload, attempts, next_attempt_at) \
         VALUES ($1, $2, $3, 'notification:external', 'notification', $4, $5, $6, now() - interval '1 minute')",
    )
    .bind(message_id)
    .bind(event_id)
    .bind(project_id)
    .bind(cicd::outbox::notification_destination(project_id))
    .bind(serde_json::json!({
        "channel": "email",
        "target": "ops@example.invalid",
        "event": "pipeline.failed",
        "project_id": project_id,
        "pipeline_id": pipeline_id,
        "status": "failed",
    }))
    .bind(cicd::outbox::MAX_ATTEMPTS - 1)
    .execute(&pool)
    .await
    .expect("insert retrying outbox message");

    // deliver_due returns a process-wide count, so parallel integration tests
    // may contribute unrelated delivered messages. The assertions below are
    // scoped to this unsupported-channel message.
    let _delivered = cicd::outbox::deliver_due(&pool, &reqwest::Client::new()).await;

    let (attempts, failed_at, last_error): (
        i32,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<String>,
    ) = sqlx::query_as("SELECT attempts, failed_at, last_error FROM outbox_messages WHERE id = $1")
        .bind(message_id)
        .fetch_one(&pool)
        .await
        .expect("fetch failed delivery");
    assert_eq!(attempts, cicd::outbox::MAX_ATTEMPTS);
    assert!(failed_at.is_some());
    assert_eq!(
        last_error.as_deref(),
        Some("unsupported notification channel: email")
    );

    let app = cicd::api::app(Some(pool.clone()));
    let response = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/api/v1/projects/{project_id}/outbox-deliveries?status=failed&channel=notification&limit=10"
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let deliveries = response_json(response).await;
    assert_eq!(deliveries.as_array().unwrap().len(), 1);
    assert_eq!(deliveries[0]["id"], message_id.to_string());
    assert_eq!(deliveries[0]["status"], "failed");
    assert_eq!(deliveries[0]["attempts"], cicd::outbox::MAX_ATTEMPTS);
    assert_eq!(deliveries[0]["generation"], 0);

    let detail = app
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/outbox-deliveries/{message_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(detail.status(), StatusCode::OK);
    let detail = response_json(detail).await;
    assert_eq!(detail["delivery"]["id"], message_id.to_string());
    assert_eq!(detail["attempts"].as_array().unwrap().len(), 1);
    assert_eq!(
        detail["attempts"][0]["attempt_number"],
        cicd::outbox::MAX_ATTEMPTS
    );
    assert_eq!(detail["attempts"][0]["outcome"], "failed");

    let requeue = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/outbox-deliveries/{message_id}/requeue"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(requeue.status(), StatusCode::OK);
    let requeue = response_json(requeue).await;
    let replay_id = Uuid::parse_str(requeue["id"].as_str().unwrap()).unwrap();
    assert_eq!(requeue["replay_of_id"], message_id.to_string());

    let (generation, replay_of_id, replay_attempts, replay_failed_at): (
        i32,
        Option<Uuid>,
        i32,
        Option<chrono::DateTime<chrono::Utc>>,
    ) = sqlx::query_as(
        "SELECT generation, replay_of_id, attempts, failed_at FROM outbox_messages WHERE id = $1",
    )
    .bind(replay_id)
    .fetch_one(&pool)
    .await
    .expect("fetch replay delivery");
    assert_eq!(generation, 1);
    assert_eq!(replay_of_id, Some(message_id));
    assert_eq!(replay_attempts, 0);
    assert!(replay_failed_at.is_none());

    let pending = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/api/v1/projects/{project_id}/outbox-deliveries?status=pending&limit=10"
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pending.status(), StatusCode::OK);
    let pending = response_json(pending).await;
    assert_eq!(pending.as_array().unwrap().len(), 1);
    assert_eq!(pending[0]["id"], replay_id.to_string());

    let non_failed_requeue = app
        .oneshot(
            Request::post(format!("/api/v1/outbox-deliveries/{replay_id}/requeue"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(non_failed_requeue.status(), StatusCode::BAD_REQUEST);

    sqlx::query("DELETE FROM outbox_messages WHERE event_id = $1")
        .bind(event_id)
        .execute(&pool)
        .await
        .expect("cleanup outbox");
    sqlx::query("DELETE FROM domain_events WHERE id = $1")
        .bind(event_id)
        .execute(&pool)
        .await
        .expect("cleanup domain event");
    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project_id)
        .execute(&pool)
        .await
        .expect("cleanup project");
}

async fn response_json(response: axum::response::Response) -> serde_json::Value {
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    serde_json::from_slice(&body).expect("json response")
}

async fn login_access_token(app: axum::Router, username: &str, password: &str) -> String {
    let response = app
        .oneshot(
            Request::post("/api/v1/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"username":"{username}","password":"{password}"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    response_json(response).await["access_token"]
        .as_str()
        .expect("access token")
        .to_owned()
}
