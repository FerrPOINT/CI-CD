pub mod api;
pub mod auth;
pub mod authz;
pub(crate) mod body_limits;
pub mod dispatch_signal;
pub mod metrics;
pub mod outbox;
pub mod rate_limit;
pub(crate) mod schedule;

/// Versioned migrations shared by the server binary and integration tests (ADR-0008).
pub fn migrations() -> sqlx::migrate::Migrator {
    sqlx::migrate!("./migrations")
}
pub mod domain;
pub mod git_host;
pub mod platform;
pub mod pulls;
pub mod runner;
pub mod runner_protocol;
pub mod store;

pub use cicd_domain as domain_types;
