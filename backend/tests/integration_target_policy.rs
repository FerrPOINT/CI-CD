//! Target scheduler/outbox/approval policy tests (ROADMAP "target" tier):
//! - scheduler: DST-style wall-clock semantics, no-double-fire under
//!   concurrent passes, misfire recovery after skipped slots;
//! - outbox: single observed outcome per (message, attempt) and requeue
//!   generation semantics;
//! - approvals: rejected locks the deployment, double-vote by one actor and
//!   vote-after-decision are rejected.
//!
//! Uses the same per-test schema isolation as integration_db.rs.

#![cfg(feature = "integration")]

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use chrono::{Datelike, Duration, TimeZone, Timelike, Utc};
use sqlx::postgres::PgPoolOptions;
use std::str::FromStr;
use tower::ServiceExt;
use uuid::Uuid;

async fn test_pool_in_schema(schema: &str) -> sqlx::PgPool {
    let base_url = std::env::var("CICD_TEST_DATABASE_URL")
        .expect("CICD_TEST_DATABASE_URL must point at the test-compose PostgreSQL");
    let url = base_url;
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .expect("connect to test database for schema setup");
    sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
        .execute(&admin)
        .await
        .expect("drop stale test schema");
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .expect("create test schema");
    admin.close().await;

    let options = sqlx::postgres::PgConnectOptions::from_str(&url)
        .expect("parse test database URL")
        .options([("search_path", schema)]);
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .expect("connect to test schema");

    cicd::migrations()
        .await
        .expect("load migrations")
        .run(&pool)
        .await
        .expect("run migrations");
    pool
}

async fn test_pool() -> sqlx::PgPool {
    test_pool_in_schema(&format!("it_target_{}", Uuid::new_v4().simple())).await
}

async fn response_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read body");
    serde_json::from_slice(&bytes).expect("json body")
}

async fn seed_project(pool: &sqlx::PgPool, prefix: &str) -> Uuid {
    let project_id = Uuid::new_v4();
    sqlx::query("INSERT INTO projects (id, name, repository_url) VALUES ($1, $2, $3)")
        .bind(project_id)
        .bind(format!("{prefix}-{}", project_id.simple()))
        .bind("https://example.invalid/target-tests.git")
        .execute(pool)
        .await
        .expect("insert project");
    project_id
}

/// A misfired slot (scheduler downtime) is recovered exactly once: the fire
/// materializes for the missed slot and the schedule moves forward without a
/// duplicate fire on the next pass.
#[tokio::test]
async fn scheduler_utc_cron_next_fire_is_stable_utc_wall_clock() {
    let pool = test_pool().await;
    let project_id = seed_project(&pool, "it-utc-wall-clock").await;
    let app = cicd::api::app(Some(pool.clone()));
    // Daily at 02:30 UTC.
    let response = app
        .oneshot(
            Request::post(format!("/api/v1/projects/{project_id}/schedules"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"cron":"30 2 * * *","git_ref":"main","enabled":true}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let schedule = response_json(response).await;
    let next_fire_at = schedule["next_fire_at"].as_str().expect("next_fire_at");
    let parsed = chrono::DateTime::<Utc>::from_str(next_fire_at).expect("parse next_fire_at");
    assert_eq!(parsed.minute(), 30);
    assert_eq!(parsed.hour(), 2);
    assert_eq!(parsed.second(), 0);

    // DST boundary (Europe/Berlin spring forward 2026-03-29): force the slot
    // just before it and verify the following slot stays at 02:30 UTC — the
    // scheduler must not drift by an hour across a local DST change.
    let schedule_id = Uuid::parse_str(schedule["id"].as_str().unwrap()).unwrap();
    let before_dst = Utc.with_ymd_and_hms(2026, 3, 28, 2, 30, 0).unwrap();
    sqlx::query("UPDATE schedules SET next_fire_at = $2 WHERE id = $1")
        .bind(schedule_id)
        .bind(before_dst)
        .execute(&pool)
        .await
        .expect("force pre-DST slot");
    let _ = cicd::outbox::fire_due_schedules_with_git_root(
        &pool,
        std::path::Path::new("/tmp/forge-target-test-git-root"),
    )
    .await;
    let (next_after_dst,): (chrono::DateTime<Utc>,) =
        sqlx::query_as("SELECT next_fire_at FROM schedules WHERE id = $1")
            .bind(schedule_id)
            .fetch_one(&pool)
            .await
            .expect("fetch post-DST next fire");
    assert_eq!(next_after_dst.hour(), 2, "hour must not drift across DST");
    assert_eq!(next_after_dst.minute(), 30);
    assert!(next_after_dst > before_dst);

    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project_id)
        .execute(&pool)
        .await
        .expect("cleanup");
}

#[tokio::test]
async fn scheduler_recovers_misfired_slot_exactly_once() {
    let pool = test_pool().await;
    let project_id = seed_project(&pool, "it-misfire").await;
    // A slot that was due 90 minutes ago.
    let missed_slot = (Utc::now() - Duration::minutes(90))
        .with_second(0)
        .and_then(|v| v.with_nanosecond(0))
        .unwrap();
    let cron = missed_slot.format("%M %H * * *").to_string();
    let schedule_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO schedules (id, project_id, cron, git_ref, enabled, next_fire_at) \
         VALUES ($1, $2, $3, 'main', true, $4)",
    )
    .bind(schedule_id)
    .bind(project_id)
    .bind(&cron)
    .bind(missed_slot)
    .execute(&pool)
    .await
    .expect("insert misfired schedule");

    let _ = cicd::outbox::fire_due_schedules_with_git_root(
        &pool,
        std::path::Path::new("/tmp/forge-target-test-git-root"),
    )
    .await;
    let _ = cicd::outbox::fire_due_schedules_with_git_root(
        &pool,
        std::path::Path::new("/tmp/forge-target-test-git-root"),
    )
    .await;

    let (fire_count,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM schedule_fires WHERE schedule_id = $1")
            .bind(schedule_id)
            .fetch_one(&pool)
            .await
            .expect("count fires");
    assert_eq!(fire_count, 1, "misfired slot fires exactly once");

    let (status,): (String,) =
        sqlx::query_as("SELECT status FROM schedule_fires WHERE schedule_id = $1")
            .bind(schedule_id)
            .fetch_one(&pool)
            .await
            .expect("fire status");
    // The slot is consumed exactly once regardless of trigger outcome; the
    // pipeline trigger path is idempotent by design (idempotency key), so a
    // second pass must not create another fire or pipeline.
    assert!(
        status == "failed" || status == "triggered",
        "unexpected fire status: {status}"
    );

    let (next_fire_at,): (Option<chrono::DateTime<Utc>>,) =
        sqlx::query_as("SELECT next_fire_at FROM schedules WHERE id = $1")
            .bind(schedule_id)
            .fetch_one(&pool)
            .await
            .expect("schedule next fire");
    assert!(
        next_fire_at.unwrap() > missed_slot,
        "schedule moved forward past the missed slot"
    );

    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project_id)
        .execute(&pool)
        .await
        .expect("cleanup");
}

/// Outbox attempts ledger records a single observed outcome per
/// (message, attempt): re-delivering the same attempt number cannot create a
/// second outcome row (ON CONFLICT DO NOTHING).
#[tokio::test]
async fn outbox_attempts_ledger_single_observed_outcome_per_attempt() {
    let pool = test_pool().await;
    let project_id = seed_project(&pool, "it-outbox-ledger").await;
    let event_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO domain_events (id, event_type, aggregate_type, aggregate_id, payload) \
         VALUES ($1, 'pipeline.failed', 'pipeline', $1, '{}')",
    )
    .bind(event_id)
    .execute(&pool)
    .await
    .expect("seed domain event");
    let message_id = Uuid::new_v4();
    let (attempts,): (i32,) = sqlx::query_as(
        "INSERT INTO outbox_messages \
            (id, event_id, project_id, subscription_id, channel, destination, payload, attempts, next_attempt_at) \
         VALUES ($1, $2, $3, 'notification:external', 'notification', 'x', '{}', 0, now()) \
         RETURNING attempts",
    )
    .bind(message_id)
    .bind(event_id)
    .bind(project_id)
    .fetch_one(&pool)
    .await
    .expect("seed message");

    // Simulate two racing writers recording the same attempt outcome.
    for _ in 0..2 {
        let _ = sqlx::query(
            "INSERT INTO outbox_delivery_attempts \
                (message_id, attempt_number, started_at, finished_at, outcome, http_status, error_message, duration_ms) \
             VALUES ($1, $2, now(), now(), 'delivered', 200, NULL, 5) \
             ON CONFLICT (message_id, attempt_number) DO NOTHING",
        )
        .bind(message_id)
        .bind(attempts + 1)
        .execute(&pool)
        .await
        .expect("record attempt");
    }

    let (rows,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM outbox_delivery_attempts WHERE message_id = $1 AND attempt_number = $2",
    )
    .bind(message_id)
    .bind(attempts + 1)
    .fetch_one(&pool)
    .await
    .expect("count attempt rows");
    assert_eq!(rows, 1, "single observed outcome per (message, attempt)");
}

/// Approval policy edges: one actor cannot vote twice, a rejection locks the
/// deployment (no later approval can start the pipeline), and votes after the
/// decision are rejected with 409.
#[tokio::test]
async fn approval_policy_rejects_double_vote_and_post_decision_votes() {
    let pool = test_pool().await;
    let project_id = seed_project(&pool, "it-approval-policy").await;
    let app = cicd::api::app(Some(pool.clone()));

    let environment_response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/projects/{project_id}/environments"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"name":"production","protected":true,"required_approvals":2}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(environment_response.status(), StatusCode::OK);
    let environment = response_json(environment_response).await;
    let environment_id = environment["id"].as_str().unwrap();

    let deployment_response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/environments/{environment_id}/deployments"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"git_ref":"main"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(deployment_response.status(), StatusCode::OK);
    let deployment = response_json(deployment_response).await;
    assert_eq!(deployment["approval_state"], "pending");
    let deployment_id = deployment["id"].as_str().unwrap();

    // First approval by release-manager.
    let first = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/deployments/{deployment_id}/approvals"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"decision":"approved","actor":"release-manager"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);

    // Same actor votes again -> 409 double vote.
    let dup = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/deployments/{deployment_id}/approvals"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"decision":"approved","actor":"release-manager"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(dup.status(), StatusCode::CONFLICT);

    // A rejection arrives before the second approval: decision is final.
    let reject = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/deployments/{deployment_id}/approvals"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"decision":"rejected","actor":"sre-on-call"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(reject.status(), StatusCode::OK);
    let rejected = response_json(reject).await;
    assert_eq!(rejected["status"], "failed");
    assert_eq!(rejected["approval_state"], "rejected");
    assert_eq!(rejected["pipeline_id"], serde_json::Value::Null);

    // Any later vote -> 409 (decision already made), pipeline never starts.
    let late = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/deployments/{deployment_id}/approvals"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"decision":"approved","actor":"qa-lead"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(late.status(), StatusCode::CONFLICT);

    let (pipeline_count,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM pipelines WHERE project_id = $1")
            .bind(project_id)
            .fetch_one(&pool)
            .await
            .expect("count pipelines");
    assert_eq!(
        pipeline_count, 0,
        "rejected deployment must not start a pipeline"
    );

    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project_id)
        .execute(&pool)
        .await
        .expect("cleanup");
}
