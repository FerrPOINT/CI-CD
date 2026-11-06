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
    assert_eq!(
        push_run.created_at, pull_run.created_at,
        "paired runs share a transaction clock"
    );
    // A later incoming run cannot jump ahead of an older outgoing run.
    sqlx::query("INSERT INTO repository_sync_runs(id,repository_id,connection_version,settings,source,state,connection_id,created_at) VALUES($1,$2,$3,$4,'schedule','queued',$5,now()+interval '1 second')")
        .bind(Uuid::new_v4()).bind(repository).bind(pull_run.connection_version)
        .bind(serde_json::to_value(&pull_run.settings).unwrap()).bind(repository)
        .execute(&pool).await.unwrap();
    assert_eq!(store.candidates().await.unwrap()[0].0, push_run.id);
    // Completion/change does not schedule a second incoming run in the same hour.
    store.signal(repository).await.unwrap();
    store.schedule(false).await.unwrap();
    assert_eq!(
        store
            .runs_at(repository, repository, 20, 0)
            .await
            .unwrap()
            .1,
        2
    );
    pool.close().await;
}

async fn administrator(pool: &sqlx::PgPool, access: Access) {
    sqlx::query("INSERT INTO users(id,username,role) VALUES($1,$2,'admin')")
        .bind(access.actor_id)
        .bind(format!("schedule-{}", access.actor_id))
        .execute(pool)
        .await
        .unwrap();
}
fn policy(
    seconds: i64,
    on_change: bool,
    expected_version: i64,
) -> cicd_domain::repository_sync::SaveSchedule {
    cicd_domain::repository_sync::SaveSchedule {
        expected_version,
        settings: cicd_domain::repository_sync::Schedule {
            interval_seconds: seconds,
            on_change,
        },
    }
}
#[tokio::test]
async fn global_sync_policy_is_versioned_audited_replayable_and_role_checked() {
    use cicd_app::repository_sync::{Error, ScheduleStore};
    use cicd_domain::repository_sync::Schedule;
    let (pool, store, _, access, _) = fixture().await;
    administrator(&pool, access).await;
    let defaults = Schedule {
        interval_seconds: 300,
        on_change: true,
    };
    let initial = store.read_schedule(defaults).await.unwrap();
    assert_eq!(initial.version, 0);
    assert_eq!(initial.settings, defaults);
    for denied in [
        Access {
            admin: false,
            ..access
        },
        Access {
            machine: true,
            ..access
        },
        Access {
            project_id: Some(Uuid::new_v4()),
            ..access
        },
        Access {
            writable: false,
            ..access
        },
    ] {
        assert!(matches!(
            app::save_schedule(
                &store,
                Uuid::new_v4(),
                denied,
                policy(3600, false, 0),
                defaults
            )
            .await,
            Err(Error::Forbidden)
        ));
    }
    let key = Uuid::new_v4();
    let first = app::save_schedule(&store, key, access, policy(3600, false, 0), defaults)
        .await
        .unwrap();
    assert_eq!(first.version, 1);
    let second = app::save_schedule(
        &store,
        Uuid::new_v4(),
        access,
        policy(7200, true, 1),
        defaults,
    )
    .await
    .unwrap();
    assert_eq!(second.version, 2);
    let replay = app::save_schedule(&store, key, access, policy(3600, false, 0), defaults)
        .await
        .unwrap();
    assert_eq!(
        replay, first,
        "retry returns its original immutable result after later changes"
    );
    assert_eq!(store.read_schedule(defaults).await.unwrap(), second);
    assert!(matches!(
        app::save_schedule(&store, key, access, policy(1800, false, 0), defaults).await,
        Err(Error::Conflict("idempotency_conflict"))
    ));
    assert!(matches!(
        app::save_schedule(&store, key, access, policy(3600, false, i64::MAX), defaults).await,
        Err(Error::Conflict("idempotency_conflict"))
    ));
    let a = app::save_schedule(
        &store,
        Uuid::new_v4(),
        access,
        policy(1800, false, 2),
        defaults,
    );
    let b = app::save_schedule(
        &store,
        Uuid::new_v4(),
        access,
        policy(900, true, 2),
        defaults,
    );
    let (a, b) = tokio::join!(a, b);
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert!(matches!(
        a.as_ref().err().or_else(|| b.as_ref().err()),
        Some(Error::Conflict("sync_schedule_version_conflict"))
    ));
    let audit:i64=sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action='repository_sync.schedule_updated' AND actor=$1")
        .bind(access.actor_id.to_string()).fetch_one(&pool).await.unwrap();
    assert_eq!(audit, 3);
    let history: i64 = sqlx::query_scalar("SELECT count(*) FROM repository_sync_schedule")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(history, 3);
    assert!(
        sqlx::query("UPDATE repository_sync_schedule SET interval_seconds=1")
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM repository_sync_schedule")
            .execute(&pool)
            .await
            .is_err()
    );
    let restarted = PgSyncStore {
        interval_seconds: 1,
        ..store.clone()
    };
    let current = restarted
        .read_schedule(Schedule {
            interval_seconds: 1,
            on_change: true,
        })
        .await
        .unwrap();
    assert_eq!(
        current.version, 3,
        "saved policy survives restart and environment changes"
    );
    sqlx::query("UPDATE users SET role='viewer' WHERE id=$1")
        .bind(access.actor_id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        matches!(
            app::save_schedule(
                &store,
                Uuid::new_v4(),
                access,
                policy(3600, false, 3),
                defaults
            )
            .await,
            Err(Error::Forbidden)
        ),
        "stale admin claim cannot save"
    );
    pool.close().await;
}

#[tokio::test]
async fn global_sync_policy_updates_existing_worker_and_next_check_without_restart() {
    use cicd_domain::repository_sync::Schedule;
    let (pool, mut store, repository, access, secrets) = fixture().await;
    administrator(&pool, access).await;
    store.interval_seconds = 300;
    app::save(
        &store,
        &secrets,
        repository,
        Uuid::new_v4(),
        access,
        command(Provider::Github, true),
        &[],
    )
    .await
    .unwrap();
    sqlx::query("UPDATE repository_sync_connections SET next_check_at=now()-interval '1 second' WHERE id=$1")
        .bind(repository).execute(&pool).await.unwrap();
    store.schedule(false).await.unwrap();
    let initial = store
        .runs_at(repository, repository, 20, 0)
        .await
        .unwrap()
        .0;
    assert_eq!(initial.len(), 1);
    let owner = Uuid::new_v4();
    let (_, generation) = store.claim(initial[0].id, owner).await.unwrap().unwrap();
    store
        .finish(initial[0].id, owner, generation, "success", None)
        .await
        .unwrap();
    // A manual run must not move the automatic schedule's original clock.
    sqlx::query("INSERT INTO repository_sync_runs(id,repository_id,connection_version,settings,source,state,connection_id,created_at,finished_at) VALUES($1,$2,1,$3,'manual','success',$2,$4+interval '30 minutes',$4+interval '30 minutes')")
        .bind(Uuid::new_v4()).bind(repository).bind(serde_json::to_value(&initial[0].settings).unwrap()).bind(initial[0].created_at).execute(&pool).await.unwrap();
    let defaults = Schedule {
        interval_seconds: 300,
        on_change: true,
    };
    app::save_schedule(
        &store,
        Uuid::new_v4(),
        access,
        policy(3600, false, 0),
        defaults,
    )
    .await
    .unwrap();
    let difference:f64=sqlx::query_scalar("SELECT EXTRACT(EPOCH FROM(next_check_at-$2::timestamptz))::double precision FROM repository_sync_connections WHERE id=$1")
        .bind(repository).bind(initial[0].created_at).fetch_one(&pool).await.unwrap();
    assert_eq!(difference, 3600.0);
    store.signal(repository).await.unwrap();
    store.schedule(true).await.unwrap();
    assert_eq!(
        store
            .runs_at(repository, repository, 20, 0)
            .await
            .unwrap()
            .1,
        2,
        "saved on_change=false overrides startup true"
    );
    sqlx::query("UPDATE repository_sync_connections SET next_check_at=now()-interval '1 second' WHERE id=$1").bind(repository).execute(&pool).await.unwrap();
    store.schedule(true).await.unwrap();
    let latest = store
        .runs_at(repository, repository, 20, 0)
        .await
        .unwrap()
        .0;
    let automatic = latest.iter().find(|r| r.state == "queued").unwrap();
    assert_eq!(automatic.source, "schedule");
    let seconds:f64=sqlx::query_scalar("SELECT EXTRACT(EPOCH FROM(next_check_at-now()))::double precision FROM repository_sync_connections WHERE id=$1").bind(repository).fetch_one(&pool).await.unwrap();
    assert!(
        seconds > 3590.0 && seconds <= 3600.0,
        "worker uses live period, not its startup300"
    );
    let added = Uuid::new_v4();
    app::save(
        &store.scoped(added),
        &secrets,
        repository,
        Uuid::new_v4(),
        access,
        command(Provider::Github, true),
        &[],
    )
    .await
    .unwrap();
    let seconds:f64=sqlx::query_scalar("SELECT EXTRACT(EPOCH FROM(next_check_at-now()))::double precision FROM repository_sync_connections WHERE id=$1")
        .bind(added).fetch_one(&pool).await.unwrap();
    assert!(
        seconds > 3590.0 && seconds <= 3600.0,
        "new connections use saved period, not startup defaults"
    );
    pool.close().await;
}

#[tokio::test]
async fn global_schedule_preserves_archived_and_disabled_connections() {
    use cicd_domain::repository_sync::Schedule;
    let (pool, store, active, access, secrets) = fixture().await;
    administrator(&pool, access).await;
    let archived = Uuid::new_v4();
    let disabled = Uuid::new_v4();
    for id in [archived, disabled] {
        sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
            .bind(id)
            .bind(format!("neighbor-{id}"))
            .execute(&pool)
            .await
            .unwrap();
    }
    for id in [active, archived, disabled] {
        let mut input = command(Provider::Github, true);
        if id == disabled {
            input.settings.direction = Direction::Disabled;
        }
        app::save(&store, &secrets, id, Uuid::new_v4(), access, input, &[])
            .await
            .unwrap();
    }
    let group = Uuid::new_v4();
    sqlx::query("INSERT INTO git_groups(id,slug,name,namespace_managed) VALUES($1,$2,'Archived project',true)")
        .bind(group).bind(format!("project-{}", group.simple())).execute(&pool).await.unwrap();
    let registry = Uuid::new_v4();
    let namespace = Uuid::new_v4();
    let mut binding = serde_json::json!({"schema_version":1,"namespace":{"registry_instance_id":registry,"namespace_id":namespace},"resource":{"instance_id":Uuid::new_v4(),"kind":"git_group","resource_id":group},"operation_id":Uuid::new_v4(),"generation":1,"state":"active","create_spec":null});
    sqlx::query("INSERT INTO forge_namespace_bindings(resource_id,registry_instance_id,namespace_id,generation,state,command) VALUES($1,$2,$3,1,'active',$4)")
        .bind(group).bind(registry).bind(namespace).bind(&binding).execute(&pool).await.unwrap();
    sqlx::query("UPDATE repository_catalog SET group_id=$2 WHERE id=$1")
        .bind(archived)
        .bind(group)
        .execute(&pool)
        .await
        .unwrap();
    binding["state"] = serde_json::json!("archived");
    binding["generation"] = serde_json::json!(2);
    binding["operation_id"] = serde_json::json!(Uuid::new_v4());
    sqlx::query("UPDATE forge_namespace_bindings SET state='archived',generation=2,command=$2 WHERE resource_id=$1")
        .bind(group).bind(binding).execute(&pool).await.unwrap();
    let before: Vec<(Uuid, Option<chrono::DateTime<chrono::Utc>>)> =
        sqlx::query_as("SELECT id,next_check_at FROM repository_sync_connections ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    let guard =
        sqlx::query("UPDATE repository_sync_connections SET next_check_at=now() WHERE id=$1")
            .bind(archived)
            .execute(&pool)
            .await
            .unwrap_err();
    assert_eq!(
        guard.as_database_error().unwrap().message(),
        "namespace_resource_read_only"
    );
    let saved = app::save_schedule(
        &store,
        Uuid::new_v4(),
        access,
        policy(900, false, 0),
        Schedule {
            interval_seconds: 3600,
            on_change: true,
        },
    )
    .await
    .unwrap();
    assert_eq!(saved.version, 1);
    let after: Vec<(Uuid, Option<chrono::DateTime<chrono::Utc>>)> =
        sqlx::query_as("SELECT id,next_check_at FROM repository_sync_connections ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    let time = |rows: &[(Uuid, Option<chrono::DateTime<chrono::Utc>>)], id| {
        rows.iter().find(|row| row.0 == id).unwrap().1
    };
    assert_eq!(
        time(&after, archived),
        time(&before, archived),
        "archive remains read-only"
    );
    assert_eq!(time(&before, disabled), None);
    assert_eq!(
        time(&after, disabled),
        None,
        "disabled connection has no next automatic check"
    );
    assert!(
        time(&after, active).unwrap() < time(&before, active).unwrap(),
        "healthy connection is rescheduled"
    );
    pool.close().await;
}
