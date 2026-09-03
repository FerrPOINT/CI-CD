use cicd::{api::app_with_git, dispatch_signal, git_host::GitConfig, outbox, platform, runner};

use sqlx::postgres::PgPoolOptions;
use tracing_subscriber::EnvFilter;

const INSECURE_GIT_INTERNAL_TOKEN: &str = "forge-internal-dev-token";

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
        token: optional_secret_env("CICD_GIT_TOKEN"),
        internal_token: git_internal_token_from_env()?,
    };
    if git.internal_token.is_none() {
        tracing::warn!(
            "CICD_GIT_INTERNAL_TOKEN is not set; post-receive ingress is trusted-local only"
        );
    }
    let _runner_queue_timeout_seconds = runner::runner_queue_timeout_seconds_from_env(
        std::env::var("CICD_RUNNER_QUEUE_TIMEOUT_SECONDS").ok(),
    )?;
    let _artifact_retention_days = platform::artifact_retention_days_from_env(
        std::env::var("CICD_ARTIFACT_RETENTION_DAYS").ok(),
    )?;
    let embedded_runner_enabled = runner::embedded_runner_enabled_from_env(
        std::env::var("CICD_EMBEDDED_RUNNER_ENABLED").ok(),
    )?;
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await?;
    // ADR-0008: apply versioned migrations; legacy databases created by the
    // historical startup bootstrap adopt cleanly because 0001 reproduces it
    // idempotently (IF NOT EXISTS).
    let migrator = cicd::migrations().await?;
    migrator.run(&pool).await?;

    let running = runner::RunningJobs::default();
    if embedded_runner_enabled {
        // Embedded runner: executes queued jobs stage by stage.
        let supervisor_pool = pool.clone();
        let supervisor_running = running.clone();
        tokio::spawn(async move {
            runner::supervisor_loop(supervisor_pool, supervisor_running).await;
        });
    } else {
        tracing::warn!(
            "CICD_EMBEDDED_RUNNER_ENABLED is false; queued jobs require an external forge-runner"
        );
        let maintenance_pool = pool.clone();
        tokio::spawn(async move {
            runner::maintenance_loop(maintenance_pool).await;
        });
    }

    // ADR-0006: outbox delivery + scheduler worker.
    let outbox_pool = pool.clone();
    tokio::spawn(async move {
        outbox::supervisor_loop(outbox_pool).await;
    });

    let artifact_retention_pool = pool.clone();
    tokio::spawn(async move {
        platform::artifact_retention_loop(artifact_retention_pool).await;
    });

    let _runner_work_listener = dispatch_signal::spawn_runner_work_listener(pool.clone());

    // Upgrade path: rewrite post-receive hooks so template fixes (e.g. the
    // blank continuation line bug) reach repositories created by older code.
    let hook_pool = pool.clone();
    let hook_git = git.clone();
    tokio::spawn(async move {
        cicd::git_host::ensure_post_receive_hooks(&hook_pool, &hook_git).await;
    });

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!(%bind, "CI/CD API listening");
    axum::serve(listener, app_with_git(Some(pool), git, Some(running))).await?;
    Ok(())
}

fn optional_secret_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn git_internal_token_from_env() -> Result<Option<String>, std::io::Error> {
    normalize_git_internal_token(std::env::var("CICD_GIT_INTERNAL_TOKEN").ok())
}

fn normalize_git_internal_token(raw: Option<String>) -> Result<Option<String>, std::io::Error> {
    let token = raw
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if token.as_deref() == Some(INSECURE_GIT_INTERNAL_TOKEN) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "CICD_GIT_INTERNAL_TOKEN uses the removed insecure development default; generate a unique value or leave it blank only for isolated local development",
        ));
    }
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::{INSECURE_GIT_INTERNAL_TOKEN, normalize_git_internal_token};

    #[test]
    fn git_internal_token_normalization_rejects_known_insecure_default() {
        assert!(normalize_git_internal_token(None).unwrap().is_none());
        assert!(
            normalize_git_internal_token(Some(" \t ".to_string()))
                .unwrap()
                .is_none()
        );
        assert_eq!(
            normalize_git_internal_token(Some(" unique-token ".to_string())).unwrap(),
            Some("unique-token".to_string())
        );
        assert!(
            normalize_git_internal_token(Some(INSECURE_GIT_INTERNAL_TOKEN.to_string())).is_err()
        );
    }
}
