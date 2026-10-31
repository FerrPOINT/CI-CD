//! Incoming CI delivery must read the event commit, including its configuration.
use super::*;
use std::path::{Path, PathBuf};
#[path = "repository_sync_common_admission.rs"]
mod common_admission;
#[path = "repository_sync_required_owner.rs"]
mod required_owner;

struct OwnedGit(PathBuf);
impl Drop for OwnedGit {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Worker(tokio::task::JoinHandle<()>);
impl Drop for Worker {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn git(directory: &Path, args: &[&str]) -> String {
    let result = tokio::process::Command::new("git")
        .current_dir(directory)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .kill_on_drop(true)
        .output()
        .await
        .unwrap();
    assert!(
        result.status.success(),
        "owned local Git fixture failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap().trim().to_owned()
}

#[tokio::test]
async fn incoming_event_compiles_configuration_and_jobs_from_exact_commit_not_later_head() {
    let pool = test_pool().await;
    let authority = if std::env::var("CICD_AUTH__CENTRAL_APPLICATION_ACCESS_REQUIRED").as_deref()
        == Ok("true")
    {
        Some(required_owner::RequiredOwner::new(&pool).await)
    } else {
        None
    };
    let repository = Uuid::new_v4();
    let project = Uuid::new_v4();
    let name = format!("sync-pinned-config-{}", repository.simple());
    let root = OwnedGit(std::env::temp_dir().join(format!("forge-pinned-config-{repository}")));
    let work = root.0.join("source");
    std::fs::create_dir_all(&work).unwrap();
    git(&work, &["init", "--initial-branch=main"]).await;
    git(&work, &["config", "user.name", "Owned CI fixture"]).await;
    git(&work, &["config", "user.email", "qa@example.invalid"]).await;
    const CONFIG_A: &str = "version: 1\ndefaults:\n  image: alpine:3.21\njobs:\n  event_a:\n    commands: [\"echo configuration-from-event-a\"]\n";
    const CONFIG_B: &str = "version: 1\ndefaults:\n  image: alpine:3.21\njobs:\n  later_b:\n    commands: [\"echo configuration-from-later-b\"]\n";
    std::fs::write(work.join(".forge-ci.yml"), CONFIG_A).unwrap();
    git(&work, &["add", ".forge-ci.yml"]).await;
    git(&work, &["commit", "-m", "Event A configuration"]).await;
    let a = git(&work, &["rev-parse", "HEAD"]).await;
    std::fs::write(work.join(".forge-ci.yml"), CONFIG_B).unwrap();
    git(&work, &["add", ".forge-ci.yml"]).await;
    git(&work, &["commit", "-m", "Later B configuration"]).await;
    let b = git(&work, &["rev-parse", "HEAD"]).await;
    assert_ne!(a, b);
    let local = root.0.join(format!("{name}.git"));
    git(
        &root.0,
        &[
            "clone",
            "--bare",
            work.to_str().unwrap(),
            local.to_str().unwrap(),
        ],
    )
    .await;
    assert_eq!(git(&local, &["rev-parse", "refs/heads/main"]).await, b);
    assert_eq!(
        git(&local, &["show", &format!("{a}:.forge-ci.yml")]).await,
        CONFIG_A.trim()
    );
    assert_eq!(
        git(&local, &["show", "main:.forge-ci.yml"]).await,
        CONFIG_B.trim()
    );

    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
        .bind(repository)
        .bind(&name)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id) VALUES($1,$2,'https://forge.invalid/owned-local-only',$3)")
        .bind(project).bind(format!("pinned-config-{project}")).bind(repository)
        .execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO repository_push_configs(repository_id,configuration_id) VALUES($1,$2)",
    )
    .bind(repository)
    .bind(project)
    .execute(&pool)
    .await
    .unwrap();
    let mut config = cicd::config::RuntimeConfig::from_env_source(|_| None, false).unwrap();
    config.git.root = root.0.clone();
    config.secrets.key = Some([19; 32]);
    // Only event delivery runs. A queued network sync must not execute here.
    config.git_sync.enabled = false;
    let store = PgSyncStore {
        pool: pool.clone(),
        worker_enabled: false,
        interval_seconds: 300,
    };
    let command = SaveConnection {
        expected_version: 0,
        settings: Settings {
            provider: Provider::Github,
            instance_url: "https://github.com".into(),
            repository_path: "fixture/local-only".into(),
            direction: Direction::Pull,
            automatic: false,
            protected_branch_updates: cicd_domain::repository_sync::ProtectedBranchUpdates::Blocked,
        },
        token: Some("fictional-unused-local-ci-credential".into()),
    };
    if authority.is_some() {
        let config = config
            .clone()
            .with_auth_secret(Some("fixture-sync-owner-auth".into()));
        let router = cicd::api::app_with_git_and_config(
            Some(pool.clone()),
            config.git.to_git_config(),
            None,
            config,
        )
        .unwrap();
        let response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/catalog/repositories/{repository}/sync"))
                    .header("authorization", "Bearer cicd_fixture_sync_owner")
                    .header("content-type", "application/json")
                    .header("idempotency-key", Uuid::new_v4().to_string())
                    .body(Body::from(serde_json::to_vec(&command).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        assert_eq!(
            status,
            StatusCode::OK,
            "verified human save: {}",
            String::from_utf8_lossy(&body)
        );
    } else {
        app::save(
            &store,
            &Secrets(config.secrets.key),
            repository,
            Uuid::new_v4(),
            Access {
                actor_id: Uuid::new_v4(),
                project_id: None,
                admin: true,
                machine: false,
                writable: true,
            },
            command,
            &[],
        )
        .await
        .unwrap();
    }
    if let Some(owner) = authority.as_ref() {
        common_admission::verify(&pool, &config, repository, owner).await;
    }
    let run = store.enqueue(repository, "manual", None).await.unwrap();
    assert_eq!(
        store.event_owner(run.id).await.unwrap(),
        authority.as_ref().map(|owner| owner.subject)
    );
    store
        .record_plan(
            run.id,
            &[cicd_domain::repository_sync::RefResult {
                ref_name: "refs/heads/main".into(),
                direction: "pull".into(),
                old_sha: None,
                new_sha: Some(a.clone()),
                state: "applied".into(),
                reason: None,
                event_delivered: false,
            }],
        )
        .await
        .unwrap();
    let worker = Worker(tokio::spawn(cicd::repository_sync::supervisor(
        pool.clone(),
        config.clone(),
    )));
    // Replay the actual durable event twice. Its original commit stays A even
    // though the named branch has already moved to the different config at B.
    for attempt in 0..2 {
        if attempt != 0 {
            if let Some(owner) = authority.as_ref() {
                use std::sync::atomic::Ordering;
                owner.allowed.store(false, Ordering::SeqCst);
                let before = owner.calls.load(Ordering::SeqCst);
                sqlx::query("UPDATE repository_sync_refs SET event_delivered=false,event_next_attempt_at=now() WHERE run_id=$1").bind(run.id).execute(&pool).await.unwrap();
                let retried=tokio::time::timeout(std::time::Duration::from_secs(10),async {
                    loop {
                        let retried:bool=sqlx::query_scalar("SELECT event_next_attempt_at>now() FROM repository_sync_refs WHERE run_id=$1").bind(run.id).fetch_one(&pool).await.unwrap();
                        if owner.calls.load(Ordering::SeqCst)>before && retried {break;}
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }
                }).await;
                assert!(
                    retried.is_ok(),
                    "revocation not observed: calls_before={before}, calls_after={}, delivered={}, worker_finished={}",
                    owner.calls.load(Ordering::SeqCst),
                    store.refs(run.id, 20, 0).await.unwrap().0[0].event_delivered,
                    worker.0.is_finished()
                );
                assert!(!store.refs(run.id, 20, 0).await.unwrap().0[0].event_delivered);
                owner.allowed.store(true, Ordering::SeqCst);
            }
            sqlx::query("UPDATE repository_sync_refs SET event_delivered=false WHERE run_id=$1 AND ref_name='refs/heads/main'")
                .bind(run.id).execute(&pool).await.unwrap();
            sqlx::query(
                "UPDATE repository_sync_refs SET event_next_attempt_at=now() WHERE run_id=$1",
            )
            .bind(run.id)
            .execute(&pool)
            .await
            .unwrap();
        }
        tokio::time::timeout(std::time::Duration::from_secs(15), async {
            loop {
                if store.refs(run.id, 20, 0).await.unwrap().0[0].event_delivered {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("production incoming event delivery");
    }
    let pipelines: Vec<(Uuid, String, String)> =
        sqlx::query_as("SELECT id,git_ref,commit_sha FROM pipelines WHERE project_id=$1")
            .bind(project)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        pipelines.len(),
        1,
        "durable event replay cannot duplicate CI"
    );
    let pipeline = pipelines[0].0;
    if let Some(owner) = authority.as_ref() {
        let saved: Uuid = sqlx::query_scalar(
            "SELECT owner_subject FROM pipeline_human_owners WHERE pipeline_id=$1",
        )
        .bind(pipeline)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(saved, owner.subject);
    }
    assert_eq!(pipelines[0].1, "main");
    assert_eq!(pipelines[0].2, a);
    let (source, resolved, raw, plan): (String, String, String, serde_json::Value) = sqlx::query_as("SELECT config_source,resolved_commit_sha,raw_config,plan FROM pipeline_plans WHERE pipeline_id=$1")
        .bind(pipeline).fetch_one(&pool).await.unwrap();
    assert_eq!(
        source, "repository",
        "legacy fallback cannot satisfy this oracle"
    );
    assert_eq!(resolved, a);
    assert_eq!(raw, CONFIG_A, "configuration bytes come from the event SHA");
    assert_ne!(raw, CONFIG_B);
    assert_eq!(plan["resolved_commit_sha"], a);
    assert_eq!(plan["config_source"], "repository");
    assert_eq!(plan["git_ref"], "main");
    assert_eq!(plan["format"], "v1-dag");
    let planned = plan["jobs"].as_array().unwrap();
    assert_eq!(planned.len(), 1);
    assert_eq!(planned[0]["key"], "event_a");
    assert_eq!(
        planned[0]["commands"],
        serde_json::json!(["echo configuration-from-event-a"])
    );
    let jobs: Vec<(String, String)> = sqlx::query_as("SELECT j.name,j.command FROM jobs j JOIN stages s ON s.id=j.stage_id WHERE s.pipeline_id=$1 ORDER BY j.position")
        .bind(pipeline).fetch_all(&pool).await.unwrap();
    assert_eq!(
        jobs,
        vec![(
            "event_a".into(),
            "set -e\necho configuration-from-event-a".into()
        )]
    );
    assert_eq!(git(&local, &["rev-parse", "refs/heads/main"]).await, b);
    assert_eq!(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM pipeline_triggers WHERE project_id=$1 AND pipeline_id=$2 AND source='git-push'")
        .bind(project).bind(pipeline).fetch_one(&pool).await.unwrap(), 1);
    if authority.is_some() {
        // A legacy/machine intent cannot borrow the most recently saved human
        // or an arbitrary local administrator merely to make delivery succeed.
        let connection = Uuid::new_v4();
        let command = SaveConnection {
            expected_version: 0,
            settings: run.settings.clone(),
            token: Some("fictional-unbound-event-credential".into()),
        };
        app::save(
            &store.scoped(connection),
            &Secrets(config.secrets.key),
            repository,
            Uuid::new_v4(),
            Access {
                actor_id: Uuid::new_v4(),
                project_id: Some(project),
                admin: false,
                machine: true,
                writable: true,
            },
            command.clone(),
            &[],
        )
        .await
        .unwrap();
        let unbound = store
            .enqueue_at(repository, connection, "manual", None)
            .await
            .unwrap();
        assert_eq!(store.event_owner(unbound.id).await.unwrap(), None);
        store
            .record_plan(
                unbound.id,
                &[cicd_domain::repository_sync::RefResult {
                    ref_name: "refs/heads/unbound-owner".into(),
                    direction: "pull".into(),
                    old_sha: None,
                    new_sha: Some(a.clone()),
                    state: "applied".into(),
                    reason: None,
                    event_delivered: false,
                }],
            )
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let retried: bool = sqlx::query_scalar(
                    "SELECT event_next_attempt_at>now() FROM repository_sync_refs WHERE run_id=$1",
                )
                .bind(unbound.id)
                .fetch_one(&pool)
                .await
                .unwrap();
                if retried {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("unbound event must remain pending, not acquire an administrator");
        assert!(!store.refs(unbound.id, 20, 0).await.unwrap().0[0].event_delivered);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM pipelines WHERE project_id=$1")
                .bind(project)
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
    }
    println!(
        "SYNC_CI_PINNED_CONFIG_OK event_sha={a} branch_head={b} raw_config=A compiled_job=event_a pipelines=1 replayed_events=2"
    );
    worker.0.abort();
    drop(worker);
    pool.close().await;
}
