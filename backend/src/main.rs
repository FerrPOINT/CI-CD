use cicd::{
    api::app_with_git_and_config, config::RuntimeConfig, config::parse_roles, dispatch_signal,
    outbox, platform, runner,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Fleet-standard tracing setup (JSON in prod via PDLC_LOG_JSON).
    pdlc_telemetry::init_tracing("forge-cicd");
    // Wire the storage-layer enqueue hook to the in-process dispatch signal
    // (the storage crate itself is transport-agnostic; ADR-0012).
    let config = RuntimeConfig::from_env()?;
    let workspace = if matches!(
        config.runner.mode,
        cicd::config::RunnerMode::Docker | cicd::config::RunnerMode::Kubernetes
    ) {
        std::path::PathBuf::from("/workspaces")
    } else {
        std::env::temp_dir()
    };
    let mut custody = vec![
        cicd::quiescence::CustodyRoot::workspace(workspace),
        cicd::quiescence::CustodyRoot::git(config.git.root.clone()),
    ];
    custody.extend(
        config
            .sdlc_workspace
            .local_delivery_root
            .iter()
            .cloned()
            .map(cicd::quiescence::CustodyRoot::delivery),
    );
    custody.extend(
        std::env::var_os("CICD_LOCAL_PG_ROOT")
            .map(std::path::PathBuf::from)
            .map(cicd::quiescence::CustodyRoot::delivery),
    );
    cicd::quiescence::initialize("server", custody)?;
    cicd::quiescence::wait_for_enrollment().await?;
    let startup = cicd::quiescence::enter_startup()?;
    let git = config.git.to_git_config();
    if git.internal_token.is_none() {
        tracing::warn!(
            "CICD_GIT_INTERNAL_TOKEN is not set; post-receive ingress is trusted-local only"
        );
    }
    let pool = cicd::request_database::pool_options()
        .max_connections(10)
        .connect(&config.database.url)
        .await?;
    // ADR-0008: apply versioned migrations; legacy databases created by the
    // historical startup bootstrap adopt cleanly because 0001 reproduces it
    // idempotently (IF NOT EXISTS).
    let migrator = cicd::migrations_from_path(&config.database.migrations_dir).await?;
    migrator.run(&pool).await?;
    // Legacy issuer is an explicit immutable operator assertion, never a name/email lookup.
    match std::env::var("CICD_IDENTITY_LEGACY_ISSUER") {
        Ok(issuer) => {
            if std::env::var("CICD_AUTH__CENTRAL_ISSUER").ok().as_deref() != Some(issuer.as_str()) {
                return Err("identity legacy issuer must match configured Auth issuer".into());
            }
            cicd::store::identity_deletion::IdentityDeletionStore::new(pool.clone())
                .bind_legacy_issuer(&issuer)
                .await?;
        }
        Err(std::env::VarError::NotPresent) => (),
        Err(error) => return Err(error.into()),
    }

    // Stage 5 scale-out: split roles across deployments. A pod started with
    // CICD_ROLES=api serves only the API; CICD_ROLES=worker runs the outbox/
    // scheduler/retention workers; CICD_ROLES=runner executes jobs. Default
    // keeps the all-in-one behaviour for local compose.
    let roles = parse_roles(&std::env::var("CICD_ROLES").unwrap_or_default());
    tracing::info!(?roles, "role selection");
    let has_role = |role: &str| roles.contains(&role);

    let running = runner::RunningJobs::default();
    let runner_runtime = runner::RuntimeRunnerConfig::from_config(&config);
    if config.runner.embedded_enabled && (roles.is_empty() || has_role("runner")) {
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
    } else if roles.is_empty() || has_role("runner") {
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
    if roles.is_empty() || has_role("worker") {
        let workspace_pool = pool.clone();
        let workspace_config = config.clone();
        tokio::spawn(async move {
            cicd::development_workspaces::worker::supervisor(workspace_pool, workspace_config)
                .await;
        });
        let catalog_pool = pool.clone();
        let sync_pool = pool.clone();
        let sync_config = config.clone();
        tokio::spawn(async move {
            cicd::repository_sync::supervisor(sync_pool, sync_config).await;
        });
        tokio::spawn(async move {
            cicd::workspace_projects::supervisor(catalog_pool).await;
        });
        let outbox_pool = pool.clone();
        let outbox_git_root = config.git.root.clone();
        tokio::spawn(async move {
            outbox::supervisor_loop_with_git_root(outbox_pool, outbox_git_root).await;
        });
    }

    if roles.is_empty() || has_role("worker") {
        let artifact_retention_pool = pool.clone();
        let artifact_retention_config = config.artifacts.clone();
        tokio::spawn(async move {
            platform::artifact_retention_loop_with_config(
                artifact_retention_pool,
                artifact_retention_config,
            )
            .await;
        });
    }

    let _runner_work_listener = dispatch_signal::spawn_runner_work_listener(pool.clone());

    // Upgrade path: rewrite post-receive hooks so template fixes (e.g. the
    // blank continuation line bug) reach repositories created by older code.
    let hook_pool = pool.clone();
    let hook_git = git.clone();
    tokio::spawn(async move {
        let _effect = startup;
        cicd::git_host::ensure_post_receive_hooks(&hook_pool, &hook_git).await;
    });

    let bind = config.http.bind.clone();
    let app = app_with_git_and_config(Some(pool), git, Some(running), config)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!(%bind, "CI/CD API listening");
    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    );
    if std::env::var_os("CICD_QUIESCENCE_DIR").is_some() {
        use std::future::IntoFuture;
        tokio::select! {
            result = server.into_future() => result?,
            result = cicd::quiescence::wait_for_exit() => result?,
        }
    } else {
        server.await?;
    }
    Ok(())
}
