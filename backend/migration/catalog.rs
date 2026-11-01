//! Transaction setup for technical migrations; released SQL bodies stay immutable.
use sqlx::migrate::{MigrateError, Migrator};
use std::{borrow::Cow, path::Path};

// A deferred request-proof clock otherwise queues an event for each backfill
// UPDATE, preventing later ALTER TABLE in that same migration transaction.
// Technical migrations check this clock immediately. Normal request transactions
// and unrelated deferrable constraints retain their original timing.
const REQUEST_CLOCK: &str = r#"DO $forge_migration_clock$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_constraint
               WHERE conname='forge_human_request_clock'
                 AND connamespace='public'::regnamespace AND condeferrable) THEN
        SET CONSTRAINTS public.forge_human_request_clock IMMEDIATE;
    END IF;
END
$forge_migration_clock$;
"#;

pub async fn load(path: &Path) -> Result<Migrator, MigrateError> {
    let mut catalog = Migrator::new(path).await?;
    for migration in catalog.migrations.to_mut() {
        if !migration.no_tx {
            // The checksum still identifies the unchanged released SQL body.
            // Transaction setup belongs to this versioned executable, just as
            // SQLx's BEGIN/COMMIT and ledger statements do.
            migration.sql = Cow::Owned(format!("{REQUEST_CLOCK}{}", migration.sql));
        }
    }
    Ok(catalog)
}
