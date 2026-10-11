//! Privileged target helper: complete immutable SQLx catalog, isolated database only.
use anyhow::{Context, ensure};
use sqlx::{Connection, Row};
use std::{path::PathBuf, time::Duration};

async fn run() -> anyhow::Result<()> {
    ensure!(
        std::env::var("CICD_LOCAL_DELIVERY_MODE").ok().as_deref() == Some("local-verification"),
        "explicit isolated mode required"
    );
    let url = std::env::var("CICD_LOCAL_PG_DATABASE_URL")?;
    let dir = PathBuf::from(std::env::var("CICD_LOCAL_PG_MIGRATIONS")?);
    ensure!(
        dir.is_absolute() && !std::fs::symlink_metadata(&dir)?.file_type().is_symlink(),
        "isolated catalog required"
    );
    let options = url
        .parse::<sqlx::postgres::PgConnectOptions>()?
        .application_name("forge_pg_migrate")
        .options([("statement_timeout", "8000"), ("lock_timeout", "3000")]);
    let mut conn = sqlx::PgConnection::connect_with(&options).await?;
    let actual = sqlx::query(
        "SELECT current_database() AS database,system_identifier::text FROM pg_control_system()",
    )
    .fetch_one(&mut conn)
    .await?;
    let name = actual.try_get::<String, _>("database")?;
    ensure!(
        name.starts_with("forge_test_pg_") && name.len() <= 63,
        "only disposable target database supported"
    );
    ensure!(
        actual.try_get::<String, _>("system_identifier")?
            == std::env::var("CICD_LOCAL_PG_SYSTEM_ID")?,
        "target identity mismatch"
    );
    if !name.contains("_rehearsal_") {
        let pid: i32 = std::env::var("CICD_LOCAL_PG_GUARD_PID")?.parse()?;
        ensure!(sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE locktype='advisory' AND granted AND pid=$1 AND classid=1179603527::oid AND objid=1162889028::oid)")
            .bind(pid).fetch_one(&mut conn).await?,"owner target guard unavailable");
    }
    let migrator = sqlx::migrate::Migrator::new(dir.as_path()).await?;
    tokio::time::timeout(Duration::from_secs(20), migrator.run(&mut conn))
        .await
        .context("migration deadline; outcome unknown")??;
    println!("{{\"schema\":\"forge/isolated-pg-migration/v1\",\"completed\":true}}");
    Ok(())
}
#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!("Изолированная миграция не подтверждена; требуется original-key readback.");
        std::process::exit(1);
    }
}
