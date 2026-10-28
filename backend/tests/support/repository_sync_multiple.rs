use super::*;
use cicd_app::repository_sync::Store;

async fn fixture() -> (sqlx::PgPool, PgSyncStore, Uuid, Access, Secrets) {
    let pool = test_pool().await;
    let repository = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
        .bind(repository)
        .bind(format!("multiple-sync-{repository}"))
        .execute(&pool)
        .await
        .unwrap();
    let store = PgSyncStore {
        pool: pool.clone(),
        worker_enabled: true,
        interval_seconds: 300,
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
            instance_url: match provider {
                Provider::Github => "https://github.com",
                Provider::Gitlab => "https://gitlab.example.test",
            }
            .into(),
            provider,
            repository_path: "fixture/service".into(),
            direction: Direction::Both,
            automatic,
        },
        token: Some("synthetic-multiple-sync-credential".into()),
    }
}
fn origins() -> Vec<String> {
    vec!["https://gitlab.example.test".into()]
}

#[tokio::test]
async fn multiple_sync_populated_migration_preserves_legacy_journal_and_readbacks() {
    let catalog = cicd::migrations().await.unwrap();
    let (pool, admin, schema) = migration_catalog_empty_pool().await;
    migration_catalog_subset(&catalog, 105)
        .run(&pool)
        .await
        .unwrap();
    let repository = Uuid::new_v4();
    let run = Uuid::new_v4();
    let operation = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
        .bind(repository)
        .bind(format!("migrate-sync-{repository}"))
        .execute(&pool)
        .await
        .unwrap();
    let settings = serde_json::to_value(command(Provider::Github, false).settings).unwrap();
    sqlx::query("INSERT INTO repository_sync_connections(repository_id,settings,version,encrypted_credential,dirty) VALUES($1,$2,7,'ciphertext-preserved',true)").bind(repository).bind(settings.clone()).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO repository_sync_runs(id,repository_id,connection_version,settings,source,state) VALUES($1,$2,7,$3,'manual','success')").bind(run).bind(repository).bind(settings).execute(&pool).await.unwrap();
    let old: serde_json::Value =
        sqlx::query_scalar("SELECT to_jsonb(r) FROM repository_sync_runs r WHERE id=$1")
            .bind(run)
            .fetch_one(&pool)
            .await
            .unwrap();
    sqlx::query("INSERT INTO repository_sync_operations(operation_id,repository_id,actor_id,kind,fingerprint,readback) VALUES($1,$2,$3,'run','manual',$4)").bind(operation).bind(repository).bind(Uuid::new_v4()).bind(old.clone()).execute(&pool).await.unwrap();
    let save_operation = Uuid::new_v4();
    let saved = serde_json::json!({"repository_id":repository,"repository_name":"fixture/migrated","version":7,"settings":old["settings"],"has_credential":true,"writable":true,"worker_enabled":false,"active_run":old,"last_run":old});
    sqlx::query("INSERT INTO repository_sync_operations(operation_id,repository_id,actor_id,kind,fingerprint,readback) VALUES($1,$2,$3,'save','keyed-fingerprint',$4)").bind(save_operation).bind(repository).bind(Uuid::new_v4()).bind(saved).execute(&pool).await.unwrap();
    // Backfill must work for archived namespaces and frozen actor history.
    let group = Uuid::new_v4();
    sqlx::query("INSERT INTO git_groups(id,slug,name) VALUES($1,$2,'Archived migration fixture')")
        .bind(group)
        .bind(format!("sync-{}", group.simple()))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE repository_catalog SET group_id=$2 WHERE id=$1")
        .bind(repository)
        .bind(group)
        .execute(&pool)
        .await
        .unwrap();
    use pdlc_shared::resource_context::{NamespaceRef, OwnerCommand, ResourceKind, ResourceRef};
    let registry = Uuid::new_v4();
    let namespace = Uuid::new_v4();
    let command = OwnerCommand {
        schema_version: 1,
        namespace: NamespaceRef {
            registry_instance_id: registry,
            namespace_id: namespace,
        },
        resource: ResourceRef {
            instance_id: Uuid::new_v4(),
            kind: ResourceKind::GitGroup,
            resource_id: group,
        },
        operation_id: Uuid::new_v4(),
        generation: 1,
        state: "archived".into(),
        create_spec: None,
    };
    sqlx::query("INSERT INTO forge_namespace_bindings(resource_id,registry_instance_id,namespace_id,generation,state,command) VALUES($1,$2,$3,1,'archived',$4)").bind(group).bind(registry).bind(namespace).bind(serde_json::to_value(command).unwrap()).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO identity_deletion_operations(operation_id,issuer,subject,inventory_revision,request,state,fence,revision,reference_count) VALUES($1,'https://auth.example.test','frozen-fixture',$2,'{}','frozen',$3,1,1)").bind(Uuid::new_v4()).bind(Uuid::new_v4()).bind(Uuid::new_v4()).execute(&pool).await.unwrap();
    catalog.run(&pool).await.unwrap();
    let preserved:(Uuid,i64,String,bool)=sqlx::query_as("SELECT id,version,encrypted_credential,dirty FROM repository_sync_connections WHERE repository_id=$1").bind(repository).fetch_one(&pool).await.unwrap();
    assert_eq!(
        preserved,
        (repository, 7, "ciphertext-preserved".into(), true)
    );
    let actual: serde_json::Value =
        sqlx::query_scalar("SELECT to_jsonb(r) FROM repository_sync_runs r WHERE id=$1")
            .bind(run)
            .fetch_one(&pool)
            .await
            .unwrap();
    let mut expected = old;
    expected["connection_id"] = serde_json::json!(repository);
    assert_eq!(actual, expected);
    let receipt: serde_json::Value =
        sqlx::query_scalar("SELECT readback FROM repository_sync_operations WHERE operation_id=$1")
            .bind(operation)
            .fetch_one(&pool)
            .await
            .unwrap();
    let restored: cicd_domain::repository_sync::SyncRun = serde_json::from_value(receipt).unwrap();
    assert_eq!(restored.id, run);
    assert_eq!(restored.connection_id, repository);
    let saved: serde_json::Value =
        sqlx::query_scalar("SELECT readback FROM repository_sync_operations WHERE operation_id=$1")
            .bind(save_operation)
            .fetch_one(&pool)
            .await
            .unwrap();
    let saved: cicd_domain::repository_sync::Connection = serde_json::from_value(saved).unwrap();
    assert_eq!(saved.id, repository);
    assert_eq!(saved.last_run.unwrap().connection_id, repository);
    assert_eq!(saved.active_run.unwrap().connection_id, repository);
    // Restore both guards before admitting application writers.
    assert!(
        sqlx::query("UPDATE repository_sync_connections SET dirty=false WHERE id=$1")
            .bind(repository)
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query(
            "UPDATE repository_sync_operations SET fingerprint='changed' WHERE operation_id=$1"
        )
        .bind(operation)
        .execute(&pool)
        .await
        .is_err()
    );
    pool.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}

#[tokio::test]
async fn multiple_sync_connections_isolate_credentials_cas_history_and_replays() {
    let (pool, store, repository, access, secrets) = fixture().await;
    let other = Uuid::new_v4();
    let key = Uuid::new_v4();
    let github = app::save(
        &store,
        &secrets,
        repository,
        key,
        access,
        command(Provider::Github, false),
        &origins(),
    )
    .await
    .unwrap();
    let mut lab = command(Provider::Gitlab, true);
    lab.token = Some("synthetic-separate-gitlab-credential".into());
    let gitlab = app::save(
        &store.scoped(other),
        &secrets,
        repository,
        Uuid::new_v4(),
        access,
        lab.clone(),
        &origins(),
    )
    .await
    .unwrap();
    assert_eq!(github.id, repository);
    assert_eq!(gitlab.id, other);
    assert_ne!(
        store.credential(repository).await.unwrap().unwrap().1,
        store
            .scoped(other)
            .credential(repository)
            .await
            .unwrap()
            .unwrap()
            .1
    );
    assert!(matches!(
        app::save(
            &store.scoped(other),
            &secrets,
            repository,
            key,
            access,
            lab.clone(),
            &origins()
        )
        .await,
        Err(app::Error::Conflict("idempotency_conflict"))
    ));
    let (list, total) = store
        .list_at(access, "", "", "", None, 1, 0, Some(repository))
        .await
        .unwrap();
    assert_eq!(total, 2);
    assert_eq!(list.len(), 1);
    assert_eq!(
        store
            .list_at(access, "", "gitlab", "", None, 20, 0, Some(repository))
            .await
            .unwrap()
            .0[0]
            .id,
        other
    );

    let a = store
        .enqueue(repository, "manual", Some((Uuid::new_v4(), access)))
        .await
        .unwrap();
    let run_key = Uuid::new_v4();
    let b = store
        .enqueue_at(repository, other, "manual", Some((run_key, access)))
        .await
        .unwrap();
    assert_ne!(a.id, b.id);
    assert_eq!(b.connection_id, other);
    assert_eq!(
        store
            .enqueue_at(repository, other, "manual", Some((run_key, access)))
            .await
            .unwrap()
            .id,
        b.id
    );
    assert!(matches!(
        store
            .enqueue(repository, "manual", Some((run_key, access)))
            .await,
        Err(app::Error::Conflict("idempotency_conflict"))
    ));
    assert_eq!(
        store.runs_at(repository, other, 20, 0).await.unwrap().0[0].id,
        b.id
    );
    assert_eq!(store.runs(repository, 20, 0).await.unwrap().0[0].id, a.id);
    assert_eq!(
        store
            .connection_at(repository, other, access)
            .await
            .unwrap()
            .unwrap()
            .active_run
            .unwrap()
            .id,
        b.id
    );
    let mut disable = command(Provider::Github, false);
    disable.expected_version = 1;
    disable.token = None;
    disable.settings.direction = Direction::Disabled;
    app::save(
        &store,
        &secrets,
        repository,
        Uuid::new_v4(),
        access,
        disable,
        &origins(),
    )
    .await
    .unwrap();
    assert_eq!(store.run(repository, a.id).await.unwrap().state, "canceled");
    assert_eq!(store.run(repository, b.id).await.unwrap().state, "queued");
    assert!(!store.run(repository, b.id).await.unwrap().cancel_requested);
    // Disabling one connection cannot invalidate the other connection's version.
    let owner = Uuid::new_v4();
    let (claimed, generation) = store.claim(b.id, owner).await.unwrap().unwrap();
    assert!(
        store
            .allowed_to_apply(&claimed, owner, generation)
            .await
            .unwrap()
    );
    store
        .finish(b.id, owner, generation, "success", None)
        .await
        .unwrap();
    lab.expected_version = 1;
    lab.token = None;
    lab.settings.repository_path = "fixture/renamed".into();
    app::save(
        &store.scoped(other),
        &secrets,
        repository,
        Uuid::new_v4(),
        access,
        lab,
        &origins(),
    )
    .await
    .unwrap();
    let history = store.runs_at(repository, other, 20, 0).await.unwrap().0;
    assert_eq!(history[0].settings.repository_path, "fixture/service");
    assert_eq!(history[0].connection_id, other);
    pool.close().await;
}

#[tokio::test]
async fn multiple_sync_connections_schedule_independently_and_serialize_claims() {
    let (pool, store, repository, access, secrets) = fixture().await;
    let ids = [repository, Uuid::new_v4(), Uuid::new_v4()];
    for (index, id) in ids.into_iter().enumerate() {
        app::save(
            &store.scoped(id),
            &secrets,
            repository,
            Uuid::new_v4(),
            access,
            command(Provider::Github, index < 2),
            &[],
        )
        .await
        .unwrap();
    }
    store.signal(repository).await.unwrap();
    let (s1, s2) = tokio::join!(store.schedule(), store.schedule());
    s1.unwrap();
    s2.unwrap();
    let a = store.runs_at(repository, ids[0], 20, 0).await.unwrap().0;
    let b = store.runs_at(repository, ids[1], 20, 0).await.unwrap().0;
    assert_eq!(a.len(), 1);
    assert_eq!(b.len(), 1);
    assert!(
        store
            .runs_at(repository, ids[2], 20, 0)
            .await
            .unwrap()
            .0
            .is_empty()
    );
    let owner = Uuid::new_v4();
    let (ca, cb) = tokio::join!(store.claim(a[0].id, owner), store.claim(b[0].id, owner));
    let ca = ca.unwrap();
    let cb = cb.unwrap();
    assert_ne!(ca.is_some(), cb.is_some());
    let (running, generation) = ca.or(cb).unwrap();
    let pending = if running.id == a[0].id {
        b[0].id
    } else {
        a[0].id
    };
    assert!(store.claim(pending, owner).await.unwrap().is_none());
    sqlx::query(
        "UPDATE repository_sync_runs SET lease_until=now()-interval '1 second' WHERE id=$1",
    )
    .bind(running.id)
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        store.claim(pending, owner).await.unwrap().is_none(),
        "lease expiry must not skip another connection's unresolved recovery"
    );
    let next_owner = Uuid::new_v4();
    let (recovered, next_generation) = store.claim(running.id, next_owner).await.unwrap().unwrap();
    assert_eq!(recovered.state, "reconciling");
    assert!(
        !store
            .allowed_to_apply(&running, owner, generation)
            .await
            .unwrap()
    );
    store.signal(repository).await.unwrap();
    store.schedule().await.unwrap();
    // Repeated signals during a run do not create another active run per connection.
    assert_eq!(store.runs_at(repository, ids[0], 20, 0).await.unwrap().1, 1);
    store
        .finish(running.id, next_owner, next_generation, "success", None)
        .await
        .unwrap();
    assert!(store.claim(pending, owner).await.unwrap().is_some());
    store.schedule().await.unwrap();
    assert_eq!(
        store
            .runs_at(repository, running.connection_id, 20, 0)
            .await
            .unwrap()
            .1,
        2
    );
    pool.close().await;
}

#[tokio::test]
async fn multiple_sync_connections_cannot_cross_repository_boundaries() {
    let (pool, store, repository, access, secrets) = fixture().await;
    let foreign = Uuid::new_v4();
    let connection = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
        .bind(foreign)
        .bind(format!("foreign-{foreign}"))
        .execute(&pool)
        .await
        .unwrap();
    app::save(
        &store.scoped(connection),
        &secrets,
        repository,
        Uuid::new_v4(),
        access,
        command(Provider::Github, false),
        &[],
    )
    .await
    .unwrap();
    assert!(
        store
            .connection_at(foreign, connection, access)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .scoped(connection)
            .credential(foreign)
            .await
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        store.enqueue_at(foreign, connection, "manual", None).await,
        Err(app::Error::NotFound)
    ));
    assert!(matches!(
        app::save(
            &store.scoped(connection),
            &secrets,
            foreign,
            Uuid::new_v4(),
            access,
            command(Provider::Github, false),
            &[]
        )
        .await,
        Err(app::Error::NotFound)
    ));
    assert!(
        store
            .runs_at(foreign, connection, 20, 0)
            .await
            .unwrap()
            .0
            .is_empty()
    );
    pool.close().await;
}
