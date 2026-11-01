//! A filled technical backfill must coexist with deferred human request clocks.
#![cfg(feature = "integration")]
use sqlx::{
    PgPool, Row,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{path::Path, str::FromStr};
use uuid::Uuid;

const BASELINE: &str = r#"
CREATE TABLE migration_clock_probe(id integer PRIMARY KEY,value text NOT NULL);
CREATE FUNCTION probe_request_clock() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF current_setting('forge.request_valid_until',true) IS NOT NULL
    AND current_setting('forge.request_valid_until',true)<>''
    AND clock_timestamp()>=current_setting('forge.request_valid_until',true)::timestamptz THEN
  RAISE EXCEPTION 'forge_request_authority_expired' USING ERRCODE='42501';
 END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER forge_human_request_clock AFTER UPDATE ON migration_clock_probe
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION probe_request_clock();
INSERT INTO migration_clock_probe VALUES(1,'retained payload');
CREATE TABLE deferred_parent(id integer PRIMARY KEY);
CREATE TABLE deferred_child(parent integer REFERENCES deferred_parent(id) DEFERRABLE INITIALLY DEFERRED);
"#;
const BACKFILL: &str = r#"
ALTER TABLE migration_clock_probe ADD COLUMN source_id integer;
UPDATE migration_clock_probe SET source_id=id;
ALTER TABLE migration_clock_probe ALTER COLUMN source_id SET NOT NULL;
INSERT INTO deferred_child VALUES(2);
INSERT INTO deferred_parent VALUES(2);
"#;

async fn database() -> (PgPool, PgPool, String) {
    let url =
        std::env::var("CICD_TEST_DATABASE_URL").expect("explicit disposable PostgreSQL required");
    let options = PgConnectOptions::from_str(&url).unwrap();
    assert!(
        options
            .get_database()
            .is_some_and(|name| name.starts_with("forge_test_"))
    );
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    let name = format!("forge_test_migration_clock_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&admin)
        .await
        .unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(3)
        .connect_with(options.database(&name))
        .await
        .unwrap();
    (admin, pool, name)
}
fn catalog(root: &Path) {
    std::fs::write(root.join("0001_probe.sql"), BASELINE).unwrap();
    std::fs::write(root.join("0002_backfill.sql"), BACKFILL).unwrap();
    std::fs::write(
        root.join("0003_no_transaction.sql"),
        "-- no-transaction\nSELECT 1;\n",
    )
    .unwrap();
}
async fn ledger(pool: &PgPool) -> Vec<(i64, Vec<u8>)> {
    sqlx::query_as("SELECT version,checksum FROM _sqlx_migrations ORDER BY version")
        .fetch_all(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn populated_backfill_preserves_body_checksums_and_normal_deferred_proofs() {
    let folder = tempfile::tempdir().unwrap();
    catalog(folder.path());
    let raw = sqlx::migrate::Migrator::new(folder.path()).await.unwrap();
    let prepared = cicd::migration_catalog::load(folder.path()).await.unwrap();
    assert!(prepared.locking && !prepared.ignore_missing);
    for (before, after) in raw.iter().zip(prepared.iter()) {
        assert_eq!(before.checksum, after.checksum);
        assert_eq!(before.no_tx, after.no_tx);
        if before.no_tx {
            assert_eq!(before.sql, after.sql);
        }
    }
    let (admin, pool, name) = database().await;
    let mut connection = pool.acquire().await.unwrap();
    let failure = raw
        .run_direct(&mut *connection)
        .await
        .unwrap_err()
        .to_string();
    assert!(failure.contains("pending trigger events"), "{failure}");
    connection.close().await.unwrap(); // SQLx retains its advisory lock on errors.
    let before = ledger(&pool).await;
    assert_eq!(before.len(), 1);
    prepared.run(&pool).await.unwrap();
    let data: (i32, String, i32) =
        sqlx::query_as("SELECT id,value,source_id FROM migration_clock_probe")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(data, (1, "retained payload".into(), 1));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM deferred_child")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    let applied = ledger(&pool).await;
    assert_eq!(&applied[..1], before.as_slice());
    assert_eq!(
        applied,
        raw.iter()
            .map(|m| (m.version, m.checksum.to_vec()))
            .collect::<Vec<_>>()
    );
    prepared.run(&pool).await.unwrap();
    assert_eq!(ledger(&pool).await, applied);
    raw.run(&pool).await.unwrap(); // A rollback reader validates the unchanged released body hashes.
    // A normal transaction still checks the original proof at commit, after
    // UPDATE succeeded; migration setup must not leak into the connection.
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('forge.request_valid_until','2000-01-01T00:00:00Z',true)")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("UPDATE migration_clock_probe SET value='must roll back'")
        .execute(&mut *transaction)
        .await
        .unwrap();
    assert!(
        transaction
            .commit()
            .await
            .unwrap_err()
            .to_string()
            .contains("forge_request_authority_expired")
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT value FROM migration_clock_probe")
            .fetch_one(&pool)
            .await
            .unwrap(),
        "retained payload"
    );
    // A changed released body still fails checksum validation.
    std::fs::write(
        folder.path().join("0002_backfill.sql"),
        format!("{BACKFILL}\nSELECT 2;\n"),
    )
    .unwrap();
    let changed = cicd::migration_catalog::load(folder.path()).await.unwrap();
    let mut connection = pool.acquire().await.unwrap();
    assert!(changed.run_direct(&mut *connection).await.is_err());
    connection.close().await.unwrap();
    assert_eq!(ledger(&pool).await, applied);
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {name}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}

#[tokio::test]
async fn concurrent_migration_loaders_keep_the_original_advisory_lock() {
    let folder = tempfile::tempdir().unwrap();
    catalog(folder.path());
    let left = cicd::migration_catalog::load(folder.path()).await.unwrap();
    let right = cicd::migration_catalog::load(folder.path()).await.unwrap();
    let (admin, pool, name) = database().await;
    let (first, second) = tokio::join!(left.run(&pool), right.run(&pool));
    first.unwrap();
    second.unwrap();
    assert_eq!(ledger(&pool).await.len(), 3);
    let row = sqlx::query("SELECT id,value,source_id FROM migration_clock_probe")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>("value"), "retained payload");
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {name}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
