//! Real-DB integration tests (TEST_PLAN §real-DB level).
//!
//! Requires a running test-compose PostgreSQL:
//!   docker compose -f backend/docker-compose.test.yml up -d postgres-test
//!   CICD_TEST_DATABASE_URL=postgres://forge_owner:...@postgres-test:5432/forge_test_cicd
//! Each test uses an isolated schema-unique UUID namespace; tables are shared
//! but rows are UUID-scoped, so parallel runs do not collide.

#![cfg(feature = "integration")]

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;
use uuid::Uuid;

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
    cicd::auth::create_session(&pool, user_id, &stored)
        .await
        .expect("create session");
    let su = cicd::auth::session_user(&pool, &stored)
        .await
        .expect("session user");
    assert_eq!(su.user_id, user_id);

    let (_uid, new_token) = cicd::auth::rotate_session(&pool, &stored)
        .await
        .expect("rotate session");
    assert!(cicd::auth::session_user(&pool, &stored).await.is_err());
    assert!(cicd::auth::session_user(&pool, &new_token).await.is_ok());

    // Disabled user cannot authenticate even with a live session.
    sqlx::query("UPDATE users SET enabled = false WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("disable user");
    assert!(cicd::auth::session_user(&pool, &new_token).await.is_err());

    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("cleanup user");
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
async fn cancel_pipeline_marks_open_attempts_canceled() {
    let pool = test_pool().await;
    let project_id = Uuid::new_v4();
    let pipeline_id = Uuid::new_v4();
    let stage_id = Uuid::new_v4();
    let running_job_id = Uuid::new_v4();
    let queued_job_id = Uuid::new_v4();
    let running_attempt_id = Uuid::new_v4();
    let queued_attempt_id = Uuid::new_v4();
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
        "INSERT INTO execution_attempts (id, job_id, attempt_no, status, trigger) \
         VALUES ($1, $2, 1, 'queued', 'initial')",
    )
    .bind(queued_attempt_id)
    .bind(queued_job_id)
    .execute(&pool)
    .await
    .expect("insert queued attempt");

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

async fn response_json(response: axum::response::Response) -> serde_json::Value {
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    serde_json::from_slice(&body).expect("json response")
}
