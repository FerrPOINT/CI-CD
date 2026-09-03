pub mod api;
pub mod auth;
pub mod authz;
pub(crate) mod body_limits;
pub mod config;
pub mod dispatch_signal;
pub mod metrics;
pub mod outbox;
pub mod rate_limit;
pub(crate) mod schedule;
use std::path::{Path, PathBuf};

/// Versioned migrations directory shared by the server binary and integration tests (ADR-0008).
pub fn migrations_path() -> PathBuf {
    config::migrations_dir_from_env()
}

/// Load versioned migrations from the configured source directory.
pub async fn migrations() -> Result<sqlx::migrate::Migrator, sqlx::migrate::MigrateError> {
    let path = migrations_path();
    migrations_from_path(&path).await
}

/// Load versioned migrations from an explicit typed runtime config path.
pub async fn migrations_from_path(
    path: &Path,
) -> Result<sqlx::migrate::Migrator, sqlx::migrate::MigrateError> {
    sqlx::migrate::Migrator::new(path).await
}
pub mod domain;
pub mod git_host;
pub mod platform;
pub mod pulls;
pub mod runner;
pub mod runner_protocol;
pub mod store;

pub use cicd_domain as domain_types;
