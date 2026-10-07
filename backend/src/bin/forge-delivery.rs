//! Privileged owner-local verification CLI; never an agent dispatch or HTTP deployment adapter.
use anyhow::{Context, ensure};
use cicd::{
    api::AppState,
    config::RuntimeConfig,
    domain::task_delivery::{DeliveryCommand, DeliveryStatus},
};
use clap::Parser;
use std::{path::PathBuf, sync::Arc};
use uuid::Uuid;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    project_id: Uuid,
    #[arg(long)]
    command_file: PathBuf,
    #[arg(long, conflicts_with = "readback")]
    reconcile: bool,
    #[arg(long)]
    readback: bool,
}

async fn run(args: Args) -> anyhow::Result<bool> {
    if !args.readback {
        ensure!(
            std::env::var("CICD_LOCAL_DELIVERY_MODE").ok().as_deref() == Some("local-verification"),
            "explicit owner local verification mode required"
        );
    }
    let config = RuntimeConfig::from_env()?;
    ensure!(
        std::fs::metadata(&args.command_file)?.len() <= 16 * 1024,
        "command exceeds bound"
    );
    let bytes = std::fs::read(&args.command_file)?;
    ensure!(bytes.len() <= 16 * 1024, "command exceeds bound");
    let command: DeliveryCommand = serde_json::from_slice(&bytes)?;
    let token = std::env::var("CICD_LOCAL_DELIVERY_TOKEN")
        .context("owner machine credential unavailable")?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .acquire_timeout(std::time::Duration::from_secs(3))
        .connect(&config.database.url)
        .await?;
    let state = Arc::new(AppState {
        pool: Some(pool),
        auth_secret: config.auth.secret.clone(),
        git: config.git.to_git_config(),
        config,
        running_jobs: None,
        rate_limiter: Arc::new(cicd::rate_limit::RateLimiter::default()),
    });
    let result = cicd::task_delivery::local_command(
        state,
        args.project_id,
        &token,
        command,
        args.reconcile,
        args.readback,
    )
    .await?;
    println!("{}", serde_json::to_string(&result)?);
    Ok(result
        .reconciled_receipt
        .as_ref()
        .unwrap_or(&result.receipt)
        .status
        == DeliveryStatus::Verified)
}

#[tokio::main]
async fn main() {
    let status = match run(Args::parse()).await {
        Ok(true) => 0,
        Ok(false) => 2,
        Err(_) => {
            eprintln!(
                "Локальная delivery operation отклонена или недоступна; состояние target сохранено для owner readback."
            );
            1
        }
    };
    std::process::exit(status);
}
