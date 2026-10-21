//! Physical worker death after pipeline commit, before the sync event receipt.
#[cfg(unix)]
use super::*;

#[cfg(unix)]
#[tokio::test]
#[ignore = "Requires explicitly owned live GitHub/GitLab repository; kills only its own worker after pipeline commit"]
async fn crash_after_pipeline_commit_replays_event_without_duplicate_pipeline_or_git() {
    use std::os::unix::fs::PermissionsExt;

    let mut f = Live::new().await;
    f.settings(Direction::Pull, false).await;
    assert_eq!(f.manual().await.state, "success");
    // Baseline imports belong to existing provider refs and must finish their
    // disabled-CI decisions before enabling CI for this fixture's new event.
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM repository_sync_refs r JOIN repository_sync_runs run ON run.id=r.run_id WHERE run.repository_id=$1 AND r.direction='pull' AND r.state='applied' AND NOT r.event_delivered")
                .bind(f.repository).fetch_one(&f.pool).await.unwrap();
            if pending == 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }).await.unwrap();
    f.stop_tasks().await;
    let incoming = f.head("pipeline-commit-crash");
    let sha = f
        .commit(None, "QA committed pipeline before event receipt")
        .await;
    f.external(&incoming, None, &sha).await;
    sqlx::query(
        "INSERT INTO repository_push_configs(repository_id,configuration_id) VALUES($1,$2)",
    )
    .bind(f.repository)
    .bind(f.project)
    .execute(&f.pool)
    .await
    .unwrap();

    let controls = f.root.join("ci-delivery-control");
    std::fs::create_dir(&controls).unwrap();
    let shim = controls.join("git");
    std::fs::write(
        &shim,
        format!(
            r#"#!/bin/sh
case " $* " in
  *" push "*) echo effect >> '{}/push-count' ;;
esac
exec /usr/bin/git "$@"
"#,
            controls.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o700)).unwrap();
    let (status, body) = f.request("POST", "/runs", None, Some(Uuid::new_v4())).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let run: SyncRun = serde_json::from_value(body).unwrap();
    let function = format!("qa_ci_delivery_{}", run.id.simple());
    let lock_key = i64::from_be_bytes(*run.id.as_bytes().first_chunk::<8>().unwrap());
    // The production delivery code commits its shared pipeline receipt before
    // this separate event_delivered UPDATE. Gate that real UPDATE, not a fake
    // journal record, and verify committed pipeline identity from another pool.
    sqlx::query(&format!("CREATE FUNCTION {function}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.run_id='{}'::uuid AND NEW.ref_name='{incoming}' AND NEW.event_delivered AND NOT OLD.event_delivered THEN PERFORM pg_advisory_xact_lock({lock_key}::bigint); END IF; RETURN NEW; END; $$", run.id))
        .execute(&f.pool).await.unwrap();
    sqlx::query(&format!("CREATE TRIGGER {function} BEFORE UPDATE ON repository_sync_refs FOR EACH ROW EXECUTE FUNCTION {function}()"))
        .execute(&f.pool).await.unwrap();
    let mut barrier = f.pool.begin().await.unwrap();
    let barrier_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *barrier)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(lock_key)
        .execute(&mut *barrier)
        .await
        .unwrap();
    let mut first = f.process(&controls).await;
    let blocked_pid = tokio::time::timeout(std::time::Duration::from_secs(180), async {
        loop {
            assert!(first.try_wait().unwrap().is_none(), "worker exited before event delivery barrier");
            let blocked: Option<i32> = sqlx::query_scalar("SELECT a.pid FROM pg_stat_activity a WHERE a.datname=current_database() AND a.wait_event_type='Lock' AND a.wait_event='advisory' AND $1=ANY(pg_blocking_pids(a.pid)) AND a.query LIKE 'UPDATE repository_sync_refs SET event_delivered=true WHERE run_id=%' LIMIT 1")
                .bind(barrier_pid).fetch_optional(&f.pool).await.unwrap();
            if let Some(pid) = blocked {
                break pid;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }).await.expect("real event_delivered UPDATE waits after pipeline creation");
    f.ref_result(run.id, &incoming, "applied").await;
    f.same(&incoming, &sha).await;
    let delivered: bool = sqlx::query_scalar(
        "SELECT event_delivered FROM repository_sync_refs WHERE run_id=$1 AND ref_name=$2",
    )
    .bind(run.id)
    .bind(&incoming)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert!(
        !delivered,
        "event receipt is not committed at the physical fault boundary"
    );
    let pipelines: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id,commit_sha FROM pipelines WHERE project_id=$1 ORDER BY id")
            .bind(f.project)
            .fetch_all(&f.pool)
            .await
            .unwrap();
    assert_eq!(
        pipelines.len(),
        1,
        "pipeline commit is independently visible before SIGKILL"
    );
    assert_eq!(pipelines[0].1, sha);
    let pushes = std::fs::read_to_string(controls.join("push-count")).unwrap();
    assert_eq!(
        pushes.lines().count(),
        1,
        "exactly one incoming Git publication"
    );
    let before = f.completed(run.id).await;
    assert_eq!(before.state, "success");

    first.kill().await.unwrap();
    first.wait().await.unwrap();
    // Keep the gate held until the exact owned database backend has exited.
    // Otherwise PostgreSQL could commit after SIGKILL before noticing the dead
    // client's socket, invalidating the intended lost-receipt fault boundary.
    let owned_waiter: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity a WHERE a.pid=$1 AND a.datname=current_database() AND a.wait_event_type='Lock' AND a.wait_event='advisory' AND $2=ANY(pg_blocking_pids(a.pid)) AND a.query LIKE 'UPDATE repository_sync_refs SET event_delivered=true WHERE run_id=%')")
        .bind(blocked_pid).bind(barrier_pid).fetch_one(&f.pool).await.unwrap();
    if owned_waiter {
        let _: bool = sqlx::query_scalar("SELECT pg_terminate_backend($1)")
            .bind(blocked_pid)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    } else {
        let alive: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE pid=$1)")
                .bind(blocked_pid)
                .fetch_one(&f.pool)
                .await
                .unwrap();
        assert!(
            !alive,
            "never terminate a backend without our exact query and lock ownership"
        );
    }
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let alive: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE pid=$1)")
                    .bind(blocked_pid)
                    .fetch_one(&f.pool)
                    .await
                    .unwrap();
            if !alive {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("owned blocked backend exits before gate release");
    barrier.rollback().await.unwrap();
    sqlx::query(&format!("DROP TRIGGER {function} ON repository_sync_refs"))
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query(&format!("DROP FUNCTION {function}()"))
        .execute(&f.pool)
        .await
        .unwrap();
    let delivered: bool = sqlx::query_scalar(
        "SELECT event_delivered FROM repository_sync_refs WHERE run_id=$1 AND ref_name=$2",
    )
    .bind(run.id)
    .bind(&incoming)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert!(
        !delivered,
        "killed worker cannot commit the blocked event receipt"
    );
    let mut second = f.process(&controls).await;
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            let delivered: bool = sqlx::query_scalar(
                "SELECT event_delivered FROM repository_sync_refs WHERE run_id=$1 AND ref_name=$2",
            )
            .bind(run.id)
            .bind(&incoming)
            .fetch_one(&f.pool)
            .await
            .unwrap();
            if delivered {
                break;
            }
            assert!(
                second.try_wait().unwrap().is_none(),
                "replacement worker must deliver the durable event"
            );
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("new process redelivers the existing event");
    let after: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id,commit_sha FROM pipelines WHERE project_id=$1 ORDER BY id")
            .bind(f.project)
            .fetch_all(&f.pool)
            .await
            .unwrap();
    assert_eq!(
        after, pipelines,
        "shared receipt preserves original pipeline UUID and exact SHA"
    );
    assert_eq!(
        f.store.run(f.repository, run.id).await.unwrap().state,
        "success"
    );
    assert_eq!(f.store.runs(f.repository, 100, 0).await.unwrap().1, 2);
    f.same(&incoming, &sha).await;
    assert_eq!(
        std::fs::read_to_string(controls.join("push-count")).unwrap(),
        pushes,
        "CI event recovery never repeats the completed Git publication"
    );
    for _ in 0..2 {
        let response = f.api.clone().oneshot(Request::post("/api/v1/internal/git-push")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::json!({"repository_id":f.repository,"repository":f.local.file_stem().unwrap().to_str().unwrap(),"ref_name":incoming,"new_rev":sha}).to_string())).unwrap())
            .await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    let replayed: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id,commit_sha FROM pipelines WHERE project_id=$1 ORDER BY id")
            .bind(f.project)
            .fetch_all(&f.pool)
            .await
            .unwrap();
    assert_eq!(
        replayed, pipelines,
        "repeated ordinary push ingress reuses the same pipeline"
    );
    second.kill().await.unwrap();
    second.wait().await.unwrap();
    println!(
        "LIVE_CI_DELIVERY_CRASH_OK provider={:?} prefix={} run={} pipeline={} blocked_backend={}",
        f.settings.provider, f.prefix, run.id, pipelines[0].0, blocked_pid
    );
    f.pool.close().await;
}
