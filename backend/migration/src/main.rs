//! `cicd-migrate` — versioned SQLx migration runner (ADR-0008, MIGRATION_CONTRACT).
//!
//! Applies `backend/migrations/*.sql` with the standard sqlx history table and
//! an advisory lock so concurrent instances cannot race. `--dry-run` lists
//! pending migrations without touching the database.

use anyhow::Context;
use clap::Parser;
use std::path::PathBuf;

#[derive(clap::Parser)]
#[command(name = "cicd-migrate", about = "Forge CI/CD migration runner", version)]
struct Args {
    /// PostgreSQL URL, e.g. postgres://forge_owner:...@host:5432/forge
    #[arg(long, env = "CICD_TEST_DATABASE_URL", hide_env_values = true)]
    database_url: String,

    /// List pending migrations without applying them.
    #[arg(long)]
    dry_run: bool,

    /// Fail instead of warning when there is nothing to apply.
    #[arg(long)]
    verify: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_secs(10))
        .connect(&args.database_url)
        .await
        .context("connect to database")?;

    let lock: i64 = 0x464F524745; // "FORGE"
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(lock)
        .execute(&pool)
        .await
        .context("acquire migration advisory lock")?;

    let migrator = load_migrator().await?;
    let result = run(&pool, &args, &migrator).await;

    let _ = sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(lock)
        .execute(&pool)
        .await;
    result
}

async fn run(
    pool: &sqlx::PgPool,
    args: &Args,
    migrator: &sqlx::migrate::Migrator,
) -> anyhow::Result<()> {
    let applied = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM _sqlx_migrations")
        .fetch_one(pool)
        .await
        .unwrap_or(0);
    let total = migrator.iter().count() as i64;
    let pending = total - applied;

    if args.dry_run {
        println!("applied: {applied}, pending: {pending} of {total}");
        for m in migrator.iter().skip(applied as usize) {
            println!("pending: {} {}", m.version, m.description);
        }
        return Ok(());
    }

    if pending == 0 {
        if args.verify {
            anyhow::bail!("verify failed: history has {applied} entries but migrator has {total}");
        }
        println!("nothing to apply ({applied} migrations in history)");
        return Ok(());
    }

    migrator
        .run(pool)
        .await
        .with_context(|| format!("apply migrations ({pending} pending)"))?;
    println!("applied {pending} migration(s), total {total}");
    Ok(())
}

fn migrations_path() -> PathBuf {
    std::env::var("CICD_MIGRATIONS_DIR")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../migrations"))
}

async fn load_migrator() -> anyhow::Result<sqlx::migrate::Migrator> {
    let path = migrations_path();
    sqlx::migrate::Migrator::new(path.as_path())
        .await
        .with_context(|| format!("load migrations from {}", path.display()))
}
