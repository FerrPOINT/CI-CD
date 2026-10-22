//! Full-schema drain checks with the real authenticated runner completion route.
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chrono::Utc;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

pub(super) struct Work {
    pub(super) group: Uuid,
    pub(super) project: Uuid,
    pub(super) pipeline: Uuid,
    pub(super) job: Uuid,
    pub(super) attempt: Uuid,
    pub(super) lease: Uuid,
    pub(super) storage: String,
}

pub(super) async fn work(pool: &PgPool, runner: Option<Uuid>) -> Work {
    let status = if runner.is_some() {
        "running"
    } else {
        "queued"
    };
    let repository = Uuid::new_v4();
    let storage = format!("drain-{}", repository.simple());
    let w = Work {
        group: Uuid::new_v4(),
        project: Uuid::new_v4(),
        pipeline: Uuid::new_v4(),
        job: Uuid::new_v4(),
        attempt: Uuid::new_v4(),
        lease: Uuid::new_v4(),
        storage: storage.clone(),
    };
    sqlx::query("INSERT INTO git_groups(id,slug,name) VALUES($1,$2,'Drain fixture')")
        .bind(w.group)
        .bind(format!("drain-{}", w.group.simple()))
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO repositories(id,name,visibility) VALUES($1,$2,'private')")
        .bind(repository)
        .bind(&storage)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("UPDATE repository_catalog SET group_id=$1 WHERE id=$2")
        .bind(w.group)
        .bind(repository)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id) VALUES($1,$2,'https://example.invalid/source.git',$3)")
        .bind(w.project).bind(format!("drain-{}",w.project)).bind(repository).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO pipelines(id,project_id,git_ref,status) VALUES($1,$2,'main',$3)")
        .bind(w.pipeline)
        .bind(w.project)
        .bind(status)
        .execute(pool)
        .await
        .unwrap();
    let stage = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO stages(id,pipeline_id,name,position,status) VALUES($1,$2,'test',0,$3)",
    )
    .bind(stage)
    .bind(w.pipeline)
    .bind(status)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO jobs(id,stage_id,name,image,command,position,status,started_at) VALUES($1,$2,'test','unused','true',0,$3,CASE WHEN $3='running' THEN now() END)")
        .bind(w.job).bind(stage).bind(status).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO execution_attempts(id,job_id,attempt_no,status,trigger,started_at) VALUES($1,$2,1,$3,'initial',CASE WHEN $3='running' THEN now() END)")
        .bind(w.attempt).bind(w.job).bind(status).execute(pool).await.unwrap();
    if let Some(runner) = runner {
        sqlx::query("INSERT INTO job_leases(id,job_id,attempt_id,runner_id,runner_name,generation,lease_expires_at,acknowledged_at,lease_token_hash) VALUES($1,$2,$3,$4,'drain-owner',1,now()+interval '1 minute',now(),$5)")
        .bind(w.lease).bind(w.job).bind(w.attempt).bind(runner).bind(cicd::auth::hash_token(&format!("drain-lease-fixture-{}",w.lease))).execute(pool).await.unwrap();
    }

    w
}

pub(super) fn completion(w: &Work, credential: &str) -> Request<Body> {
    Request::post(format!("/api/v1/runner/leases/{}/complete", w.lease))
        .header("authorization", format!("Bearer {credential}"))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "protocolVersion":1, "leaseToken":format!("drain-lease-fixture-{}",w.lease),
                "fencingToken":1,"attemptId":w.attempt,"outcome":"success","finishedAt":Utc::now()
            })
            .to_string(),
        ))
        .unwrap()
}

pub async fn verify(pool: &PgPool) {
    let runner = Uuid::new_v4();
    let foreign = Uuid::new_v4();
    for (id, secret) in [
        (runner, "drain-owner-fixture"),
        (foreign, "drain-foreign-fixture"),
    ] {
        sqlx::query("INSERT INTO runners(id,name,credential_hash,credential_expires_at) VALUES($1,$2,$3,now()+interval '1 hour')")
            .bind(id).bind(format!("drain-{id}")).bind(cicd::auth::hash_token(secret)).execute(pool).await.unwrap();
    }
    let a = work(pool, Some(runner)).await;
    let b = work(pool, Some(runner)).await;
    let app = cicd::api::app_with_auth_secret(Some(pool.clone()), None);
    assert!(
        !cicd::namespace::confirmed_group_drain_on(pool, a.group)
            .await
            .unwrap()
    );
    // An expired lease and reconciled terminal status are not a completion receipt.
    sqlx::query("UPDATE job_leases SET lease_expires_at=now()-interval '1 hour',lease_status='expired',completed_at=now(),terminal_status='failed' WHERE id=$1")
        .bind(a.lease).execute(pool).await.unwrap();
    sqlx::query("UPDATE jobs SET status='failed',finished_at=now() WHERE id=$1")
        .bind(a.job)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("UPDATE execution_attempts SET status='failed',finished_at=now() WHERE id=$1")
        .bind(a.attempt)
        .execute(pool)
        .await
        .unwrap();
    assert!(
        !cicd::namespace::confirmed_group_drain_on(pool, a.group)
            .await
            .unwrap(),
        "time and status cannot acknowledge process termination"
    );
    assert!(
        !cicd::namespace::confirmed_group_drain_on(pool, b.group)
            .await
            .unwrap()
    );
    assert_eq!(
        app.clone()
            .oneshot(completion(&b, "drain-foreign-fixture"))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert!(
        !cicd::namespace::confirmed_group_drain_on(pool, b.group)
            .await
            .unwrap()
    );
    assert_eq!(
        app.clone()
            .oneshot(completion(&a, "drain-foreign-fixture"))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND,
        "foreign runner cannot distinguish expired and live leases"
    );
    let response = app
        .clone()
        .oneshot(completion(&b, "drain-owner-fixture"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        cicd::namespace::confirmed_group_drain_on(pool, b.group)
            .await
            .unwrap(),
        "real accepted runner receipt drains its own group"
    );
    assert!(
        !cicd::namespace::confirmed_group_drain_on(pool, a.group)
            .await
            .unwrap(),
        "another group receipt does not mask unknown termination"
    );
    let marker: bool =
        sqlx::query_scalar("SELECT completion_received_at IS NOT NULL FROM job_leases WHERE id=$1")
            .bind(b.lease)
            .fetch_one(pool)
            .await
            .unwrap();
    assert!(marker);
    // Earlier attempts are independent: a later accepted success cannot bless
    // a preceding execution whose owner never confirmed completion.
    let older = Uuid::new_v4();
    sqlx::query("INSERT INTO execution_attempts(id,job_id,attempt_no,status,trigger,started_at,finished_at) VALUES($1,$2,2,'failed','retry',now()-interval '2 hours',now()-interval '1 hour')")
        .bind(older).bind(b.job).execute(pool).await.unwrap();
    assert!(
        !cicd::namespace::confirmed_group_drain_on(pool, b.group)
            .await
            .unwrap(),
        "one missing attempt receipt still blocks"
    );
    verify_embedded_completion(pool).await;
    let empty = Uuid::new_v4();
    sqlx::query("INSERT INTO git_groups(id,slug,name) VALUES($1,$2,'Empty knowledge space')")
        .bind(empty)
        .bind(format!("empty-{}", empty.simple()))
        .execute(pool)
        .await
        .unwrap();
    assert!(
        cicd::namespace::confirmed_group_drain_on(pool, empty)
            .await
            .unwrap()
    );
}

async fn verify_embedded_completion(pool: &PgPool) {
    let w = work(pool, None).await;
    let root = std::env::temp_dir().join(format!("forge-drain-native-{}", w.pipeline));
    let source = root.join("source");
    let bare = root.join("bare");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&bare).unwrap();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(&source)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "owned Git fixture failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "--initial-branch=main"]);
    git(&["config", "user.name", "Drain fixture"]);
    git(&["config", "user.email", "drain@example.invalid"]);
    std::fs::write(source.join("tracked.txt"), "owned fixture").unwrap();
    git(&["add", "tracked.txt"]);
    git(&["commit", "-m", "fixture"]);
    let owned_bare = bare.join(format!("{}.git", w.storage));
    git(&[
        "clone",
        "--bare",
        source.to_str().unwrap(),
        owned_bare.to_str().unwrap(),
    ]);
    let mut config = cicd::runner::RuntimeRunnerConfig::from_config(
        &cicd::config::RuntimeConfig::test_default(),
    );
    config.mode = cicd::config::RunnerMode::HostShell;
    config.git_root = bare;
    config.artifacts.root = root.join("artifacts");
    config.keep_workspace = false;
    unsafe {
        std::env::set_var(
            "CICD_NAMESPACE__PUBLIC_GIT_ORIGIN",
            "https://fixture.invalid",
        );
    }
    sqlx::query("UPDATE jobs SET command='test -f tracked.txt && printf receipt-ready', timeout_seconds=5 WHERE id=$1")
        .bind(w.job).execute(pool).await.unwrap();
    cicd::runner::run_job_with_config(
        pool.clone(),
        w.job,
        cicd::runner::RunningJobs::default(),
        config,
    )
    .await;
    let row:(String,bool)=sqlx::query_as("SELECT j.status,l.completion_received_at IS NOT NULL FROM jobs j JOIN job_leases l ON l.job_id=j.id WHERE j.id=$1 AND l.runner_id IS NULL")
        .bind(w.job).fetch_one(pool).await.unwrap();
    assert_eq!(
        row,
        ("success".into(), true),
        "ordinary embedded CI records its real terminal receipt"
    );
    assert!(
        cicd::namespace::confirmed_group_drain_on(pool, w.group)
            .await
            .unwrap()
    );
    let logged: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM job_logs WHERE job_id=$1 AND message LIKE '%receipt-ready%')",
    )
    .bind(w.job)
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(logged, "actual shell command ran in the owned checkout");
    // All paths are newly allocated fixtures within the QA container's task TMPDIR.
    assert!(root.starts_with(std::env::temp_dir()));
    std::fs::remove_dir_all(root).unwrap();
}
