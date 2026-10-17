use super::*;
use cicd_app::repository_sync::{self as app, Access, Crypto};
use cicd_domain::repository_sync::{Direction, Provider, SaveConnection, Settings};
use cicd_infra::{repository_sync::PgSyncStore, secrets::Secrets};

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
            axum::Json(cicd::pulls::PrAction {
                action: "merge".into(),
            }),
        )
    };
    let (first, second) = tokio::join!(merge(), merge());
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    let (accepted, rejected) = match (first, second) {
        (Ok(value), Err(error)) | (Err(error), Ok(value)) => (value.0, error),
        _ => panic!("one merge must win"),
    };
    use axum::response::IntoResponse;
    assert_eq!(rejected.into_response().status(), StatusCode::CONFLICT);
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
