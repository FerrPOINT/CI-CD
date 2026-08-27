//! Real-DB integration tests (TEST_PLAN §real-DB level).
//!
//! Requires a running test-compose PostgreSQL:
//!   docker compose -f backend/docker-compose.test.yml up -d postgres-test
//!   CICD_TEST_DATABASE_URL=postgres://forge_owner:...@postgres-test:5432/forge_test_cicd
//! Each test uses an isolated schema-unique UUID namespace; tables are shared
//! but rows are UUID-scoped, so parallel runs do not collide.

#![cfg(feature = "integration")]

use sqlx::postgres::PgPoolOptions;
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
