use cicd::{
    api::app_with_git_and_config, config::RuntimeConfig, dispatch_signal, outbox, platform, runner,
};

use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Fleet-standard tracing setup (JSON in prod via SDLC_LOG_JSON).
    sdlc_telemetry::init_tracing("forge-cicd");
    let config = RuntimeConfig::from_env()?;
    let git = config.git.to_git_config();
    if git.internal_token.is_none() {
        tracing::warn!(
            "CICD_GIT_INTERNAL_TOKEN is not set; post-receive ingress is trusted-local only"
        );
    }
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database.url)
        .await?;
    // ADR-0008: apply versioned migrations; legacy databases created by the
    // historical startup bootstrap adopt cleanly because 0001 reproduces it
    // idempotently (IF NOT EXISTS).
    let migrator = cicd::migrations_from_path(&config.database.migrations_dir).await?;
    migrator.run(&pool).await?;

    let running = runner::RunningJobs::default();
    let runner_runtime = runner::RuntimeRunnerConfig::from_config(&config);
    if config.runner.embedded_enabled {
        // Embedded runner: executes queued jobs stage by stage.
        let supervisor_pool = pool.clone();
        let supervisor_running = running.clone();
        let supervisor_config = runner_runtime.clone();
        tokio::spawn(async move {
            runner::supervisor_loop_with_config(
                supervisor_pool,
                supervisor_running,
                supervisor_config,
            )
            .await;
        });
    } else {
        tracing::warn!(
            "CICD_EMBEDDED_RUNNER_ENABLED is false; queued jobs require an external forge-runner"
        );
        let maintenance_pool = pool.clone();
        let maintenance_config = runner_runtime.clone();
        tokio::spawn(async move {
            runner::maintenance_loop_with_config(maintenance_pool, maintenance_config).await;
        });
    }

    // ADR-0006: outbox delivery + scheduler worker.
    let outbox_pool = pool.clone();
    let outbox_git_root = config.git.root.clone();
    tokio::spawn(async move {
        outbox::supervisor_loop_with_git_root(outbox_pool, outbox_git_root).await;
    });

    let artifact_retention_pool = pool.clone();
    let artifact_retention_config = config.artifacts.clone();
    tokio::spawn(async move {
        platform::artifact_retention_loop_with_config(
            artifact_retention_pool,
            artifact_retention_config,
        )
        .await;
    });

    let _runner_work_listener = dispatch_signal::spawn_runner_work_listener(pool.clone());

    // Upgrade path: rewrite post-receive hooks so template fixes (e.g. the
    // blank continuation line bug) reach repositories created by older code.
    let hook_pool = pool.clone();
    let hook_git = git.clone();
    tokio::spawn(async move {
        cicd::git_host::ensure_post_receive_hooks(&hook_pool, &hook_git).await;
    });

    let bind = config.http.bind.clone();
    let app = app_with_git_and_config(Some(pool), git, Some(running), config)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!(%bind, "CI/CD API listening");
    axum::serve(listener, app).await?;
    Ok(())
}
