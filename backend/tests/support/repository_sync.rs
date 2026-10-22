use super::*;
use cicd_app::repository_sync::{self as app, Access, Crypto};
use cicd_domain::repository_sync::{Direction, Provider, SaveConnection, Settings};
use cicd_infra::{repository_sync::PgSyncStore, secrets::Secrets};

#[path = "repository_sync_api_contract.rs"]
mod api_contract;
#[path = "repository_sync_ci_pinned_config.rs"]
mod ci_pinned_config;

#[path = "repository_sync_available.rs"]
mod available;
#[path = "repository_sync_list_scope.rs"]
mod list_scope;
#[path = "repository_sync_live.rs"]
mod live;

#[tokio::test]
async fn sync_protects_linked_project_defaults_without_explicit_entries() {
    use cicd_domain::repository_sync::{RefPlan, plan_ref};
    use std::collections::BTreeSet;

    let pool = test_pool().await;
    let repository = Uuid::new_v4();
    let foreign = Uuid::new_v4();
    for id in [repository, foreign] {
        sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
            .bind(id)
            .bind(format!("sync-default-{}", id.simple()))
            .execute(&pool)
            .await
            .unwrap();
    }
    for (repository_id, default, protected) in [
        (repository, "release", Vec::<String>::new()),
        (repository, "trunk", vec!["stable".to_owned()]),
        (foreign, "foreign", vec!["foreign-only".to_owned()]),
    ] {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id,default_branch,protected_branches) VALUES($1,$2,$3,$4,$5,$6)")
            .bind(id)
            .bind(format!("sync-default-{id}"))
            .bind(format!("https://forge.invalid/{id}"))
            .bind(repository_id)
            .bind(default)
            .bind(protected)
            .execute(&pool)
            .await
            .unwrap();
    }
    let store = PgSyncStore {
        pool: pool.clone(),
        worker_enabled: true,
        interval_seconds: 300,
    };
    let protected: BTreeSet<_> = store
        .protected_branches(repository)
        .await
        .unwrap()
        .into_iter()
        .collect();
    assert_eq!(
        protected,
        BTreeSet::from(["release".into(), "trunk".into(), "stable".into()])
    );
    for branch in ["release", "trunk", "stable", "feature"] {
        assert_eq!(
            plan_ref(
                Direction::Pull,
                false,
                protected.contains(branch),
                None,
                Some("candidate"),
                false,
                false,
            ),
            if branch == "feature" {
                RefPlan::Pull
            } else {
                RefPlan::Protected
            }
        );
    }
    pool.close().await;
}

#[tokio::test]
#[ignore = "Requires an owned Compose PostgreSQL killed/restarted by the QA coordinator"]
async fn postgres_restart_preserves_unknown_intent_and_idempotency() {
    use cicd_domain::repository_sync::RefResult;
    let marker = std::path::PathBuf::from(
        std::env::var("FORGE_SYNC_RESTART_MARKER").expect("explicit disposable QA coordinator"),
    );
    assert!(marker.starts_with("/evidence"));
    assert!(!marker.with_extension("ready").exists());
    assert!(!marker.with_extension("resumed").exists());
    let pool = test_pool().await;
    let repository = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
        .bind(repository)
        .bind(format!("restart-{}", repository.simple()))
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
    let command = SaveConnection {
        expected_version: 0,
        settings: Settings {
            provider: Provider::Github,
            instance_url: "https://github.com".into(),
            repository_path: "fixture/repository".into(),
            direction: Direction::Both,
            automatic: false,
        },
        token: Some("fictional-restart-credential".into()),
    };
    let operation = Uuid::new_v4();
    let secrets = Secrets(Some([7; 32]));
    app::save(
        &store,
        &secrets,
        repository,
        operation,
        access,
        command.clone(),
        &[],
    )
    .await
    .unwrap();
    let run_key = Uuid::new_v4();
    let run = store
        .enqueue(repository, "manual", Some((run_key, access)))
        .await
        .unwrap();
    let owner = Uuid::new_v4();
    let (_, generation) = store.claim(run.id, owner).await.unwrap().unwrap();
    let reference = RefResult {
        ref_name: "refs/heads/main".into(),
        direction: "push".into(),
        old_sha: Some("a".repeat(40)),
        new_sha: Some("b".repeat(40)),
        state: "unknown".into(),
        reason: Some("outcome_unknown".into()),
        event_delivered: true,
    };
    store
        .record_plan(run.id, std::slice::from_ref(&reference))
        .await
        .unwrap();
    let mut slot = store.slot(1).await.unwrap().unwrap();
    assert!(store.slot(1).await.unwrap().is_none());
    let mut uncommitted = pool.begin().await.unwrap();
    sqlx::query("UPDATE repository_sync_connections SET version=999 WHERE repository_id=$1")
        .bind(repository)
        .execute(&mut *uncommitted)
        .await
        .unwrap();
    std::fs::write(marker.with_extension("ready"), serde_json::to_vec(&serde_json::json!({"repository_id":repository,"run_id":run.id,"generation":generation})).unwrap()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(180), async {
        while !marker.with_extension("resumed").exists() {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("QA coordinator must restart only its disposable database");
    assert!(
        sqlx::query("SELECT 1").execute(&mut slot).await.is_err(),
        "old advisory slot session must die"
    );
    assert!(
        sqlx::query("SELECT 1")
            .execute(&mut *uncommitted)
            .await
            .is_err(),
        "old transaction must die"
    );
    drop(slot);
    drop(uncommitted);
    let connection = store.connection(repository, access).await.unwrap().unwrap();
    assert_eq!(connection.version, 1, "uncommitted update must roll back");
    assert!(connection.has_credential);
    let restored = store.run(repository, run.id).await.unwrap();
    assert_eq!(restored.connection_version, 1);
    assert!(
        restored.finished_at.is_none(),
        "unknown Git outcome cannot become terminal on DB restart"
    );
    let (refs, total) = store.refs(run.id, 20, 0).await.unwrap();
    assert_eq!(total, 1);
    assert_eq!(
        serde_json::to_value(&refs[0]).unwrap(),
        serde_json::to_value(reference).unwrap()
    );
    assert_eq!(
        store
            .enqueue(repository, "manual", Some((run_key, access)))
            .await
            .unwrap()
            .id,
        run.id
    );
    assert_eq!(
        app::save(
            &store,
            &secrets,
            repository,
            operation,
            access,
            command,
            &[]
        )
        .await
        .unwrap()
        .version,
        1
    );
    assert!(
        store.slot(1).await.unwrap().is_some(),
        "restart must release the advisory slot"
    );
    sqlx::query(
        "UPDATE repository_sync_runs SET lease_until=now()-interval '1 second' WHERE id=$1",
    )
    .bind(run.id)
    .execute(&pool)
    .await
    .unwrap();
    let new_owner = Uuid::new_v4();
    let (recovered, new_generation) = store.claim(run.id, new_owner).await.unwrap().unwrap();
    assert_eq!(recovered.state, "reconciling");
    assert!(new_generation > generation);
    assert!(store.heartbeat(run.id, owner, generation).await.is_err());
    assert!(
        store
            .ref_state(
                run.id,
                "refs/heads/main",
                "applied",
                None,
                owner,
                generation
            )
            .await
            .is_err()
    );
    assert_eq!(
        store.refs(run.id, 20, 0).await.unwrap().0[0].state,
        "unknown"
    );
    pool.close().await;
}

#[tokio::test]
async fn sync_guard_serializes_concurrent_pr_merge_without_duplicate_commit() {
    async fn git(path: &std::path::Path, args: &[&str]) -> String {
        let output = tokio::process::Command::new("git")
            .arg(format!("--git-dir={}", path.display()))
            .args([
                "-c",
                "user.name=Forge QA",
                "-c",
                "user.email=qa@example.invalid",
            ])
            .args(args)
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }
    let pool = test_pool().await;
    let repository = Uuid::new_v4();
    let name = format!("sync-pr-{}", repository.simple());
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
        .bind(repository)
        .bind(&name)
        .execute(&pool)
        .await
        .unwrap();
    let root = std::env::temp_dir().join(format!("forge-sync-pr-{}", Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let local = root.join(format!("{name}.git"));
    assert!(
        tokio::process::Command::new("git")
            .args(["init", "--bare", "--initial-branch=main"])
            .arg(&local)
            .status()
            .await
            .unwrap()
            .success()
    );
    let tree = git(&local, &["mktree"]).await;
    let initial = git(&local, &["commit-tree", &tree, "-m", "initial"]).await;
    let feature = git(
        &local,
        &["commit-tree", &tree, "-p", &initial, "-m", "feature"],
    )
    .await;
    git(&local, &["update-ref", "refs/heads/main", &initial]).await;
    git(&local, &["update-ref", "refs/heads/feature", &feature]).await;
    sqlx::query("INSERT INTO pull_requests(id,repository_id,repository_name,number,title,source_branch,target_branch) VALUES($1,$2,$3,1,'QA concurrent merge','feature','main')")
        .bind(Uuid::new_v4()).bind(repository).bind(&name).execute(&pool).await.unwrap();
    // Count native target publications rather than treating an idempotent HTTP
    // confirmation as a second merge. Durable recovery returns the original OID.
    let publications = root.join("main-publications");
    let hook = local.join("hooks/reference-transaction");
    std::fs::write(&hook, format!("#!/bin/sh\nif [ \"$1\" = prepared ]; then\n while read old new reference; do\n  if [ \"$reference\" = refs/heads/main ] && [ \"$old\" != \"$new\" ]; then printf '%s\\n' \"$new\" >> '{}'; fi\n done\nfi\n",publications.display())).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut config = cicd::config::RuntimeConfig::from_env_source(|_| None, false).unwrap();
    config.git.root = root.clone();
    let state = std::sync::Arc::new(cicd::api::AppState {
        pool: Some(pool.clone()),
        namespace_admission_pool: Some(
            PgPoolOptions::new()
                .max_connections(2)
                .connect_lazy_with((*pool.connect_options()).clone()),
        ),
        auth_secret: None,
        git: config.git.to_git_config(),
        config,
        running_jobs: None,
        rate_limiter: std::sync::Arc::new(cicd::rate_limit::RateLimiter::default()),
    });
    let merge = || {
        cicd::pulls::pr_action(
            axum::extract::State(state.clone()),
            axum::extract::Path((name.clone(), 1)),
            None,
            axum::Json(cicd::pulls::PrAction {
                action: "merge".into(),
            }),
        )
    };
    let (first, second) = tokio::join!(merge(), merge());
    let first = first.expect("original merge succeeds").0;
    let accepted = second
        .expect("concurrent retry confirms the durable merge")
        .0;
    assert_eq!(first.id, accepted.id);
    assert_eq!(first.status, "merged");
    assert_eq!(accepted.status, "merged");
    assert_eq!(first.merge_commit_sha, accepted.merge_commit_sha);
    let attempts: i64 = sqlx::query_scalar("SELECT count(*) FROM pull_request_merge_attempts WHERE pull_request_id=$1 AND state='confirmed'")
        .bind(accepted.id).fetch_one(&pool).await.unwrap();
    assert_eq!(attempts, 1, "one durable merge, not two accepted effects");
    let observed = std::fs::read_to_string(&publications).unwrap();
    assert_eq!(
        observed.lines().collect::<Vec<_>>(),
        vec![accepted.merge_commit_sha.as_deref().unwrap()],
        "target publication hook runs once"
    );
    assert_eq!(
        accepted.merge_commit_sha.as_deref(),
        Some(git(&local, &["rev-parse", "main"]).await.as_str())
    );
    assert_eq!(git(&local, &["rev-list", "--count", "main"]).await, "3");
    drop(state);
    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn sync_http_defaults_cas_history_and_effective_permissions() {
    let pool = test_pool().await;
    let repository = Uuid::new_v4();
    let actor = Uuid::new_v4();
    let name = format!("sync-{}", repository.simple());
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
        .bind(repository)
        .bind(&name)
        .execute(&pool)
        .await
        .unwrap();
    let root = std::env::temp_dir().join(format!("forge-sync-http-{}", Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    assert!(
        tokio::process::Command::new("git")
            .args(["init", "--bare", "--initial-branch=main"])
            .arg(root.join(format!("{name}.git")))
            .status()
            .await
            .unwrap()
            .success()
    );
    let mut config = cicd::config::RuntimeConfig::from_env_source(|_| None, false).unwrap();
    config.git.root = root.clone();
    config.git_sync.enabled = true;
    config.secrets.key = Some([11; 32]);
    let claims = cicd::auth::AccessClaims {
        sub: actor,
        service_account_name: None,
        sid: None,
        token_id: None,
        token_project_id: None,
        token_scopes: vec![],
        role: "admin".into(),
        ver: 0,
        iat: 0,
        exp: i64::MAX,
    };
    let state = std::sync::Arc::new(cicd::api::AppState {
        pool: Some(pool.clone()),
        namespace_admission_pool: Some(
            PgPoolOptions::new()
                .max_connections(2)
                .connect_lazy_with((*pool.connect_options()).clone()),
        ),
        auth_secret: None,
        git: config.git.to_git_config(),
        config,
        running_jobs: None,
        rate_limiter: std::sync::Arc::new(cicd::rate_limit::RateLimiter::default()),
    });
    let router = cicd::repository_sync::routes()
        .with_state(state.clone())
        .layer(axum::Extension(claims.clone()));
    let path = format!("/api/v1/catalog/repositories/{repository}/sync");
    let response = router
        .clone()
        .oneshot(Request::get(&path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let initial = response_json(response).await;
    assert_eq!(initial["version"], 0);
    assert_eq!(initial["settings"]["direction"], "disabled");
    assert_eq!(initial["settings"]["automatic"], false);
    assert_eq!(initial["writable"], true);
    let mut body = serde_json::json!({"expected_version":0,"settings":{"provider":"github","instance_url":"https://github.com","repository_path":"a/b","direction":"both","automatic":false},"token":"fictional-http-token"});
    let key = Uuid::new_v4();
    for _ in 0..2 {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(&path)
                    .header("content-type", "application/json")
                    .header("Idempotency-Key", key.to_string())
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let value = response_json(response).await;
        assert_eq!(value["version"], 1);
        assert!(!value.to_string().contains("fictional-http-token"));
        assert!(value.get("token").is_none());
    }
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(&path)
                .header("content-type", "application/json")
                .header("Idempotency-Key", Uuid::new_v4().to_string())
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let mut run_id = None;
    for _ in 0..2 {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("{path}/runs"))
                    .header("Idempotency-Key", Uuid::new_v4().to_string())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let value = response_json(response).await;
        if let Some(id) = run_id.as_ref() {
            assert_eq!(&value["id"], id);
        } else {
            run_id = Some(value["id"].clone());
        }
    }
    body["expected_version"] = 1.into();
    body["settings"]["direction"] = "disabled".into();
    body.as_object_mut().unwrap().remove("token");
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(&path)
                .header("content-type", "application/json")
                .header("Idempotency-Key", Uuid::new_v4().to_string())
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = router
        .clone()
        .oneshot(
            Request::get(format!("{path}/runs?limit=1&offset=0"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let history = response_json(response).await;
    assert_eq!(history["total"], 1);
    assert_eq!(history["items"][0]["state"], "canceled");
    let response = router
        .clone()
        .oneshot(
            Request::get("/api/v1/repository-syncs?provider=github&state=disabled&limit=1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response_json(response).await["total"], 1);
    let read_claims = cicd::auth::AccessClaims {
        token_id: Some(Uuid::new_v4()),
        token_scopes: vec!["api:read".into()],
        ..claims
    };
    let reader = cicd::repository_sync::routes()
        .with_state(state)
        .layer(axum::Extension(read_claims));
    let response = reader
        .clone()
        .oneshot(Request::get(&path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response_json(response).await["writable"], false);
    let response = reader
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(&path)
                .header("content-type", "application/json")
                .header("Idempotency-Key", Uuid::new_v4().to_string())
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn additive_sync_migration_preserves_populated_repository_and_pipeline() {
    let url = std::env::var("CICD_TEST_DATABASE_URL").unwrap();
    let schema = format!("it_sync_upgrade_{}", Uuid::new_v4().simple());
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
            sqlx::postgres::PgConnectOptions::from_str(&url)
                .unwrap()
                .options([("search_path", schema.as_str())]),
        )
        .await
        .unwrap();
    let full = cicd::migrations().await.unwrap();
    let mut old = sqlx::migrate::Migrator::DEFAULT;
    old.migrations = full
        .iter()
        .filter(|migration| migration.version < 93)
        .cloned()
        .collect::<Vec<_>>()
        .into();
    old.run(&pool).await.unwrap();
    let repository = Uuid::new_v4();
    let project = Uuid::new_v4();
    let pipeline = Uuid::new_v4();
    let sha = "a".repeat(40);
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,'migration-sync-repo')")
        .bind(repository)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id) VALUES($1,'migration-sync-project','https://forge.invalid/disposable',$2)").bind(project).bind(repository).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO pipelines(id,project_id,git_ref,status,commit_sha) VALUES($1,$2,'main','success',$3)").bind(pipeline).bind(project).bind(&sha).execute(&pool).await.unwrap();
    full.run(&pool).await.unwrap();
    full.run(&pool).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT commit_sha FROM pipelines WHERE id=$1")
            .bind(pipeline)
            .fetch_one(&pool)
            .await
            .unwrap(),
        sha
    );
    assert_eq!(
        sqlx::query_scalar::<_, Uuid>("SELECT repository_id FROM projects WHERE id=$1")
            .bind(project)
            .fetch_one(&pool)
            .await
            .unwrap(),
        repository
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM repository_sync_connections")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    pool.close().await;
}

#[tokio::test]
async fn sync_access_respects_memberships_and_machine_project_binding() {
    let pool = test_pool().await;
    let user = Uuid::new_v4();
    let project = Uuid::new_v4();
    let repository = Uuid::new_v4();
    let other = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,username,role) VALUES($1,$2,'developer')")
        .bind(user)
        .bind(format!("qa-{user}"))
        .execute(&pool)
        .await
        .unwrap();
    for id in [repository, other] {
        sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
            .bind(id)
            .bind(format!("sync-{}", id.simple()))
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id) VALUES($1,$2,'https://forge.invalid/disposable',$3)").bind(project).bind(format!("qa-{project}")).bind(repository).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO project_memberships(project_id,user_id,role) VALUES($1,$2,'viewer')")
        .bind(project)
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let store = PgSyncStore {
        pool: pool.clone(),
        worker_enabled: true,
        interval_seconds: 300,
    };
    let human = Access {
        actor_id: user,
        project_id: None,
        admin: false,
        machine: false,
        writable: true,
    };
    assert!(store.authorize(repository, human, false).await.is_ok());
    assert!(store.authorize(repository, human, true).await.is_err());
    assert!(store.authorize(other, human, false).await.is_err());
    let machine = Access {
        actor_id: Uuid::new_v4(),
        project_id: Some(project),
        admin: false,
        machine: true,
        writable: true,
    };
    assert!(store.authorize(repository, machine, true).await.is_ok());
    assert!(store.authorize(other, machine, false).await.is_err());
    assert!(
        store
            .authorize(
                repository,
                Access {
                    project_id: None,
                    ..machine
                },
                false
            )
            .await
            .is_err()
    );
    pool.close().await;
}

#[tokio::test]
async fn incoming_event_ci_uses_exact_sha_and_shared_push_receipt() {
    let pool = test_pool().await;
    let repository = Uuid::new_v4();
    let project = Uuid::new_v4();
    let name = format!("sync-{}", repository.simple());
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
        .bind(repository)
        .bind(&name)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id) VALUES($1,$2,'https://forge.invalid/disposable',$3)").bind(project).bind(format!("qa-{project}")).bind(repository).execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO repository_push_configs(repository_id,configuration_id) VALUES($1,$2)",
    )
    .bind(repository)
    .bind(project)
    .execute(&pool)
    .await
    .unwrap();
    let root = std::env::temp_dir().join(format!("forge-event-{}", Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let local = root.join(format!("{name}.git"));
    assert!(
        tokio::process::Command::new("git")
            .args(["init", "--bare", "--initial-branch=main"])
            .arg(&local)
            .status()
            .await
            .unwrap()
            .success()
    );
    async fn git(path: &std::path::Path, args: &[&str]) -> String {
        let out = tokio::process::Command::new("git")
            .arg(format!("--git-dir={}", path.display()))
            .args(args)
            .output()
            .await
            .unwrap();
        assert!(out.status.success());
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }
    let tree = git(&local, &["hash-object", "-t", "tree", "-w", "--stdin"]).await;
    let a = git(
        &local,
        &[
            "-c",
            "user.name=QA",
            "-c",
            "user.email=qa@example.invalid",
            "commit-tree",
            &tree,
            "-m",
            "A",
        ],
    )
    .await;
    let b = git(
        &local,
        &[
            "-c",
            "user.name=QA",
            "-c",
            "user.email=qa@example.invalid",
            "commit-tree",
            &tree,
            "-p",
            &a,
            "-m",
            "B",
        ],
    )
    .await;
    git(&local, &["update-ref", "refs/heads/main", &b]).await;
    let mut config = cicd::config::RuntimeConfig::from_env_source(|_| None, false).unwrap();
    config.git.root = root.clone();
    config.secrets.key = Some([9; 32]);
    let app = cicd::api::app_with_git_and_config(
        Some(pool.clone()),
        config.git.to_git_config(),
        None,
        config.clone(),
    )
    .unwrap();
    let event = serde_json::json!({"repository_id":repository,"repository":name,"ref_name":"refs/heads/main","new_rev":a});
    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/internal/git-push")
                    .header("content-type", "application/json")
                    .body(Body::from(event.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    let shas: Vec<String> =
        sqlx::query_scalar("SELECT commit_sha FROM pipelines WHERE project_id=$1")
            .bind(project)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(shas, vec![a.clone()]);
    let store = PgSyncStore {
        pool: pool.clone(),
        worker_enabled: false,
        interval_seconds: 300,
    };
    let access = Access {
        actor_id: Uuid::new_v4(),
        project_id: None,
        admin: true,
        machine: false,
        writable: true,
    };
    app::save(
        &store,
        &Secrets(config.secrets.key),
        repository,
        Uuid::new_v4(),
        access,
        SaveConnection {
            expected_version: 0,
            settings: Settings {
                provider: Provider::Github,
                instance_url: "https://github.com".into(),
                repository_path: "a/b".into(),
                direction: Direction::Pull,
                automatic: false,
            },
            token: Some("unused-test-token".into()),
        },
        &[],
    )
    .await
    .unwrap();
    let run = store.enqueue(repository, "manual", None).await.unwrap();
    store
        .record_plan(
            run.id,
            &[cicd_domain::repository_sync::RefResult {
                ref_name: "refs/heads/main".into(),
                direction: "pull".into(),
                old_sha: None,
                new_sha: Some(a),
                state: "applied".into(),
                reason: None,
                event_delivered: false,
            }],
        )
        .await
        .unwrap();
    let blob = git(&local, &["hash-object", "-w", "--stdin"]).await;
    git(&local, &["update-ref", "refs/tags/blob-tag", &blob]).await;
    store
        .record_plan(
            run.id,
            &[cicd_domain::repository_sync::RefResult {
                ref_name: "refs/tags/blob-tag".into(),
                direction: "pull".into(),
                old_sha: None,
                new_sha: Some(blob),
                state: "applied".into(),
                reason: None,
                event_delivered: false,
            }],
        )
        .await
        .unwrap();
    let worker = tokio::spawn(cicd::repository_sync::supervisor(pool.clone(), config));
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            if store
                .refs(run.id, 20, 0)
                .await
                .unwrap()
                .0
                .iter()
                .all(|r| r.event_delivered)
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM pipelines WHERE project_id=$1")
            .bind(project)
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    assert!(
        store
            .refs(run.id, 20, 0)
            .await
            .unwrap()
            .0
            .iter()
            .any(|r| r.ref_name == "refs/tags/blob-tag"
                && r.reason.as_deref() == Some("ci_non_commit_tag"))
    );
    sqlx::query("UPDATE repository_push_configs SET configuration_id=NULL WHERE repository_id=$1")
        .bind(repository)
        .execute(&pool)
        .await
        .unwrap();
    let response=app.oneshot(Request::builder().method("POST").uri("/api/v1/internal/git-push").header("content-type","application/json").body(Body::from(serde_json::json!({"repository_id":repository,"repository":name,"ref_name":"refs/heads/main","new_rev":b}).to_string())).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM pipelines WHERE project_id=$1")
            .bind(project)
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    worker.abort();
    let _ = worker.await;
    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

async fn terminal(
    store: &PgSyncStore,
    repository: Uuid,
    run: Uuid,
) -> cicd_domain::repository_sync::SyncRun {
    tokio::time::timeout(std::time::Duration::from_secs(90), async {
        loop {
            let value = store.run(repository, run).await.unwrap();
            if value.finished_at.is_some() {
                return value;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    })
    .await
    .expect("sync worker must complete")
}

#[tokio::test]
#[ignore = "Requires a disposable repository seeded by live_provider_https and credentials supplied privately"]
async fn live_provider_worker_protection_automatic_and_lost_ack_recovery() {
    let pool = test_pool().await;
    let repository = Uuid::new_v4();
    let project = Uuid::new_v4();
    let name = format!("sync-{}", repository.simple());
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
        .bind(repository)
        .bind(&name)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id,protected_branches) VALUES($1,$2,'https://forge.invalid/disposable',$3,ARRAY['main'])").bind(project).bind(format!("qa-{project}")).bind(repository).execute(&pool).await.unwrap();
    let root = std::env::temp_dir().join(format!("forge-worker-{}", Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let local = root.join(format!("{name}.git"));
    assert!(
        tokio::process::Command::new("git")
            .args(["init", "--bare", "--initial-branch=main"])
            .arg(&local)
            .status()
            .await
            .unwrap()
            .success()
    );
    let mut config = cicd::config::RuntimeConfig::from_env_source(|_| None, false).unwrap();
    config.git.root = root.clone();
    config.secrets.key = Some([9; 32]);
    config.git_sync.enabled = true;
    let store = PgSyncStore {
        pool: pool.clone(),
        worker_enabled: true,
        interval_seconds: 300,
    };
    let secrets = Secrets(config.secrets.key);
    let access = Access {
        actor_id: Uuid::new_v4(),
        project_id: None,
        admin: true,
        machine: false,
        writable: true,
    };
    let path = std::env::var("CICD_SYNC_LIVE_REPOSITORY")
        .or_else(|_| std::env::var("CICD_SYNC_GITHUB_REPOSITORY"))
        .unwrap();
    assert!(
        path.rsplit('/')
            .next()
            .unwrap()
            .starts_with("forge-sync-qa-")
    );
    let token = std::env::var("CICD_SYNC_LIVE_TOKEN")
        .or_else(|_| std::env::var("CICD_SYNC_GITHUB_TOKEN"))
        .unwrap();
    let origin =
        std::env::var("CICD_SYNC_LIVE_ORIGIN").unwrap_or_else(|_| "https://github.com".into());
    let gitlab = std::env::var("CICD_SYNC_LIVE_PROVIDER").as_deref() == Ok("gitlab");
    let allowed = if gitlab { vec![origin.clone()] } else { vec![] };
    config.git_sync.allowed_gitlab_origins = allowed.clone();
    let mut command = SaveConnection {
        expected_version: 0,
        settings: Settings {
            provider: if gitlab {
                Provider::Gitlab
            } else {
                Provider::Github
            },
            instance_url: origin,
            repository_path: path,
            direction: Direction::Pull,
            automatic: false,
        },
        token: Some(token),
    };
    app::save(
        &store,
        &secrets,
        repository,
        Uuid::new_v4(),
        access,
        command.clone(),
        &allowed,
    )
    .await
    .unwrap();
    let follower = tokio::spawn(cicd::repository_sync::supervisor(
        pool.clone(),
        config.clone(),
    ));
    let worker = tokio::spawn(cicd::repository_sync::supervisor(pool.clone(), config));
    let first = store.enqueue(repository, "manual", None).await.unwrap();
    assert_eq!(
        terminal(&store, repository, first.id).await.state,
        "partial"
    );
    let refs = store.refs(first.id, 50, 0).await.unwrap().0;
    assert!(refs.iter().any(|r| r.ref_name == "refs/heads/main"
        && r.state == "blocked"
        && r.reason.as_deref() == Some("protected_branch")));
    assert!(refs.iter().any(|r| r.state == "applied"));
    sqlx::query("UPDATE projects SET protected_branches='{}' WHERE id=$1")
        .bind(project)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        store.protected_branches(repository).await.unwrap(),
        vec!["main".to_owned()]
    );
    // Changing the project default, rather than clearing explicit protection,
    // is required before this fixture's main branch may be synchronized.
    sqlx::query("UPDATE projects SET default_branch='qa-protected-default' WHERE id=$1")
        .bind(project)
        .execute(&pool)
        .await
        .unwrap();
    let second = store.enqueue(repository, "manual", None).await.unwrap();
    assert_eq!(
        terminal(&store, repository, second.id).await.state,
        "success"
    );
    command.expected_version = 1;
    command.settings.direction = Direction::Both;
    command.settings.automatic = true;
    command.token = None;
    app::save(
        &store,
        &secrets,
        repository,
        Uuid::new_v4(),
        access,
        command,
        &allowed,
    )
    .await
    .unwrap();
    let commit = tokio::process::Command::new("git")
        .arg(format!("--git-dir={}", local.display()))
        .args([
            "-c",
            "user.name=Forge QA",
            "-c",
            "user.email=qa@example.invalid",
            "commit-tree",
            "main^{tree}",
            "-p",
            "main",
            "-m",
            "Forge automatic feature",
        ])
        .output()
        .await
        .unwrap();
    assert!(commit.status.success());
    let sha = String::from_utf8(commit.stdout).unwrap().trim().to_owned();
    assert!(
        tokio::process::Command::new("git")
            .arg(format!("--git-dir={}", local.display()))
            .args(["update-ref", "refs/heads/forge-feature", &sha])
            .status()
            .await
            .unwrap()
            .success()
    );
    store.signal(repository).await.unwrap();
    let automatic = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            let runs = store.runs(repository, 20, 0).await.unwrap().0;
            if let Some(run) = runs.into_iter().find(|r| r.source == "git_change") {
                break run;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        terminal(&store, repository, automatic.id).await.state,
        "success"
    );
    // Reconstruct a durable intent after a remote push succeeded but its ACK was lost.
    worker.abort();
    follower.abort();
    let _ = worker.await;
    let _ = follower.await;
    let recovery = store.enqueue(repository, "manual", None).await.unwrap();
    store
        .record_plan(
            recovery.id,
            &[cicd_domain::repository_sync::RefResult {
                ref_name: "refs/heads/forge-feature".into(),
                direction: "push".into(),
                old_sha: None,
                new_sha: Some(sha),
                state: "unknown".into(),
                reason: Some("outcome_unknown".into()),
                event_delivered: true,
            }],
        )
        .await
        .unwrap();
    sqlx::query("UPDATE repository_sync_runs SET state='reconciling',lease_until=now()-interval '1 second' WHERE id=$1").bind(recovery.id).execute(&pool).await.unwrap();
    let mut config = cicd::config::RuntimeConfig::from_env_source(|_| None, false).unwrap();
    config.git.root = root.clone();
    config.secrets.key = Some([9; 32]);
    config.git_sync.enabled = true;
    config.git_sync.allowed_gitlab_origins = allowed;
    let worker = tokio::spawn(cicd::repository_sync::supervisor(pool.clone(), config));
    assert_eq!(
        terminal(&store, repository, recovery.id).await.state,
        "success"
    );
    assert_eq!(
        store.refs(recovery.id, 50, 0).await.unwrap().0[0].state,
        "applied"
    );
    worker.abort();
    let _ = worker.await;
    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn durable_sync_cas_idempotency_coalescing_and_fencing() {
    let pool = test_pool().await;
    let repository = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
        .bind(repository)
        .bind(format!("sync-{}", repository.simple()))
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
    let secrets = Secrets(Some([7; 32]));
    let settings = Settings {
        provider: Provider::Github,
        instance_url: "https://github.com".into(),
        repository_path: "example/disposable".into(),
        direction: Direction::Both,
        automatic: true,
    };
    let command = SaveConnection {
        expected_version: 0,
        settings: settings.clone(),
        token: Some("test-credential-never-returned".into()),
    };
    let operation = Uuid::new_v4();
    let saved = app::save(
        &store,
        &secrets,
        repository,
        operation,
        access,
        command.clone(),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(saved.version, 1);
    assert!(saved.has_credential);
    assert!(saved.next_check_at.unwrap() > Utc::now());
    assert_eq!(
        app::save(
            &store,
            &secrets,
            repository,
            operation,
            access,
            command.clone(),
            &[]
        )
        .await
        .unwrap()
        .version,
        1
    );
    let mut different = command.clone();
    different.token = Some("another-token".into());
    assert!(matches!(
        app::save(
            &store,
            &secrets,
            repository,
            operation,
            access,
            different,
            &[]
        )
        .await,
        Err(app::Error::Conflict(_))
    ));
    assert!(matches!(
        app::save(
            &store,
            &secrets,
            repository,
            Uuid::new_v4(),
            access,
            command,
            &[]
        )
        .await,
        Err(app::Error::Conflict(_))
    ));
    let encrypted: String = sqlx::query_scalar(
        "SELECT encrypted_credential FROM repository_sync_connections WHERE repository_id=$1",
    )
    .bind(repository)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!encrypted.contains("test-credential"));
    assert_eq!(
        secrets.open(&encrypted).unwrap(),
        "test-credential-never-returned"
    );
    let readback: String = sqlx::query_scalar(
        "SELECT readback::text FROM repository_sync_operations WHERE operation_id=$1",
    )
    .bind(operation)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!readback.contains("test-credential"));
    assert!(!readback.contains("encrypted_credential"));
    let first_key = Uuid::new_v4();
    let (a, b) = tokio::join!(
        store.enqueue(repository, "manual", Some((first_key, access))),
        store.enqueue(repository, "manual", Some((Uuid::new_v4(), access)))
    );
    let run = a.unwrap();
    assert_eq!(run.id, b.unwrap().id);
    store.signal(repository).await.unwrap();
    store.signal(repository).await.unwrap();
    store.schedule().await.unwrap();
    assert_eq!(store.runs(repository, 20, 0).await.unwrap().1, 1);
    let owner = Uuid::new_v4();
    let (claimed, generation) = store.claim(run.id, owner).await.unwrap().unwrap();
    assert!(store.claim(run.id, Uuid::new_v4()).await.unwrap().is_none());
    assert!(
        store
            .allowed_to_apply(&claimed, owner, generation)
            .await
            .unwrap()
    );
    sqlx::query(
        "UPDATE repository_sync_runs SET lease_until=now()-interval '1 second' WHERE id=$1",
    )
    .bind(run.id)
    .execute(&pool)
    .await
    .unwrap();
    let new_owner = Uuid::new_v4();
    let (_, new_generation) = store.claim(run.id, new_owner).await.unwrap().unwrap();
    assert!(new_generation > generation);
    assert!(
        !store
            .allowed_to_apply(&claimed, owner, generation)
            .await
            .unwrap()
    );
    assert!(
        store
            .allowed_to_apply(&claimed, new_owner, new_generation)
            .await
            .unwrap()
    );
    let blocked = SaveConnection {
        expected_version: 1,
        settings: Settings {
            repository_path: "example/changed".into(),
            ..settings.clone()
        },
        token: None,
    };
    assert!(matches!(
        app::save(
            &store,
            &secrets,
            repository,
            Uuid::new_v4(),
            access,
            blocked,
            &[]
        )
        .await,
        Err(app::Error::Conflict(_))
    ));
    let disabled = SaveConnection {
        expected_version: 1,
        settings: Settings {
            direction: Direction::Disabled,
            ..settings.clone()
        },
        token: None,
    };
    app::save(
        &store,
        &secrets,
        repository,
        Uuid::new_v4(),
        access,
        disabled,
        &[],
    )
    .await
    .unwrap();
    assert_eq!(
        store.run(repository, run.id).await.unwrap().state,
        "stopping"
    );
    assert!(
        !store
            .allowed_to_apply(&claimed, new_owner, new_generation)
            .await
            .unwrap()
    );
    store
        .finish(run.id, new_owner, new_generation, "canceled", None)
        .await
        .unwrap();
    assert!(
        store
            .enqueue(repository, "manual", Some((Uuid::new_v4(), access)))
            .await
            .is_err()
    );
    assert_eq!(
        store
            .enqueue(repository, "manual", Some((first_key, access)))
            .await
            .unwrap()
            .id,
        run.id
    );
    let enabled = SaveConnection {
        expected_version: 2,
        settings,
        token: None,
    };
    app::save(
        &store,
        &secrets,
        repository,
        Uuid::new_v4(),
        access,
        enabled,
        &[],
    )
    .await
    .unwrap();
    store.signal(repository).await.unwrap();
    store.schedule().await.unwrap();
    store.schedule().await.unwrap();
    assert_eq!(store.runs(repository, 20, 0).await.unwrap().1, 2);
    assert_eq!(
        store
            .connection(repository, access)
            .await
            .unwrap()
            .unwrap()
            .version,
        3
    );
    // A slot belongs to a dedicated DB session and cannot be re-used by a second worker.
    let slot = store.slot(1).await.unwrap().unwrap();
    assert!(store.slot(1).await.unwrap().is_none());
    drop(slot);
    pool.close().await;
}

#[tokio::test]
async fn queued_disable_keeps_history_and_manual_mode_ignores_automatic_signals() {
    let pool = test_pool().await;
    let repository = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
        .bind(repository)
        .bind(format!("sync-{}", repository.simple()))
        .execute(&pool)
        .await
        .unwrap();
    let store = PgSyncStore {
        pool: pool.clone(),
        worker_enabled: true,
        interval_seconds: 300,
    };
    let secrets = Secrets(Some([8; 32]));
    let access = Access {
        actor_id: Uuid::new_v4(),
        project_id: None,
        admin: true,
        machine: false,
        writable: true,
    };
    let mut command = SaveConnection {
        expected_version: 0,
        settings: Settings {
            provider: Provider::Github,
            instance_url: "https://github.com".into(),
            repository_path: "a/b".into(),
            direction: Direction::Pull,
            automatic: false,
        },
        token: Some("test-only".into()),
    };
    app::save(
        &store,
        &secrets,
        repository,
        Uuid::new_v4(),
        access,
        command.clone(),
        &[],
    )
    .await
    .unwrap();
    store.signal(repository).await.unwrap();
    store.schedule().await.unwrap();
    assert_eq!(store.runs(repository, 20, 0).await.unwrap().1, 0);
    let run = store.enqueue(repository, "manual", None).await.unwrap();
    command.expected_version = 1;
    command.settings.direction = Direction::Disabled;
    command.token = None;
    app::save(
        &store,
        &secrets,
        repository,
        Uuid::new_v4(),
        access,
        command.clone(),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(
        store.run(repository, run.id).await.unwrap().state,
        "canceled"
    );
    command.expected_version = 2;
    command.settings.repository_path = "other/repo".into();
    command.token = Some("replacement".into());
    app::save(
        &store,
        &secrets,
        repository,
        Uuid::new_v4(),
        access,
        command,
        &[],
    )
    .await
    .unwrap();
    assert_eq!(
        store
            .run(repository, run.id)
            .await
            .unwrap()
            .settings
            .repository_path,
        "a/b"
    );
    pool.close().await;
}

#[tokio::test]
async fn namespace_archive_pauses_queued_and_blocks_commands_without_losing_history() {
    let pool = test_pool().await;
    let repository = Uuid::new_v4();
    let group = Uuid::new_v4();
    let registry = Uuid::new_v4();
    let namespace = Uuid::new_v4();
    sqlx::query("INSERT INTO git_groups(id,slug,name) VALUES($1,$2,'Sync QA')")
        .bind(group)
        .bind(format!("g-{}", group.simple()))
        .execute(&pool)
        .await
        .unwrap();
    let binding = serde_json::json!({"schema_version":1,"operation_id":Uuid::new_v4(),"generation":1,"state":"active","namespace":{"registry_instance_id":registry,"namespace_id":namespace},"resource":{"resource_id":group,"instance_id":Uuid::new_v4(),"kind":"git_group"}});
    sqlx::query("INSERT INTO forge_namespace_bindings(resource_id,registry_instance_id,namespace_id,generation,state,command) VALUES($1,$2,$3,1,'active',$4)").bind(group).bind(registry).bind(namespace).bind(binding).execute(&pool).await.unwrap();
    sqlx::query("UPDATE git_groups SET namespace_managed=true WHERE id=$1")
        .bind(group)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
        .bind(repository)
        .bind(format!("sync-{}", repository.simple()))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE repository_catalog SET group_id=$2 WHERE id=$1")
        .bind(repository)
        .bind(group)
        .execute(&pool)
        .await
        .unwrap();
    let store = PgSyncStore {
        pool: pool.clone(),
        worker_enabled: true,
        interval_seconds: 300,
    };
    let secrets = Secrets(Some([8; 32]));
    let access = Access {
        actor_id: Uuid::new_v4(),
        project_id: None,
        admin: true,
        machine: false,
        writable: true,
    };
    let mut command = SaveConnection {
        expected_version: 0,
        settings: Settings {
            provider: Provider::Github,
            instance_url: "https://github.com".into(),
            repository_path: "a/b".into(),
            direction: Direction::Pull,
            automatic: true,
        },
        token: Some("test-only".into()),
    };
    app::save(
        &store,
        &secrets,
        repository,
        Uuid::new_v4(),
        access,
        command.clone(),
        &[],
    )
    .await
    .unwrap();
    let run = store.enqueue(repository, "manual", None).await.unwrap();
    sqlx::query("UPDATE forge_namespace_bindings SET state='archived',generation=2,command=command||'{\"state\":\"archived\",\"generation\":2}'::jsonb WHERE resource_id=$1").bind(group).execute(&pool).await.unwrap();
    assert!(store.candidates().await.unwrap().is_empty());
    command.expected_version = 1;
    command.token = None;
    assert!(
        app::save(
            &store,
            &secrets,
            repository,
            Uuid::new_v4(),
            access,
            command,
            &[]
        )
        .await
        .is_err()
    );
    assert_eq!(store.runs(repository, 20, 0).await.unwrap().1, 1);
    assert_eq!(store.run(repository, run.id).await.unwrap().state, "queued");
    assert_eq!(
        store
            .connection(repository, access)
            .await
            .unwrap()
            .unwrap()
            .paused_reason
            .as_deref(),
        Some("namespace_read_only")
    );
    sqlx::query("UPDATE forge_namespace_bindings SET state='active',generation=3,command=command||'{\"state\":\"active\",\"generation\":3}'::jsonb WHERE resource_id=$1").bind(group).execute(&pool).await.unwrap();
    assert_eq!(store.candidates().await.unwrap().len(), 1);
    pool.close().await;
}

#[tokio::test]
async fn machine_sync_settings_pass_full_http_middleware() {
    let pool = test_pool().await;
    let repository = Uuid::new_v4();
    let project = Uuid::new_v4();
    let account = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
        .bind(repository)
        .bind(format!("audit-{}", repository.simple()))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id) VALUES($1,$2,'https://forge.invalid/disposable',$3)")
        .bind(project).bind(format!("audit-{project}")).bind(repository)
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO service_accounts(id,name) VALUES($1,$2)")
        .bind(account)
        .bind(format!("audit-{account}"))
        .execute(&pool)
        .await
        .unwrap();
    let raw = "forge_sat_fictional_audit_fixture_never_a_real_credential";
    sqlx::query("INSERT INTO api_tokens(id,name,token_hash,token_hint,principal_type,service_account_id,project_id,scopes) VALUES($1,'audit-token',$2,'fictional','service_account',$3,$4,ARRAY['api:read','api:write','git:write'])")
        .bind(Uuid::new_v4()).bind(cicd::auth::hash_token(raw)).bind(account).bind(project)
        .execute(&pool).await.unwrap();
    let access = Access {
        actor_id: account,
        project_id: Some(project),
        admin: false,
        machine: true,
        writable: true,
    };
    let store = PgSyncStore {
        pool: pool.clone(),
        worker_enabled: true,
        interval_seconds: 300,
    };
    store.authorize(repository, access, true).await.unwrap();
    let mut config = cicd::config::RuntimeConfig::from_env_source(|_| None, false)
        .unwrap()
        .with_auth_secret(Some("fictional-audit-http-secret".into()));
    config.secrets.key = Some([7; 32]);
    config.git_sync.enabled = true;
    let root = std::env::temp_dir().join(format!("forge-machine-http-{repository}"));
    std::fs::create_dir(&root).unwrap();
    config.git.root = root.clone();
    assert!(
        tokio::process::Command::new("git")
            .args(["init", "--bare", "--initial-branch=main"])
            .arg(root.join(format!("audit-{}.git", repository.simple())))
            .stdout(std::process::Stdio::null())
            .status()
            .await
            .unwrap()
            .success()
    );
    let app = cicd::api::app_with_git_and_config(
        Some(pool.clone()),
        config.git.to_git_config(),
        None,
        config,
    )
    .unwrap();
    let principal = app
        .clone()
        .oneshot(
            Request::get("/api/v1/auth/principal")
                .header("authorization", format!("Bearer {raw}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(principal.status(), StatusCode::OK);
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/catalog/repositories/{repository}/sync"))
                .header("authorization", format!("Bearer {raw}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    assert_eq!(
        status,
        StatusCode::OK,
        "valid project-bound machine token must reach sync settings through the production router"
    );
    let uri = format!("/api/v1/catalog/repositories/{repository}/sync");
    for suffix in [
        "/runs".to_owned(),
        format!("/runs/{}", Uuid::new_v4()),
        format!("/runs/{}/refs", Uuid::new_v4()),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::get(format!("{uri}{suffix}"))
                    .header("authorization", format!("Bearer {raw}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            matches!(response.status(), StatusCode::OK | StatusCode::NOT_FOUND),
            "history reaches its handler"
        );
    }
    let settings = serde_json::json!({"provider":"github","instance_url":"https://github.com","repository_path":"fixture/repository","direction":"disabled","automatic":false});
    let command = serde_json::json!({"expected_version":0,"settings":settings,"token":"fictional-provider-credential"});
    let response = app
        .clone()
        .oneshot(
            Request::put(&uri)
                .header("authorization", format!("Bearer {raw}"))
                .header("content-type", "application/json")
                .header("Idempotency-Key", Uuid::new_v4().to_string())
                .body(Body::from(command.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("{uri}/runs"))
                .header("authorization", format!("Bearer {raw}"))
                .header("Idempotency-Key", Uuid::new_v4().to_string())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::CONFLICT,
        "disabled connection reaches command admission"
    );
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("{uri}/check"))
                .header("authorization", format!("Bearer {raw}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"settings":settings,"token":""}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "credential validation reached without network IO"
    );
    for scopes in [
        vec!["api:read"],
        vec!["api:write"],
        vec!["git:write"],
        vec!["api:read", "api:write"],
    ] {
        sqlx::query("UPDATE api_tokens SET scopes=$1 WHERE token_hash=$2")
            .bind(&scopes)
            .bind(cicd::auth::hash_token(raw))
            .execute(&pool)
            .await
            .unwrap();
        let response = app
            .clone()
            .oneshot(
                Request::get(&uri)
                    .header("authorization", format!("Bearer {raw}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            if scopes.contains(&"api:read") {
                StatusCode::OK
            } else {
                StatusCode::FORBIDDEN
            }
        );
        let response = app
            .clone()
            .oneshot(
                Request::post(format!("{uri}/runs"))
                    .header("authorization", format!("Bearer {raw}"))
                    .header("Idempotency-Key", Uuid::new_v4().to_string())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "write requires both api:write and git:write"
        );
    }
    sqlx::query("UPDATE api_tokens SET scopes=ARRAY['api:read','api:write','git:write'],project_id=NULL WHERE token_hash=$1").bind(cicd::auth::hash_token(raw)).execute(&pool).await.unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::get(&uri)
                .header("authorization", format!("Bearer {raw}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "unbound machine token cannot read repository"
    );
    sqlx::query("UPDATE api_tokens SET project_id=$1 WHERE token_hash=$2")
        .bind(project)
        .bind(cicd::auth::hash_token(raw))
        .execute(&pool)
        .await
        .unwrap();
    let foreign_project = Uuid::new_v4();
    sqlx::query("INSERT INTO projects(id,name,repository_url) VALUES($1,$2,'https://forge.invalid/foreign')").bind(foreign_project).bind(format!("foreign-{foreign_project}")).execute(&pool).await.unwrap();
    sqlx::query("UPDATE api_tokens SET project_id=$1 WHERE service_account_id=$2")
        .bind(foreign_project)
        .bind(account)
        .execute(&pool)
        .await
        .unwrap();
    for method in ["GET", "POST"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(if method == "GET" {
                        uri.clone()
                    } else {
                        format!("{uri}/runs")
                    })
                    .header("authorization", format!("Bearer {raw}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "foreign project binding cannot reach repository commands or readback"
        );
    }
    sqlx::query("UPDATE api_tokens SET project_id=$1 WHERE service_account_id=$2")
        .bind(project)
        .bind(account)
        .execute(&pool)
        .await
        .unwrap();
    for mutation in [
        "UPDATE api_tokens SET revoked_at=now() WHERE service_account_id=$1",
        "UPDATE api_tokens SET revoked_at=NULL,expires_at=now()-interval '1 second' WHERE service_account_id=$1",
        "UPDATE api_tokens SET expires_at=NULL WHERE service_account_id=$1",
        "UPDATE service_accounts SET enabled=false WHERE id=$1",
    ] {
        sqlx::query(mutation)
            .bind(account)
            .execute(&pool)
            .await
            .unwrap();
        if mutation.contains("expires_at=NULL") {
            continue;
        }
        let response = app
            .clone()
            .oneshot(
                Request::get(&uri)
                    .header("authorization", format!("Bearer {raw}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "revoked, expired or disabled identity is rejected"
        );
    }
    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn automatic_signals_cover_all_enabled_directions_and_coalesce() {
    let pool = test_pool().await;
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
    for direction in [
        Direction::Disabled,
        Direction::Pull,
        Direction::Push,
        Direction::Both,
    ] {
        let repository = Uuid::new_v4();
        sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
            .bind(repository)
            .bind(format!("signals-{}", repository.simple()))
            .execute(&pool)
            .await
            .unwrap();
        let command = SaveConnection {
            expected_version: 0,
            settings: Settings {
                provider: Provider::Github,
                instance_url: "https://github.com".into(),
                repository_path: "fixture/repository".into(),
                direction,
                automatic: true,
            },
            token: Some("fictional-signal-fixture".into()),
        };
        app::save(
            &store,
            &Secrets(Some([3; 32])),
            repository,
            Uuid::new_v4(),
            access,
            command,
            &[],
        )
        .await
        .unwrap();
        store.schedule().await.unwrap();
        assert_eq!(
            store.runs(repository, 10, 0).await.unwrap().1,
            0,
            "save waits for the configured interval"
        );
        for _ in 0..3 {
            store.signal(repository).await.unwrap();
        }
        store.schedule().await.unwrap();
        let (runs, total) = store.runs(repository, 10, 0).await.unwrap();
        assert_eq!(
            total,
            if direction == Direction::Disabled {
                0
            } else {
                1
            }
        );
        if direction == Direction::Disabled {
            continue;
        }
        assert_eq!(runs[0].source, "git_change");
        for _ in 0..3 {
            store.signal(repository).await.unwrap();
            store.schedule().await.unwrap();
        }
        assert_eq!(
            store.runs(repository, 10, 0).await.unwrap().1,
            1,
            "signals during active work coalesce"
        );
        let owner = Uuid::new_v4();
        let (_, generation) = store.claim(runs[0].id, owner).await.unwrap().unwrap();
        store
            .finish(runs[0].id, owner, generation, "success", None)
            .await
            .unwrap();
        store.schedule().await.unwrap();
        assert_eq!(
            store.runs(repository, 10, 0).await.unwrap().1,
            2,
            "one follow-up survives signals during work"
        );
    }
    pool.close().await;
}

#[tokio::test]
async fn sync_server_pages_large_lists_history_and_refs() {
    let pool = test_pool().await;
    let store = PgSyncStore {
        pool: pool.clone(),
        worker_enabled: false,
        interval_seconds: 300,
    };
    let access = Access {
        actor_id: Uuid::new_v4(),
        project_id: None,
        admin: true,
        machine: false,
        writable: true,
    };
    let mut repositories = Vec::new();
    for i in 0..45 {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
            .bind(id)
            .bind(format!("page-{i:03}"))
            .execute(&pool)
            .await
            .unwrap();
        let settings = Settings {
            provider: if i % 2 == 0 {
                Provider::Github
            } else {
                Provider::Gitlab
            },
            instance_url: if i % 2 == 0 {
                "https://github.com"
            } else {
                "https://git.example.invalid"
            }
            .into(),
            repository_path: format!("fixture/page-{i:03}"),
            direction: Direction::Disabled,
            automatic: false,
        };
        app::save(
            &store,
            &Secrets(Some([4; 32])),
            id,
            Uuid::new_v4(),
            access,
            SaveConnection {
                expected_version: 0,
                settings,
                token: Some("fictional-pagination-canary".into()),
            },
            &["https://git.example.invalid".into()],
        )
        .await
        .unwrap();
        repositories.push(id);
    }
    let config = cicd::config::RuntimeConfig::from_env_source(|_| None, false).unwrap();
    let state = std::sync::Arc::new(cicd::api::AppState {
        pool: Some(pool.clone()),
        namespace_admission_pool: None,
        auth_secret: None,
        git: config.git.to_git_config(),
        config,
        running_jobs: None,
        rate_limiter: std::sync::Arc::new(cicd::rate_limit::RateLimiter::default()),
    });
    let claims = cicd::auth::AccessClaims {
        sub: access.actor_id,
        service_account_name: None,
        sid: None,
        token_id: None,
        token_project_id: None,
        token_scopes: vec![],
        role: "admin".into(),
        ver: 0,
        iat: 0,
        exp: i64::MAX,
    };
    let router = cicd::repository_sync::routes()
        .with_state(state)
        .layer(axum::Extension(claims));
    async fn page(router: &axum::Router, path: String) -> serde_json::Value {
        let response = router
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert!(!body.to_string().contains("fictional-pagination-canary"));
        body
    }
    let first = page(&router, "/api/v1/repository-syncs?limit=20".into()).await;
    let last = page(
        &router,
        "/api/v1/repository-syncs?limit=20&offset=40".into(),
    )
    .await;
    assert_eq!(first["total"], 45);
    assert_eq!(first["items"].as_array().unwrap().len(), 20);
    assert_eq!(last["items"].as_array().unwrap().len(), 5);
    assert_eq!(last["items"][0]["repository_name"], "page-040");
    assert_eq!(
        page(
            &router,
            "/api/v1/repository-syncs?provider=github&state=disabled".into()
        )
        .await["total"],
        23
    );
    assert_eq!(
        page(
            &router,
            "/api/v1/repository-syncs?provider=gitlab&search=page-043".into()
        )
        .await["total"],
        1
    );
    let repository = repositories[0];
    let mut last_run = Uuid::nil();
    for i in 0..25 {
        last_run = Uuid::new_v4();
        sqlx::query("INSERT INTO repository_sync_runs(id,repository_id,connection_version,settings,source,state,created_at,finished_at) SELECT $1,repository_id,version,settings,'manual','success',now()+make_interval(secs=>$3::double precision),now() FROM repository_sync_connections WHERE repository_id=$2").bind(last_run).bind(repository).bind(i as f64).execute(&pool).await.unwrap();
    }
    for i in 0..55 {
        sqlx::query("INSERT INTO repository_sync_refs(run_id,ref_name,direction,state,event_delivered) VALUES($1,$2,'none','unchanged',true)").bind(last_run).bind(format!("refs/heads/page-{i:03}")).execute(&pool).await.unwrap();
    }
    let runs = page(
        &router,
        format!("/api/v1/catalog/repositories/{repository}/sync/runs?limit=20&offset=20"),
    )
    .await;
    assert_eq!(runs["total"], 25);
    assert_eq!(runs["items"].as_array().unwrap().len(), 5);
    let refs = page(
        &router,
        format!(
            "/api/v1/catalog/repositories/{repository}/sync/runs/{last_run}/refs?limit=50&offset=50"
        ),
    )
    .await;
    assert_eq!(refs["total"], 55);
    assert_eq!(refs["items"].as_array().unwrap().len(), 5);
    assert_eq!(refs["items"][0]["ref_name"], "refs/heads/page-050");
    for query in [
        "provider=invalid",
        "state=invalid",
        "limit=abc",
        "group_id=invalid",
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::get(format!("/api/v1/repository-syncs?{query}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(response.status().is_client_error());
    }
    pool.close().await;
}

#[tokio::test]
async fn busy_repository_guards_do_not_starve_other_queued_work() {
    let pool = test_pool().await;
    let store = PgSyncStore {
        pool: pool.clone(),
        worker_enabled: true,
        interval_seconds: 300,
    };
    let root = std::env::temp_dir().join(format!("forge-sync-fairness-{}", Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let access = Access {
        actor_id: Uuid::new_v4(),
        project_id: None,
        admin: true,
        machine: false,
        writable: true,
    };
    let mut guards = Vec::new();
    let mut free = None;
    for i in 0..3 {
        let repository = Uuid::new_v4();
        sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
            .bind(repository)
            .bind(format!("busy-{}", repository.simple()))
            .execute(&pool)
            .await
            .unwrap();
        app::save(
            &store,
            &Secrets(Some([3; 32])),
            repository,
            Uuid::new_v4(),
            access,
            SaveConnection {
                expected_version: 0,
                settings: Settings {
                    provider: Provider::Github,
                    instance_url: "https://github.com".into(),
                    repository_path: "fixture/repository".into(),
                    direction: Direction::Pull,
                    automatic: false,
                },
                token: Some("fictional-fairness-fixture".into()),
            },
            &[],
        )
        .await
        .unwrap();
        let run = store.enqueue(repository, "manual", None).await.unwrap();
        if i < 2 {
            guards.push(
                cicd_infra::sync_git::MutationGuard::acquire(&root, repository)
                    .await
                    .unwrap(),
            );
        } else {
            free = Some((repository, run.id));
        }
    }
    let mut config = cicd::config::RuntimeConfig::from_env_source(|_| None, false).unwrap();
    config.git.root = root.clone();
    config.git_sync.enabled = true;
    config.secrets.key = Some([3; 32]);
    let worker = tokio::spawn(cicd::repository_sync::supervisor(pool.clone(), config));
    let (repository, run) = free.unwrap();
    let progressed = tokio::time::timeout(std::time::Duration::from_secs(8), async {
        loop {
            let run = store.run(repository, run).await.unwrap();
            if run.started_at.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .is_ok();
    worker.abort();
    let _ = worker.await;
    drop(guards);
    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
    assert!(
        progressed,
        "two busy repository guards must not consume all scheduler capacity"
    );
}

#[tokio::test]
async fn sync_put_is_allowed_by_configured_cors_preflight() {
    let config = cicd::config::RuntimeConfig::from_env_source(
        |key| {
            (key == "CICD_CORS_ALLOWED_ORIGINS").then(|| "https://forge-ui.example.invalid".into())
        },
        false,
    )
    .unwrap();
    let app =
        cicd::api::app_with_git_and_config(None, config.git.to_git_config(), None, config).unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri(format!(
                    "/api/v1/catalog/repositories/{}/sync",
                    Uuid::new_v4()
                ))
                .header("origin", "https://forge-ui.example.invalid")
                .header("access-control-request-method", "PUT")
                .header(
                    "access-control-request-headers",
                    "authorization,content-type,idempotency-key",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["access-control-allow-origin"],
        "https://forge-ui.example.invalid"
    );
    let methods = response.headers()["access-control-allow-methods"]
        .to_str()
        .unwrap();
    assert!(
        methods.split(',').any(|method| method.trim() == "PUT"),
        "saving sync settings requires PUT in configured CORS: {methods}"
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri(format!(
                    "/api/v1/catalog/repositories/{}/sync",
                    Uuid::new_v4()
                ))
                .header("origin", "https://untrusted.example.invalid")
                .header("access-control-request-method", "PUT")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        !response
            .headers()
            .contains_key("access-control-allow-origin")
    );
}
