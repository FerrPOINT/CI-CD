use cicd::{api::app_with_git, git_host::GitConfig, runner};

/// Versioned SQLx migrations embedded from backend/migrations (ADR-0008).
const MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");
use sqlx::postgres::PgPoolOptions;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let database_url = std::env::var("CICD_DATABASE_URL").expect("CICD_DATABASE_URL is required");
    let bind = std::env::var("CICD_BIND").unwrap_or_else(|_| "0.0.0.0:22801".into());
    let git = GitConfig {
        root: std::path::PathBuf::from(
            std::env::var("CICD_GIT_ROOT").unwrap_or_else(|_| "/var/lib/forge/git".into()),
        ),
        token: std::env::var("CICD_GIT_TOKEN").ok(),
        internal_token: std::env::var("CICD_GIT_INTERNAL_TOKEN").ok(),
    };
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await?;
    // ADR-0008: apply versioned migrations; legacy databases created by the
    // historical startup bootstrap adopt cleanly because 0001 reproduces it
    // idempotently (IF NOT EXISTS).
    MIGRATOR.run(&pool).await?;

    // Embedded runner: executes queued jobs stage by stage.
    let running = runner::RunningJobs::default();
    let supervisor_pool = pool.clone();
    let supervisor_running = running.clone();
    tokio::spawn(async move {
        runner::supervisor_loop(supervisor_pool, supervisor_running).await;
    });

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!(%bind, "CI/CD API listening");
    axum::serve(listener, app_with_git(Some(pool), git, Some(running))).await?;
    Ok(())
}
