//! Physical PostgreSQL restart while a real, orphaned Git exchange owns custody.
#[cfg(unix)]
use super::*;

#[cfg(unix)]
struct RestartGitGate(PathBuf);

#[cfg(unix)]
impl RestartGitGate {
    fn child(&self) -> Option<i32> {
        use std::os::unix::ffi::OsStrExt;
        let pid = std::fs::read_to_string(self.0.join("child-pid"))
            .ok()?
            .trim()
            .parse::<i32>()
            .ok()
            .filter(|pid| *pid > 1)?;
        let script = self.0.join("git");
        let command = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
        command
            .split(|byte| *byte == 0)
            .any(|argument| argument == script.as_os_str().as_bytes())
            .then_some(pid)
    }

    fn count(&self, name: &str) -> usize {
        std::fs::read_to_string(self.0.join(name))
            .unwrap_or_default()
            .lines()
            .count()
    }
}

#[cfg(unix)]
impl Drop for RestartGitGate {
    fn drop(&mut self) {
        let _ = std::fs::write(self.0.join("abort-before-effect"), b"abort own old writer");
        let _ = std::fs::write(
            self.0.join("release"),
            b"release owned orphan on every exit",
        );
        if let Some(pid) = self.child() {
            // SAFETY: this group leader still names the unique owned Git shim.
            unsafe { libc::kill(-pid, libc::SIGKILL) };
        }
    }
}

#[cfg(unix)]
async fn prove_restart_child_custody(f: &Live, gate: &RestartGitGate) -> i32 {
    use cicd_infra::sync_git::MutationGuard;
    let pid = gate
        .child()
        .expect("the real owned orphan Git exchange is alive");
    assert!(
        MutationGuard::try_acquire(&f.root, f.repository)
            .unwrap()
            .is_none()
    );
    // Default worker concurrency is two. The orphan owns one slot, so exactly
    // one unrelated reservation can coexist with it, even with PostgreSQL dead.
    let mut spare = MutationGuard::acquire(&f.root, Uuid::new_v4())
        .await
        .unwrap();
    assert!(spare.try_reserve_sync_capacity(&f.root, 2).unwrap());
    let mut blocked = MutationGuard::acquire(&f.root, Uuid::new_v4())
        .await
        .unwrap();
    assert!(!blocked.try_reserve_sync_capacity(&f.root, 2).unwrap());
    pid
}

#[cfg(unix)]
async fn restart_database_identity(
    pool: &sqlx::PgPool,
) -> (chrono::DateTime<chrono::Utc>, String, String) {
    tokio::time::timeout(std::time::Duration::from_secs(60), async {
        loop {
            match sqlx::query_as(
                "SELECT pg_postmaster_start_time(),current_database(),current_schema()",
            )
            .fetch_one(pool)
            .await
            {
                Ok(identity) => break identity,
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(200)).await,
            }
        }
    })
    .await
    .expect("owned PostgreSQL restarts with the original database and schema")
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "Requires owned Compose PostgreSQL coordinator and owned live provider; physically restarts PG in before/accepted Git phases"]
async fn full_postgres_restart_with_live_git_before_effect_and_after_accepted_push() {
    let marker = PathBuf::from(
        std::env::var("FORGE_SYNC_LIVE_RESTART_MARKER")
            .expect("explicit owned PostgreSQL restart coordinator"),
    );
    assert!(marker.starts_with("/evidence"));
    assert!(marker.file_name().is_some());
    for phase in ["before", "accepted"] {
        postgres_live_restart_phase(&marker, phase).await;
    }
}

#[cfg(unix)]
async fn postgres_live_restart_phase(marker: &Path, phase: &str) {
    use std::{os::unix::fs::PermissionsExt, time::Duration};
    let before = phase == "before";
    let ready = PathBuf::from(format!("{}.{phase}.ready.json", marker.display()));
    let temporary = PathBuf::from(format!("{}.{phase}.ready.tmp", marker.display()));
    let resumed = PathBuf::from(format!("{}.{phase}.resumed.json", marker.display()));
    assert!(!ready.exists() && !temporary.exists() && !resumed.exists());
    let mut f = Live::new().await;
    // Import existing refs with auto-CI disabled, then isolate this phase's two
    // new refs. No existing provider ref is rewritten or removed.
    f.settings(Direction::Pull, false).await;
    assert_eq!(f.manual().await.state, "success");
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM repository_sync_refs r JOIN repository_sync_runs run ON run.id=r.run_id WHERE run.repository_id=$1 AND r.direction='pull' AND r.state='applied' AND NOT r.event_delivered")
                .bind(f.repository).fetch_one(&f.pool).await.unwrap();
            if pending == 0 { break; }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }).await.unwrap();
    f.stop_tasks().await;
    f.settings(Direction::Both, false).await;
    let sha = f
        .commit(None, "QA PostgreSQL restart during real Git exchange")
        .await;
    let incoming = f.head("00-pg-restart-incoming");
    let outgoing = f.head("zz-pg-restart-outgoing");
    f.external(&incoming, None, &sha).await;
    f.local(&outgoing, &sha).await;
    sqlx::query(
        "INSERT INTO repository_push_configs(repository_id,configuration_id) VALUES($1,$2)",
    )
    .bind(f.repository)
    .bind(f.project)
    .execute(&f.pool)
    .await
    .unwrap();
    let control = f.root.join("postgres-live-restart");
    std::fs::create_dir(&control).unwrap();
    let gate = RestartGitGate(control.clone());
    let script = format!(
        r#"#!/bin/sh
control='{}'
case " $* " in
  *" push "*" https://"*"{}"*)
    echo attempt >> "$control/push-attempts"
    if test -f "$control/arm"; then
      rm "$control/arm"
      echo $$ > "$control/child-pid"
      if test -f "$control/before"; then
        touch "$control/reached"
        while test ! -f "$control/release"; do /bin/sleep 0.1; done
        if test -f "$control/abort-before-effect"; then exit 75; fi
      fi
      echo effect >> "$control/provider-effects"
      /usr/bin/git "$@"
      result=$?
      echo "$result" > "$control/accepted"
      touch "$control/reached"
      while test ! -f "$control/release"; do /bin/sleep 0.1; done
      exit "$result"
    fi
    echo effect >> "$control/provider-effects"
    exec /usr/bin/git "$@" ;;
esac
case " $* " in
  *" push "*"{}"*) echo incoming >> "$control/incoming-effects" ;;
esac
exec /usr/bin/git "$@"
"#,
        control.display(),
        outgoing,
        incoming
    );
    std::fs::write(control.join("git"), script).unwrap();
    std::fs::set_permissions(control.join("git"), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(
        control.join("arm"),
        b"gate only the first owned provider push",
    )
    .unwrap();
    if before {
        std::fs::write(control.join("before"), b"hold before actual effect").unwrap();
    }
    let mut first = f.process(&control).await;
    let first_pid = first.id().unwrap();
    let (status, body) = f.request("POST", "/runs", None, Some(Uuid::new_v4())).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let run: SyncRun = serde_json::from_value(body).unwrap();
    tokio::time::timeout(Duration::from_secs(180), async {
        while !control.join("reached").exists() {
            assert!(first.try_wait().unwrap().is_none());
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("actual owned Git exchange reaches the specified effect boundary");
    f.ref_result(run.id, &incoming, "applied").await;
    f.ref_result(run.id, &outgoing, "planned").await;
    f.same(&incoming, &sha).await;
    if before {
        assert!(
            !f.remote
                .as_ref()
                .unwrap()
                .refs(true)
                .await
                .unwrap()
                .contains_key(&outgoing)
        );
        assert_eq!(gate.count("provider-effects"), 0);
    } else {
        assert_eq!(
            std::fs::read_to_string(control.join("accepted"))
                .unwrap()
                .trim(),
            "0"
        );
        f.same(&outgoing, &sha).await;
        assert_eq!(gate.count("provider-effects"), 1);
    }
    let (generation, owner): (i64, Uuid) =
        sqlx::query_as("SELECT generation,owner_id FROM repository_sync_runs WHERE id=$1")
            .bind(run.id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(generation, 1);
    // Kill only our worker. Its already-started real Git child remains alive
    // while the coordinator kills/restarts PostgreSQL; no cancellation watcher
    // can end that child on behalf of the dead supervisor.
    first.kill().await.unwrap();
    first.wait().await.unwrap();
    let git_pid = prove_restart_child_custody(&f, &gate).await;
    let identity_before = restart_database_identity(&f.pool).await;
    assert!(identity_before.1.starts_with("forge_test_"));
    let mut old_session = f.pool.acquire().await.unwrap().detach();
    let old_backend_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut old_session)
        .await
        .unwrap();
    let ci_before: i64 = sqlx::query_scalar("SELECT count(*) FROM pipelines WHERE project_id=$1")
        .bind(f.project)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert!(ci_before <= 1);
    let proof = serde_json::json!({
        "purpose":"forge-owned-postgres-live-git-restart","phase":phase,"provider":f.settings.provider,
        "repository_id":f.repository,"run_id":run.id,"generation":generation,"owner_id":owner,
        "dead_worker_pid":first_pid,"live_git_child_pid":git_pid,"database_backend_pid":old_backend_pid,
        "database_name":identity_before.1,"schema":identity_before.2,"postmaster_started_at":identity_before.0,
        "incoming_ref":incoming,"outgoing_ref":outgoing,"old_outgoing_sha":null,"expected_sha":sha,
        "incoming_state":"applied","outgoing_state":"planned","provider_effects":gate.count("provider-effects"),
        "pipeline_count_before":ci_before,"repository_guard_held":true,"shared_capacity_held":true,
        "source_test_module":"repository_sync::live::postgres_live_restart"
    });
    std::fs::write(&temporary, serde_json::to_vec_pretty(&proof).unwrap()).unwrap();
    std::fs::rename(&temporary, &ready).unwrap();
    println!("LIVE_PG_RESTART_READY {proof}");
    tokio::time::timeout(Duration::from_secs(240), async {
        while !resumed.exists() {
            assert_eq!(
                gate.child(),
                Some(git_pid),
                "Git orphan stays alive throughout real PostgreSQL restart"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("owned Compose coordinator must SIGKILL and restart only its PostgreSQL service");
    let acknowledgement: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&resumed).unwrap()).unwrap();
    assert_eq!(acknowledgement["phase"], phase);
    assert_eq!(acknowledgement["postgres_sigkill"], true);
    assert_eq!(acknowledgement["restart_healthy"], true);
    let identity_after = restart_database_identity(&f.pool).await;
    assert!(
        identity_after.0 > identity_before.0,
        "a genuinely new PostgreSQL postmaster is required"
    );
    assert_eq!(identity_after.1, identity_before.1);
    assert_eq!(identity_after.2, identity_before.2);
    assert!(
        matches!(
            tokio::time::timeout(
                Duration::from_secs(5),
                sqlx::query("SELECT 1").execute(&mut old_session)
            )
            .await,
            Ok(Err(_))
        ),
        "the pre-crash PostgreSQL session must really be dead"
    );
    drop(old_session);
    assert_eq!(prove_restart_child_custody(&f, &gate).await, git_pid);
    let persisted = f.store.run(f.repository, run.id).await.unwrap();
    assert!(persisted.finished_at.is_none());
    assert_eq!(persisted.state, "running");
    f.ref_result(run.id, &incoming, "applied").await;
    f.ref_result(run.id, &outgoing, "planned").await;
    sqlx::query(
        "UPDATE repository_sync_runs SET lease_until=now()-interval '1 second' WHERE id=$1",
    )
    .bind(run.id)
    .execute(&f.pool)
    .await
    .unwrap();
    let mut second = f.process(&control).await;
    let second_pid = second.id().unwrap();
    tokio::time::sleep(Duration::from_secs(5)).await;
    let still_generation: i64 =
        sqlx::query_scalar("SELECT generation FROM repository_sync_runs WHERE id=$1")
            .bind(run.id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(
        still_generation, 1,
        "new worker cannot pass the real orphan's storage guard after DB restart"
    );
    assert_eq!(gate.count("push-attempts"), 1);
    assert_eq!(prove_restart_child_custody(&f, &gate).await, git_pid);
    if before {
        std::fs::write(
            control.join("abort-before-effect"),
            b"old owner must never start a new write",
        )
        .unwrap();
    }
    std::fs::write(
        control.join("release"),
        b"release orphan only after restart and custody proof",
    )
    .unwrap();
    assert_eq!(f.completed(run.id).await.state, "success");
    let (new_generation, new_owner): (i64, Uuid) =
        sqlx::query_as("SELECT generation,owner_id FROM repository_sync_runs WHERE id=$1")
            .bind(run.id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(new_generation, 2);
    assert_ne!(new_owner, owner);
    f.ref_result(run.id, &incoming, "applied").await;
    f.ref_result(run.id, &outgoing, "applied").await;
    f.same(&incoming, &sha).await;
    f.same(&outgoing, &sha).await;
    assert_eq!(gate.count("push-attempts"), if before { 2 } else { 1 });
    assert_eq!(gate.count("provider-effects"), 1);
    assert_eq!(gate.count("incoming-effects"), 1);
    tokio::time::timeout(Duration::from_secs(30), async {
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
            assert!(second.try_wait().unwrap().is_none());
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("durable incoming event is delivered after PostgreSQL restart");
    let pipelines: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id,commit_sha FROM pipelines WHERE project_id=$1 ORDER BY id")
            .bind(f.project)
            .fetch_all(&f.pool)
            .await
            .unwrap();
    assert_eq!(pipelines.len(), 1);
    assert_eq!(pipelines[0].1, sha);
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
        "restart and shared ingress replay retain one exact-SHA pipeline UUID"
    );
    assert_eq!(f.store.runs(f.repository, 100, 0).await.unwrap().1, 2);
    let mut actual = f.remote.as_ref().unwrap().refs(true).await.unwrap();
    assert_eq!(actual.remove(&incoming).as_deref(), Some(sha.as_str()));
    assert_eq!(actual.remove(&outgoing).as_deref(), Some(sha.as_str()));
    assert_eq!(
        actual, f.initial,
        "unrelated provider refs remain byte-identical"
    );
    second.kill().await.unwrap();
    second.wait().await.unwrap();
    println!(
        "LIVE_POSTGRES_LIVE_GIT_RESTART_OK {}",
        serde_json::json!({"provider":f.settings.provider,"prefix":f.prefix,"phase":phase,"run_id":run.id,"generation":new_generation,"old_owner":owner,"new_owner":new_owner,"old_worker_pid":first_pid,"new_worker_pid":second_pid,"orphan_git_pid":git_pid,"postmaster_before":identity_before.0,"postmaster_after":identity_after.0,"provider_effects":1,"incoming_effects":1,"pipeline_id":pipelines[0].0,"sha":sha})
    );
    f.pool.close().await;
}
