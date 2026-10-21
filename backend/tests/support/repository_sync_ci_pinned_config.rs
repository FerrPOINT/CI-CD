//! Incoming CI delivery must read the event commit, including its configuration.
use super::*;
use std::path::{Path, PathBuf};

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
        SaveConnection {
            expected_version: 0,
            settings: Settings {
                provider: Provider::Github,
                instance_url: "https://github.com".into(),
                repository_path: "fixture/local-only".into(),
                direction: Direction::Pull,
                automatic: false,
            },
            token: Some("fictional-unused-local-ci-credential".into()),
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
            sqlx::query("UPDATE repository_sync_refs SET event_delivered=false WHERE run_id=$1 AND ref_name='refs/heads/main'")
                .bind(run.id).execute(&pool).await.unwrap();
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
    println!(
        "SYNC_CI_PINNED_CONFIG_OK event_sha={a} branch_head={b} raw_config=A compiled_job=event_a pipelines=1 replayed_events=2"
    );
    worker.0.abort();
    drop(worker);
    pool.close().await;
}
