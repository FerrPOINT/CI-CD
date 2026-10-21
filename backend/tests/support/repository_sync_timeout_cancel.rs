//! Native 300-second Git deadline followed by real disable/reconciliation.
use super::*;
use std::{os::unix::fs::PermissionsExt, time::Duration};

struct ReleaseOwnedChildren(PathBuf);
impl Drop for ReleaseOwnedChildren {
    fn drop(&mut self) {
        use std::os::unix::ffi::OsStrExt;
        let _ = std::fs::write(self.0.join("abort"), b"abort own QA continuation");
        let _ = std::fs::write(self.0.join("release"), b"release read-only observation");
        for name in ["push-pid", "observation-pid"] {
            let Some(pid) = std::fs::read_to_string(self.0.join(name))
                .ok()
                .and_then(|s| s.trim().parse::<i32>().ok())
                .filter(|pid| *pid > 1)
            else {
                continue;
            };
            let command = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
            if command
                .split(|byte| *byte == 0)
                .any(|arg| arg == self.0.join("git").as_os_str().as_bytes())
            {
                // SAFETY: GitSession created this process group; argv still
                // names the unique owned fixture shim before targeting it.
                unsafe { libc::kill(-pid, libc::SIGKILL) };
            }
        }
    }
}

async fn wait_marker(control: &Path, name: &str, child: &mut tokio::process::Child, seconds: u64) {
    tokio::time::timeout(Duration::from_secs(seconds), async {
        while !control.join(name).exists() {
            assert!(child.try_wait().unwrap().is_none());
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("owned native-timeout fixture reached its named phase");
}

#[tokio::test]
#[ignore = "Requires owned live provider; waits unchanged native 300s Git deadline before real API disable"]
async fn disable_after_native_git_timeout_reconciles_accepted_push_without_duplicate_effect() {
    let mut f = Live::new().await;
    f.stop_tasks().await;
    f.settings(Direction::Push, false).await;
    let sha = f.commit(None, "QA native timeout and disable").await;
    let name = f.head("native-timeout-disable");
    f.local(&name, &sha).await;
    let before = f.remote.as_ref().unwrap().refs(true).await.unwrap();
    let (_, saved) = f.request("GET", "", None, None).await;
    let control = f.root.join("native-timeout-disable");
    std::fs::create_dir(&control).unwrap();
    let _cleanup = ReleaseOwnedChildren(control.clone());
    let script = format!(
        r#"#!/bin/sh
control='{}'
case " $* " in
  *" push "*" https://"*)
    echo effect >> "$control/effects"
    if test -f "$control/arm"; then
      rm "$control/arm"
      echo $$ > "$control/push-pid"
      read command_started rest < /proc/uptime
      printf '%s\n' "$command_started" > "$control/push-started-uptime"
      /usr/bin/git "$@"
      result=$?
      echo "$result" > "$control/accepted"
      if test "$result" -ne 0; then exit "$result"; fi
      touch "$control/accepted-held"
      # No fixture timer or self-exit: only the native GitSession deadline
      # ends this exchange during a successful experiment.
      while test ! -f "$control/abort"; do /bin/sleep 0.1; done
      exit 75
    fi ;;
  *" ls-remote "*" https://"*)
    if test -f "$control/accepted-held"; then
      if test ! -f "$control/timeout-observed"; then
        touch "$control/timeout-observed"
        exit 128
      fi
      if test ! -f "$control/release"; then
        echo $$ > "$control/observation-pid"
        touch "$control/observation-held"
        while test ! -f "$control/release"; do /bin/sleep 0.1; done
        if test -f "$control/abort"; then exit 75; fi
      fi
    fi ;;
esac
exec /usr/bin/git "$@"
"#,
        control.display()
    );
    std::fs::write(control.join("git"), script).unwrap();
    std::fs::set_permissions(control.join("git"), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(control.join("arm"), b"hold first accepted push").unwrap();
    let mut worker = f.process(&control).await;
    let started = tokio::time::Instant::now();
    let (status, body) = f.request("POST", "/runs", None, Some(Uuid::new_v4())).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let run: SyncRun = serde_json::from_value(body).unwrap();
    wait_marker(&control, "accepted-held", &mut worker, 90).await;
    assert_eq!(
        std::fs::read_to_string(control.join("accepted"))
            .unwrap()
            .trim(),
        "0"
    );
    f.same(&name, &sha).await;
    f.ref_result(run.id, &name, "planned").await;
    assert!(!control.join("abort").exists());
    assert!(!control.join("release").exists());
    wait_marker(&control, "timeout-observed", &mut worker, 330).await;
    let unknown = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let current = f.store.run(f.repository, run.id).await.unwrap();
            if current.state == "reconciling" {
                break current;
            }
            assert!(current.finished_at.is_none());
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("native timeout with unavailable observation leaves durable reconciling");
    let timeout_elapsed = started.elapsed();
    let command_started: f64 = std::fs::read_to_string(control.join("push-started-uptime"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let observed_uptime: f64 = std::fs::read_to_string("/proc/uptime")
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let command_elapsed = observed_uptime - command_started;
    assert!(
        timeout_elapsed >= Duration::from_secs(300),
        "the real unchanged GitSession 300-second deadline must occur"
    );
    // Linux uptime has centisecond precision. Permit at most half a second
    // for dispatch between the production timer and the shim's first read;
    // queue/fetch preparation cannot contribute to this command-only oracle.
    assert!(
        command_elapsed >= 299.5,
        "the held Git command itself must reach the unchanged native deadline: {command_elapsed}s"
    );
    assert!(!unknown.cancel_requested);
    assert!(unknown.finished_at.is_none());
    println!(
        "LIVE_NATIVE_TIMEOUT_OBSERVED provider={:?} run={} native_deadline_seconds=300 total_observed_seconds={} command_observed_seconds={command_elapsed} uptime_precision_seconds=0.01 dispatch_tolerance_seconds=0.5 cancel_requested={} message={:?}",
        f.settings.provider,
        run.id,
        timeout_elapsed.as_secs_f64(),
        unknown.cancel_requested,
        unknown.message
    );
    f.ref_result(run.id, &name, "unknown").await;
    // Only a completed reconciling attempt is made eligible for observation;
    // no running generation is fenced or artificially completed by the fixture.
    sqlx::query(
        "UPDATE repository_sync_runs SET lease_until=now()-interval '1 second' WHERE id=$1 AND state='reconciling'",
    )
    .bind(run.id)
    .execute(&f.pool)
    .await
    .unwrap();
    wait_marker(&control, "observation-held", &mut worker, 20).await;
    f.settings.direction = Direction::Disabled;
    let (status, stopping) = f
        .request(
            "PUT",
            "",
            Some(serde_json::json!({"expected_version":saved["version"],"settings":f.settings})),
            Some(Uuid::new_v4()),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stopping["active_run"]["state"], "stopping");
    assert_eq!(stopping["active_run"]["cancel_requested"], true);
    assert_eq!(stopping["has_credential"], true);
    // Release only the read-only observation. The accepted push gate was never
    // released: its child was killed by the native command deadline.
    std::fs::write(
        control.join("release"),
        b"observe accepted SHA after API disable",
    )
    .unwrap();
    let completed = tokio::time::timeout(Duration::from_secs(80), async {
        loop {
            let current = f.store.run(f.repository, run.id).await.unwrap();
            if current.finished_at.is_some() {
                break current;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("disabled recovery observes the already accepted SHA without another push");
    assert!(
        matches!(completed.state.as_str(), "success" | "partial"),
        "a timed-out accepted push must not become a false canceled result"
    );
    assert!(completed.cancel_requested);
    assert_eq!(completed.connection_version, run.connection_version);
    assert_eq!(completed.settings, run.settings);
    f.ref_result(run.id, &name, "applied").await;
    f.same(&name, &sha).await;
    let mut after = f.remote.as_ref().unwrap().refs(true).await.unwrap();
    assert_eq!(after.remove(&name).as_deref(), Some(sha.as_str()));
    assert_eq!(after, before);
    assert_eq!(
        std::fs::read_to_string(control.join("effects"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    assert_eq!(f.store.runs(f.repository, 20, 0).await.unwrap().1, 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM pipelines WHERE project_id=$1")
            .bind(f.project)
            .fetch_one(&f.pool)
            .await
            .unwrap(),
        0
    );
    let (_, saved_after) = f.request("GET", "", None, None).await;
    assert_eq!(saved_after["settings"]["direction"], "disabled");
    assert_eq!(saved_after["has_credential"], true);
    // Final harness cleanup only; it does not induce the timeout or recovery.
    worker.kill().await.unwrap();
    worker.wait().await.unwrap();
    println!(
        "LIVE_NATIVE_TIMEOUT_DISABLE_OK provider={:?} prefix={} run={} native_deadline_seconds=300 total_observed_seconds={} command_observed_seconds={command_elapsed} state={} effects=1 ci=0 sha={sha}",
        f.settings.provider,
        f.prefix,
        run.id,
        timeout_elapsed.as_secs_f64(),
        completed.state
    );
    f.pool.close().await;
}
