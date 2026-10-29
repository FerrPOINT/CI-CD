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
    expected["owner_subject"] = serde_json::Value::Null;
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
async fn multiple_sync_candidates_do_not_starve_recovery_or_other_repositories() {
    let (pool, store, repository, access, secrets) = fixture().await;
    let waiting = Uuid::new_v4();
    let foreign = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
        .bind(foreign)
        .bind(format!("candidate-{foreign}"))
        .execute(&pool)
        .await
        .unwrap();
    for (repo, connection) in [
        (repository, repository),
        (repository, waiting),
        (foreign, foreign),
    ] {
        app::save(
            &store.scoped(connection),
            &secrets,
            repo,
            Uuid::new_v4(),
            access,
            command(Provider::Github, false),
            &[],
        )
        .await
        .unwrap();
    }
    let queued = store
        .enqueue_at(repository, waiting, "manual", None)
        .await
        .unwrap();
    let active = store.enqueue(repository, "manual", None).await.unwrap();
    let independent = store.enqueue(foreign, "manual", None).await.unwrap();
    // Transaction timestamps/commit order need not match worker claim order.
    sqlx::query("UPDATE repository_sync_runs SET created_at=now()-interval '1 day' WHERE id=$1")
        .bind(queued.id)
        .execute(&pool)
        .await
        .unwrap();
    let owner = Uuid::new_v4();
    let (_, generation) = store.claim(active.id, owner).await.unwrap().unwrap();
    let candidates = store.candidates().await.unwrap();
    assert_eq!(
        candidates.iter().map(|c| c.0).collect::<Vec<_>>(),
        vec![independent.id]
    );
    for state in ["running", "stopping", "reconciling"] {
        sqlx::query("UPDATE repository_sync_runs SET state=$2,lease_until=now()-interval '1 second' WHERE id=$1")
            .bind(active.id)
            .bind(state)
            .execute(&pool)
            .await
            .unwrap();
        let candidates = store.candidates().await.unwrap();
        assert!(
            candidates.iter().any(|c| c.0 == active.id),
            "{state} recovery missing"
        );
        assert!(candidates.iter().any(|c| c.0 == independent.id));
        assert!(
            !candidates.iter().any(|c| c.0 == queued.id),
            "queued connection must wait for {state} recovery"
        );
    }
    store
        .finish(active.id, owner, generation, "success", None)
        .await
        .unwrap();
    assert!(
        store
            .candidates()
            .await
            .unwrap()
            .iter()
            .any(|c| c.0 == queued.id)
    );
    assert!(store.claim(queued.id, owner).await.unwrap().is_some());
    pool.close().await;
}

#[tokio::test]
async fn multiple_sync_candidate_batch_reserves_space_for_independent_repositories() {
    let (pool, store, repository, access, secrets) = fixture().await;
    let foreign = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
        .bind(foreign)
        .bind(format!("batch-{foreign}"))
        .execute(&pool)
        .await
        .unwrap();
    for repo in [repository, foreign] {
        app::save(
            &store,
            &secrets,
            repo,
            Uuid::new_v4(),
            access,
            command(Provider::Github, false),
            &[],
        )
        .await
        .unwrap();
    }
    // More than one candidate page on a single repository must not hide other
    // repositories from the supervisor, which can execute only one of them.
    sqlx::query("WITH added AS (INSERT INTO repository_sync_connections(repository_id,id,version,settings,encrypted_credential) SELECT c.repository_id,gen_random_uuid(),1,c.settings,c.encrypted_credential FROM repository_sync_connections c CROSS JOIN generate_series(1,101) WHERE c.id=$1 RETURNING id,repository_id,settings) INSERT INTO repository_sync_runs(id,repository_id,connection_id,connection_version,settings,source,state,created_at) SELECT gen_random_uuid(),repository_id,id,1,settings,'manual','queued',now()-interval '1 day' FROM added")
        .bind(repository).execute(&pool).await.unwrap();
    let independent = store.enqueue(foreign, "manual", None).await.unwrap();
    let candidates = store.candidates().await.unwrap();
    assert_eq!(
        candidates.len(),
        2,
        "one runnable candidate per repository per batch"
    );
    assert_eq!(candidates.iter().filter(|c| c.1 == repository).count(), 1);
    assert!(candidates.iter().any(|c| c.0 == independent.id));
    assert_eq!(
        store.candidates().await.unwrap(),
        candidates,
        "ties must have stable ordering"
    );
    pool.close().await;
}

#[tokio::test]
async fn multiple_sync_concurrent_cas_and_original_save_receipt_remain_isolated() {
    let (pool, store, repository, access, secrets) = fixture().await;
    let connection = Uuid::new_v4();
    let scoped = store.scoped(connection);
    let first = command(Provider::Github, false);
    let mut second = first.clone();
    second.token = Some("synthetic-concurrent-other-token".into());
    let keys = [Uuid::new_v4(), Uuid::new_v4()];
    let (a, b) = tokio::join!(
        app::save(
            &scoped,
            &secrets,
            repository,
            keys[0],
            access,
            first.clone(),
            &[]
        ),
        app::save(
            &scoped,
            &secrets,
            repository,
            keys[1],
            access,
            second.clone(),
            &[]
        )
    );
    let (key, original) = match (a, b) {
        (Ok(saved), Err(app::Error::Conflict("sync_version_conflict"))) => {
            assert_eq!(saved.version, 1);
            (keys[0], first)
        }
        (Err(app::Error::Conflict("sync_version_conflict")), Ok(saved)) => {
            assert_eq!(saved.version, 1);
            (keys[1], second)
        }
        _ => panic!("one concurrent creation must win and the other must report CAS conflict"),
    };
    let mut update = original.clone();
    assert!(
        store
            .connection(repository, access)
            .await
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        store.enqueue(repository, "manual", None).await,
        Err(app::Error::NotFound)
    ));
    update.expected_version = 1;
    update.token = Some("synthetic-rotated-multiple-sync-token".into());
    app::save(
        &scoped,
        &secrets,
        repository,
        Uuid::new_v4(),
        access,
        update,
        &[],
    )
    .await
    .unwrap();
    let receipt = app::save(&scoped, &secrets, repository, key, access, original, &[])
        .await
        .unwrap();
    assert_eq!(receipt.id, connection);
    assert_eq!(
        receipt.version, 1,
        "replay must return the original receipt"
    );
    assert_eq!(
        store
            .connection_at(repository, connection, access)
            .await
            .unwrap()
            .unwrap()
            .version,
        2
    );
    let (_, encrypted) = scoped.credential(repository).await.unwrap().unwrap();
    assert_eq!(
        secrets.open(&encrypted).unwrap(),
        "synthetic-rotated-multiple-sync-token"
    );
    pool.close().await;
}

#[tokio::test]
async fn multiple_sync_concurrent_creation_cannot_capture_another_repository_connection() {
    let (pool, store, repository, access, secrets) = fixture().await;
    let foreign = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
        .bind(foreign)
        .bind(format!("collision-{foreign}"))
        .execute(&pool)
        .await
        .unwrap();
    let connection = Uuid::new_v4();
    let scoped = store.scoped(connection);
    let (a, b) = tokio::join!(
        app::save(
            &scoped,
            &secrets,
            repository,
            Uuid::new_v4(),
            access,
            command(Provider::Github, false),
            &[]
        ),
        app::save(
            &scoped,
            &secrets,
            foreign,
            Uuid::new_v4(),
            access,
            command(Provider::Github, false),
            &[]
        )
    );
    let (winner, loser) = match (a, b) {
        (Ok(saved), Err(app::Error::NotFound)) => (saved.repository_id, foreign),
        (Err(app::Error::NotFound), Ok(saved)) => (saved.repository_id, repository),
        _ => panic!("a connection UUID must belong to exactly one repository"),
    };
    assert!(scoped.credential(winner).await.unwrap().is_some());
    assert!(scoped.credential(loser).await.unwrap().is_none());
    assert!(
        store
            .runs_at(loser, connection, 20, 0)
            .await
            .unwrap()
            .0
            .is_empty()
    );
    assert!(matches!(
        store.enqueue_at(loser, connection, "manual", None).await,
        Err(app::Error::NotFound)
    ));
    let stored: i64 =
        sqlx::query_scalar("SELECT count(*) FROM repository_sync_connections WHERE id=$1")
            .bind(connection)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, 1);
    pool.close().await;
}

#[tokio::test]
async fn multiple_sync_stopping_one_connection_preserves_the_other_queue_and_intent() {
    let (pool, store, repository, access, secrets) = fixture().await;
    let other = Uuid::new_v4();
    for connection in [repository, other] {
        app::save(
            &store.scoped(connection),
            &secrets,
            repository,
            Uuid::new_v4(),
            access,
            command(Provider::Github, true),
            &[],
        )
        .await
        .unwrap();
    }
    let run = store.enqueue(repository, "manual", None).await.unwrap();
    let waiting = store
        .enqueue_at(repository, other, "manual", None)
        .await
        .unwrap();
    let owner = Uuid::new_v4();
    let (claimed, generation) = store.claim(run.id, owner).await.unwrap().unwrap();
    let planned = cicd_domain::repository_sync::RefResult {
        ref_name: "refs/heads/stopping-fixture".into(),
        direction: "push".into(),
        old_sha: Some("a".repeat(40)),
        new_sha: Some("b".repeat(40)),
        state: "unknown".into(),
        reason: Some("outcome_unknown".into()),
        event_delivered: true,
    };
    store
        .record_plan(run.id, std::slice::from_ref(&planned))
        .await
        .unwrap();
    let mut disable = command(Provider::Github, true);
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
        &[],
    )
    .await
    .unwrap();
    let stopping = store.run(repository, run.id).await.unwrap();
    assert_eq!(stopping.state, "stopping");
    assert!(stopping.cancel_requested);
    assert!(
        stopping.finished_at.is_none(),
        "unknown Git effect must not be declared canceled"
    );
    assert!(
        !store
            .allowed_to_apply(&claimed, owner, generation)
            .await
            .unwrap()
    );
    assert!(
        store
            .claim(waiting.id, Uuid::new_v4())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store.run(repository, waiting.id).await.unwrap().state,
        "queued"
    );
    assert!(
        !store
            .run(repository, waiting.id)
            .await
            .unwrap()
            .cancel_requested
    );
    let refs = store.refs(run.id, 20, 0).await.unwrap().0;
    assert_eq!(refs[0].old_sha, planned.old_sha);
    assert_eq!(refs[0].new_sha, planned.new_sha);
    assert_eq!(refs[0].state, "unknown");
    sqlx::query(
        "UPDATE repository_sync_runs SET lease_until=now()-interval '1 second' WHERE id=$1",
    )
    .bind(run.id)
    .execute(&pool)
    .await
    .unwrap();
    let recovered_owner = Uuid::new_v4();
    let (recovered, recovered_generation) =
        store.claim(run.id, recovered_owner).await.unwrap().unwrap();
    assert_eq!(recovered.state, "reconciling");
    assert!(
        !store
            .allowed_to_apply(&recovered, recovered_owner, recovered_generation)
            .await
            .unwrap()
    );
    // Only an observed terminal receipt releases the next connection.
    store
        .finish(
            run.id,
            recovered_owner,
            recovered_generation,
            "canceled",
            None,
        )
        .await
        .unwrap();
    let (next, next_generation) = store.claim(waiting.id, owner).await.unwrap().unwrap();
    assert!(
        store
            .allowed_to_apply(&next, owner, next_generation)
            .await
            .unwrap()
    );
    store.signal(repository).await.unwrap();
    store.schedule().await.unwrap();
    assert_eq!(store.runs(repository, 20, 0).await.unwrap().1, 1);
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

// Regressions for confirmed general-review failures.
#[tokio::test]
async fn multiple_sync_archived_v2_space_does_not_block_healthy_schedule() {
    use pdlc_shared::resource_context::{NamespaceRef, OwnerCommand, ResourceKind, ResourceRef};
    use pdlc_shared::space_access::SpaceRef;
    use pdlc_shared::space_lifecycle::{DesiredSpaceState, SpaceMetadata, SpaceOwnerCommandV2};
    let (pool, store, repository, access, secrets) = fixture().await;
    let foreign = Uuid::new_v4();
    let group = Uuid::new_v4();
    let registry = Uuid::new_v4();
    let space = Uuid::new_v4();
    let instance = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
        .bind(foreign)
        .bind(format!("review-healthy-{foreign}"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO git_groups(id,slug,name) VALUES($1,$2,'Review archived Space')")
        .bind(group)
        .bind(format!("review-{group}"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE repository_catalog SET group_id=$2 WHERE id=$1")
        .bind(repository)
        .bind(group)
        .execute(&pool)
        .await
        .unwrap();
    let legacy = OwnerCommand {
        schema_version: 1,
        namespace: NamespaceRef {
            registry_instance_id: registry,
            namespace_id: space,
        },
        resource: ResourceRef {
            instance_id: instance,
            kind: ResourceKind::GitGroup,
            resource_id: group,
        },
        operation_id: Uuid::new_v4(),
        generation: 1,
        state: "active".into(),
        create_spec: None,
    };
    sqlx::query("INSERT INTO forge_namespace_bindings(resource_id,registry_instance_id,namespace_id,generation,state,command) VALUES($1,$2,$3,1,'active',$4)")
        .bind(group).bind(registry).bind(space).bind(serde_json::to_value(legacy).unwrap()).execute(&pool).await.unwrap();
    for repo in [repository, foreign] {
        app::save(
            &store,
            &secrets,
            repo,
            Uuid::new_v4(),
            access,
            command(Provider::Github, true),
            &[],
        )
        .await
        .unwrap();
        store.signal(repo).await.unwrap();
    }
    let archived = SpaceOwnerCommandV2 {
        schema_version: 2,
        space: SpaceRef {
            registry_instance_id: registry,
            space_id: space,
        },
        participant_instance_id: instance,
        operation_id: Uuid::new_v4(),
        revision: 2,
        desired_state: DesiredSpaceState::Archived,
        metadata: SpaceMetadata {
            slug: "review-space".into(),
            name: "Review archived Space".into(),
            description: String::new(),
            responsible_subject: "fixture-subject".into(),
        },
    };
    sqlx::query("INSERT INTO forge_space_catalog(registry_instance_id,space_id,group_id,revision,state,operation_id,command) VALUES($1,$2,$3,2,'archived',$4,$5)")
        .bind(registry).bind(space).bind(group).bind(archived.operation_id).bind(serde_json::to_value(archived).unwrap()).execute(&pool).await.unwrap();
    let state: String = sqlx::query_scalar("SELECT forge_effective_group_state($1)")
        .bind(group)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "archived");
    for _ in 0..2 {
        store.schedule().await.unwrap();
        assert_eq!(store.runs(foreign, 20, 0).await.unwrap().1, 1);
        assert_eq!(store.runs(repository, 20, 0).await.unwrap().1, 0);
        let connection = store.connection(repository, access).await.unwrap().unwrap();
        assert!(!connection.writable);
        assert_eq!(
            connection.paused_reason.as_deref(),
            Some("namespace_read_only")
        );
        assert_eq!(store.candidates().await.unwrap().len(), 1);
    }
    pool.close().await;
}

#[tokio::test]
async fn multiple_sync_routes_reject_unconfirmed_local_admin_when_common_policy_active() {
    let (pool, store, repository, access, secrets) = fixture().await;
    app::save(
        &store,
        &secrets,
        repository,
        Uuid::new_v4(),
        access,
        command(Provider::Github, false),
        &[],
    )
    .await
    .unwrap();
    let actor = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,username,role) VALUES($1,$2,'admin')")
        .bind(actor)
        .bind(format!("review-{actor}"))
        .execute(&pool)
        .await
        .unwrap();
    let token = "cicd_fixture_review_local_admin";
    sqlx::query("INSERT INTO api_tokens(id,name,token_hash,token_hint,principal_type,user_id,scopes) VALUES($1,'Review local principal',$2,'fixture','user',$3,ARRAY['api:read','api:write','git:write'])")
        .bind(Uuid::new_v4()).bind(cicd::auth::hash_token(token)).bind(actor).execute(&pool).await.unwrap();
    let root = std::env::temp_dir().join(format!("forge-review-admission-{repository}"));
    std::fs::create_dir(&root).unwrap();
    let storage: String =
        sqlx::query_scalar("SELECT storage_name FROM repository_catalog WHERE id=$1")
            .bind(repository)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        tokio::process::Command::new("git")
            .args(["init", "--bare"])
            .arg(root.join(format!("{storage}.git")))
            .output()
            .await
            .unwrap()
            .status
            .success()
    );
    let mut config = cicd::config::RuntimeConfig::from_env_source(|_| None, false)
        .unwrap()
        .with_auth_secret(Some("fixture-review-auth-secret".into()));
    config.git.root = root.clone();
    config.git_sync.enabled = true;
    config.secrets.key = Some([7; 32]);
    let router = cicd::api::app_with_git_and_config(
        Some(pool.clone()),
        config.git.to_git_config(),
        None,
        config,
    )
    .unwrap();
    sqlx::query("UPDATE forge_space_admission_state SET active=true WHERE singleton")
        .execute(&pool)
        .await
        .unwrap();
    async fn request(
        router: &axum::Router,
        token: &str,
        method: &str,
        path: &str,
    ) -> axum::response::Response {
        router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("authorization", format!("Bearer {token}"))
                    .header("Idempotency-Key", Uuid::new_v4().to_string())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }
    let catalog = request(&router, token, "GET", "/api/v1/catalog/repositories").await;
    assert!(
        !catalog.status().is_success(),
        "control: required common policy must reject an unconfirmed local principal"
    );
    let list = request(&router, token, "GET", "/api/v1/repository-syncs").await;
    assert_eq!(list.status(), StatusCode::FORBIDDEN);
    let detail = request(
        &router,
        token,
        "GET",
        &format!("/api/v1/catalog/repositories/{repository}/sync"),
    )
    .await;
    assert_eq!(detail.status(), StatusCode::FORBIDDEN);
    let run = request(
        &router,
        token,
        "POST",
        &format!("/api/v1/catalog/repositories/{repository}/sync/runs"),
    )
    .await;
    assert_eq!(run.status(), StatusCode::FORBIDDEN);
    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn multiple_sync_stopped_reconciliation_allows_only_same_settings_reauthentication() {
    let (pool, store, repository, access, secrets) = fixture().await;
    let other = Uuid::new_v4();
    for id in [repository, other] {
        app::save(
            &store.scoped(id),
            &secrets,
            repository,
            Uuid::new_v4(),
            access,
            command(Provider::Github, false),
            &[],
        )
        .await
        .unwrap();
    }
    let run = store.enqueue(repository, "manual", None).await.unwrap();
    let waiting = store
        .enqueue_at(repository, other, "manual", None)
        .await
        .unwrap();
    let owner = Uuid::new_v4();
    let (_, generation) = store.claim(run.id, owner).await.unwrap().unwrap();
    store
        .record_plan(
            run.id,
            &[cicd_domain::repository_sync::RefResult {
                ref_name: "refs/heads/review-unknown".into(),
                direction: "push".into(),
                old_sha: None,
                new_sha: Some("a".repeat(40)),
                state: "unknown".into(),
                reason: Some("outcome_unknown".into()),
                event_delivered: true,
            }],
        )
        .await
        .unwrap();
    store
        .finish(run.id, owner, generation, "reconciling", None)
        .await
        .unwrap();
    let mut disable = command(Provider::Github, false);
    disable.expected_version = 1;
    disable.settings.direction = Direction::Disabled;
    disable.token = None;
    let saved = app::save(
        &store,
        &secrets,
        repository,
        Uuid::new_v4(),
        access,
        disable.clone(),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(saved.active_run.unwrap().state, "stopping");
    disable.expected_version = 2;
    disable.token = Some("fixture-replacement-token".into());
    let key = Uuid::new_v4();
    let saved = app::save(
        &store,
        &secrets,
        repository,
        key,
        access,
        disable.clone(),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(saved.version, 3);
    assert_eq!(saved.settings.direction, Direction::Disabled);
    assert!(saved.active_run.unwrap().cancel_requested);
    assert_eq!(
        app::credential(&store, &secrets, repository, &disable.settings, None)
            .await
            .unwrap(),
        "fixture-replacement-token"
    );
    assert_eq!(
        app::save(
            &store,
            &secrets,
            repository,
            key,
            access,
            disable.clone(),
            &[]
        )
        .await
        .unwrap()
        .version,
        3
    );
    assert!(
        !store
            .allowed_to_apply(&run, owner, generation)
            .await
            .unwrap()
    );
    disable.expected_version = 3;
    disable.settings.repository_path = "fixture/other".into();
    assert!(matches!(
        app::save(
            &store,
            &secrets,
            repository,
            Uuid::new_v4(),
            access,
            disable,
            &[]
        )
        .await,
        Err(app::Error::Conflict("sync_running"))
    ));
    assert!(
        store
            .claim(waiting.id, Uuid::new_v4())
            .await
            .unwrap()
            .is_none()
    );
    let (refs, _) = store.refs(run.id, 20, 0).await.unwrap();
    assert_eq!(refs[0].state, "unknown");
    pool.close().await;
}

#[tokio::test]
async fn multiple_sync_owner_snapshot_survives_settings_takeover_and_active_reauthentication() {
    let (pool, store, repository, access, secrets) = fixture().await;
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let mut input = command(Provider::Github, true);
    app::save(
        &store.scoped(repository).with_owner(Some(first)),
        &secrets,
        repository,
        Uuid::new_v4(),
        access,
        input.clone(),
        &[],
    )
    .await
    .unwrap();
    store.signal(repository).await.unwrap();
    store.schedule().await.unwrap();
    let run = store.runs(repository, 20, 0).await.unwrap().0.remove(0);
    assert_eq!(store.event_owner(run.id).await.unwrap(), Some(first));
    let owner = Uuid::new_v4();
    let (_, generation) = store.claim(run.id, owner).await.unwrap().unwrap();
    input.expected_version = 1;
    input.settings.direction = Direction::Disabled;
    input.token = None;
    app::save(
        &store.scoped(repository).with_owner(Some(second)),
        &secrets,
        repository,
        Uuid::new_v4(),
        access,
        input.clone(),
        &[],
    )
    .await
    .unwrap();
    input.expected_version = 2;
    input.token = Some("fixture-new-recovery-credential".into());
    app::save(
        &store.scoped(repository).with_owner(Some(second)),
        &secrets,
        repository,
        Uuid::new_v4(),
        access,
        input.clone(),
        &[],
    )
    .await
    .unwrap();
    let connection_owner: Option<Uuid> =
        sqlx::query_scalar("SELECT owner_subject FROM repository_sync_connections WHERE id=$1")
            .bind(repository)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(connection_owner, Some(first));
    assert_eq!(store.event_owner(run.id).await.unwrap(), Some(first));
    store
        .finish(run.id, owner, generation, "canceled", None)
        .await
        .unwrap();
    input.expected_version = 3;
    input.settings.direction = Direction::Both;
    app::save(
        &store.scoped(repository).with_owner(Some(second)),
        &secrets,
        repository,
        Uuid::new_v4(),
        access,
        input,
        &[],
    )
    .await
    .unwrap();
    let next = store.enqueue(repository, "manual", None).await.unwrap();
    assert_eq!(store.event_owner(next.id).await.unwrap(), Some(second));
    assert_eq!(store.event_owner(run.id).await.unwrap(), Some(first));
    sqlx::query(
        "INSERT INTO identity_legacy_issuer(issuer) VALUES('https://auth.fixture.invalid')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let refs:Vec<(String,String,bool)>=sqlx::query_as("SELECT ref.issuer,ref.subject,ref.unresolved FROM repository_sync_runs run CROSS JOIN LATERAL forge_identity_row_refs('repository_sync_runs',to_jsonb(run)) ref WHERE run.id=$1").bind(next.id).fetch_all(&pool).await.unwrap();
    assert_eq!(
        refs,
        vec![(
            "https://auth.fixture.invalid".into(),
            second.to_string(),
            false
        )]
    );
    // A backlog of frozen owners must be excluded before the scheduler limit.
    sqlx::query("INSERT INTO repository_sync_connections(id,repository_id,version,settings,encrypted_credential,owner_subject,dirty) SELECT gen_random_uuid(),repository_id,version,settings,encrypted_credential,owner_subject,true FROM repository_sync_connections CROSS JOIN generate_series(1,110) WHERE id=$1")
        .bind(repository).execute(&pool).await.unwrap();
    let healthy = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
        .bind(healthy)
        .bind(format!("healthy-owner-{healthy}"))
        .execute(&pool)
        .await
        .unwrap();
    app::save(
        &store,
        &secrets,
        healthy,
        Uuid::new_v4(),
        access,
        command(Provider::Github, true),
        &[],
    )
    .await
    .unwrap();
    store.signal(healthy).await.unwrap();
    sqlx::query("INSERT INTO identity_deletion_operations(operation_id,issuer,subject,inventory_revision,request,state,fence,revision,reference_count) VALUES($1,'https://auth.fixture.invalid',$2,$3,'{}','frozen',$4,1,0)").bind(Uuid::new_v4()).bind(second.to_string()).bind(Uuid::new_v4()).bind(Uuid::new_v4()).execute(&pool).await.unwrap();
    for (query, id) in [
        (
            "UPDATE repository_sync_connections SET dirty=true WHERE id=$1",
            repository,
        ),
        (
            "UPDATE repository_sync_runs SET message='fixture fenced write' WHERE id=$1",
            next.id,
        ),
    ] {
        let error = sqlx::query(query)
            .bind(id)
            .execute(&pool)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
        assert_eq!(
            error.as_database_error().unwrap().message(),
            "identity_deletion_fenced"
        );
    }
    store.schedule().await.unwrap();
    assert_eq!(store.runs(healthy, 20, 0).await.unwrap().1, 1);
    assert_eq!(store.runs(repository, 20, 0).await.unwrap().1, 2);
    pool.close().await;
}

#[tokio::test]
async fn multiple_sync_common_group_filter_precedes_count_and_page_even_for_admin() {
    let (pool, store, repository, access, secrets) = fixture().await;
    let group = Uuid::new_v4();
    sqlx::query("INSERT INTO git_groups(id,slug,name) VALUES($1,$2,'Allowed group')")
        .bind(group)
        .bind(format!("admitted-{group}"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE repository_catalog SET group_id=$1 WHERE id=$2")
        .bind(group)
        .bind(repository)
        .execute(&pool)
        .await
        .unwrap();
    for connection in [repository, Uuid::new_v4()] {
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
    }
    let (none, total) = store
        .list_admitted_at(access, "", "", "", None, 1, 0, None, Some(&[]))
        .await
        .unwrap();
    assert_eq!(total, 0);
    assert!(none.is_empty());
    let (one, total) = store
        .list_admitted_at(access, "", "", "", None, 1, 1, None, Some(&[group]))
        .await
        .unwrap();
    assert_eq!(total, 2);
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].repository_id, repository);
    let (_, total) = store
        .list_admitted_at(
            access,
            "",
            "",
            "",
            None,
            1,
            0,
            None,
            Some(&[Uuid::new_v4()]),
        )
        .await
        .unwrap();
    assert_eq!(total, 0);
    pool.close().await;
}

#[tokio::test]
async fn multiple_sync_scheduler_admission_race_does_not_poison_other_connections() {
    let (pool, store, repository, access, secrets) = fixture().await;
    let healthy = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
        .bind(healthy)
        .bind(format!("scheduler-race-{healthy}"))
        .execute(&pool)
        .await
        .unwrap();
    for repo in [repository, healthy] {
        app::save(
            &store,
            &secrets,
            repo,
            Uuid::new_v4(),
            access,
            command(Provider::Github, true),
            &[],
        )
        .await
        .unwrap();
        store.signal(repo).await.unwrap();
    }
    // Inject the real admission SQLSTATE after selection, at the final INSERT.
    // A savepoint must preserve the other connection and the rejected signal.
    sqlx::raw_sql(&format!("CREATE FUNCTION fixture_scheduler_guard() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.repository_id='{repository}'::uuid THEN RAISE EXCEPTION 'namespace_resource_read_only' USING ERRCODE='42501'; END IF; RETURN NEW; END; $$; CREATE TRIGGER z_fixture_scheduler_guard BEFORE INSERT ON repository_sync_runs FOR EACH ROW EXECUTE FUNCTION fixture_scheduler_guard();"))
        .execute(&pool).await.unwrap();
    store.schedule().await.unwrap();
    assert_eq!(store.runs(repository, 20, 0).await.unwrap().1, 0);
    assert_eq!(store.runs(healthy, 20, 0).await.unwrap().1, 1);
    let dirty: bool =
        sqlx::query_scalar("SELECT dirty FROM repository_sync_connections WHERE id=$1")
            .bind(repository)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(dirty);
    // Storage failures are not silently classified as a rejected admission.
    sqlx::raw_sql("CREATE OR REPLACE FUNCTION fixture_scheduler_guard() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture_storage_failure' USING ERRCODE='XX000'; END; $$;")
        .execute(&pool).await.unwrap();
    assert!(matches!(
        store.schedule().await,
        Err(app::Error::Unavailable(_))
    ));
    assert_eq!(store.runs(healthy, 20, 0).await.unwrap().1, 1);
    pool.close().await;
}

#[tokio::test]
async fn multiple_sync_technical_save_cannot_borrow_previous_human_pipeline_owner() {
    let (pool, store, repository, access, secrets) = fixture().await;
    let human = Uuid::new_v4();
    let mut input = command(Provider::Github, false);
    app::save(
        &store.scoped(repository).with_owner(Some(human)),
        &secrets,
        repository,
        Uuid::new_v4(),
        access,
        input.clone(),
        &[],
    )
    .await
    .unwrap();
    input.expected_version = 1;
    input.token = Some("fictional-technical-replacement-credential".into());
    let project = Uuid::new_v4();
    sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id) VALUES($1,$2,'https://forge.fixture.invalid/owned',$3)").bind(project).bind(format!("technical-{project}")).bind(repository).execute(&pool).await.unwrap();
    let technical = Access {
        machine: true,
        project_id: Some(project),
        admin: false,
        ..access
    };
    app::save(
        &store.scoped(repository),
        &secrets,
        repository,
        Uuid::new_v4(),
        technical,
        input,
        &[],
    )
    .await
    .unwrap();
    let run = store.enqueue(repository, "manual", None).await.unwrap();
    assert_eq!(store.event_owner(run.id).await.unwrap(), None);
    pool.close().await;
}

#[tokio::test]
async fn multiple_sync_ci_backlog_retries_do_not_starve_unattempted_events() {
    let (pool, store, repository, access, secrets) = fixture().await;
    app::save(
        &store,
        &secrets,
        repository,
        Uuid::new_v4(),
        access,
        command(Provider::Github, false),
        &[],
    )
    .await
    .unwrap();
    let old = store.enqueue(repository, "manual", None).await.unwrap();
    let owner = Uuid::new_v4();
    let (_, generation) = store.claim(old.id, owner).await.unwrap().unwrap();
    store
        .finish(old.id, owner, generation, "success", None)
        .await
        .unwrap();
    let fresh = store.enqueue(repository, "manual", None).await.unwrap();
    let reference = |name: String| cicd_domain::repository_sync::RefResult {
        ref_name: name,
        direction: "pull".into(),
        old_sha: None,
        new_sha: Some("a".repeat(40)),
        state: "applied".into(),
        reason: None,
        event_delivered: false,
    };
    store
        .record_plan(
            old.id,
            &(0..100)
                .map(|i| reference(format!("refs/heads/old-{i}")))
                .collect::<Vec<_>>(),
        )
        .await
        .unwrap();
    store
        .record_plan(fresh.id, &[reference("refs/heads/fresh".into())])
        .await
        .unwrap();
    // A slow/unavailable owner can make the oldest batch due again before its
    // delivery pass finishes. Its later retry time must not hide untouched work.
    sqlx::query("UPDATE repository_sync_refs SET event_next_attempt_at=now()-interval '1 hour' WHERE run_id=$1").bind(fresh.id).execute(&pool).await.unwrap();
    for _ in 0..2 {
        sqlx::query("UPDATE repository_sync_refs SET event_next_attempt_at=now()-interval '1 second' WHERE run_id=$1").bind(old.id).execute(&pool).await.unwrap();
        let events = store.pending_events().await.unwrap();
        assert_eq!(events.len(), 100);
        assert_eq!(events[0].0, fresh.id);
        assert_eq!(events[0].2.ref_name, "refs/heads/fresh");
    }
    pool.close().await;
}
