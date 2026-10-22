//! Filled dev-copy rehearsal; never connects to a working database implicitly.
use serde_json::Value;
use sqlx::PgPool;
use std::collections::BTreeMap;
async fn columns(pool: &PgPool) -> BTreeMap<String, Vec<String>> {
    let rows:Vec<(String,String)>=sqlx::query_as("SELECT c.table_name,c.column_name FROM information_schema.columns c JOIN information_schema.tables t ON t.table_schema=c.table_schema AND t.table_name=c.table_name WHERE c.table_schema='public' AND t.table_type='BASE TABLE' AND c.table_name<>'_sqlx_migrations' ORDER BY c.table_name,c.ordinal_position").fetch_all(pool).await.unwrap();
    let mut result = BTreeMap::<String, Vec<String>>::new();
    for (table, column) in rows {
        result.entry(table).or_default().push(column);
    }
    result
}
async fn fingerprints(
    pool: &PgPool,
    columns: &BTreeMap<String, Vec<String>>,
) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    for (table, cols) in columns {
        let names = cols
            .iter()
            .map(|col| format!("\"{}\"", col.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(",");
        let query = format!(
            "SELECT md5(coalesce(string_agg(to_jsonb(t)::text,E'\\n' ORDER BY to_jsonb(t)::text),'')) FROM (SELECT {names} FROM public.\"{}\") t",
            table.replace('"', "\"\"")
        );
        result.insert(
            table.clone(),
            sqlx::query_scalar(&query).fetch_one(pool).await.unwrap(),
        );
    }
    result
}
#[tokio::test]
#[ignore = "requires a restored filled pre-policy database and isolated process"]
async fn filled_schema_preserves_history_and_does_not_invent_execution_owners() {
    let url = std::env::var("FORGE_FILLED_TEST_DATABASE_URL")
        .expect("explicit owned restored database required");
    let pool = PgPool::connect(&url).await.unwrap();
    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(database, "forge_product_transport_test");
    let version: i64 = sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version, 95);
    for table in ["projects", "repositories", "pipelines"] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(count > 0, "filled {table} required");
    }
    let ledger: Value = sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(t) ORDER BY version) FROM _sqlx_migrations t",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let control = columns(&pool).await;
    let before = fingerprints(&pool, &control).await;
    cicd::migrations().await.unwrap().run(&pool).await.unwrap();
    assert_eq!(
        fingerprints(&pool, &control).await,
        before,
        "legacy columns changed"
    );
    let after: Value = sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(t) ORDER BY version) FROM _sqlx_migrations t WHERE version<=95",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(after, ledger);
    for table in [
        "pipeline_human_owners",
        "execution_attempt_human_owners",
        "forge_confirmed_user_roles",
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0, "historical privileges/owners cannot be invented");
    }
    let guessed: i64 =
        sqlx::query_scalar("SELECT count(*) FROM schedules WHERE owner_subject IS NOT NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(guessed, 0);
    let spaces_active: bool =
        sqlx::query_scalar("SELECT active FROM forge_space_admission_state WHERE singleton")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        !spaces_active,
        "independent space policy must not activate during role migration"
    );
    cicd::migrations().await.unwrap().run(&pool).await.unwrap();
    assert_eq!(fingerprints(&pool, &control).await, before);
    println!(
        "Filled Forge migration95->104: {} original tables/columns and receipts preserved; repeat, no fabricated owners/roles, space activation remains OFF",
        control.len()
    );
}
