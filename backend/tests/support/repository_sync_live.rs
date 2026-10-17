//! Opt-in acceptance against explicitly owned provider repositories. No provider token output.
use super::*;
use cicd_domain::repository_sync::SyncRun;
use cicd_infra::sync_git::GitSession;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};
fn api_segment(value: &str) -> String {
    let mut url = reqwest::Url::parse("https://fixture.invalid/").unwrap();
    url.path_segments_mut().unwrap().clear().push(value);
    url.path().trim_start_matches('/').to_owned()
}

async fn git(path: &Path, args: &[&str]) -> String {
    let output = tokio::process::Command::new("git")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .await
        .unwrap();
    assert!(output.status.success(), "local QA Git operation failed");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

struct Live {
    pool: sqlx::PgPool,
    store: PgSyncStore,
    repository: Uuid,
    project: Uuid,
    root: PathBuf,
    local: PathBuf,
    actor: PathBuf,
    remote: Option<GitSession>,
    initial: BTreeMap<String, String>,
    prefix: String,
    settings: Settings,
    api: axum::Router,
    authorization: String,
    workers: Vec<tokio::task::JoinHandle<()>>,
}
struct ReleaseGate(PathBuf);
impl Drop for ReleaseGate {
    fn drop(&mut self) {
        let _ = std::fs::write(
            self.0.join("abort-before-effect"),
            b"do not write after QA failure",
        );
        let _ = std::fs::write(
            self.0.join("release"),
            b"release own QA child on every exit",
        );
    }
}

#[tokio::test]
#[ignore = "Private helper for the opt-in process crash acceptance test"]
async fn worker_process() {
    use std::io::Read;
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();
    let input: serde_json::Value = serde_json::from_str(&input).unwrap();
    assert_eq!(input["purpose"], "forge-owned-process-fault-fixture");
    let root = PathBuf::from(input["root"].as_str().unwrap());
    assert!(
        root.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("forge-capabilities-")
    );
    let options = sqlx::postgres::PgConnectOptions::from_str(input["database"].as_str().unwrap())
        .unwrap()
        .options([("search_path", input["schema"].as_str().unwrap())]);
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .unwrap();
    let mut config = cicd::config::RuntimeConfig::from_env_source(|_| None, false).unwrap();
    config.git.root = root;
    config.secrets.key = Some([19; 32]);
    config.git_sync.enabled = true;
    config.git_sync.allowed_gitlab_origins = vec![input["origin"].as_str().unwrap().to_owned()];
    cicd::repository_sync::supervisor(pool, config).await;
}

impl Live {
    async fn stop_tasks(&mut self) {
        for worker in self.workers.drain(..) {
            worker.abort();
            let _ = worker.await;
        }
    }
    async fn process(&self, path: &Path) -> tokio::process::Child {
        use tokio::io::AsyncWriteExt;
        let schema: String = sqlx::query_scalar("SELECT current_schema()")
            .fetch_one(&self.pool)
            .await
            .unwrap();
        let input = serde_json::json!({"purpose":"forge-owned-process-fault-fixture","root":self.root,"origin":self.settings.instance_url,"database":std::env::var("CICD_TEST_DATABASE_URL").unwrap(),"schema":schema});
        let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "repository_sync::live::worker_process",
                "--ignored",
                "--nocapture",
            ])
            .env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", path.display()))
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
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "Requires owned live provider repository; kills only its own QA worker process"]
async fn actual_worker_crash_with_live_git_child_and_lost_ack() {
    process_recovery(false, false).await;
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "Requires owned live provider repository; disables only its own QA connection after remote acceptance"]
async fn disable_after_accepted_push_preserves_actual_result() {
    process_recovery(true, false).await;
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "Requires owned live provider repository; kills only its own worker before remote effect"]
async fn crash_before_push_retries_only_after_old_sha_observation() {
    process_recovery(false, true).await;
}
#[cfg(unix)]
#[tokio::test]
#[ignore = "Requires owned live provider repository; stops only its own queued Git effect"]
async fn disable_before_push_never_sends_changes() {
    process_recovery(true, true).await;
}

#[cfg(unix)]
async fn process_recovery(disable: bool, before: bool) {
    use std::os::unix::fs::PermissionsExt;
    let mut f = Live::new().await;
    f.stop_tasks().await;
    f.settings(Direction::Push, false).await;
    let sha = f.commit(None, "QA actual worker crash").await;
    let name = f.head("lost-ack-process");
    f.local(&name, &sha).await;
    let controls = f.root.join("fault-control");
    std::fs::create_dir(&controls).unwrap();
    let _release_on_failure = ReleaseGate(controls.clone());
    let shim = controls.join("git");
    let script = format!(
        r#"#!/bin/sh
control='{}'
case " $* " in
  *" https://"*) if test -f "$control/offline"; then echo 'QA controlled connection unavailable' >&2; exit 128; fi ;;
esac
case " $* " in
  *" push "*)
    echo push >> "$control/push-count"
    if test -f "$control/arm"; then
      rm "$control/arm"
      echo $$ > "$control/child-pid"
      if test -f "$control/before"; then
        echo gated > "$control/reached"
        while test ! -f "$control/release"; do /bin/sleep 0.1; done
        if test -f "$control/abort-before-effect"; then exit 75; fi
      fi
      echo effect >> "$control/effect-count"
      /usr/bin/git "$@"
      result=$?
      echo "$result" > "$control/accepted"
      echo accepted > "$control/reached"
      while test ! -f "$control/release"; do /bin/sleep 0.1; done
      exit "$result"
    fi ;;
esac
case " $* " in *" push "*) echo effect >> "$control/effect-count" ;; esac
exec /usr/bin/git "$@"
"#,
        controls.display()
    );
    std::fs::write(&shim, script).unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(controls.join("arm"), b"owned QA push only").unwrap();
    if before {
        std::fs::write(controls.join("before"), b"gate before real Git").unwrap();
    }
    let mut first = f.process(&controls).await;
    let (status, body) = f.request("POST", "/runs", None, Some(Uuid::new_v4())).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let run: SyncRun = serde_json::from_value(body).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(90), async {
        while !controls.join("reached").exists() {
            assert!(
                first.try_wait().unwrap().is_none(),
                "QA worker exited before effect"
            );
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    if before {
        assert!(
            !f.remote
                .as_ref()
                .unwrap()
                .refs(true)
                .await
                .unwrap()
                .contains_key(&name)
        );
    } else {
        assert_eq!(
            std::fs::read_to_string(controls.join("accepted"))
                .unwrap()
                .trim(),
            "0"
        );
        f.same(&name, &sha).await;
    }
    first.kill().await.unwrap();
    first.wait().await.unwrap();
    if disable {
        f.settings(Direction::Disabled, false).await;
        assert_eq!(
            f.store.run(f.repository, run.id).await.unwrap().state,
            "stopping"
        );
        assert_eq!(
            f.request("POST", "/runs", None, Some(Uuid::new_v4()))
                .await
                .0,
            StatusCode::CONFLICT
        );
    }
    // Expiry alone must not permit another process to pass the live Git child guard.
    sqlx::query(
        "UPDATE repository_sync_runs SET lease_until=now()-interval '1 second' WHERE id=$1",
    )
    .bind(run.id)
    .execute(&f.pool)
    .await
    .unwrap();
    let mut second = f.process(&controls).await;
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    let generation: i64 =
        sqlx::query_scalar("SELECT generation FROM repository_sync_runs WHERE id=$1")
            .bind(run.id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(
        generation, 1,
        "live orphan Git process retains interprocess mutation guard"
    );
    // Once the child releases its guard, failed observation must stay reconciling.
    std::fs::write(controls.join("offline"), b"temporary QA network fault").unwrap();
    if before {
        std::fs::write(
            controls.join("abort-before-effect"),
            b"old child must not write",
        )
        .unwrap();
    }
    std::fs::write(controls.join("release"), b"release own child").unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(35), async {
        loop {
            let observed = f.store.run(f.repository, run.id).await.unwrap();
            // claim also enters reconciling. Wait for the failed observation to
            // finish before expiring its lease, rather than fencing a live task.
            if observed.state == "reconciling"
                && observed.message.as_deref() == Some("git_network_failed")
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        f.store
            .run(f.repository, run.id)
            .await
            .unwrap()
            .finished_at
            .is_none()
    );
    std::fs::remove_file(controls.join("offline")).unwrap();
    sqlx::query(
        "UPDATE repository_sync_runs SET lease_until=now()-interval '1 second' WHERE id=$1",
    )
    .bind(run.id)
    .execute(&f.pool)
    .await
    .unwrap();
    let completed = terminal(&f.store, f.repository, run.id).await;
    assert_eq!(
        completed.state,
        if disable && before {
            "canceled"
        } else if disable {
            "partial"
        } else {
            "success"
        }
    );
    f.ref_result(
        run.id,
        &name,
        if disable && before {
            "canceled"
        } else {
            "applied"
        },
    )
    .await;
    if disable && before {
        assert!(
            !f.remote
                .as_ref()
                .unwrap()
                .refs(true)
                .await
                .unwrap()
                .contains_key(&name)
        );
    } else {
        f.same(&name, &sha).await;
    }
    second.kill().await.unwrap();
    second.wait().await.unwrap();
    assert_eq!(
        std::fs::read_to_string(controls.join("push-count"))
            .unwrap()
            .lines()
            .count(),
        if before && !disable { 2 } else { 1 },
        "recovery observes the accepted push without repeating it"
    );
    let effects = std::fs::read_to_string(controls.join("effect-count")).unwrap_or_default();
    assert_eq!(
        effects.lines().count(),
        if before && disable { 0 } else { 1 },
        "only the intended Git effect reaches the provider"
    );
    f.pool.close().await;
    println!(
        "LIVE_PROCESS_RECOVERY_OK provider={:?} prefix={} run={} disable={disable} before={before}",
        f.settings.provider, f.prefix, run.id
    );
}
impl Drop for Live {
    fn drop(&mut self) {
        for worker in &self.workers {
            worker.abort();
        }
        self.remote.take();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
impl Live {
    async fn new() -> Self {
        let pool = test_pool().await;
        let repository = Uuid::new_v4();
        let project = Uuid::new_v4();
        let account = Uuid::new_v4();
        let prefix = format!("qa/{}", Uuid::new_v4().simple());
        let root = std::env::temp_dir().join(format!("forge-capabilities-{repository}"));
        std::fs::create_dir(&root).unwrap();
        let name = format!("sync-{}", repository.simple());
        let local = root.join(format!("{name}.git"));
        let actor = root.join("actor.git");
        for path in [&local, &actor] {
            std::fs::create_dir(path).unwrap();
            git(path, &["init", "--bare", "--initial-branch=main"]).await;
        }
        sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
            .bind(repository)
            .bind(&name)
            .execute(&pool)
            .await
            .unwrap();
        // These provider/fault cases exercise unprotected refs. The default has
        // its own negative regression and remains protected independently.
        sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id,default_branch) VALUES($1,$2,'https://forge.invalid/disposable',$3,'qa-protected-default')").bind(project).bind(format!("qa-{project}")).bind(repository).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO service_accounts(id,name) VALUES($1,$2)")
            .bind(account)
            .bind(format!("qa-{account}"))
            .execute(&pool)
            .await
            .unwrap();
        let authorization = format!("forge_sat_fixture_{}", Uuid::new_v4().simple());
        sqlx::query("INSERT INTO api_tokens(id,name,token_hash,token_hint,principal_type,service_account_id,project_id,scopes) VALUES($1,'QA sync', $2,'fixture','service_account',$3,$4,ARRAY['api:read','api:write','git:read','git:write'])").bind(Uuid::new_v4()).bind(cicd::auth::hash_token(&authorization)).bind(account).bind(project).execute(&pool).await.unwrap();
        let provider = std::env::var("CICD_SYNC_LIVE_PROVIDER").unwrap();
        let origin = std::env::var("CICD_SYNC_LIVE_ORIGIN").unwrap();
        let path = std::env::var("CICD_SYNC_LIVE_REPOSITORY").unwrap();
        assert!(
            path.rsplit('/')
                .next()
                .unwrap()
                .starts_with("forge-sync-qa-")
        );
        let token = std::env::var("CICD_SYNC_LIVE_TOKEN").expect("credential supplied privately");
        let mut settings = Settings {
            provider: if provider == "github" {
                Provider::Github
            } else {
                Provider::Gitlab
            },
            instance_url: origin.clone(),
            repository_path: path,
            direction: Direction::Disabled,
            automatic: false,
        };
        let mut config = cicd::config::RuntimeConfig::from_env_source(|_| None, false)
            .unwrap()
            .with_auth_secret(Some("fixture-live-api-secret".into()));
        config.git.root = root.clone();
        config.secrets.key = Some([19; 32]);
        config.git_sync.enabled = true;
        config.git_sync.interval_seconds = 300;
        if provider == "gitlab" {
            config.git_sync.allowed_gitlab_origins = vec![origin];
        }
        settings
            .validate(&config.git_sync.allowed_gitlab_origins)
            .unwrap();
        println!("LIVE_FIXTURE provider={provider} prefix={prefix} repository_id={repository}");
        let remote = GitSession::new(
            &root,
            actor.clone(),
            settings.clone(),
            token.clone(),
            Uuid::new_v4(),
            1,
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .unwrap();
        let initial = remote.refs(true).await.unwrap();
        let store = PgSyncStore {
            pool: pool.clone(),
            worker_enabled: true,
            interval_seconds: 300,
        };
        let api = cicd::api::app_with_git_and_config(
            Some(pool.clone()),
            config.git.to_git_config(),
            None,
            config.clone(),
        )
        .unwrap();
        let mut value = Self {
            pool,
            store,
            repository,
            project,
            root,
            local,
            actor,
            remote: Some(remote),
            initial,
            prefix,
            settings,
            api,
            authorization,
            workers: Vec::new(),
        };
        let (_, body) = value.request("GET", "", None, None).await;
        assert_eq!(body["version"], 0);
        assert_eq!(body["settings"]["direction"], "disabled");
        assert_eq!(body["settings"]["automatic"], false);
        assert_eq!(body["writable"], true);
        let (status, body) = value.request("PUT", "", Some(serde_json::json!({"expected_version":0,"settings":value.settings,"token":token})),Some(Uuid::new_v4())).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["has_credential"], true);
        assert!(body.get("token").is_none());
        let (status, _) = value
            .request("POST", "/runs", None, Some(Uuid::new_v4()))
            .await;
        assert_eq!(status, StatusCode::CONFLICT);
        for _ in 0..2 {
            value
                .workers
                .push(tokio::spawn(cicd::repository_sync::supervisor(
                    value.pool.clone(),
                    config.clone(),
                )));
        }
        value
    }
    async fn request(
        &self,
        method: &str,
        suffix: &str,
        body: Option<serde_json::Value>,
        operation: Option<Uuid>,
    ) -> (StatusCode, serde_json::Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(format!(
                "/api/v1/catalog/repositories/{}/sync{suffix}",
                self.repository
            ))
            .header("authorization", format!("Bearer {}", self.authorization));
        if let Some(operation) = operation {
            request = request.header("Idempotency-Key", operation.to_string());
        }
        if body.is_some() {
            request = request.header("content-type", "application/json");
        }
        let response = self
            .api
            .clone()
            .oneshot(
                request
                    .body(
                        body.map(|value| Body::from(value.to_string()))
                            .unwrap_or_else(Body::empty),
                    )
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        (status, response_json(response).await)
    }
    async fn settings(&mut self, direction: Direction, automatic: bool) {
        let (_, saved) = self.request("GET", "", None, None).await;
        self.settings.direction = direction;
        self.settings.automatic = automatic;
        let (status,_)=self.request("PUT","",Some(serde_json::json!({"expected_version":saved["version"],"settings":self.settings})),Some(Uuid::new_v4())).await;
        assert_eq!(status, StatusCode::OK);
        println!(
            "LIVE_MODE provider={:?} prefix={} direction={direction:?} automatic={automatic}",
            self.settings.provider, self.prefix
        );
    }
    async fn manual(&self) -> SyncRun {
        let key = Uuid::new_v4();
        let (status, body) = self.request("POST", "/runs", None, Some(key)).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        let run: SyncRun = serde_json::from_value(body.clone()).unwrap();
        let (status, replayed) = self.request("POST", "/runs", None, Some(key)).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(replayed["id"], body["id"]);
        let completed = self.completed(run.id).await;
        println!(
            "LIVE_RUN provider={:?} prefix={} run={} state={}",
            self.settings.provider, self.prefix, completed.id, completed.state
        );
        completed
    }
    async fn completed(&self, run: Uuid) -> SyncRun {
        tokio::time::timeout(std::time::Duration::from_secs(360), async {
            loop {
                let current = self.store.run(self.repository, run).await.unwrap();
                if current.finished_at.is_some() {
                    return current;
                }
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        })
        .await
        .expect("Git execution reaches a terminal result within its command budget")
    }
    async fn commit(&self, parent: Option<&str>, label: &str) -> String {
        let tree = git(&self.actor, &["mktree"]).await;
        let mut args = vec![
            "-c",
            "user.name=Forge QA",
            "-c",
            "user.email=qa@example.invalid",
            "commit-tree",
            &tree,
            "-m",
            label,
        ];
        if let Some(parent) = parent {
            args.extend(["-p", parent]);
        }
        git(&self.actor, &args).await
    }
    fn head(&self, name: &str) -> String {
        format!("refs/heads/{}/{name}", self.prefix)
    }
    async fn external(&self, reference: &str, old: Option<&str>, sha: &str) {
        assert!(reference.contains(&self.prefix));
        git(&self.actor, &["update-ref", reference, sha]).await;
        let remote = self.remote.as_ref().unwrap();
        remote.fetch().await.unwrap();
        remote.apply(true, reference, old, sha).await.unwrap();
    }
    async fn local(&self, reference: &str, sha: &str) {
        assert!(reference.contains(&self.prefix));
        git(
            &self.actor,
            &[
                "push",
                self.local.to_str().unwrap(),
                &format!("{sha}:{reference}"),
            ],
        )
        .await;
    }
    async fn ref_result(&self, run: Uuid, name: &str, state: &str) {
        let (status, body) = self
            .request("GET", &format!("/runs/{run}/refs?limit=1000"), None, None)
            .await;
        assert_eq!(status, StatusCode::OK);
        let item = body["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["ref_name"] == name)
            .expect("ref result exists");
        assert_eq!(item["state"], state, "per-ref outcome");
    }
    async fn same(&self, name: &str, sha: &str) {
        assert_eq!(
            git(&self.local, &["rev-parse", "--verify", name]).await,
            sha
        );
        assert_eq!(
            self.remote
                .as_ref()
                .unwrap()
                .refs(true)
                .await
                .unwrap()
                .get(name)
                .map(String::as_str),
            Some(sha)
        );
    }
    async fn automatic(&self, previous: i64, source: &str, seconds: u64) -> SyncRun {
        let run = tokio::time::timeout(std::time::Duration::from_secs(seconds), async {
            loop {
                let (runs, total) = self.store.runs(self.repository, 100, 0).await.unwrap();
                if total > previous {
                    let run = runs
                        .into_iter()
                        .find(|run| run.source == source)
                        .expect("expected automatic source");
                    return run;
                }
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        })
        .await
        .expect("automatic run is queued within configured interval");
        println!(
            "LIVE_AUTOMATIC_QUEUED provider={:?} prefix={} run={} source={source}",
            self.settings.provider, self.prefix, run.id
        );
        // Scheduling latency and Git completion are separate contracts. A Git
        // command has its own 300s bound; do not destroy a valid queued run at 330s.
        self.completed(run.id).await
    }
}

#[tokio::test]
#[ignore = "Requires explicitly owned GitHub/GitLab QA repository and protected stdin credential"]
async fn all_directions_modes_tags_conflicts_and_actual_schedule() {
    let mut f = Live::new().await;
    let a = f.commit(None, "QA root").await;
    let b = f.commit(Some(&a), "QA fast forward").await;
    let c = f.commit(Some(&a), "QA diverged").await;
    let incoming = f.head("incoming");
    f.external(&incoming, None, &a).await;
    f.settings(Direction::Pull, false).await;
    let run = f.manual().await;
    assert_eq!(run.state, "success");
    f.ref_result(run.id, &incoming, "applied").await;
    f.same(&incoming, &a).await;
    f.external(&incoming, Some(&a), &b).await;
    let run = f.manual().await;
    f.ref_result(run.id, &incoming, "applied").await;
    f.same(&incoming, &b).await;
    let outgoing = f.head("outgoing");
    f.local(&outgoing, &a).await;
    let run = f.manual().await;
    f.ref_result(run.id, &outgoing, "unchanged").await;
    assert!(
        !f.remote
            .as_ref()
            .unwrap()
            .refs(true)
            .await
            .unwrap()
            .contains_key(&outgoing)
    );
    f.settings(Direction::Push, false).await;
    let run = f.manual().await;
    f.ref_result(run.id, &outgoing, "applied").await;
    f.same(&outgoing, &a).await;
    f.local(&outgoing, &b).await;
    let run = f.manual().await;
    f.ref_result(run.id, &outgoing, "applied").await;
    f.same(&outgoing, &b).await;
    f.settings(Direction::Both, false).await;
    let remote_only = f.head("remote-only");
    let local_only = f.head("local-only");
    f.external(&remote_only, None, &a).await;
    f.local(&local_only, &a).await;
    let run = f.manual().await;
    assert_eq!(run.state, "success");
    f.same(&remote_only, &a).await;
    f.same(&local_only, &a).await;
    f.external(&remote_only, Some(&a), &b).await;
    f.local(&local_only, &b).await;
    let run = f.manual().await;
    assert_eq!(run.state, "success");
    f.same(&remote_only, &b).await;
    f.same(&local_only, &b).await;
    let run = f.manual().await;
    assert_eq!(run.state, "success");
    assert!(
        f.store
            .refs(run.id, 1000, 0)
            .await
            .unwrap()
            .0
            .iter()
            .all(|r| r.state == "unchanged")
    );
    let tag = format!("{}/annotated", f.prefix);
    git(
        &f.actor,
        &[
            "-c",
            "user.name=Forge QA",
            "-c",
            "user.email=qa@example.invalid",
            "tag",
            "-a",
            &tag,
            &b,
            "-m",
            "QA annotation",
        ],
    )
    .await;
    let tag_ref = format!("refs/tags/{tag}");
    let tag_sha = git(&f.actor, &["rev-parse", &tag_ref]).await;
    f.external(&tag_ref, None, &tag_sha).await;
    let run = f.manual().await;
    f.ref_result(run.id, &tag_ref, "applied").await;
    f.same(&tag_ref, &tag_sha).await;
    assert_eq!(git(&f.local, &["cat-file", "-t", &tag_sha]).await, "tag");
    let (_, before_check) = f.request("GET", "", None, None).await;
    let before_refs = f.remote.as_ref().unwrap().refs(true).await.unwrap();
    let checks = f.root.join(".forge-sync-checks/.forge-sync-scratch");
    let orphan = checks.join(format!("{}-0-{}", f.repository, Uuid::new_v4()));
    let foreign = checks.join(format!("{}-0-{}", Uuid::new_v4(), Uuid::new_v4()));
    std::fs::create_dir_all(&orphan).unwrap();
    std::fs::create_dir_all(&foreign).unwrap();
    std::fs::write(orphan.join("marker"), b"own interrupted check").unwrap();
    std::fs::write(foreign.join("marker"), b"other repository scratch").unwrap();
    let (status, checked) = f
        .request(
            "POST",
            "/check",
            Some(serde_json::json!({"settings":f.settings})),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(checked["read"], true);
    assert!(!orphan.exists());
    assert!(foreign.join("marker").exists());
    let (_, after_check) = f.request("GET", "", None, None).await;
    assert_eq!(before_check["version"], after_check["version"]);
    assert_eq!(
        before_refs,
        f.remote.as_ref().unwrap().refs(true).await.unwrap()
    );
    let (status, _) = f
        .request(
            "POST",
            "/check",
            Some(serde_json::json!({"settings":f.settings,"token":"fictional-invalid-qa-token"})),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    // Source deletion does not delete the destination; both restores the missing ref.
    git(&f.local, &["update-ref", "-d", &local_only, &b]).await;
    f.settings(Direction::Push, false).await;
    let run = f.manual().await;
    f.ref_result(run.id, &local_only, "unchanged").await;
    assert_eq!(
        f.remote.as_ref().unwrap().refs(true).await.unwrap()[&local_only],
        b
    );
    f.settings(Direction::Both, false).await;
    f.manual().await;
    f.same(&local_only, &b).await;
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM repository_sync_refs ref JOIN repository_sync_runs run ON run.id=ref.run_id WHERE run.repository_id=$1 AND ref.direction='pull' AND ref.state='applied' AND NOT ref.event_delivered").bind(f.repository).fetch_one(&f.pool).await.unwrap();
            if pending == 0 {break;}
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }).await.unwrap();
    // Incoming CI binds the event SHA even when the branch advances before delivery.
    sqlx::query(
        "INSERT INTO repository_push_configs(repository_id,configuration_id) VALUES($1,$2)",
    )
    .bind(f.repository)
    .bind(f.project)
    .execute(&f.pool)
    .await
    .unwrap();
    let ci = f.head("ci");
    f.external(&ci, None, &a).await;
    let ci_run = f.manual().await;
    f.local(&ci, &b).await;
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let shas: Vec<String> = sqlx::query_scalar(
                "SELECT commit_sha FROM pipelines WHERE project_id=$1 AND git_ref=$2",
            )
            .bind(f.project)
            .bind(ci.strip_prefix("refs/heads/").unwrap())
            .fetch_all(&f.pool)
            .await
            .unwrap();
            if !shas.is_empty() {
                assert_eq!(shas, vec![a.clone()]);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    let response=f.api.clone().oneshot(Request::post("/api/v1/internal/git-push").header("content-type","application/json").body(Body::from(serde_json::json!({"repository_id":f.repository,"repository":f.local.file_stem().unwrap().to_str().unwrap(),"ref_name":ci,"new_rev":a}).to_string())).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM pipelines WHERE project_id=$1")
            .bind(f.project)
            .fetch_one(&f.pool)
            .await
            .unwrap(),
        1
    );
    let blob = git(&f.actor, &["hash-object", "-w", "--stdin"]).await;
    let blob_tag = format!("refs/tags/{}/blob", f.prefix);
    f.external(&blob_tag, None, &blob).await;
    let blob_run = f.manual().await;
    f.ref_result(blob_run.id, &blob_tag, "applied").await;
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            let rows = f.store.refs(blob_run.id, 1000, 0).await.unwrap().0;
            if rows
                .iter()
                .any(|r| r.ref_name == blob_tag && r.reason.as_deref() == Some("ci_non_commit_tag"))
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM pipelines WHERE project_id=$1")
            .bind(f.project)
            .fetch_one(&f.pool)
            .await
            .unwrap(),
        1
    );
    assert!(
        f.store
            .refs(ci_run.id, 1000, 0)
            .await
            .unwrap()
            .0
            .iter()
            .find(|r| r.ref_name == ci)
            .unwrap()
            .event_delivered
    );
    sqlx::query("UPDATE repository_push_configs SET configuration_id=NULL WHERE repository_id=$1")
        .bind(f.repository)
        .execute(&f.pool)
        .await
        .unwrap();
    // Automatic pull uses the real default interval, not a backdated DB timestamp.
    f.settings(Direction::Pull, true).await;
    let previous = f.store.runs(f.repository, 1, 0).await.unwrap().1;
    let scheduled = f.head("scheduled");
    f.external(&scheduled, None, &a).await;
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    assert_eq!(
        f.store.runs(f.repository, 1, 0).await.unwrap().1,
        previous,
        "save must not perform a hidden launch"
    );
    let run = f.automatic(previous, "schedule", 330).await;
    f.ref_result(run.id, &scheduled, "applied").await;
    f.same(&scheduled, &a).await;
    for direction in [Direction::Push, Direction::Both] {
        f.settings(direction, true).await;
        let previous = f.store.runs(f.repository, 1, 0).await.unwrap().1;
        let name = f.head(if direction == Direction::Push {
            "automatic-push"
        } else {
            "automatic-both"
        });
        f.local(&name, &b).await;
        // The existing post-receive HTTP ingress signals the real scheduler.
        let response=f.api.clone().oneshot(Request::post("/api/v1/internal/git-push").header("content-type","application/json").body(Body::from(serde_json::json!({"repository_id":f.repository,"repository":f.local.file_stem().unwrap().to_str().unwrap(),"ref_name":name,"new_rev":b}).to_string())).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let run = f.automatic(previous, "git_change", 30).await;
        f.ref_result(run.id, &name, "applied").await;
        f.same(&name, &b).await;
        let manual = f.manual().await;
        assert_eq!(
            manual.state, "success",
            "manual remains available in automatic mode"
        );
    }
    f.settings(Direction::Both, false).await;
    let protected = f.head("protected");
    f.external(&protected, None, &a).await;
    sqlx::query("UPDATE projects SET protected_branches=ARRAY[$2] WHERE id=$1")
        .bind(f.project)
        .bind(protected.strip_prefix("refs/heads/").unwrap())
        .execute(&f.pool)
        .await
        .unwrap();
    let run = f.manual().await;
    f.ref_result(run.id, &protected, "blocked").await;
    let protected_output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(&f.local)
        .args(["rev-parse", "--verify", &protected])
        .output()
        .await
        .unwrap();
    assert!(!protected_output.status.success());
    let conflict = f.head("diverged");
    f.external(&conflict, None, &b).await;
    f.local(&conflict, &c).await;
    let run = f.manual().await;
    f.ref_result(run.id, &conflict, "conflict").await;
    assert_eq!(git(&f.local, &["rev-parse", &conflict]).await, c);
    assert_eq!(
        f.remote.as_ref().unwrap().refs(true).await.unwrap()[&conflict],
        b
    );
    let replacement = format!("{}/different-annotation", f.prefix);
    git(
        &f.actor,
        &[
            "-c",
            "user.name=Forge QA",
            "-c",
            "user.email=qa@example.invalid",
            "tag",
            "-a",
            &replacement,
            &b,
            "-m",
            "Different annotation on same commit",
        ],
    )
    .await;
    let replacement_sha = git(
        &f.actor,
        &["rev-parse", &format!("refs/tags/{replacement}")],
    )
    .await;
    git(
        &f.local,
        &[
            "fetch",
            "--no-tags",
            "--no-write-fetch-head",
            f.actor.to_str().unwrap(),
            &replacement_sha,
        ],
    )
    .await;
    git(
        &f.local,
        &["update-ref", &tag_ref, &replacement_sha, &tag_sha],
    )
    .await;
    let run = f.manual().await;
    f.ref_result(run.id, &tag_ref, "conflict").await;
    assert_eq!(
        f.remote.as_ref().unwrap().refs(true).await.unwrap()[&tag_ref],
        tag_sha
    );
    // The provider enforces its own protection at the real push boundary.
    let provider_branch = f.head("provider-protected");
    f.external(&provider_branch, None, &a).await;
    f.local(&provider_branch, &b).await;
    f.settings(Direction::Push, false).await;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let credential = std::env::var("CICD_SYNC_LIVE_TOKEN").unwrap();
    let branch = provider_branch.strip_prefix("refs/heads/").unwrap();
    let encoded = api_segment(branch);
    let (create, delete) = if f.settings.provider == Provider::Gitlab {
        let project = api_segment(&f.settings.repository_path);
        let base = format!(
            "{}/api/v4/projects/{project}/protected_branches",
            f.settings.instance_url
        );
        (client.post(&base).header("PRIVATE-TOKEN",&credential).json(&serde_json::json!({"name":branch,"push_access_level":0,"merge_access_level":0})),client.delete(format!("{base}/{encoded}")).header("PRIVATE-TOKEN",&credential))
    } else {
        let base = format!(
            "https://api.github.com/repos/{}/branches/{encoded}/protection",
            f.settings.repository_path
        );
        (client.put(&base).bearer_auth(&credential).header("User-Agent","Forge-owned-QA").json(&serde_json::json!({"required_status_checks":null,"enforce_admins":true,"required_pull_request_reviews":{"required_approving_review_count":1},"restrictions":null})),client.delete(&base).bearer_auth(&credential).header("User-Agent","Forge-owned-QA"))
    };
    let status = create.send().await.unwrap().status();
    if status.is_success() {
        let protected_run = f.manual().await;
        assert!(
            delete.send().await.unwrap().status().is_success(),
            "remove only own temporary provider protection"
        );
        f.ref_result(protected_run.id, &provider_branch, "blocked")
            .await;
        assert_eq!(
            f.remote.as_ref().unwrap().refs(true).await.unwrap()[&provider_branch],
            a
        );
        println!(
            "LIVE_PROVIDER_PROTECTION_OK provider={:?}",
            f.settings.provider
        );
    } else {
        assert!(
            f.settings.provider == Provider::Github && status == reqwest::StatusCode::FORBIDDEN,
            "provider protection fixture creation failed: {status}"
        );
        println!("LIVE_LIMIT provider=Github case=private_provider_protection status=403");
    }
    let (_, history) = f.request("GET", "/runs?limit=2&offset=2", None, None).await;
    assert_eq!(history["items"].as_array().unwrap().len(), 2);
    let history_ids: Vec<_> = history["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].clone())
        .collect();
    assert_ne!(history_ids[0], history_ids[1]);
    f.settings(Direction::Disabled, false).await;
    let current = f.remote.as_ref().unwrap().refs(true).await.unwrap();
    for (name, sha) in &f.initial {
        assert_eq!(
            current.get(name),
            Some(sha),
            "pre-existing provider refs remain intact"
        );
    }
    for worker in f.workers.drain(..) {
        worker.abort();
        let _ = worker.await;
    }
    f.pool.close().await;
    println!(
        "LIVE_MATRIX_OK provider={:?} prefix={} repository_id={}",
        f.settings.provider, f.prefix, f.repository
    );
}

#[tokio::test]
#[ignore = "Requires GitLab QA project owner; creates and revokes project-scoped Git-only tokens"]
async fn gitlab_git_only_scopes_and_revoked_credentials() {
    let mut f = Live::new().await;
    assert_eq!(f.settings.provider, Provider::Gitlab);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let owner = std::env::var("CICD_SYNC_LIVE_TOKEN").unwrap();
    let project = api_segment(&f.settings.repository_path);
    let endpoint = format!(
        "{}/api/v4/projects/{project}/access_tokens",
        f.settings.instance_url
    );
    let mut credentials = Vec::new();
    for (scope, level) in [("read_repository", 20), ("write_repository", 30)] {
        let response=client.post(&endpoint).header("PRIVATE-TOKEN",&owner).json(&serde_json::json!({"name":format!("forge-qa-{}-{scope}",f.prefix.replace('/',"-")),"scopes":[scope],"access_level":level,"expires_at":(Utc::now()+Duration::days(1)).format("%Y-%m-%d").to_string()})).send().await.unwrap();
        assert!(
            response.status().is_success(),
            "create minimal QA token status={}",
            response.status()
        );
        let value: serde_json::Value = response.json().await.unwrap();
        let id = value["id"].as_i64().unwrap();
        println!(
            "LIVE_OWNED_TOKEN_ID project={} token_id={id}",
            f.settings.repository_path
        );
        credentials.push((id, value["token"].as_str().unwrap().to_owned()));
    }
    let a = f.commit(None, "QA minimal Git scopes").await;
    let incoming = f.head("git-scope-read");
    f.external(&incoming, None, &a).await;
    f.settings(Direction::Pull, false).await;
    async fn rotate(f: &Live, credential: &str) {
        let (_, saved) = f.request("GET", "", None, None).await;
        let (status,_)=f.request("PUT","",Some(serde_json::json!({"expected_version":saved["version"],"settings":f.settings,"token":credential})),Some(Uuid::new_v4())).await;
        assert_eq!(status, StatusCode::OK);
    }
    rotate(&f, &credentials[0].1).await;
    let read_run = f.manual().await;
    f.same(&incoming, &a).await;
    f.settings(Direction::Push, false).await;
    let outgoing = f.head("git-scope-write");
    f.local(&outgoing, &a).await;
    let denied = f.manual().await;
    rotate(&f, &credentials[1].1).await;
    let write_run = f.manual().await;
    f.same(&outgoing, &a).await;
    for (id, _) in &credentials {
        assert!(
            client
                .delete(format!("{endpoint}/{id}"))
                .header("PRIVATE-TOKEN", &owner)
                .send()
                .await
                .unwrap()
                .status()
                .is_success(),
            "revoke own project-scoped QA token"
        );
    }
    let (revoked, _) = f
        .request(
            "POST",
            "/check",
            Some(serde_json::json!({"settings":f.settings,"token":credentials[1].1})),
            None,
        )
        .await;
    assert_eq!(read_run.state, "success");
    assert_eq!(write_run.state, "success");
    assert_eq!(denied.state, "failed");
    f.ref_result(denied.id, &outgoing, "failed").await;
    assert_eq!(revoked, StatusCode::SERVICE_UNAVAILABLE);
    f.stop_tasks().await;
    f.pool.close().await;
    println!("LIVE_GIT_ONLY_SCOPES_OK prefix={}", f.prefix);
}

#[tokio::test]
#[ignore = "Requires an owned GitHub QA repository and a separate Contents:Read token via protected stdin"]
async fn github_read_only_credential_cannot_push() {
    let mut f = Live::new().await;
    assert_eq!(f.settings.provider, Provider::Github);
    let read_token = std::env::var("CICD_SYNC_LIVE_READ_TOKEN")
        .expect("separate read-only credential supplied privately");
    let sha = f.commit(None, "QA GitHub Contents Read").await;
    let incoming = f.head("contents-read");
    f.external(&incoming, None, &sha).await;
    f.settings(Direction::Pull, false).await;
    let (_, saved) = f.request("GET", "", None, None).await;
    assert_eq!(f.request("PUT","",Some(serde_json::json!({"expected_version":saved["version"],"settings":f.settings,"token":read_token})),Some(Uuid::new_v4())).await.0,StatusCode::OK);
    assert_eq!(f.manual().await.state, "success");
    f.same(&incoming, &sha).await;
    f.settings(Direction::Push, false).await;
    let outgoing = f.head("contents-write-denied");
    f.local(&outgoing, &sha).await;
    let denied = f.manual().await;
    f.ref_result(denied.id, &outgoing, "failed").await;
    assert_eq!(denied.state, "failed");
    assert!(
        !f.remote
            .as_ref()
            .unwrap()
            .refs(true)
            .await
            .unwrap()
            .contains_key(&outgoing)
    );
    let (_, saved) = f.request("GET", "", None, None).await;
    assert_eq!(f.request("PUT","",Some(serde_json::json!({"expected_version":saved["version"],"settings":f.settings,"token":std::env::var("CICD_SYNC_LIVE_TOKEN").unwrap()})),Some(Uuid::new_v4())).await.0,StatusCode::OK);
    assert_eq!(f.manual().await.state, "success");
    f.same(&outgoing, &sha).await;
    f.stop_tasks().await;
    f.pool.close().await;
    println!("LIVE_GITHUB_READ_ONLY_OK prefix={}", f.prefix);
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "Requires owned provider QA repository; changes only its own remote ref between plan and push"]
async fn concurrent_external_change_is_never_overwritten() {
    use std::os::unix::fs::PermissionsExt;
    let mut f = Live::new().await;
    f.stop_tasks().await;
    f.settings(Direction::Push, false).await;
    let intended = f.commit(None, "QA intended destination").await;
    let concurrent = f.commit(None, "QA concurrent external writer").await;
    let name = f.head("concurrent-external");
    f.local(&name, &intended).await;
    let controls = f.root.join("concurrent-control");
    std::fs::create_dir(&controls).unwrap();
    let _release = ReleaseGate(controls.clone());
    let script = format!(
        "#!/bin/sh\ncase \" $* \" in *\" push \"*) touch '{0}/reached'; while test ! -f '{0}/release'; do /bin/sleep 0.1; done; if test -f '{0}/abort-before-effect'; then exit 75; fi ;; esac\nexec /usr/bin/git \"$@\"\n",
        controls.display()
    );
    let shim = controls.join("git");
    std::fs::write(&shim, script).unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut worker = f.process(&controls).await;
    let (status, body) = f.request("POST", "/runs", None, Some(Uuid::new_v4())).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let run: SyncRun = serde_json::from_value(body).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(90), async {
        while !controls.join("reached").exists() {
            assert!(worker.try_wait().unwrap().is_none());
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    f.external(&name, None, &concurrent).await;
    std::fs::write(controls.join("release"), b"resume only own Git process").unwrap();
    let completed = f.completed(run.id).await;
    assert_eq!(completed.state, "conflict");
    f.ref_result(run.id, &name, "conflict").await;
    assert_eq!(
        f.remote.as_ref().unwrap().refs(true).await.unwrap()[&name],
        concurrent
    );
    assert_eq!(git(&f.local, &["rev-parse", &name]).await, intended);
    worker.kill().await.unwrap();
    worker.wait().await.unwrap();
    f.pool.close().await;
    println!(
        "LIVE_EXTERNAL_RACE_OK provider={:?} prefix={} run={}",
        f.settings.provider, f.prefix, run.id
    );
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "Requires owned provider QA repository; uses real Git HTTP receive-pack, generated hook and PR merge"]
async fn actual_receive_pack_and_pr_merge_signal_automatic_sync() {
    use std::io::Write;
    use std::os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::fs::PermissionsExt,
    };
    let mut f = Live::new().await;
    // Sync API authorization is exercised with a project-bound service account.
    // Ordinary Git/PR ingress uses a developer login, matching the installed UI.
    let developer = Uuid::new_v4();
    let username = format!("qa-git-{}", developer.simple());
    let password = "Owned-Git-HTTP-Fixture-Only!1";
    sqlx::query("INSERT INTO users(id,username,role) VALUES($1,$2,'developer')")
        .bind(developer)
        .bind(&username)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO user_credentials(user_id,password_hash) VALUES($1,$2)")
        .bind(developer)
        .bind(cicd::auth::hash_password(password).unwrap())
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO project_memberships(project_id,user_id,role) VALUES($1,$2,'developer')",
    )
    .bind(f.project)
    .bind(developer)
    .execute(&f.pool)
    .await
    .unwrap();
    let git_authorization = login_access_token(f.api.clone(), &username, password).await;
    f.settings(Direction::Pull, false).await;
    assert_eq!(f.manual().await.state, "success");
    let git_config = cicd::git_host::GitConfig {
        root: f.root.clone(),
        token: None,
        internal_token: None,
    };
    cicd::git_host::ensure_post_receive_hooks(&f.pool, &git_config).await;
    std::fs::set_permissions(
        f.local.join("hooks/post-receive"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:22801")
        .await
        .unwrap();
    let api = f.api.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, api).await.unwrap();
    });
    struct Server(tokio::task::JoinHandle<()>);
    impl Drop for Server {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _server = Server(server);
    f.settings(Direction::Both, true).await;
    let base = f.head("http-base\"quoted");
    let feature = f.head("http-feature\"quoted");
    let a = f.commit(None, "QA HTTP base").await;
    let b = f.commit(Some(&a), "QA HTTP feature").await;
    let previous = f.store.runs(f.repository, 1, 0).await.unwrap().1;
    let (reader, mut writer) = std::os::unix::net::UnixStream::pair().unwrap();
    writer
        .write_all(format!("username=qa\npassword={git_authorization}\n\n").as_bytes())
        .unwrap();
    writer.shutdown(std::net::Shutdown::Write).unwrap();
    let fd = unsafe { libc::fcntl(reader.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 200) };
    assert!(fd >= 0);
    let descriptor = unsafe { OwnedFd::from_raw_fd(fd) };
    let mut command = tokio::process::Command::new("git");
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .arg("-C")
        .arg(&f.actor)
        .args([
            "-c",
            "credential.helper=",
            "-c",
            "credential.helper=!f() { if test \"$1\" = get; then cat <&9; fi; }; f",
            "push",
        ])
        .arg(format!(
            "http://127.0.0.1:22801/git/{}.git",
            f.local.file_stem().unwrap().to_str().unwrap()
        ))
        .arg(format!("{a}:{base}"))
        .arg(format!("{b}:{feature}"));
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(descriptor.as_raw_fd(), 9) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        command.kill_on_drop(true).output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        output.status.success(),
        "real Git HTTP client authenticates after challenge and pushes with its credential helper: {}",
        String::from_utf8_lossy(&output.stderr).replace(&git_authorization, "[redacted]")
    );
    let run = f.automatic(previous, "git_change", 30).await;
    assert_eq!(run.state, "success");
    f.same(&base, &a).await;
    f.same(&feature, &b).await;
    // Two post-receive lines can legitimately coalesce into one follow-up.
    // Drain that work before attributing the next automatic run to PR merge.
    tokio::time::timeout(std::time::Duration::from_secs(45), async {
        loop {
            let pending: bool = sqlx::query_scalar("SELECT dirty OR EXISTS(SELECT 1 FROM repository_sync_runs r WHERE r.repository_id=c.repository_id AND r.finished_at IS NULL) FROM repository_sync_connections c WHERE c.repository_id=$1")
                .bind(f.repository).fetch_one(&f.pool).await.unwrap();
            if !pending { break; }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    }).await.expect("coalesced receive-pack signals finish before PR merge");
    let storage = f.local.file_stem().unwrap().to_str().unwrap();
    let response=f.api.clone().oneshot(Request::post(format!("/api/v1/repos/{storage}/pulls")).header("authorization",format!("Bearer {git_authorization}")).header("content-type","application/json").body(Body::from(serde_json::json!({"repository_name":storage,"title":"QA actual merge automatic signal","source_branch":feature.strip_prefix("refs/heads/").unwrap(),"target_branch":base.strip_prefix("refs/heads/").unwrap()}).to_string())).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let pr = response_json(response).await;
    let previous = f.store.runs(f.repository, 1, 0).await.unwrap().1;
    let response = f
        .api
        .clone()
        .oneshot(
            Request::post(format!(
                "/api/v1/repos/{storage}/pulls/{}/action",
                pr["number"]
            ))
            .header("authorization", format!("Bearer {git_authorization}"))
            .header("content-type", "application/json")
            .body(Body::from("{\"action\":\"merge\"}"))
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let merged = response_json(response).await;
    assert_eq!(merged["status"], "merged");
    let sha = merged["merge_commit_sha"].as_str().unwrap();
    let run = f.automatic(previous, "git_change", 30).await;
    assert_eq!(run.state, "success");
    f.same(&base, sha).await;
    f.stop_tasks().await;
    f.pool.close().await;
    println!(
        "LIVE_RECEIVE_PACK_PR_MERGE_OK provider={:?} prefix={}",
        f.settings.provider, f.prefix
    );
}
