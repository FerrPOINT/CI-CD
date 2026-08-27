pub mod api;
pub mod auth;
pub mod authz;

/// Versioned migrations shared by the server binary and integration tests (ADR-0008).
pub fn migrations() -> sqlx::migrate::Migrator {
    sqlx::migrate!("./migrations")
}
pub mod domain;
pub mod git_host;
pub mod platform;
pub mod pulls;
pub mod runner;
pub mod store;

pub use cicd_domain as domain_types;
