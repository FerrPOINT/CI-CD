//! Qualify both released SQLx histories on independent populated databases.
#![cfg(feature = "integration")]
use serde_json::Value;
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{borrow::Cow, path::Path, str::FromStr};
use uuid::Uuid;

fn subset(catalog: &sqlx::migrate::Migrator, maximum: i64) -> sqlx::migrate::Migrator {
    sqlx::migrate::Migrator {
        migrations: Cow::Owned(
            catalog
                .iter()
                .filter(|m| m.version <= maximum)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    }
}
async fn snapshot(pool: &PgPool, maximum: i64) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(m) ORDER BY version) FROM _sqlx_migrations m WHERE version<=$1",
    )
    .bind(maximum)
    .fetch_one(pool)
    .await
    .unwrap()
}
async fn rows(pool: &PgPool) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('projects',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM projects t),'pipelines',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM pipelines t),'environments',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM environments t),'deployments',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM deployments t))")
        .fetch_one(pool).await.unwrap()
}

fn original_columns(mut observed: Value, original: &Value) -> Value {
    for (table, rows) in observed.as_object_mut().unwrap() {
        let columns = original[table].as_array().unwrap()[0].as_object().unwrap();
        for row in rows.as_array_mut().unwrap() {
            row.as_object_mut()
                .unwrap()
                .retain(|key, _| columns.contains_key(key));
        }
    }
    observed
}

#[tokio::test]
async fn released_lineages_preserve_populated_rows_and_checksums() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::env::temp_dir().join(format!("forge-lineage-{}", Uuid::new_v4()));
    assert!(
        std::process::Command::new("python3")
            .arg(root.join("../scripts/migration_lineages.py"))
            .arg("--backend")
            .arg(root)
            .arg("--output")
            .arg(&output)
            .status()
            .unwrap()
            .success()
    );
    let main = cicd::migrations_from_path(&root.join("migrations"))
        .await
        .unwrap();
    let native = cicd::migrations_from_path(&output).await.unwrap();
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
    for (catalog, wrong, historical) in [(&main, &native, 107), (&native, &main, 99)] {
        let name = format!("forge_test_lineage_{}", Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE DATABASE {name}"))
            .execute(&admin)
            .await
            .unwrap();
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(options.clone().database(&name))
            .await
            .unwrap();
        subset(catalog, 95).run(&pool).await.unwrap();
        let project = Uuid::new_v4();
        let environment = Uuid::new_v4();
        let pipeline = Uuid::new_v4();
        sqlx::query("INSERT INTO projects(id,name,repository_url) VALUES($1,'retained-lineage','https://example.test/source.git')")
            .bind(project).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO environments(id,project_id,name) VALUES($1,$2,'retained')")
            .bind(environment)
            .bind(project)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO pipelines(id,project_id,git_ref,status) VALUES($1,$2,'main','success')",
        )
        .bind(pipeline)
        .bind(project)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO deployments(id,environment_id,pipeline_id,git_ref,status,request_key) VALUES($1,$2,$3,'main','success',$4)")
            .bind(Uuid::new_v4()).bind(environment).bind(pipeline).bind(Uuid::new_v4()).execute(&pool).await.unwrap();
        subset(catalog, historical).run(&pool).await.unwrap();
        let before = rows(&pool).await;
        let history = snapshot(&pool, historical).await;
        // Close the failed-validation connection: SQLx retains its advisory lock on errors.
        let mut connection = pool.acquire().await.unwrap();
        assert!(
            wrong.run_direct(&mut *connection).await.is_err(),
            "opposite released lineage must be rejected"
        );
        connection.close().await.unwrap();
        assert_eq!(snapshot(&pool, historical).await, history);
        catalog.run(&pool).await.unwrap();
        assert_eq!(snapshot(&pool, historical).await, history);
        assert_eq!(original_columns(rows(&pool).await, &before), before);
        let applied = snapshot(&pool, i64::MAX).await;
        catalog.run(&pool).await.unwrap();
        assert_eq!(snapshot(&pool, i64::MAX).await, applied);
        assert_eq!(original_columns(rows(&pool).await, &before), before);
        pool.close().await;
        sqlx::query(&format!("DROP DATABASE {name}"))
            .execute(&admin)
            .await
            .unwrap();
    }
    admin.close().await;
    std::fs::remove_dir_all(output).unwrap();
}
