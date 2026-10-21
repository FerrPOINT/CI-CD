//! An actually unknown accepted push must conflict with a third remote SHA.
use super::*;
use std::{os::unix::fs::PermissionsExt, time::Duration};

struct OwnedAckGate(PathBuf);
impl Drop for OwnedAckGate {
    fn drop(&mut self) {
        use std::os::unix::ffi::OsStrExt;
        let _ = std::fs::write(self.0.join("abort"), b"abort owned QA continuation");
        let _ = std::fs::write(self.0.join("release"), b"release owned accepted ACK gate");
        let Some(pid) = std::fs::read_to_string(self.0.join("child-pid"))
            .ok()
            .and_then(|value| value.trim().parse::<i32>().ok())
            .filter(|pid| *pid > 1)
        else {
            return;
        };
        let command = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        if command
            .split(|byte| *byte == 0)
            .any(|argument| argument == self.0.join("git").as_os_str().as_bytes())
        {
            // SAFETY: GitSession created the process group, and the leader's
            // command still names this fixture's unique shim before targeting it.
            unsafe { libc::kill(-pid, libc::SIGKILL) };
        }
    }
}

fn count(control: &Path, name: &str) -> usize {
    std::fs::read_to_string(control.join(name))
        .unwrap_or_default()
        .lines()
        .count()
}

#[tokio::test]
#[ignore = "Requires owned live provider; actual accepted push loses ACK, production records unknown, then a third SHA wins before new-owner recovery"]
async fn unknown_push_then_third_sha_recovers_as_conflict_without_retry() {
    let mut f = Live::new().await;
    f.stop_tasks().await;
    f.settings(Direction::Push, false).await;
    let name = f.head("unknown-then-third-sha");
    let old = f.commit(None, "QA unknown recovery old remote O").await;
    let wanted = f.commit(Some(&old), "QA unknown recovery wanted A").await;
    let third = f
        .commit(Some(&wanted), "QA independent third remote B")
        .await;
    assert_ne!(old, wanted);
    assert_ne!(wanted, third);
    assert_ne!(old, third);
    f.external(&name, None, &old).await;
    f.local(&name, &wanted).await;
    let head_before = std::fs::read(f.local.join("HEAD")).unwrap();
    // Enable the actual ingress policy so an accidental pull during outgoing
    // recovery cannot hide behind a disabled CI configuration.
    sqlx::query(
        "INSERT INTO repository_push_configs(repository_id,configuration_id) VALUES($1,$2)",
    )
    .bind(f.repository)
    .bind(f.project)
    .execute(&f.pool)
    .await
    .unwrap();
    let control = f.root.join("unknown-third-sha-control");
    std::fs::create_dir(&control).unwrap();
    let _cleanup = OwnedAckGate(control.clone());
    // Only the worker uses this PATH. The independent provider oracle and
    // expected-old external fast-forward keep using the actual Git transport.
    let script = format!(
        r#"#!/bin/sh
control='{}'
case " $* " in
  *" https://"*)
    if test -f "$control/offline"; then
      echo 'QA controlled observation unavailable' >&2
      exit 128
    fi ;;
esac
case " $* " in
  *" push "*" https://"*)
    echo push >> "$control/push-count"
    if test ! -f "$control/arm"; then
      # Any retry is an oracle failure. Never let a regression send a second
      # provider write while the fixture is proving observation-only recovery.
      exit 75
    fi
    rm "$control/arm"
    echo $$ > "$control/child-pid"
    echo effect >> "$control/effect-count"
    /usr/bin/git "$@"
    result=$?
    echo "$result" > "$control/accepted"
    if test "$result" -ne 0; then exit "$result"; fi
    touch "$control/accepted-held"
    while test ! -f "$control/release"; do /bin/sleep 0.1; done
    if test -f "$control/abort"; then exit 75; fi
    # The provider accepted A, but the worker gets neither an ACK nor a usable
    # subsequent HTTPS observation. Only production may write the UNKNOWN row.
    touch "$control/offline"
    exit 75 ;;
esac
exec /usr/bin/git "$@"
"#,
        control.display()
    );
    std::fs::write(control.join("git"), script).unwrap();
    std::fs::set_permissions(control.join("git"), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(control.join("arm"), b"hold exactly one accepted owned push").unwrap();
    let mut first = f.process(&control).await;
    let (status, body) = f.request("POST", "/runs", None, Some(Uuid::new_v4())).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let run: SyncRun = serde_json::from_value(body).unwrap();
    tokio::time::timeout(Duration::from_secs(90), async {
        while !control.join("accepted-held").exists() {
            assert!(first.try_wait().unwrap().is_none());
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("real provider push was accepted before holding its ACK");
    assert_eq!(
        std::fs::read_to_string(control.join("accepted"))
            .unwrap()
            .trim(),
        "0"
    );
    f.same(&name, &wanted).await;
    f.ref_result(run.id, &name, "planned").await;
    assert_eq!(count(&control, "push-count"), 1);
    assert_eq!(count(&control, "effect-count"), 1);
    std::fs::write(control.join("release"), b"lose ACK and observation").unwrap();
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            assert!(first.try_wait().unwrap().is_none());
            let current = f.store.run(f.repository, run.id).await.unwrap();
            let (refs, _) = f.store.refs(run.id, 1000, 0).await.unwrap();
            if current.state == "reconciling"
                && current.message.is_none()
                && refs.iter().any(|reference| {
                    reference.ref_name == name
                        && reference.state == "unknown"
                        && reference.reason.as_deref() == Some("outcome_unknown")
                })
            {
                assert!(current.finished_at.is_none());
                break;
            }
            assert!(current.finished_at.is_none());
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("production persists UNKNOWN after the real accepted push and unavailable observation");
    let (owner_before, generation_before): (Uuid, i64) =
        sqlx::query_as("SELECT owner_id,generation FROM repository_sync_runs WHERE id=$1")
            .bind(run.id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(generation_before, 1);
    let (refs, _) = f.store.refs(run.id, 1000, 0).await.unwrap();
    let unknown = refs
        .iter()
        .find(|reference| reference.ref_name == name)
        .unwrap();
    assert_eq!(unknown.direction, "push");
    assert_eq!(unknown.old_sha.as_deref(), Some(old.as_str()));
    assert_eq!(unknown.new_sha.as_deref(), Some(wanted.as_str()));
    assert_eq!(unknown.state, "unknown");
    println!(
        "LIVE_UNKNOWN_OTHER_BEFORE provider={:?} prefix={} run={} owner={} generation={} old_sha={} wanted_sha={} state=unknown reason=outcome_unknown push_count=1 accepted_effects=1",
        f.settings.provider, f.prefix, run.id, owner_before, generation_before, old, wanted
    );
    // Kill only after independently seeing the production-written UNKNOWN
    // receipt. No synthetic journal, receipt mutation or active-task fencing.
    first.kill().await.unwrap();
    first.wait().await.unwrap();
    f.external(&name, Some(&wanted), &third).await;
    assert_eq!(
        f.remote
            .as_ref()
            .unwrap()
            .refs(true)
            .await
            .unwrap()
            .get(&name),
        Some(&third)
    );
    assert_eq!(
        git(&f.local, &["rev-parse", "--verify", &name]).await,
        wanted
    );
    f.ref_result(run.id, &name, "unknown").await;
    std::fs::remove_file(control.join("offline")).unwrap();
    let expired = sqlx::query(
        "UPDATE repository_sync_runs SET lease_until=now()-interval '1 second' WHERE id=$1 AND state='reconciling' AND generation=1",
    )
    .bind(run.id)
    .execute(&f.pool)
    .await
    .unwrap();
    assert_eq!(expired.rows_affected(), 1);
    let mut second = f.process(&control).await;
    let completed = terminal(&f.store, f.repository, run.id).await;
    assert_eq!(completed.state, "conflict");
    assert_eq!(completed.settings, run.settings);
    assert_eq!(completed.connection_version, run.connection_version);
    assert!(!completed.cancel_requested);
    let (owner_after, generation_after): (Uuid, i64) =
        sqlx::query_as("SELECT owner_id,generation FROM repository_sync_runs WHERE id=$1")
            .bind(run.id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_ne!(owner_before, owner_after);
    assert_eq!(generation_after, 2);
    let (refs, _) = f.store.refs(run.id, 1000, 0).await.unwrap();
    let conflict = refs
        .iter()
        .find(|reference| reference.ref_name == name)
        .unwrap();
    assert_eq!(conflict.state, "conflict");
    assert_eq!(conflict.reason.as_deref(), Some("destination_changed"));
    assert_eq!(conflict.old_sha, unknown.old_sha);
    assert_eq!(conflict.new_sha, unknown.new_sha);
    assert_eq!(conflict.direction, "push");
    assert_eq!(
        git(&f.local, &["rev-parse", "--verify", &name]).await,
        wanted
    );
    let mut remote = f.remote.as_ref().unwrap().refs(true).await.unwrap();
    assert_eq!(remote.remove(&name), Some(third.clone()));
    assert_eq!(
        remote, f.initial,
        "every preserved provider baseline ref is unchanged"
    );
    assert_eq!(std::fs::read(f.local.join("HEAD")).unwrap(), head_before);
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(
        count(&control, "push-count"),
        1,
        "recovery must not even attempt a repeated push"
    );
    assert_eq!(count(&control, "effect-count"), 1);
    assert_eq!(f.store.runs(f.repository, 20, 0).await.unwrap().1, 1);
    let pipelines: i64 = sqlx::query_scalar("SELECT count(*) FROM pipelines WHERE project_id=$1")
        .bind(f.project)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        pipelines, 0,
        "third remote SHA must not be imported or trigger CI"
    );
    second.kill().await.unwrap();
    second.wait().await.unwrap();
    println!(
        "LIVE_UNKNOWN_OTHER_CONFLICT_OK provider={:?} prefix={} run={} owner={} generation={} old_sha={} wanted_sha={} remote_third_sha={} state=conflict reason=destination_changed local_preserved=true remote_preserved=true push_count=1 accepted_effects=1 pipelines=0",
        f.settings.provider, f.prefix, run.id, owner_after, generation_after, old, wanted, third
    );
    f.pool.close().await;
}
