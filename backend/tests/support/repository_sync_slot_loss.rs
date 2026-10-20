//! Actual Git-process concurrency after losing a detached database slot session.
#[cfg(unix)]
use super::*;

#[cfg(unix)]
struct SlotGates(PathBuf);

#[cfg(unix)]
impl SlotGates {
    fn alive(&self, index: usize) -> Option<i32> {
        use std::os::unix::ffi::OsStrExt;
        let pid = std::fs::read_to_string(self.0.join(format!("gate-{index}.pid")))
            .ok()?
            .trim()
            .parse::<i32>()
            .ok()
            .filter(|pid| *pid > 1)?;
        let command = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
        let script = self.0.join("git");
        command
            .split(|byte| *byte == 0)
            .any(|argument| argument == script.as_os_str().as_bytes())
            .then_some(pid)
    }

    fn release(&self, index: usize) {
        std::fs::write(
            self.0.join(format!("release-{index}")),
            b"abort own push gate",
        )
        .unwrap();
    }

    fn sample(&self) -> Vec<Option<i32>> {
        (0..3).map(|index| self.alive(index)).collect()
    }
}

#[cfg(unix)]
impl Drop for SlotGates {
    fn drop(&mut self) {
        // Every HTTPS push exits 75 even when released. A failing assertion must
        // additionally terminate all proven owned orphan process groups.
        let _ = std::fs::write(self.0.join("abort-all"), b"abort every owned gate");
        for index in 0..3 {
            if let Some(pid) = self.alive(index) {
                // SAFETY: the unique shim path still identifies this group leader.
                unsafe { libc::kill(-pid, libc::SIGKILL) };
            }
        }
    }
}

#[cfg(unix)]
async fn slot_worker(f: &Live, gates: &SlotGates, application: &str) -> tokio::process::Child {
    use tokio::io::AsyncWriteExt;
    let schema: String = sqlx::query_scalar("SELECT current_schema()")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    // A unique PostgreSQL application_name proves slot-session ownership without
    // exposing credentials or relying on process timing/last query heuristics.
    let mut database =
        reqwest::Url::parse(&std::env::var("CICD_TEST_DATABASE_URL").unwrap()).unwrap();
    assert!(
        !database
            .query_pairs()
            .any(|(key, _)| key == "application_name")
    );
    database
        .query_pairs_mut()
        .append_pair("application_name", application);
    let input = serde_json::json!({"purpose":"forge-owned-process-fault-fixture","root":f.root,"origin":f.settings.instance_url,"database":database.as_str(),"schema":schema});
    let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "repository_sync::live::worker_process",
            "--ignored",
            "--nocapture",
        ])
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", gates.0.display()))
        .stdin(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .await
        .unwrap();
    child
}

#[cfg(unix)]
async fn slot_snapshot(f: &Live, runs: &[Uuid], gates: &SlotGates, phase: &str, peak: usize) {
    let rows: Vec<serde_json::Value> = sqlx::query_scalar("SELECT jsonb_build_object('run_id',id,'repository_id',repository_id,'owner_id',owner_id,'generation',generation,'state',state,'lease_until',lease_until,'cancel_requested',cancel_requested) FROM repository_sync_runs WHERE id=ANY($1) ORDER BY created_at,id")
        .bind(runs).fetch_all(&f.pool).await.unwrap();
    let pids: Vec<Option<i32>> = (0..3)
        .map(|index| {
            std::fs::read_to_string(gates.0.join(format!("gate-{index}.pid")))
                .ok()
                .and_then(|s| s.trim().parse().ok())
        })
        .collect();
    println!(
        "LIVE_SLOT_LOSS_SNAPSHOT {}",
        serde_json::json!({"provider":f.settings.provider,"prefix":f.prefix,"phase":phase,"gate_pids":pids,"alive_gate_pids":gates.sample(),"peak":peak,"runs":rows})
    );
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "Requires owned live provider; proves actual three-repository Git concurrency after losing only an owned database slot; never sends HTTPS push"]
async fn lost_slot_session_never_exceeds_live_git_capacity_and_waiting_repository_advances() {
    use cicd_infra::sync_git::MutationGuard;
    use std::{os::unix::fs::PermissionsExt, time::Duration};

    let mut f = Live::new().await;
    f.stop_tasks().await;
    f.settings(Direction::Push, false).await;
    let slots_before: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_locks l JOIN pg_stat_activity a ON a.pid=l.pid WHERE a.datname=current_database() AND l.locktype='advisory' AND l.classid=93 AND l.objsubid=2 AND l.objid IN (0,1) AND l.granted")
        .fetch_one(&f.pool).await.unwrap();
    assert_eq!(
        slots_before, 0,
        "run this opt-in only in its isolated QA database"
    );
    let repositories = [f.repository, Uuid::new_v4(), Uuid::new_v4()];
    let projects = [f.project, Uuid::new_v4(), Uuid::new_v4()];
    let mut local = vec![f.local.clone()];
    for index in 1..3 {
        let name = format!("sync-{}", repositories[index].simple());
        let path = f.root.join(format!("{name}.git"));
        std::fs::create_dir(&path).unwrap();
        git(&path, &["init", "--bare", "--initial-branch=main"]).await;
        sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
            .bind(repositories[index])
            .bind(&name)
            .execute(&f.pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id,default_branch) VALUES($1,$2,'https://forge.invalid/disposable',$3,'qa-protected-default')")
            .bind(projects[index]).bind(format!("qa-{}", projects[index])).bind(repositories[index]).execute(&f.pool).await.unwrap();
        // All three owned repositories share one database, storage root and
        // provider endpoint. Copy ciphertext only inside this disposable DB.
        sqlx::query("INSERT INTO repository_sync_connections(repository_id,settings,version,encrypted_credential,next_check_at,dirty) SELECT $1,settings,1,encrypted_credential,NULL,false FROM repository_sync_connections WHERE repository_id=$2")
            .bind(repositories[index]).bind(f.repository).execute(&f.pool).await.unwrap();
        local.push(path);
    }
    let sha = f.commit(None, "QA slot loss with no provider write").await;
    let refs: Vec<String> = (0..3)
        .map(|index| f.head(&format!("slot-loss-{index}")))
        .collect();
    for index in 0..3 {
        git(
            &f.actor,
            &[
                "push",
                local[index].to_str().unwrap(),
                &format!("{sha}:{}", refs[index]),
            ],
        )
        .await;
    }
    let control = f.root.join("slot-loss-control");
    std::fs::create_dir(&control).unwrap();
    let gates = SlotGates(control.clone());
    let script = format!(
        r#"#!/bin/sh
control='{}'
case " $* " in
  *" push "*" https://"*)
    case " $* " in
      *"{}"*) gate=0 ;;
      *"{}"*) gate=1 ;;
      *"{}"*) gate=2 ;;
      *) touch "$control/unexpected-push"; exit 75 ;;
    esac
    echo $$ > "$control/gate-$gate.pid"
    touch "$control/reached-$gate"
    while test ! -f "$control/release-$gate" && test ! -f "$control/abort-all"; do /bin/sleep 0.1; done
    # NEVER execute an HTTPS push, including after release or assertion failure.
    exit 75 ;;
esac
exec /usr/bin/git "$@"
"#,
        control.display(),
        refs[0],
        refs[1],
        refs[2]
    );
    std::fs::write(control.join("git"), script).unwrap();
    std::fs::set_permissions(control.join("git"), std::fs::Permissions::from_mode(0o700)).unwrap();
    let first = f
        .store
        .enqueue(repositories[0], "manual", None)
        .await
        .unwrap();
    let second = f
        .store
        .enqueue(repositories[1], "manual", None)
        .await
        .unwrap();
    let application_a = format!("qa-slot-a-{}", f.repository.simple());
    let application_b = format!("qa-slot-b-{}", f.repository.simple());
    let mut worker_a = slot_worker(&f, &gates, &application_a).await;
    tokio::time::timeout(Duration::from_secs(180), async {
        while gates.alive(0).is_none() || gates.alive(1).is_none() {
            assert!(worker_a.try_wait().unwrap().is_none());
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("one worker fills both slots with live owned Git gate processes");
    for repository in &repositories[..2] {
        assert!(
            MutationGuard::try_acquire(&f.root, *repository)
                .unwrap()
                .is_none()
        );
    }
    let slots: Vec<(i32, i64)> = sqlx::query_as("SELECT l.pid,l.objid::bigint FROM pg_locks l JOIN pg_stat_activity a ON a.pid=l.pid WHERE a.datname=current_database() AND a.usename=current_user AND a.application_name=$1 AND l.locktype='advisory' AND l.classid=93 AND l.objsubid=2 AND l.objid IN (0,1) AND l.mode='ExclusiveLock' AND l.granted ORDER BY l.objid")
        .bind(&application_a).fetch_all(&f.pool).await.unwrap();
    assert_eq!(slots.len(), 2);
    assert_eq!(
        slots.iter().map(|(_, slot)| *slot).collect::<Vec<_>>(),
        vec![0, 1]
    );
    let third = f
        .store
        .enqueue(repositories[2], "manual", None)
        .await
        .unwrap();
    let runs = [first.id, second.id, third.id];
    let mut worker_b = slot_worker(&f, &gates, &application_b).await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(
        f.store.run(repositories[2], third.id).await.unwrap().state,
        "queued"
    );
    assert!(gates.alive(2).is_none());
    assert!(gates.alive(0).is_some() && gates.alive(1).is_some());
    let mut peak = 2;
    slot_snapshot(
        &f,
        &runs,
        &gates,
        "two-git-processes-before-slot-loss",
        peak,
    )
    .await;
    println!(
        "LIVE_SLOT_LOSS_OWNED_SESSION {}",
        serde_json::json!({"application_name":application_a,"slot_backend_pid":slots[0].0,"slot":slots[0].1,"other_slot_backend_pid":slots[1].0,"worker_a_pid":worker_a.id(),"worker_b_pid":worker_b.id()})
    );
    // Revalidate PID, database, application identity and exact advisory-lock key
    // in the terminating SQL statement itself; never terminate by a cached PID alone.
    let terminated: Option<bool> = sqlx::query_scalar("SELECT pg_terminate_backend(a.pid) FROM pg_stat_activity a WHERE a.pid=$1 AND a.datname=current_database() AND a.usename=current_user AND a.application_name=$2 AND EXISTS(SELECT 1 FROM pg_locks l WHERE l.pid=a.pid AND l.locktype='advisory' AND l.classid=93 AND l.objsubid=2 AND l.objid::bigint=$3 AND l.mode='ExclusiveLock' AND l.granted)")
        .bind(slots[0].0).bind(&application_a).bind(slots[0].1).fetch_optional(&f.pool).await.unwrap();
    assert_eq!(terminated, Some(true));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while tokio::time::Instant::now() < deadline {
        let live = gates.sample();
        peak = peak.max(live.iter().flatten().count());
        if peak > 2 {
            slot_snapshot(&f, &runs, &gates, "red-three-live-git-processes", peak).await;
            for repository in repositories {
                assert!(
                    MutationGuard::try_acquire(&f.root, repository)
                        .unwrap()
                        .is_none()
                );
            }
            break;
        }
        assert!(worker_a.try_wait().unwrap().is_none());
        assert!(worker_b.try_wait().unwrap().is_none());
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        peak <= 2,
        "lost DB slot allowed three simultaneous real Git processes with concurrency=2"
    );
    // A safe implementation must also make forward progress, rather than
    // preserving the cap forever by preventing the waiting repository from running.
    gates.release(0);
    tokio::time::timeout(Duration::from_secs(40), async {
        loop {
            let live = gates.sample();
            peak = peak.max(live.iter().flatten().count());
            assert!(
                peak <= 2,
                "capacity exceeds two during waiting-repository promotion"
            );
            if live[2].is_some() {
                break;
            }
            assert!(worker_a.try_wait().unwrap().is_none());
            assert!(worker_b.try_wait().unwrap().is_none());
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("third repository advances after a real first Git gate exits");
    assert!(
        MutationGuard::try_acquire(&f.root, repositories[2])
            .unwrap()
            .is_none()
    );
    slot_snapshot(&f, &runs, &gates, "waiting-repository-progressed", peak).await;
    for index in 0..3 {
        gates.release(index);
    }
    tokio::time::timeout(Duration::from_secs(10), async {
        while !gates.sample().iter().all(Option::is_none) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("all owned Git gates drain without external pushes");
    worker_a.kill().await.unwrap();
    worker_a.wait().await.unwrap();
    worker_b.kill().await.unwrap();
    worker_b.wait().await.unwrap();
    assert!(!control.join("unexpected-push").exists());
    assert_eq!(
        f.remote.as_ref().unwrap().refs(true).await.unwrap(),
        f.initial,
        "the entire external ref map remains unchanged; no HTTPS push reaches the provider"
    );
    for index in 0..3 {
        assert_eq!(
            git(&local[index], &["rev-parse", "--verify", &refs[index]]).await,
            sha
        );
    }
    let pipelines: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pipelines WHERE project_id=ANY($1)")
            .bind(&projects[..])
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(pipelines, 0);
    slot_snapshot(&f, &runs, &gates, "completed-no-provider-effects", peak).await;
    println!(
        "LIVE_SLOT_LOSS_CAPACITY_OK provider={:?} prefix={} peak={peak} provider_effects=0",
        f.settings.provider, f.prefix
    );
    f.pool.close().await;
}
