pub mod api;
pub mod auth;
pub mod authz;
pub(crate) mod body_limits;
pub mod dispatch_signal;
pub mod metrics;
pub mod outbox;
pub mod rate_limit;
pub(crate) mod schedule;
use std::path::PathBuf;

/// Versioned migrations directory shared by the server binary and integration tests (ADR-0008).
pub fn migrations_path() -> PathBuf {
    std::env::var("CICD_MIGRATIONS_DIR")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("migrations"))
}

/// Load versioned migrations from the configured source directory.
pub async fn migrations() -> Result<sqlx::migrate::Migrator, sqlx::migrate::MigrateError> {
    let path = migrations_path();
    sqlx::migrate::Migrator::new(path.as_path()).await
}
pub mod domain;
pub mod git_host;
pub mod platform;
pub mod pulls;
pub mod runner;
pub mod runner_protocol;
pub mod store;

pub use cicd_domain as domain_types;
