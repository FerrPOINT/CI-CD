//! Opt-in namespace/Git interleavings using the real owner handler and OS workers.
use super::*;
use cicd_app::repository_sync::Store;
use pdlc_shared::resource_context::{NamespaceRef, OwnerCommand, ResourceKind, ResourceRef};
use std::{os::unix::fs::PermissionsExt, time::Duration};

struct OwnedGitGate(PathBuf);
impl Drop for OwnedGitGate {
    fn drop(&mut self) {
        use std::os::unix::ffi::OsStrExt;
        // A failing assertion must never release an orphan into a new Git effect.
        let _ = std::fs::write(self.0.join("abort-before-effect"), b"abort own QA write");
        let _ = std::fs::write(self.0.join("release"), b"release own QA gate");
        let Some(pid) = std::fs::read_to_string(self.0.join("child-pid"))
            .ok()
            .and_then(|pid| pid.trim().parse::<i32>().ok())
            .filter(|pid| *pid > 1)
        else {
            return;
        };
        let script = self.0.join("git");
        let command = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        if command
            .split(|byte| *byte == 0)
            .any(|argument| argument == script.as_os_str().as_bytes())
        {
            // SAFETY: GitSession creates this process group; argv still names
            // our unique shim before targeting its leader and owned descendants.
            unsafe { libc::kill(-pid, libc::SIGKILL) };
        }
    }
}

struct Namespace {
    state: Arc<cicd::api::AppState>,
    command: OwnerCommand,
}

impl Namespace {
    async fn attach(f: &Live) -> Self {
        let instance = std::env::var("CICD_NAMESPACE__INSTANCE_ID")
            .expect("namespace instance fixture configured")
            .parse()
            .unwrap();
        let registry = std::env::var("CICD_NAMESPACE__REGISTRY_INSTANCE_ID")
            .expect("namespace registry fixture configured")
            .parse()
            .unwrap();
        let group = Uuid::new_v4();
        sqlx::query("INSERT INTO git_groups(id,slug,name) VALUES($1,$2,'Sync archive QA')")
            .bind(group)
            .bind(format!("archive-{}", group.simple()))
            .execute(&f.pool)
            .await
            .unwrap();
        let config = cicd::config::RuntimeConfig::from_env_source(|_| None, false).unwrap();
        let state = Arc::new(cicd::api::AppState {
            pool: Some(f.pool.clone()),
            namespace_admission_pool: Some(f.pool.clone()),
            auth_secret: None,
            git: config.git.to_git_config(),
            config,
            running_jobs: None,
            rate_limiter: Arc::new(cicd::rate_limit::RateLimiter::default()),
        });
        let command = OwnerCommand {
            schema_version: 1,
            namespace: NamespaceRef {
                registry_instance_id: registry,
                namespace_id: Uuid::new_v4(),
            },
            resource: ResourceRef {
                instance_id: instance,
                kind: ResourceKind::GitGroup,
                resource_id: group,
            },
            operation_id: Uuid::new_v4(),
            generation: 1,
            state: "active".into(),
            create_spec: None,
        };
        // Exercise production lifecycle validation and locks, not direct state UPDATE.
        apply(state.clone(), command.clone()).await;
        sqlx::query("UPDATE repository_catalog SET group_id=$2 WHERE id=$1")
            .bind(f.repository)
            .bind(group)
            .execute(&f.pool)
            .await
            .unwrap();
        Self { state, command }
    }

    fn next(&mut self, lifecycle: &str) -> OwnerCommand {
        self.command.generation += 1;
        self.command.operation_id = Uuid::new_v4();
        self.command.state = lifecycle.into();
        self.command.clone()
    }

    async fn change(&mut self, lifecycle: &str) {
        let command = self.next(lifecycle);
        apply(self.state.clone(), command).await;
    }

    fn archive(&mut self) -> tokio::task::JoinHandle<()> {
        let state = self.state.clone();
        let command = self.next("archived");
        tokio::spawn(async move { apply(state, command).await })
    }
}

async fn apply(state: Arc<cicd::api::AppState>, command: OwnerCommand) {
    let _ = cicd::namespace::apply(
        axum::extract::State(state),
        axum::extract::Path(command.resource.resource_id),
        axum::Json(command),
    )
    .await
    .unwrap_or_else(|_| panic!("production namespace owner apply must succeed"));
}

async fn worker(f: &Live, path: &Path) -> tokio::process::Child {
    use tokio::io::AsyncWriteExt;
    let schema: String = sqlx::query_scalar("SELECT current_schema()")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    let input = serde_json::json!({"purpose":"forge-owned-process-fault-fixture","root":f.root,"origin":f.settings.instance_url,"database":std::env::var("CICD_TEST_DATABASE_URL").unwrap(),"schema":schema});
    let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "repository_sync::live::worker_process",
            "--ignored",
            "--nocapture",
        ])
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", path.display()))
        .env(
            "CICD_NAMESPACE__INSTANCE_ID",
            std::env::var("CICD_NAMESPACE__INSTANCE_ID").unwrap(),
        )
        .env(
            "CICD_NAMESPACE__REGISTRY_INSTANCE_ID",
            std::env::var("CICD_NAMESPACE__REGISTRY_INSTANCE_ID").unwrap(),
        )
        .env(
            "CICD_NAMESPACE__PUBLIC_GIT_ORIGIN",
            std::env::var("CICD_NAMESPACE__PUBLIC_GIT_ORIGIN").unwrap(),
        )
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

async fn queue(f: &Live) -> SyncRun {
    let (status, body) = f.request("POST", "/runs", None, Some(Uuid::new_v4())).await;
    assert_eq!(
        status,
        StatusCode::ACCEPTED,
        "sync queue rejected: error={} message={}",
        body.get("error").unwrap_or(&serde_json::Value::Null),
        body.get("message").unwrap_or(&serde_json::Value::Null)
    );
    serde_json::from_value(body).unwrap()
}

async fn gate_reached(control: &Path, worker: &mut tokio::process::Child) {
    tokio::time::timeout(Duration::from_secs(90), async {
        while !control.join("reached").exists() {
            assert!(worker.try_wait().unwrap().is_none());
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("owned Git child reached the requested real phase");
}

async fn lifecycle(f: &Live, group: Uuid) -> String {
    sqlx::query_scalar("SELECT state FROM forge_namespace_bindings WHERE resource_id=$1")
        .bind(group)
        .fetch_one(&f.pool)
        .await
        .unwrap()
}

async fn generation(f: &Live, run: Uuid) -> i64 {
    sqlx::query_scalar("SELECT generation FROM repository_sync_runs WHERE id=$1")
        .bind(run)
        .fetch_one(&f.pool)
        .await
        .unwrap()
}

async fn waiting_archive(f: &Live, namespace: &Namespace) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE locktype='advisory' AND mode='ExclusiveLock' AND NOT granted AND classid::bigint=((hashtextextended($1,0)>>32)&4294967295) AND objid::bigint=(hashtextextended($1,0)&4294967295))")
                .bind(namespace.command.resource.resource_id.to_string())
                .fetch_one(&f.pool).await.unwrap();
            if waiting { break; }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }).await.expect("real archive handler waits for shared namespace admission");
}

fn lines(control: &Path, name: &str) -> usize {
    std::fs::read_to_string(control.join(name))
        .unwrap_or_default()
        .lines()
        .count()
}

async fn stop(worker: &mut tokio::process::Child) {
    worker.kill().await.unwrap();
    worker.wait().await.unwrap();
}

fn push_gate(control: &Path) {
    std::fs::create_dir(control).unwrap();
    let script = format!(
        r#"#!/bin/sh
control='{}'
case " $* " in
  *" push "*" https://"*)
    if test -f "$control/arm"; then
      rm "$control/arm"
      echo $$ > "$control/child-pid"
      touch "$control/reached"
      while test ! -f "$control/release"; do /bin/sleep 0.1; done
      if test -f "$control/abort-before-effect"; then exit 75; fi
    fi
    echo effect >> "$control/effects" ;;
esac
exec /usr/bin/git "$@"
"#,
        control.display()
    );
    std::fs::write(control.join("git"), script).unwrap();
    std::fs::set_permissions(control.join("git"), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(control.join("arm"), b"gate first real outgoing effect").unwrap();
}

fn gate_alive(control: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Some(pid) = std::fs::read_to_string(control.join("child-pid"))
        .ok()
        .and_then(|pid| pid.trim().parse::<i32>().ok())
        .filter(|pid| *pid > 1)
    else {
        return false;
    };
    let script = control.join("git");
    std::fs::read(format!("/proc/{pid}/cmdline"))
        .unwrap_or_default()
        .split(|byte| *byte == 0)
        .any(|argument| argument == script.as_os_str().as_bytes())
}

async fn canceled_gate(control: &Path) -> bool {
    tokio::time::timeout(Duration::from_secs(12), async {
        while gate_alive(control) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .is_ok()
}

async fn guard_released(f: &Live) {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if cicd_infra::sync_git::MutationGuard::try_acquire(&f.root, f.repository)
                .unwrap()
                .is_some()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("own Git execution releases guard after DB fault");
}

#[tokio::test]
#[ignore = "Requires owned live provider; terminates only its own held namespace admission DB connection"]
async fn namespace_admission_connection_loss_during_gated_push_prevents_archived_write() {
    let mut f = Live::new().await;
    f.stop_tasks().await;
    f.settings(Direction::Push, false).await;
    let mut namespace = Namespace::attach(&f).await;
    let sha = f
        .commit(None, "QA namespace admission connection loss")
        .await;
    let name = f.head("namespace-connection-loss");
    f.local(&name, &sha).await;
    let remote_before = f.remote.as_ref().unwrap().refs(true).await.unwrap();
    let control = f.root.join("namespace-connection-loss");
    push_gate(&control);
    let _release = OwnedGitGate(control.clone());
    let mut child = worker(&f, &control).await;
    let run = queue(&f).await;
    gate_reached(&control, &mut child).await;
    f.ref_result(run.id, &name, "planned").await;
    assert_eq!(lines(&control, "effects"), 0);
    let group = namespace.command.resource.resource_id.to_string();
    let pids: Vec<i32> = sqlx::query_scalar("SELECT DISTINCT l.pid FROM pg_locks l JOIN pg_stat_activity a ON a.pid=l.pid WHERE a.datid=(SELECT oid FROM pg_database WHERE datname=current_database()) AND a.pid<>pg_backend_pid() AND l.locktype='advisory' AND l.mode='ShareLock' AND l.granted AND l.classid::bigint=((hashtextextended($1,0)>>32)&4294967295) AND l.objid::bigint=(hashtextextended($1,0)&4294967295)")
        .bind(&group).fetch_all(&f.pool).await.unwrap();
    assert_eq!(
        pids.len(),
        1,
        "unique own namespace group identifies exactly one live admission backend"
    );
    let pid = pids[0];
    // Recheck database, exact lock key and shared ownership in the terminating query.
    let terminated: Option<bool> = sqlx::query_scalar("SELECT pg_terminate_backend(a.pid) FROM pg_stat_activity a WHERE a.pid=$1 AND a.datid=(SELECT oid FROM pg_database WHERE datname=current_database()) AND a.pid<>pg_backend_pid() AND EXISTS(SELECT 1 FROM pg_locks l WHERE l.pid=a.pid AND l.locktype='advisory' AND l.mode='ShareLock' AND l.granted AND l.classid::bigint=((hashtextextended($2,0)>>32)&4294967295) AND l.objid::bigint=(hashtextextended($2,0)&4294967295))")
        .bind(pid).bind(&group).fetch_optional(&f.pool).await.unwrap();
    assert_eq!(terminated, Some(true));
    namespace.change("archived").await;
    assert_eq!(
        lifecycle(&f, namespace.command.resource.resource_id).await,
        "archived"
    );
    let stopped_without_release = canceled_gate(&control).await;
    if !stopped_without_release {
        // Allow the suspected violation only on this new, isolated owned QA ref.
        // This records a real provider effect rather than claiming a timeout is proof.
        std::fs::write(
            control.join("release"),
            b"observe bounded own QA effect after admission loss",
        )
        .unwrap();
    }
    guard_released(&f).await;
    let observed = f.store.run(f.repository, run.id).await.unwrap();
    stop(&mut child).await;
    let completed = if observed.finished_at.is_some() {
        observed
    } else {
        sqlx::query(
            "UPDATE repository_sync_runs SET lease_until=now()-interval '1 second' WHERE id=$1",
        )
        .bind(run.id)
        .execute(&f.pool)
        .await
        .unwrap();
        let mut recovery = worker(&f, &control).await;
        let completed = f.completed(run.id).await;
        stop(&mut recovery).await;
        completed
    };
    let after = f.remote.as_ref().unwrap().refs(true).await.unwrap();
    let created = after.get(&name).cloned();
    println!(
        "LIVE_NAMESPACE_CONNECTION_LOSS_OBSERVED provider={:?} prefix={} run={} namespace_backend={} stopped_without_release={} effects={} state={} actual_sha={:?}",
        f.settings.provider,
        f.prefix,
        run.id,
        pid,
        stopped_without_release,
        lines(&control, "effects"),
        completed.state,
        created
    );
    assert_eq!(
        after, remote_before,
        "losing namespace admission must not permit an archived Git write"
    );
    assert_ne!(
        completed.state, "success",
        "archive admission loss cannot be falsely reported as success"
    );
    assert_eq!(lines(&control, "effects"), 0);
    assert!(
        stopped_without_release,
        "namespace admission failure cancels the held Git process before gate release"
    );
    assert_eq!(f.store.runs(f.repository, 20, 0).await.unwrap().1, 1);
    f.pool.close().await;
}

#[tokio::test]
#[ignore = "Requires owned live provider; terminates only the blocked heartbeat DB query of its own run"]
async fn heartbeat_connection_loss_cancels_git_and_reconciles_same_intent() {
    let mut f = Live::new().await;
    f.stop_tasks().await;
    f.settings(Direction::Push, false).await;
    let sha = f.commit(None, "QA heartbeat connection loss").await;
    let name = f.head("heartbeat-connection-loss");
    f.local(&name, &sha).await;
    let before = f.remote.as_ref().unwrap().refs(true).await.unwrap();
    let control = f.root.join("heartbeat-connection-loss");
    push_gate(&control);
    let _release = OwnedGitGate(control.clone());
    let mut first = worker(&f, &control).await;
    let run = queue(&f).await;
    gate_reached(&control, &mut first).await;
    f.ref_result(run.id, &name, "planned").await;
    let mut fence = f.pool.begin().await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *fence)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM repository_sync_runs WHERE id=$1 FOR UPDATE")
        .bind(run.id)
        .fetch_one(&mut *fence)
        .await
        .unwrap();
    let heartbeat = tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            let pids: Vec<i32> = sqlx::query_scalar("SELECT pid FROM pg_stat_activity WHERE datid=(SELECT oid FROM pg_database WHERE datname=current_database()) AND pid<>pg_backend_pid() AND $1=ANY(pg_blocking_pids(pid)) AND query LIKE 'UPDATE repository_sync_runs SET lease_until=%'")
                .bind(blocker).fetch_all(&f.pool).await.unwrap();
            if !pids.is_empty() {
                assert_eq!(pids.len(), 1, "only own heartbeat waits for the exact owned run row");
                break pids[0];
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }).await.expect("real worker heartbeat blocks on own run row");
    let terminated: Option<bool> = sqlx::query_scalar("SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE pid=$1 AND datid=(SELECT oid FROM pg_database WHERE datname=current_database()) AND pid<>pg_backend_pid() AND $2=ANY(pg_blocking_pids(pid)) AND query LIKE 'UPDATE repository_sync_runs SET lease_until=%'")
        .bind(heartbeat).bind(blocker).fetch_optional(&f.pool).await.unwrap();
    assert_eq!(terminated, Some(true));
    assert!(
        canceled_gate(&control).await,
        "real heartbeat connection error cancels Git without releasing gate"
    );
    assert!(!control.join("release").exists());
    assert_eq!(lines(&control, "effects"), 0);
    assert_eq!(f.remote.as_ref().unwrap().refs(true).await.unwrap(), before);
    // finish UPDATE needs this row; release only after proving child cancellation.
    fence.commit().await.unwrap();
    guard_released(&f).await;
    let unknown = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let current = f.store.run(f.repository, run.id).await.unwrap();
            if current.state == "reconciling" {
                break current;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("original worker durably records reconciliation after canceled exchange");
    assert_eq!(unknown.state, "reconciling");
    assert!(unknown.finished_at.is_none());
    assert_eq!(generation(&f, run.id).await, 1);
    stop(&mut first).await;
    sqlx::query(
        "UPDATE repository_sync_runs SET lease_until=now()-interval '1 second' WHERE id=$1",
    )
    .bind(run.id)
    .execute(&f.pool)
    .await
    .unwrap();
    let mut second = worker(&f, &control).await;
    let completed = f.completed(run.id).await;
    assert_eq!(completed.state, "success");
    assert_eq!(completed.connection_version, run.connection_version);
    assert_eq!(completed.settings, run.settings);
    assert_eq!(generation(&f, run.id).await, 2);
    f.ref_result(run.id, &name, "applied").await;
    f.same(&name, &sha).await;
    let mut after = f.remote.as_ref().unwrap().refs(true).await.unwrap();
    assert_eq!(after.remove(&name).as_deref(), Some(sha.as_str()));
    assert_eq!(after, before);
    assert_eq!(lines(&control, "effects"), 1);
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(lines(&control, "effects"), 1);
    assert_eq!(f.store.runs(f.repository, 20, 0).await.unwrap().1, 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM pipelines WHERE project_id=$1")
            .bind(f.project)
            .fetch_one(&f.pool)
            .await
            .unwrap(),
        0
    );
    stop(&mut second).await;
    println!(
        "LIVE_HEARTBEAT_CONNECTION_LOSS_OK provider={:?} prefix={} run={} heartbeat_backend={} generation=2 effects=1 ci=0",
        f.settings.provider, f.prefix, run.id, heartbeat
    );
    f.pool.close().await;
}

#[tokio::test]
#[ignore = "Requires owned live provider; loses namespace admission after accepted outgoing push without killing worker"]
async fn namespace_admission_loss_after_accepted_push_reconciles_truthfully_without_repeating_effect()
 {
    let mut f = Live::new().await;
    f.stop_tasks().await;
    f.settings(Direction::Push, false).await;
    let mut namespace = Namespace::attach(&f).await;
    let sha = f
        .commit(None, "QA admission loss after accepted outgoing push")
        .await;
    let name = f.head("admission-loss-accepted");
    f.local(&name, &sha).await;
    let remote_before = f.remote.as_ref().unwrap().refs(true).await.unwrap();
    let control = f.root.join("admission-loss-accepted");
    std::fs::create_dir(&control).unwrap();
    let _release = OwnedGitGate(control.clone());
    let script = format!(
        r#"#!/bin/sh
control='{}'
case " $* " in
  *" push "*" https://"*)
    echo effect >> "$control/effects"
    if test -f "$control/arm"; then
      rm "$control/arm"
      echo $$ > "$control/child-pid"
      /usr/bin/git "$@"
      result=$?
      echo "$result" > "$control/accepted"
      touch "$control/reached"
      while test ! -f "$control/release"; do /bin/sleep 0.1; done
      exit "$result"
    fi ;;
esac
exec /usr/bin/git "$@"
"#,
        control.display()
    );
    std::fs::write(control.join("git"), script).unwrap();
    std::fs::set_permissions(control.join("git"), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(
        control.join("arm"),
        b"hold first accepted push acknowledgment",
    )
    .unwrap();
    let mut child = worker(&f, &control).await;
    let run = queue(&f).await;
    gate_reached(&control, &mut child).await;
    assert_eq!(
        std::fs::read_to_string(control.join("accepted"))
            .unwrap()
            .trim(),
        "0"
    );
    f.same(&name, &sha).await;
    f.ref_result(run.id, &name, "planned").await;
    assert_eq!(lines(&control, "effects"), 1);
    let group = namespace.command.resource.resource_id.to_string();
    let pids: Vec<i32> = sqlx::query_scalar("SELECT DISTINCT l.pid FROM pg_locks l JOIN pg_stat_activity a ON a.pid=l.pid WHERE a.datid=(SELECT oid FROM pg_database WHERE datname=current_database()) AND a.pid<>pg_backend_pid() AND l.locktype='advisory' AND l.mode='ShareLock' AND l.granted AND l.classid::bigint=((hashtextextended($1,0)>>32)&4294967295) AND l.objid::bigint=(hashtextextended($1,0)&4294967295)")
        .bind(&group).fetch_all(&f.pool).await.unwrap();
    assert_eq!(
        pids.len(),
        1,
        "unique own group identifies the actual admitted worker connection"
    );
    let pid = pids[0];
    let terminated: Option<bool> = sqlx::query_scalar("SELECT pg_terminate_backend(a.pid) FROM pg_stat_activity a WHERE a.pid=$1 AND a.datid=(SELECT oid FROM pg_database WHERE datname=current_database()) AND a.pid<>pg_backend_pid() AND EXISTS(SELECT 1 FROM pg_locks l WHERE l.pid=a.pid AND l.locktype='advisory' AND l.mode='ShareLock' AND l.granted AND l.classid::bigint=((hashtextextended($2,0)>>32)&4294967295) AND l.objid::bigint=(hashtextextended($2,0)&4294967295))")
        .bind(pid).bind(&group).fetch_optional(&f.pool).await.unwrap();
    assert_eq!(terminated, Some(true));
    namespace.change("archived").await;
    assert!(
        canceled_gate(&control).await,
        "admission monitor stops accepted-push acknowledgment process without external worker kill or gate release"
    );
    assert!(!control.join("release").exists());
    assert!(
        child.try_wait().unwrap().is_none(),
        "supervisor survives its individual admission failure"
    );
    let unknown = tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            let current = f.store.run(f.repository, run.id).await.unwrap();
            if current.state == "reconciling" {
                break current;
            }
            assert!(
                current.finished_at.is_none(),
                "unknown accepted effect must not become a false terminal result"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("accepted outcome remains reconciling after admission connection loss");
    assert!(!unknown.cancel_requested);
    assert!(unknown.finished_at.is_none());
    assert_eq!(generation(&f, run.id).await, 1);
    let receipt = f
        .store
        .refs(run.id, 1000, 0)
        .await
        .unwrap()
        .0
        .into_iter()
        .find(|r| r.ref_name == name)
        .unwrap();
    assert!(matches!(receipt.state.as_str(), "unknown" | "planned"));
    assert_eq!(receipt.new_sha.as_deref(), Some(sha.as_str()));
    guard_released(&f).await;
    // No OS worker is killed or restarted: its next claim must reconcile read-only
    // even though current namespace admission is archived.
    sqlx::query(
        "UPDATE repository_sync_runs SET lease_until=now()-interval '1 second' WHERE id=$1",
    )
    .bind(run.id)
    .execute(&f.pool)
    .await
    .unwrap();
    let completed = f.completed(run.id).await;
    assert!(
        matches!(completed.state.as_str(), "success" | "partial"),
        "accepted push remains confirmed instead of false cancellation"
    );
    assert!(!completed.cancel_requested);
    assert_eq!(completed.connection_version, run.connection_version);
    assert_eq!(completed.settings, run.settings);
    assert_eq!(generation(&f, run.id).await, 2);
    f.ref_result(run.id, &name, "applied").await;
    f.same(&name, &sha).await;
    assert_eq!(
        lifecycle(&f, namespace.command.resource.resource_id).await,
        "archived"
    );
    let mut after = f.remote.as_ref().unwrap().refs(true).await.unwrap();
    assert_eq!(after.remove(&name).as_deref(), Some(sha.as_str()));
    assert_eq!(after, remote_before);
    assert_eq!(lines(&control, "effects"), 1);
    assert!(!control.join("release").exists());
    assert_eq!(f.store.runs(f.repository, 20, 0).await.unwrap().1, 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM pipelines WHERE project_id=$1")
            .bind(f.project)
            .fetch_one(&f.pool)
            .await
            .unwrap(),
        0
    );
    println!(
        "LIVE_NAMESPACE_LOSS_ACCEPTED_RECOVERY_OK provider={:?} prefix={} run={} namespace_backend={} state={} generation=2 effects=1 ci=0 sha={sha}",
        f.settings.provider, f.prefix, run.id, pid, completed.state
    );
    // Final harness cleanup only; the fault and recovery above never kill worker.
    stop(&mut child).await;
    f.pool.close().await;
}

#[tokio::test]
#[ignore = "Requires owned live provider; archive wins real selection/admission gap without Git effects"]
async fn archive_wins_selected_run_admission_and_unarchive_resumes_same_run() {
    let mut f = Live::new().await;
    f.stop_tasks().await;
    f.settings(Direction::Push, false).await;
    let mut namespace = Namespace::attach(&f).await;
    let name = f.head("archive-admission");
    let sha = f.commit(None, "QA archive selected admission").await;
    f.local(&name, &sha).await;
    let run = queue(&f).await;
    // Real execute acquires this shared repository lease before namespace admission.
    // Block it after candidate selection, while archive can still acquire group lock.
    let mut fence = f.pool.begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *fence)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,3))")
        .bind(f.repository.to_string())
        .execute(&mut *fence)
        .await
        .unwrap();
    let mut child = worker(&f, Path::new("/usr/bin")).await;
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)) AND query LIKE '%pg_advisory_xact_lock_shared%')")
                .bind(pid).fetch_one(&f.pool).await.unwrap();
            if blocked { break; }
            assert!(child.try_wait().unwrap().is_none());
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }).await.expect("selected worker reached repository admission lock");
    namespace.change("archived").await;
    fence.commit().await.unwrap();
    // Quiesce the selected process, then independently verify durable paused state.
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(
        f.store.run(f.repository, run.id).await.unwrap().state,
        "queued"
    );
    assert_eq!(generation(&f, run.id).await, 0);
    assert!(f.store.candidates().await.unwrap().is_empty());
    assert_eq!(f.store.refs(run.id, 1000, 0).await.unwrap().1, 0);
    assert!(
        !f.remote
            .as_ref()
            .unwrap()
            .refs(true)
            .await
            .unwrap()
            .contains_key(&name)
    );
    assert_eq!(
        f.request("POST", "/runs", None, Some(Uuid::new_v4()))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        f.request("GET", "/runs", None, None).await.0,
        StatusCode::OK
    );
    namespace.change("active").await;
    assert_eq!(f.completed(run.id).await.state, "success");
    f.same(&name, &sha).await;
    f.ref_result(run.id, &name, "applied").await;
    assert_eq!(f.store.runs(f.repository, 20, 0).await.unwrap().1, 1);
    stop(&mut child).await;
    println!(
        "LIVE_ARCHIVE_ADMISSION_OK provider={:?} prefix={} run={}",
        f.settings.provider, f.prefix, run.id
    );
    f.pool.close().await;
}

#[tokio::test]
#[ignore = "Requires owned live provider; real archive serializes after in-flight HTTPS fetch"]
async fn archive_during_fetch_waits_for_real_namespace_lease() {
    archive_serialization("fetch").await;
}

#[tokio::test]
#[ignore = "Requires owned live provider; real archive serializes after admitted push"]
async fn archive_before_push_waits_for_real_namespace_lease() {
    archive_serialization("push").await;
}

#[tokio::test]
#[ignore = "Requires owned live provider; SIGKILL own workers before claim and after claim before plan"]
async fn crash_after_claim_before_plan_recovers_without_duplicate_effect() {
    use cicd_infra::sync_git::MutationGuard;
    let mut f = Live::new().await;
    f.stop_tasks().await;
    f.settings(Direction::Push, false).await;
    let sha = f.commit(None, "QA crash after claim before plan").await;
    let name = f.head("claim-before-plan");
    f.local(&name, &sha).await;
    let local_before = git(&f.local, &["for-each-ref"]).await;
    let remote_before = f.remote.as_ref().unwrap().refs(true).await.unwrap();
    let control = f.root.join("claim-before-plan");
    std::fs::create_dir(&control).unwrap();
    let _release = OwnedGitGate(control.clone());
    let script = format!(
        r#"#!/bin/sh
control='{}'
case " $* " in
  *" ls-remote "*" https://"*)
    if test -f "$control/arm"; then
      rm "$control/arm"
      echo $$ > "$control/child-pid"
      /usr/bin/git "$@"
      result=$?
      echo "$result" > "$control/read-completed"
      touch "$control/reached"
      while test ! -f "$control/release"; do /bin/sleep 0.1; done
      if test -f "$control/abort-before-effect"; then exit 75; fi
      exit "$result"
    fi ;;
esac
case " $* " in *" push "*" https://"*) echo push >> "$control/pushes" ;; esac
exec /usr/bin/git "$@"
"#,
        control.display()
    );
    std::fs::write(control.join("git"), script).unwrap();
    std::fs::set_permissions(control.join("git"), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(
        control.join("arm"),
        b"hold only first post-claim observation",
    )
    .unwrap();
    let preclaim_guard = MutationGuard::try_acquire(&f.root, f.repository)
        .unwrap()
        .expect("parent owns Git guard before queue and supervisor selection");
    let run = queue(&f).await;
    let mut preclaim = worker(&f, &control).await;
    // The real supervisor probes the held guard before admission, slot and claim.
    // Keep it alive across several normal supervisor cycles before killing it.
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert!(preclaim.try_wait().unwrap().is_none());
    let queued = f.store.run(f.repository, run.id).await.unwrap();
    assert_eq!(queued.state, "queued");
    assert!(queued.started_at.is_none());
    assert_eq!(generation(&f, run.id).await, 0);
    assert_eq!(f.store.refs(run.id, 1000, 0).await.unwrap().1, 0);
    assert!(control.join("arm").exists());
    assert!(!control.join("reached").exists());
    assert_eq!(lines(&control, "pushes"), 0);
    assert_eq!(git(&f.local, &["for-each-ref"]).await, local_before);
    assert_eq!(
        f.remote.as_ref().unwrap().refs(true).await.unwrap(),
        remote_before
    );
    stop(&mut preclaim).await;
    // Verify durable queue survives a real process death before any claim.
    assert_eq!(
        f.store.run(f.repository, run.id).await.unwrap().state,
        "queued"
    );
    assert_eq!(generation(&f, run.id).await, 0);
    assert_eq!(f.store.refs(run.id, 1000, 0).await.unwrap().1, 0);
    assert_eq!(f.store.runs(f.repository, 20, 0).await.unwrap().1, 1);
    assert_eq!(
        f.remote.as_ref().unwrap().refs(true).await.unwrap(),
        remote_before
    );
    println!(
        "LIVE_PRECLAIM_CRASH_OK provider={:?} prefix={} run={} generation=0 refs=0 effects=0",
        f.settings.provider, f.prefix, run.id
    );
    drop(preclaim_guard);
    let mut first = worker(&f, &control).await;
    gate_reached(&control, &mut first).await;
    assert_eq!(
        std::fs::read_to_string(control.join("read-completed"))
            .unwrap()
            .trim(),
        "0"
    );
    let claimed = f.store.run(f.repository, run.id).await.unwrap();
    assert_eq!(claimed.state, "running");
    assert!(claimed.started_at.is_some());
    assert_eq!(generation(&f, run.id).await, 1);
    assert_eq!(f.store.refs(run.id, 1000, 0).await.unwrap().1, 0);
    assert_eq!(
        f.remote.as_ref().unwrap().refs(true).await.unwrap(),
        remote_before
    );
    assert_eq!(lines(&control, "pushes"), 0);
    stop(&mut first).await;
    sqlx::query(
        "UPDATE repository_sync_runs SET lease_until=now()-interval '1 second' WHERE id=$1",
    )
    .bind(run.id)
    .execute(&f.pool)
    .await
    .unwrap();
    let mut second = worker(&f, &control).await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert_eq!(
        generation(&f, run.id).await,
        1,
        "expired lease cannot bypass live orphan Git guard"
    );
    assert!(
        MutationGuard::try_acquire(&f.root, f.repository)
            .unwrap()
            .is_none()
    );
    assert_eq!(f.store.refs(run.id, 1000, 0).await.unwrap().1, 0);
    assert_eq!(lines(&control, "pushes"), 0);
    assert_eq!(
        f.remote.as_ref().unwrap().refs(true).await.unwrap(),
        remote_before
    );
    std::fs::write(
        control.join("release"),
        b"release read-only orphan after ownership proof",
    )
    .unwrap();
    let completed = f.completed(run.id).await;
    assert_eq!(completed.state, "success");
    assert_eq!(completed.id, claimed.id);
    assert_eq!(completed.connection_version, claimed.connection_version);
    assert_eq!(completed.settings, claimed.settings);
    assert_eq!(generation(&f, run.id).await, 2);
    f.ref_result(run.id, &name, "applied").await;
    f.same(&name, &sha).await;
    assert_eq!(git(&f.local, &["for-each-ref"]).await, local_before);
    let mut after = f.remote.as_ref().unwrap().refs(true).await.unwrap();
    assert_eq!(after.remove(&name).as_deref(), Some(sha.as_str()));
    assert_eq!(
        after, remote_before,
        "unrelated provider refs remain identical"
    );
    assert_eq!(lines(&control, "pushes"), 1);
    assert_eq!(f.store.runs(f.repository, 20, 0).await.unwrap().1, 1);
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(lines(&control, "pushes"), 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM pipelines WHERE project_id=$1")
            .bind(f.project)
            .fetch_one(&f.pool)
            .await
            .unwrap(),
        0,
        "outgoing recovery never creates incoming CI events"
    );
    stop(&mut second).await;
    println!(
        "LIVE_CLAIM_BEFORE_PLAN_RECOVERY_OK provider={:?} prefix={} run={} generation=2 push_count=1 ci_count=0 sha={sha}",
        f.settings.provider, f.prefix, run.id
    );
    f.pool.close().await;
}

#[tokio::test]
#[ignore = "Requires owned live provider; disable cancels real HTTPS fetch without releasing the Git gate"]
async fn disable_during_fetch_stops_before_any_git_write() {
    use cicd_infra::sync_git::MutationGuard;
    let mut f = Live::new().await;
    f.stop_tasks().await;
    f.settings(Direction::Push, false).await;
    let namespace = Namespace::attach(&f).await;
    let sha = f.commit(None, "QA disable during HTTPS fetch").await;
    let name = f.head("disable-fetch");
    f.local(&name, &sha).await;
    let local_before = git(&f.local, &["for-each-ref"]).await;
    let remote_before = f.remote.as_ref().unwrap().refs(true).await.unwrap();
    let (_, saved) = f.request("GET", "", None, None).await;
    let credential_before = f.store.credential(f.repository).await.unwrap().unwrap();
    let control = f.root.join("disable-fetch");
    std::fs::create_dir(&control).unwrap();
    let _release = OwnedGitGate(control.clone());
    let script = format!(
        r#"#!/bin/sh
control='{}'
case " $* " in
  *" fetch "*" https://"*)
    echo $$ > "$control/child-pid"
    /usr/bin/git "$@"
    result=$?
    echo "$result" > "$control/read-completed"
    touch "$control/reached"
    while test ! -f "$control/release"; do /bin/sleep 0.1; done
    if test -f "$control/abort-before-effect"; then exit 75; fi
    exit "$result" ;;
esac
case " $* " in *" push "*) echo push >> "$control/pushes" ;; esac
exec /usr/bin/git "$@"
"#,
        control.display()
    );
    std::fs::write(control.join("git"), script).unwrap();
    std::fs::set_permissions(control.join("git"), std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut child = worker(&f, &control).await;
    let run = queue(&f).await;
    gate_reached(&control, &mut child).await;
    assert_eq!(
        std::fs::read_to_string(control.join("read-completed"))
            .unwrap()
            .trim(),
        "0"
    );
    assert!(
        MutationGuard::try_acquire(&f.root, f.repository)
            .unwrap()
            .is_none()
    );
    f.settings.direction = Direction::Disabled;
    let (status, disabling) = f
        .request(
            "PUT",
            "",
            Some(serde_json::json!({"expected_version":saved["version"],"settings":f.settings})),
            Some(Uuid::new_v4()),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(disabling["active_run"]["state"], "stopping");
    assert_eq!(disabling["active_run"]["cancel_requested"], true);
    assert_eq!(disabling["has_credential"], true);
    // Do not release the shell gate: production heartbeat/cancellation must kill
    // its process group and finish independently within the heartbeat budget.
    let completed = tokio::time::timeout(Duration::from_secs(20), f.completed(run.id))
        .await
        .expect("disable interrupts held HTTPS exchange through real heartbeat");
    assert_eq!(completed.state, "canceled");
    assert!(completed.cancel_requested);
    assert_eq!(
        completed.connection_version,
        saved["version"].as_i64().unwrap()
    );
    assert!(!control.join("release").exists());
    assert_eq!(lines(&control, "pushes"), 0);
    assert_eq!(f.store.refs(run.id, 1000, 0).await.unwrap().1, 0);
    assert_eq!(git(&f.local, &["for-each-ref"]).await, local_before);
    assert_eq!(
        f.remote.as_ref().unwrap().refs(true).await.unwrap(),
        remote_before
    );
    assert!(
        MutationGuard::try_acquire(&f.root, f.repository)
            .unwrap()
            .is_some(),
        "cancellation closes Git child guard without gate release"
    );
    let credential_after = f.store.credential(f.repository).await.unwrap().unwrap();
    assert!(
        credential_before.1 == credential_after.1,
        "omitted credential survives disable unchanged"
    );
    let (_, after) = f.request("GET", "", None, None).await;
    assert_eq!(after["settings"]["direction"], "disabled");
    assert_eq!(after["has_credential"], true);
    assert_eq!(after["last_run"]["id"], run.id.to_string());
    assert_eq!(f.store.runs(f.repository, 20, 0).await.unwrap().1, 1);
    assert_eq!(
        lifecycle(&f, namespace.command.resource.resource_id).await,
        "active"
    );
    stop(&mut child).await;
    println!(
        "LIVE_DISABLE_FETCH_OK provider={:?} prefix={} run={} no_gate_release=true",
        f.settings.provider, f.prefix, run.id
    );
    f.pool.close().await;
}

async fn archive_serialization(phase: &str) {
    let mut f = Live::new().await;
    f.stop_tasks().await;
    f.settings(Direction::Push, false).await;
    let mut namespace = Namespace::attach(&f).await;
    let sha = f.commit(None, "QA archive serialization").await;
    let name = f.head(&format!("archive-{phase}"));
    f.local(&name, &sha).await;
    let control = f.root.join("archive-serialization");
    std::fs::create_dir(&control).unwrap();
    let _release = OwnedGitGate(control.clone());
    let script = format!(
        r#"#!/bin/sh
control='{}'
case " $* " in
  *" {} "*" https://"*)
    if test -f "$control/arm"; then
      rm "$control/arm"
      echo $$ > "$control/child-pid"
      touch "$control/reached"
      while test ! -f "$control/release"; do /bin/sleep 0.1; done
      if test -f "$control/abort-before-effect"; then exit 75; fi
    fi ;;
esac
case " $* " in *" push "*" https://"*) echo effect >> "$control/effects" ;; esac
exec /usr/bin/git "$@"
"#,
        control.display(),
        phase
    );
    std::fs::write(control.join("git"), script).unwrap();
    std::fs::set_permissions(control.join("git"), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(control.join("arm"), b"gate own worker only").unwrap();
    let mut child = worker(&f, &control).await;
    let run = queue(&f).await;
    gate_reached(&control, &mut child).await;
    assert_eq!(generation(&f, run.id).await, 1);
    assert!(
        !f.remote
            .as_ref()
            .unwrap()
            .refs(true)
            .await
            .unwrap()
            .contains_key(&name)
    );
    let archive = namespace.archive();
    waiting_archive(&f, &namespace).await;
    assert!(!archive.is_finished());
    assert_eq!(
        lifecycle(&f, namespace.command.resource.resource_id).await,
        "active"
    );
    if phase == "push" {
        f.ref_result(run.id, &name, "planned").await;
    } else {
        assert_eq!(f.store.refs(run.id, 1000, 0).await.unwrap().1, 0);
    }
    std::fs::write(control.join("release"), b"release admitted operation").unwrap();
    assert_eq!(f.completed(run.id).await.state, "success");
    tokio::time::timeout(Duration::from_secs(10), archive)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        lifecycle(&f, namespace.command.resource.resource_id).await,
        "archived"
    );
    f.same(&name, &sha).await;
    f.ref_result(run.id, &name, "applied").await;
    assert_eq!(lines(&control, "effects"), 1);
    assert_eq!(
        f.request("POST", "/runs", None, Some(Uuid::new_v4()))
            .await
            .0,
        StatusCode::CONFLICT
    );
    namespace.change("active").await;
    let replay = queue(&f).await;
    assert_eq!(f.completed(replay.id).await.state, "success");
    f.ref_result(replay.id, &name, "unchanged").await;
    assert_eq!(
        lines(&control, "effects"),
        1,
        "unarchive no-op never repeats push"
    );
    stop(&mut child).await;
    println!(
        "LIVE_ARCHIVE_SERIALIZATION_OK provider={:?} prefix={} run={} phase={phase}",
        f.settings.provider, f.prefix, run.id
    );
    f.pool.close().await;
}

#[tokio::test]
#[ignore = "Requires owned live provider; SIGKILL own worker after incoming effect then archived recovery/unarchive"]
async fn archive_after_unknown_effect_reconciles_without_new_writes_and_delivers_ci_after_unarchive()
 {
    let mut f = Live::new().await;
    f.stop_tasks().await;
    f.settings(Direction::Both, false).await;
    let mut namespace = Namespace::attach(&f).await;
    let sha = f.commit(None, "QA archive accepted incoming effect").await;
    let incoming = f.head("00-archive-accepted");
    let pending = f.head("zz-archive-pending");
    f.external(&incoming, None, &sha).await;
    f.local(&pending, &sha).await;
    let control = f.root.join("archive-recovery");
    std::fs::create_dir(&control).unwrap();
    let _release = OwnedGitGate(control.clone());
    let script = format!(
        r#"#!/bin/sh
control='{}'
case " $* " in
  *" push "*"{}"*)
    if test -f "$control/arm"; then
      rm "$control/arm"
      echo $$ > "$control/child-pid"
      echo incoming >> "$control/incoming-effects"
      /usr/bin/git "$@"
      result=$?
      echo "$result" > "$control/accepted"
      touch "$control/reached"
      while test ! -f "$control/release"; do /bin/sleep 0.1; done
      exit "$result"
    fi ;;
esac
case " $* " in *" push "*" https://"*) echo outgoing >> "$control/outgoing-effects" ;; esac
exec /usr/bin/git "$@"
"#,
        control.display(),
        incoming
    );
    std::fs::write(control.join("git"), script).unwrap();
    std::fs::set_permissions(control.join("git"), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(
        control.join("arm"),
        b"lose acknowledgment after incoming effect",
    )
    .unwrap();
    let mut first = worker(&f, &control).await;
    let run = queue(&f).await;
    gate_reached(&control, &mut first).await;
    assert_eq!(
        std::fs::read_to_string(control.join("accepted"))
            .unwrap()
            .trim(),
        "0"
    );
    f.same(&incoming, &sha).await;
    f.ref_result(run.id, &incoming, "planned").await;
    f.ref_result(run.id, &pending, "planned").await;
    // Policy is enabled only once the worker is held after incoming Git commit.
    sqlx::query(
        "INSERT INTO repository_push_configs(repository_id,configuration_id) VALUES($1,$2)",
    )
    .bind(f.repository)
    .bind(f.project)
    .execute(&f.pool)
    .await
    .unwrap();
    stop(&mut first).await;
    // The dead worker's DB lease is released; the live orphan retains the OS guard.
    namespace.change("archived").await;
    sqlx::query(
        "UPDATE repository_sync_runs SET lease_until=now()-interval '1 second' WHERE id=$1",
    )
    .bind(run.id)
    .execute(&f.pool)
    .await
    .unwrap();
    let mut second = worker(&f, &control).await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert_eq!(generation(&f, run.id).await, 1);
    assert_eq!(lines(&control, "outgoing-effects"), 0);
    std::fs::write(
        control.join("release"),
        b"release orphan only after archive committed",
    )
    .unwrap();
    let completed = f.completed(run.id).await;
    assert_eq!(completed.state, "partial");
    assert!(generation(&f, run.id).await > 1);
    f.ref_result(run.id, &incoming, "applied").await;
    f.ref_result(run.id, &pending, "canceled").await;
    assert!(
        !f.remote
            .as_ref()
            .unwrap()
            .refs(true)
            .await
            .unwrap()
            .contains_key(&pending)
    );
    assert_eq!(lines(&control, "incoming-effects"), 1);
    assert_eq!(lines(&control, "outgoing-effects"), 0);
    tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            let retried: bool = sqlx::query_scalar("SELECT NOT event_delivered AND event_next_attempt_at>now()+interval '30 seconds' FROM repository_sync_refs WHERE run_id=$1 AND ref_name=$2")
                .bind(run.id).bind(&incoming).fetch_one(&f.pool).await.unwrap();
            if retried { break; }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }).await.expect("archived CI delivery attempt preserves event with real retry backoff");
    let reference = f
        .store
        .refs(run.id, 1000, 0)
        .await
        .unwrap()
        .0
        .into_iter()
        .find(|r| r.ref_name == incoming)
        .unwrap();
    assert!(
        !reference.event_delivered,
        "archive preserves durable undelivered incoming event"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM pipelines WHERE project_id=$1 AND git_ref=$2"
        )
        .bind(f.project)
        .bind(incoming.strip_prefix("refs/heads/").unwrap())
        .fetch_one(&f.pool)
        .await
        .unwrap(),
        0
    );
    assert_eq!(
        f.request("GET", &format!("/runs/{}", run.id), None, None)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        f.request("POST", "/runs", None, Some(Uuid::new_v4()))
            .await
            .0,
        StatusCode::CONFLICT
    );
    namespace.change("active").await;
    let resumed = queue(&f).await;
    assert_eq!(f.completed(resumed.id).await.state, "success");
    f.same(&pending, &sha).await;
    f.ref_result(resumed.id, &incoming, "unchanged").await;
    f.ref_result(resumed.id, &pending, "applied").await;
    // Keep the production 60-second delivery retry interval unchanged.
    tokio::time::timeout(Duration::from_secs(75), async {
        loop {
            let refs = f.store.refs(run.id, 1000, 0).await.unwrap().0;
            if refs
                .iter()
                .any(|r| r.ref_name == incoming && r.event_delivered)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("unarchive delivers the retained exact-SHA CI event");
    let shas: Vec<String> =
        sqlx::query_scalar("SELECT commit_sha FROM pipelines WHERE project_id=$1 AND git_ref=$2")
            .bind(f.project)
            .bind(incoming.strip_prefix("refs/heads/").unwrap())
            .fetch_all(&f.pool)
            .await
            .unwrap();
    assert_eq!(shas, vec![sha.clone()]);
    assert_eq!(lines(&control, "incoming-effects"), 1);
    assert_eq!(lines(&control, "outgoing-effects"), 1);
    assert_eq!(f.store.runs(f.repository, 20, 0).await.unwrap().1, 2);
    // Observe another supervisor delivery cycle: no duplicated CI or Git effect.
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM pipelines WHERE project_id=$1 AND git_ref=$2"
        )
        .bind(f.project)
        .bind(incoming.strip_prefix("refs/heads/").unwrap())
        .fetch_one(&f.pool)
        .await
        .unwrap(),
        1
    );
    assert_eq!(lines(&control, "outgoing-effects"), 1);
    stop(&mut second).await;
    println!(
        "LIVE_ARCHIVE_UNKNOWN_RECOVERY_OK provider={:?} prefix={} run={} resumed={} sha={sha}",
        f.settings.provider, f.prefix, run.id, resumed.id
    );
    f.pool.close().await;
}
