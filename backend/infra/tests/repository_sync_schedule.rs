//! Manual PostgreSQL integration boundary; production store and full released migrations.
use cicd_app::repository_sync::{self as app, Access};
use cicd_domain::repository_sync::{Direction, Provider, SaveConnection, Settings};
use cicd_infra::{repository_sync::PgSyncStore, secrets::Secrets};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::str::FromStr;
use uuid::Uuid;
#[path = "../../migration/catalog.rs"]
mod catalog;

async fn fixture() -> (sqlx::PgPool, PgSyncStore, Uuid, Access, Secrets) {
    let url =
        std::env::var("CICD_TEST_DATABASE_URL").expect("explicit isolated PostgreSQL required");
    let schema = format!("hourly_{}", Uuid::new_v4().simple());
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(
            PgConnectOptions::from_str(&url)
                .unwrap()
                .options([("search_path", schema.as_str())]),
        )
        .await
        .unwrap();
    catalog::load(std::path::Path::new("../migrations"))
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();
    let repository = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
        .bind(repository)
        .bind(format!("hourly-{repository}"))
        .execute(&pool)
        .await
        .unwrap();
    let store = PgSyncStore {
        pool: pool.clone(),
        worker_enabled: true,
        interval_seconds: 3600,
    };
    let access = Access {
        actor_id: Uuid::new_v4(),
        project_id: None,
        admin: true,
        machine: false,
        writable: true,
    };
    (pool, store, repository, access, Secrets(Some([7; 32])))
}
fn command(provider: Provider, automatic: bool) -> SaveConnection {
    SaveConnection {
        expected_version: 0,
        settings: Settings {
            provider,
            instance_url: "https://github.com".into(),
            repository_path: "fixture/service".into(),
            direction: Direction::Pull,
            automatic,
            branch: Some("master".into()),
            protected_branch_updates: Default::default(),
        },
        token: Some("synthetic-hourly-sync-credential".into()),
    }
}

#[tokio::test]
async fn multiple_sync_hourly_ignores_dirty_and_pulls_before_pushes() {
    let (pool, mut store, repository, access, secrets) = fixture().await;
    store.interval_seconds = 3600;
    let push = Uuid::new_v4();
    for (id, direction) in [(push, Direction::Push), (repository, Direction::Pull)] {
        let mut input = command(Provider::Github, true);
        input.settings.direction = direction;
        input.settings.branch = Some("master".into());
        app::save(
            &store.scoped(id),
            &secrets,
            repository,
            Uuid::new_v4(),
            access,
            input,
            &[],
        )
        .await
        .unwrap();
    }
    sqlx::query("UPDATE repository_sync_connections SET dirty=true,next_check_at=now()+interval '1 hour' WHERE repository_id=$1").bind(repository).execute(&pool).await.unwrap();
    store.schedule(false).await.unwrap();
    assert!(
        store.candidates().await.unwrap().is_empty(),
        "dirty must not bypass hourly mode"
    );
    sqlx::query("UPDATE repository_sync_connections SET next_check_at=now()-interval '1 second' WHERE id=$1").bind(push).execute(&pool).await.unwrap();
    store.schedule(false).await.unwrap();
    let candidates = store.candidates().await.unwrap();
    assert_eq!(candidates.len(), 1);
    let pull_run = store.run(repository, candidates[0].0).await.unwrap();
    assert_eq!(pull_run.settings.direction, Direction::Pull);
    assert_eq!(pull_run.settings.branch.as_deref(), Some("master"));
    assert_eq!(pull_run.source, "schedule");
    let seconds: f64 = sqlx::query_scalar("SELECT EXTRACT(EPOCH FROM(next_check_at-now()))::double precision FROM repository_sync_connections WHERE id=$1").bind(repository).fetch_one(&pool).await.unwrap();
    assert!(seconds > 3590.0 && seconds <= 3600.0);
    let owner = Uuid::new_v4();
    let (_, generation) = store.claim(pull_run.id, owner).await.unwrap().unwrap();
    store
        .finish(pull_run.id, owner, generation, "success", None)
        .await
        .unwrap();
    let next = store.candidates().await.unwrap();
    let push_run = store.run(repository, next[0].0).await.unwrap();
    assert_eq!(push_run.connection_id, push);
    assert_eq!(push_run.settings.direction, Direction::Push);
    // Completion/change does not schedule a second incoming run in the same hour.
    store.signal(repository).await.unwrap();
    store.schedule(false).await.unwrap();
    assert_eq!(
        store
            .runs_at(repository, repository, 20, 0)
            .await
            .unwrap()
            .1,
        1
    );
    pool.close().await;
}
