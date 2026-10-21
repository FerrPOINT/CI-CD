//! Physical worker death after committed ref receipts, before run finalization.
#[cfg(unix)]
use super::*;

#[cfg(unix)]
#[tokio::test]
#[ignore = "Requires explicitly owned live GitHub/GitLab repository; kills only its own worker at finalization"]
async fn crash_after_final_receipt_recovers_terminal_without_git_or_ci_replay() {
    use std::os::unix::fs::PermissionsExt;

    let mut f = Live::new().await;
    // Import the existing provider baseline while CI remains disabled. The fault
    // run then has exactly two effects, both in this fixture's unique namespace.
    f.settings(Direction::Pull, false).await;
    assert_eq!(f.manual().await.state, "success");
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
    f.settings(Direction::Both, false).await;
    let incoming = f.head("final-receipt-incoming");
    let outgoing = f.head("final-receipt-outgoing");
    let incoming_sha = f.commit(None, "QA final receipt incoming").await;
    let outgoing_sha = f.commit(None, "QA final receipt outgoing").await;
    f.external(&incoming, None, &incoming_sha).await;
    f.local(&outgoing, &outgoing_sha).await;
    sqlx::query(
        "INSERT INTO repository_push_configs(repository_id,configuration_id) VALUES($1,$2)",
    )
    .bind(f.repository)
    .bind(f.project)
    .execute(&f.pool)
    .await
    .unwrap();

    let controls = f.root.join("final-receipt-control");
    std::fs::create_dir(&controls).unwrap();
    let shim = controls.join("git");
    // Record classifications only; never write Git argv or credentials.
    std::fs::write(
        &shim,
        format!(
            r#"#!/bin/sh
case " $* " in
  *" push "*)
    case " $* " in
      *" https://"*) echo remote >> '{}/push-count' ;;
      *) echo local >> '{}/push-count' ;;
    esac ;;
esac
exec /usr/bin/git "$@"
"#,
            controls.display(),
            controls.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o700)).unwrap();

    let (status, body) = f.request("POST", "/runs", None, Some(Uuid::new_v4())).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let run: SyncRun = serde_json::from_value(body).unwrap();
    let function = format!("qa_final_receipt_{}", run.id.simple());
    let lock_key = i64::from_be_bytes(*run.id.as_bytes().first_chunk::<8>().unwrap());
    // A disposable database trigger gates only this run's real final UPDATE.
    // Ref receipts use a different table and have already committed when this
    // trigger waits. No production hook or reconstructed receipt is involved.
    sqlx::query(&format!("CREATE FUNCTION {function}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.id='{}'::uuid AND NEW.finished_at IS NOT NULL THEN PERFORM pg_advisory_xact_lock({lock_key}::bigint); END IF; RETURN NEW; END; $$", run.id))
        .execute(&f.pool).await.unwrap();
    sqlx::query(&format!("CREATE TRIGGER {function} BEFORE UPDATE ON repository_sync_runs FOR EACH ROW EXECUTE FUNCTION {function}()"))
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
            assert!(first.try_wait().unwrap().is_none(), "worker exited before final UPDATE");
            let blocked: Option<i32> = sqlx::query_scalar("SELECT a.pid FROM pg_stat_activity a WHERE a.datname=current_database() AND a.wait_event_type='Lock' AND a.wait_event='advisory' AND $1=ANY(pg_blocking_pids(a.pid)) AND a.query LIKE 'UPDATE repository_sync_runs SET state=$4,message=$5,finished_at=%' LIMIT 1")
                .bind(barrier_pid).fetch_optional(&f.pool).await.unwrap();
            if let Some(pid) = blocked {
                break pid;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }).await.expect("real final UPDATE reaches the advisory barrier");
    let before = f.store.run(f.repository, run.id).await.unwrap();
    assert_eq!(before.state, "running");
    assert!(before.finished_at.is_none());
    let receipts = f.store.refs(run.id, 1000, 0).await.unwrap();
    assert_eq!(receipts.0.len() as i64, receipts.1);
    assert_eq!(
        receipts.0.iter().filter(|r| r.state == "applied").count(),
        2
    );
    assert!(
        receipts
            .0
            .iter()
            .all(|r| matches!(r.state.as_str(), "applied" | "unchanged"))
    );
    f.ref_result(run.id, &incoming, "applied").await;
    f.ref_result(run.id, &outgoing, "applied").await;
    f.same(&incoming, &incoming_sha).await;
    f.same(&outgoing, &outgoing_sha).await;
    let pushes_before = std::fs::read_to_string(controls.join("push-count")).unwrap();
    assert_eq!(pushes_before.lines().filter(|s| *s == "local").count(), 1);
    assert_eq!(pushes_before.lines().filter(|s| *s == "remote").count(), 1);

    // SIGKILL happens only after another connection proved both the blocked
    // terminal statement and all committed terminal ref receipts.
    first.kill().await.unwrap();
    first.wait().await.unwrap();
    // A dead client does not guarantee PostgreSQL noticed socket closure while
    // waiting inside an advisory lock. End only the proven owned backend before
    // releasing the gate, or it could commit finalization after worker death.
    let owned_waiter: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity a WHERE a.pid=$1 AND a.datname=current_database() AND a.wait_event_type='Lock' AND a.wait_event='advisory' AND $2=ANY(pg_blocking_pids(a.pid)) AND a.query LIKE 'UPDATE repository_sync_runs SET state=$4,message=$5,finished_at=%')")
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
    assert!(
        f.store
            .run(f.repository, run.id)
            .await
            .unwrap()
            .finished_at
            .is_none()
    );
    barrier.rollback().await.unwrap();
    sqlx::query(&format!("DROP TRIGGER {function} ON repository_sync_runs"))
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query(&format!("DROP FUNCTION {function}()"))
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE repository_sync_runs SET lease_until=now()-interval '1 second' WHERE id=$1",
    )
    .bind(run.id)
    .execute(&f.pool)
    .await
    .unwrap();
    let mut second = f.process(&controls).await;
    assert_eq!(f.completed(run.id).await.state, "success");
    let generation: i64 =
        sqlx::query_scalar("SELECT generation FROM repository_sync_runs WHERE id=$1")
            .bind(run.id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(
        generation, 2,
        "a new process reclaims and finalizes the same run"
    );
    let recovered = f.store.refs(run.id, 1000, 0).await.unwrap();
    let intent_receipts = |rows: Vec<cicd_domain::repository_sync::RefResult>| {
        rows.into_iter()
            .map(|r| {
                (
                    r.ref_name,
                    r.direction,
                    r.old_sha,
                    r.new_sha,
                    r.state,
                    r.reason,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(intent_receipts(receipts.0), intent_receipts(recovered.0));
    f.same(&incoming, &incoming_sha).await;
    f.same(&outgoing, &outgoing_sha).await;
    assert_eq!(
        std::fs::read_to_string(controls.join("push-count")).unwrap(),
        pushes_before,
        "finalization recovery must not repeat either Git effect"
    );

    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            let rows = f.store.refs(run.id, 1000, 0).await.unwrap().0;
            if rows
                .iter()
                .find(|r| r.ref_name == incoming)
                .unwrap()
                .event_delivered
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    let pipelines: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id,commit_sha FROM pipelines WHERE project_id=$1 ORDER BY id")
            .bind(f.project)
            .fetch_all(&f.pool)
            .await
            .unwrap();
    assert_eq!(pipelines.len(), 1);
    assert_eq!(pipelines[0].1, incoming_sha);
    // Replay the same ordinary push event through the production ingress. It
    // must return the pipeline already created from the durable sync receipt.
    for _ in 0..2 {
        let response = f.api.clone().oneshot(Request::post("/api/v1/internal/git-push")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::json!({"repository_id":f.repository,"repository":f.local.file_stem().unwrap().to_str().unwrap(),"ref_name":incoming,"new_rev":incoming_sha}).to_string())).unwrap())
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
        "recovery and repeated ingress preserve one exact-SHA pipeline"
    );
    assert_eq!(f.store.runs(f.repository, 100, 0).await.unwrap().1, 2);
    second.kill().await.unwrap();
    second.wait().await.unwrap();
    println!(
        "LIVE_FINAL_RECEIPT_CRASH_OK provider={:?} prefix={} run={} blocked_backend={} generation={generation}",
        f.settings.provider, f.prefix, run.id, blocked_pid
    );
    f.pool.close().await;
}
