//! Actual Forge HTTP/PG/embedded runner path. Only Tracker authority is a controlled HTTP peer.
use super::*;
use cicd::domain::sdlc_workspace::{
    TaskWorkspaceIntent, WorkspaceOperationReceipt, WorkspaceRole, WorkspaceTaskBinding,
};
use pdlc_shared::resource_context::{NamespaceRef, OwnerCommand, ResourceKind, ResourceRef};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration as StdDuration,
};
use tokio::sync::Mutex;

#[cfg(feature = "oci-integration")]
#[path = "task_pipeline_image_build_native.rs"]
mod image_build_native;

#[path = "task_pipeline_workspace_deploy.rs"]
mod workspace_deploy;

struct TaskPipeline {
    pool: sqlx::PgPool,
    http: reqwest::Client,
    api_url: String,
    config: cicd::config::RuntimeConfig,
    temp: PathBuf,
    barrier: PathBuf,
    project: Uuid,
    token: String,
    key: Uuid,
    intent: TaskWorkspaceIntent,
    yaml: String,
    tracker: Arc<Mutex<serde_json::Value>>,
    running: cicd::runner::RunningJobs,
    worker: Mutex<Option<tokio::task::JoinHandle<()>>>,
    quiescent: bool,
    peer: tokio::task::JoinHandle<()>,
    server: tokio::task::JoinHandle<()>,
}

type ConfigureTaskPipeline =
    fn(&mut cicd::config::RuntimeConfig, Uuid, &TaskWorkspaceIntent, &Path);

impl Drop for TaskPipeline {
    fn drop(&mut self) {
        self.peer.abort();
        self.server.abort();
        if let Some(worker) = self.worker.get_mut().take() {
            worker.abort();
        }
        if self.quiescent {
            let _ = std::fs::remove_dir_all(&self.temp);
        }
    }
}

fn git(cwd: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(out.status.success(), "owned Git fixture failed");
    String::from_utf8(out.stdout).unwrap().trim().into()
}

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn canonical(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(object) => serde_json::Value::Object(
            object
                .into_iter()
                .map(|(key, value)| (key, canonical(value)))
                .collect::<std::collections::BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(canonical).collect())
        }
        value => value,
    }
}

impl TaskPipeline {
    async fn cleanup(&mut self) -> anyhow::Result<()> {
        let released = std::fs::write(&self.barrier, b"release\n");
        if let Some(mut worker) = self.worker.get_mut().take() {
            match tokio::time::timeout(StdDuration::from_secs(25), &mut worker).await {
                Ok(_) => (),
                Err(_) => {
                    worker.abort();
                    let _ = tokio::time::timeout(StdDuration::from_secs(5), &mut worker)
                        .await
                        .map_err(|_| anyhow::anyhow!("owned runner cancellation timed out"))?;
                }
            }
        }
        let running = self.running.lock().await;
        anyhow::ensure!(
            !running.values().any(|pid| *pid == 0),
            "owned preparation quiescence remains unknown"
        );
        let pids: Vec<u32> = running.values().copied().collect();
        drop(running);
        // Cancellation/panic drops production Child (kill_on_drop); observe actual exit too.
        tokio::time::timeout(StdDuration::from_secs(5), async {
            loop {
                let mut alive = false;
                for pid in &pids {
                    let pid = libc::pid_t::try_from(*pid)?;
                    // Signal zero observes this retained PID; it cannot stop any process.
                    let result = unsafe { libc::kill(pid, 0) };
                    alive |= result == 0
                        || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH);
                }
                if !alive {
                    break Ok::<(), anyhow::Error>(());
                }
                tokio::time::sleep(StdDuration::from_millis(20)).await;
            }
        })
        .await
        .map_err(|_| anyhow::anyhow!("owned shell exit remains unknown"))??;
        self.peer.abort();
        self.server.abort();
        let _ = tokio::time::timeout(StdDuration::from_secs(5), &mut self.peer)
            .await
            .map_err(|_| anyhow::anyhow!("owned Tracker peer cancellation timed out"))?;
        let _ = tokio::time::timeout(StdDuration::from_secs(5), &mut self.server)
            .await
            .map_err(|_| anyhow::anyhow!("owned HTTP server cancellation timed out"))?;
        released?;
        let request: Option<Option<serde_json::Value>> = sqlx::query_scalar(
            "SELECT preparation_request FROM task_pipeline_intents WHERE project_id=$1 AND operation_key=$2",
        ).bind(self.project).bind(&self.intent.operation_key).fetch_optional(&self.pool).await?;
        if let Some(Some(request)) = request {
            let request: cicd::domain::sdlc_workspace::WorkspaceOperationRequest =
                serde_json::from_value(request)?;
            request.validate().map_err(anyhow::Error::msg)?;
            anyhow::ensure!(
                request.binding == self.intent.binding
                    && request.repository_id == self.intent.repository_id
                    && request.source_commit == self.intent.source_commit
                    && request.operation_key == self.intent.operation_key
                    && request.role == self.intent.role,
                "foreign cleanup custody"
            );
            let root = if self.config.runner.mode == cicd::config::RunnerMode::Docker {
                PathBuf::from("/workspaces")
            } else {
                std::env::temp_dir()
            }
            .canonicalize()?;
            let path = root.join(&request.workspace_id);
            match std::fs::symlink_metadata(&path) {
                Ok(_) => {
                    let owned = cicd::runner_workspace::OwnedWorkspace::reopen(
                        &root,
                        &request.workspace_id,
                    )?;
                    owned.verify_identity(
                        request.attempt_id,
                        request.lease_id,
                        request.workspace_generation,
                    )?;
                    match std::fs::symlink_metadata(path.join(".forge-preparation-active.json")) {
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                        Ok(_) => anyhow::bail!("owned Git preparation may still be active"),
                        Err(error) => return Err(error.into()),
                    }
                    // Disposable test teardown after quiescence, never fabricated product ACK.
                    tokio::fs::remove_dir_all(&path).await?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => return Err(error.into()),
            }
        }
        self.quiescent = true;
        fixture_cleanup::projects(&self.pool).await;
        self.pool.close().await;
        Ok(())
    }

    async fn new() -> Self {
        Self::new_with_source(None, &[], None).await
    }

    async fn new_with_source(
        yaml: Option<String>,
        files: &[(&str, &[u8])],
        configure: Option<ConfigureTaskPipeline>,
    ) -> Self {
        let url = reqwest::Url::parse(&std::env::var("CICD_TEST_DATABASE_URL").unwrap()).unwrap();
        assert!(
            url.path().trim_start_matches('/').starts_with("forge_test"),
            "disposable DB only"
        );
        let pool = test_pool().await;
        // These are the existing namespace projection verifier inputs; no process-env mutation.
        let instance: Uuid = std::env::var("CICD_NAMESPACE__INSTANCE_ID")
            .expect("QA Forge instance")
            .parse()
            .unwrap();
        let registry: Uuid = std::env::var("CICD_NAMESPACE__REGISTRY_INSTANCE_ID")
            .expect("QA registry instance")
            .parse()
            .unwrap();
        let temp = std::env::temp_dir().join(format!("forge-task-http-{}", Uuid::new_v4()));
        std::fs::create_dir(&temp).unwrap();
        let temp = temp.canonicalize().unwrap();
        let source = temp.join("source");
        let git_root = temp.join("git");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&git_root).unwrap();
        let barrier = temp.join("release");
        // Finite shell wait; the real runner's timeout remains authoritative.
        let command = format!(
            "n=0; while test ! -f '{}'; do n=$((n+1)); test \"$n\" -lt 200 || exit 71; sleep 0.05; done; test \"$(cat source.txt)\" = pinned || exit 72; printf 'actual task artifact\\n' > product.txt",
            barrier.display()
        );
        let yaml = yaml.unwrap_or_else(|| serde_yaml::to_string(&serde_json::json!({"version":1,"jobs":{"build":{"commands":[command],"timeout":"20s","artifacts":{"paths":["product.txt"]}}}})).unwrap());
        git(&source, &["init", "--initial-branch=main"]);
        std::fs::write(source.join("source.txt"), "pinned\n").unwrap();
        std::fs::write(source.join(".forge-ci.yml"), &yaml).unwrap();
        for (name, bytes) in files {
            assert!(
                cicd::domain::image_build::valid_relative_path(name, false),
                "fixture source path must be relative"
            );
            let path = source.join(name);
            assert!(!path.exists(), "fixture source is create-new");
            std::fs::write(path, bytes).unwrap();
        }
        git(&source, &["add", "."]);
        git(
            &source,
            &[
                "-c",
                "user.name=QA",
                "-c",
                "user.email=qa@example.invalid",
                "commit",
                "-m",
                "task source",
            ],
        );
        let pin = git(&source, &["rev-parse", "HEAD"]);
        let repository = Uuid::new_v4();
        let name = format!("task_{}", repository.simple());
        git(
            &temp,
            &[
                "clone",
                "--bare",
                source.to_str().unwrap(),
                git_root.join(format!("{name}.git")).to_str().unwrap(),
            ],
        );
        let project = Uuid::new_v4();
        let group = Uuid::new_v4();
        let namespace = Uuid::new_v4();
        let tracker_instance = Uuid::new_v4();
        let tracker_project = Uuid::new_v4();
        let owner_command = OwnerCommand {
            schema_version: 1,
            namespace: NamespaceRef {
                registry_instance_id: registry,
                namespace_id: namespace,
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
        sqlx::query(
            "INSERT INTO git_groups(id,slug,name,namespace_managed) VALUES($1,$2,'Task QA',true)",
        )
        .bind(group)
        .bind(format!("task-{}", group.simple()))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO forge_namespace_bindings(resource_id,registry_instance_id,namespace_id,generation,state,command) VALUES($1,$2,$3,1,'active',$4)")
            .bind(group).bind(registry).bind(namespace).bind(serde_json::to_value(owner_command).unwrap()).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
            .bind(repository)
            .bind(&name)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE repository_catalog SET group_id=$2 WHERE id=$1")
            .bind(repository)
            .bind(group)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO projects(id,name,repository_url,repository_id) VALUES($1,$2,$3,$4)",
        )
        .bind(project)
        .bind(format!("task-{project}"))
        .bind(format!(
            "file://{}",
            git_root.join(format!("{name}.git")).display()
        ))
        .bind(repository)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO forge_workspace_projects(registry_instance_id,namespace_id,tracker_instance_id,tracker_project_id,generation,name,project_key,state,observed_at) VALUES($1,$2,$3,$4,1,'Task QA','QA','active',now())")
            .bind(registry).bind(namespace).bind(tracker_instance).bind(tracker_project).execute(&pool).await.unwrap();
        let subject = Uuid::new_v4();
        sqlx::query("INSERT INTO service_accounts(id,name) VALUES($1,$2)")
            .bind(subject)
            .bind(format!("task-{subject}"))
            .execute(&pool)
            .await
            .unwrap();
        let token = format!("forge_sat_{}", Uuid::new_v4().simple());
        sqlx::query("INSERT INTO api_tokens(id,name,token_hash,token_hint,project_id,scopes,principal_type,service_account_id) VALUES($1,'task-http',$2,'qa-only',$3,ARRAY['api:read','api:write'],'service_account',$4)")
            .bind(Uuid::new_v4()).bind(cicd::auth::hash_token(&token)).bind(project).bind(subject).execute(&pool).await.unwrap();
        let task = Uuid::new_v4();
        let mut intent = TaskWorkspaceIntent {
            operation_key: format!("task:{}", Uuid::new_v4()),
            repository_id: repository,
            source_commit: pin,
            role: WorkspaceRole::Developer,
            agent_id: Uuid::new_v4(),
            tracker_lease_id: Uuid::new_v4(),
            holder_subject: "qa-scheduler".into(),
            expected_lease_version: 2,
            binding: WorkspaceTaskBinding {
                tracker_instance_id: tracker_instance.to_string(),
                tracker_project_id: tracker_project,
                task_id: task,
                root_task_id: task,
                assignment_id: Uuid::new_v4(),
                execution_id: Uuid::new_v4(),
                routing_snapshot_id: Uuid::new_v4(),
                requirement_revision: 1,
                fencing_token: 3,
                assignment_hash: String::new(),
                workflow_task_ref: "SDLC-27".into(),
            },
        };
        let mut routing = serde_json::json!({"tracker_instance_id":tracker_instance,"project_id":tracker_project,"task_id":task,"root_task_id":task,"snapshot_id":intent.binding.routing_snapshot_id,"requirement_revision":1,"policy":{"tracker_instance_id":tracker_instance,"project_id":tracker_project,"routes":{"developer":{"agent_id":intent.agent_id}}}});
        if configure.is_some() {
            routing["policy"]["routes"]["devops"] = serde_json::json!({"agent_id":Uuid::new_v4()});
        }
        let mut assignment = serde_json::json!({"task_id":task,"assignment_id":intent.binding.assignment_id,"execution_id":intent.binding.execution_id,"execution":{"ordinal":"27","key":"SDLC-27"},"agent_id":intent.agent_id,"lease_id":intent.tracker_lease_id,"role":"developer","stage":"Development","cycle":0,"attempt":1,"owner_subject":"qa-owner","fencing_token":3,"routing":routing});
        intent.binding.assignment_hash =
            sha(&serde_json::to_vec(&canonical(assignment.clone())).unwrap());
        assignment["assignment_hash"] = serde_json::json!(intent.binding.assignment_hash);
        intent.validate().unwrap();
        let now = Utc::now();
        let tracker = Arc::new(Mutex::new(
            serde_json::json!({"observed_at":now,"runtime_state":"running","receipt":{"task":{"task_id":task,"stage":"Development","status":"InProgress","runtime":"running","owner_subject":"qa-owner","cycle":0,"waiting_reason":null,"routing":routing},"assignment":assignment,"lease":{"lease_id":intent.tracker_lease_id,"version":2,"holder_subject":"qa-scheduler","claimed_at":now,"heartbeat_at":now,"expires_at":now+Duration::seconds(30)},"binding":{"run_id":Uuid::new_v4(),"chat_id":Uuid::new_v4(),"machine_subject":"qa-role"}}}),
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let data = tracker.clone();
        let refresh_lease = configure.is_some();
        let peer_app = axum::Router::new().route(
            &format!("/api/v1/issues/{task}/sdlc/lifecycle"),
            axum::routing::get(move |headers: HeaderMap| {
                let data = data.clone();
                async move {
                    assert_eq!(headers["authorization"], "Bearer qa-tracker-reader");
                    let mut value = data.lock().await.clone();
                    let now = Utc::now();
                    value["observed_at"] = serde_json::json!(now);
                    if refresh_lease {
                        // Only native fixture: the controlled Tracker peer maintains its 30s lease.
                        value["receipt"]["lease"]["heartbeat_at"] = serde_json::json!(now);
                        value["receipt"]["lease"]["expires_at"] =
                            serde_json::json!(now + Duration::seconds(30));
                    }
                    value.as_object_mut().unwrap().remove("native_result");
                    axum::Json(value)
                }
            }),
        );
        let source_data = tracker.clone();
        let peer_app = peer_app.route(
            &format!("/api/v1/issues/{task}/sdlc/source-binding"),
            axum::routing::get(move |headers: HeaderMap| {
                let data = source_data.clone();
                async move {
                    assert_eq!(headers["authorization"], "Bearer qa-tracker-reader");
                    let mut data = data.lock().await;
                    let source = data
                        .get("git_source")
                        .cloned()
                        .ok_or(StatusCode::NOT_FOUND)?;
                    let reads = data["git_reads"].as_u64().unwrap_or(0);
                    data["git_reads"] = serde_json::json!(reads + 1);
                    let mut current = data.clone();
                    current.as_object_mut().unwrap().remove("git_source");
                    current.as_object_mut().unwrap().remove("git_reads");
                    let now = Utc::now();
                    current["observed_at"] = serde_json::json!(now);
                    if refresh_lease {
                        current["receipt"]["lease"]["heartbeat_at"] = serde_json::json!(now);
                        current["receipt"]["lease"]["expires_at"] =
                            serde_json::json!(now + Duration::seconds(30));
                    }
                    Ok::<_, StatusCode>(axum::Json(serde_json::json!({
                        "state":"bound", "binding":source, "current":current
                    })))
                }
            }),
        );
        #[cfg(feature = "oci-integration")]
        let peer_app = {
            let result = tracker.clone();
            peer_app.route(
                &format!("/api/v1/issues/{task}/sdlc/lifecycle/results/{{result_id}}"),
                axum::routing::get(move |headers: HeaderMap, axum::extract::Path(result_id): axum::extract::Path<Uuid>| {
                    let result = result.clone();
                    async move {
                        assert_eq!(headers["authorization"], "Bearer qa-tracker-reader");
                        let value = result.lock().await["native_result"].clone();
                        if value["receipt"]["result_id"] != serde_json::json!(result_id) {
                            return Err(StatusCode::NOT_FOUND);
                        }
                        Ok(axum::Json(value))
                    }
                }),
            )
        };
        let peer = tokio::spawn(async move { axum::serve(listener, peer_app).await.unwrap() });
        let mut config = cicd::config::RuntimeConfig::test_default()
            .with_auth_secret(Some("task-http-test-secret".into()));
        config.git.root = git_root.clone();
        config.runner.mode = cicd::config::RunnerMode::HostShell;
        config.runner.keep_workspace = false;
        config.artifacts.root = temp.join("artifacts");
        config.sdlc_workspace.operation_subject = Some(subject);
        config.sdlc_workspace.tracker_read_url = Some(origin);
        config.sdlc_workspace.tracker_read_token = Some("qa-tracker-reader".into());
        if let Some(configure) = configure {
            configure(&mut config, project, &intent, &temp);
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api_url = format!("http://{}", listener.local_addr().unwrap());
        if config.git.internal_token.is_some() {
            config.http.bind = listener.local_addr().unwrap().to_string();
        }
        let app = cicd::api::app_with_git_and_config(
            Some(pool.clone()),
            cicd::git_host::GitConfig {
                root: git_root,
                token: None,
                internal_token: config.git.internal_token.clone(),
            },
            None,
            config.clone(),
        )
        .unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let http = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(StdDuration::from_secs(if configure.is_some() {
                90
            } else {
                10
            }))
            .build()
            .unwrap();
        Self {
            pool,
            http,
            api_url,
            config,
            temp,
            barrier,
            project,
            token,
            key: Uuid::new_v4(),
            intent,
            yaml,
            tracker,
            running: Arc::new(Mutex::new(std::collections::HashMap::new())),
            worker: Mutex::new(None),
            quiescent: false,
            peer,
            server,
        }
    }

    async fn request(
        &self,
        method: &str,
        path: &str,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let response = self
            .http
            .request(
                reqwest::Method::from_bytes(method.as_bytes()).unwrap(),
                format!("{}{path}", self.api_url),
            )
            .bearer_auth(&self.token)
            .header("Idempotency-Key", self.key.to_string())
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.bytes().await.unwrap();
        assert!(bytes.len() <= 256 * 1024);
        (status, serde_json::from_slice(&bytes).unwrap())
    }
    async fn trigger(&self, intent: &TaskWorkspaceIntent) -> (StatusCode, serde_json::Value) {
        self.request(
            "POST",
            &format!("/api/v1/projects/{}/pipelines", self.project),
            serde_json::json!({"git_ref":intent.source_commit,"task_intent":intent}),
        )
        .await
    }
}

#[tokio::test]
#[ignore = "requires disposable CICD_TEST_DATABASE_URL, configured QA namespace IDs, Git and sh"]
async fn task_http_trigger_embedded_checkout_shell_ack_artifact_and_original_candidate() {
    let f = Arc::new(TaskPipeline::new().await);
    let case_fixture = f.clone();
    // Tokio captures assertion panics while the owner retains teardown handles.
    let result = tokio::spawn(async move { case(&case_fixture).await }).await;
    let mut f = Arc::try_unwrap(f).unwrap_or_else(|_| panic!("fixture still borrowed after case"));
    let cleanup = f.cleanup().await;
    if let Err(error) = result {
        if cleanup.is_err() {
            eprintln!("owned task pipeline cleanup failed; disposable resources retained");
        }
        if error.is_panic() {
            std::panic::resume_unwind(error.into_panic());
        }
        panic!("task pipeline case was unexpectedly cancelled");
    }
    cleanup.expect("owned task pipeline cleanup");
}

#[tokio::test]
#[ignore = "requires disposable CICD_TEST_DATABASE_URL, configured QA namespace IDs, Git and curl"]
async fn task_git_receive_waits_for_busy_repository_sync_guard_without_pipeline_dispatch() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    const SELECTOR: &str = "task_pipeline_http::task_git_receive_waits_for_busy_repository_sync_guard_without_pipeline_dispatch";
    const CHILD: &str = "FORGE_TASK_GIT_SYNC_GUARD_TEST_PROCESS";
    const SUBJECT: &str = "00000000-0000-0000-0000-000000000003";
    if std::env::var_os(CHILD).is_none() {
        // Isolate central-auth environment from every other server/PG test.
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = calls.clone();
        let app = axum::Router::new().route(
            "/auth/tokens/introspect",
            axum::routing::get(move |uri: axum::http::Uri, headers: HeaderMap| {
                let calls = counted.clone();
                async move {
                    assert_eq!(uri.query(), Some("include_lifetime=true"));
                    let digest = headers["authorization"]
                        .to_str()
                        .unwrap()
                        .strip_prefix("Bearer sdlc_pat_")
                        .unwrap();
                    assert_eq!(digest.len(), 64);
                    assert!(digest.bytes().all(|b| b.is_ascii_hexdigit()));
                    calls.fetch_add(1, Ordering::SeqCst);
                    let now = Utc::now();
                    axum::Json(
                        serde_json::json!({"sub":SUBJECT,"email":"fixture@example.invalid",
                        "scopes":["ci-cd:read","ci-cd:write",format!("ci-cd:sdlc:git:v1:{digest}")],
                        "observed_at":now,"expires_at":now+Duration::seconds(30)}),
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let uri = format!("http://{}/oidc/jwks", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", SELECTOR, "--ignored", "--nocapture"])
            .env(CHILD, "1")
            .env("CICD_AUTH__CENTRAL_JWKS_URI", uri)
            .env("CICD_AUTH__CENTRAL_TIMEOUT_SECS", "5")
            .env("NO_PROXY", "127.0.0.1,localhost")
            .env("no_proxy", "127.0.0.1,localhost")
            .kill_on_drop(true);
        let result = tokio::time::timeout(StdDuration::from_secs(120), command.output()).await;
        server.abort();
        let output = result
            .expect("isolated Task Git guard test timed out")
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        // Two fixture discoveries + push discovery/receive + both native hook callbacks.
        assert!(calls.load(Ordering::SeqCst) >= 6);
        return;
    }

    fn configure(
        config: &mut cicd::config::RuntimeConfig,
        _: Uuid,
        _: &TaskWorkspaceIntent,
        _: &Path,
    ) {
        config.sdlc_workspace.delegated_subject = Some(SUBJECT.parse().unwrap());
        config.git.internal_token = Some("task-git-sync-test-internal".into());
    }
    let f = Arc::new(TaskPipeline::new_with_source(None, &[], Some(configure)).await);
    let case_fixture = f.clone();
    let result = tokio::spawn(async move { task_git_sync_guard_case(&case_fixture).await }).await;
    let mut f = Arc::try_unwrap(f).unwrap_or_else(|_| panic!("fixture still borrowed after case"));
    let cleanup = f.cleanup().await;
    if let Err(error) = result {
        if error.is_panic() {
            std::panic::resume_unwind(error.into_panic());
        }
        panic!("Task Git guard case was unexpectedly cancelled");
    }
    cleanup.expect("owned task Git fixture cleanup");
}

async fn task_git_sync_guard_case(f: &TaskPipeline) {
    use cicd::task_git::{GitContext, SourceBinding};
    use cicd_infra::sync_git::MutationGuard;

    let (repository, command): (String, serde_json::Value) = sqlx::query_as(
        "SELECT g.slug||'/'||r.slug,b.command FROM repository_catalog r JOIN git_groups g ON g.id=r.group_id JOIN forge_namespace_bindings b ON b.resource_id=g.id WHERE r.id=$1",
    ).bind(f.intent.repository_id).fetch_one(&f.pool).await.unwrap();
    let command: OwnerCommand = serde_json::from_value(command).unwrap();
    let current = f.tracker.lock().await.clone();
    let mut source = SourceBinding {
        contract_version: 1,
        tracker_instance_id: f.intent.binding.tracker_instance_id.clone(),
        project_id: f.intent.binding.tracker_project_id,
        task_id: f.intent.binding.task_id,
        root_task_id: f.intent.binding.root_task_id,
        confirmation_id: Uuid::new_v4(),
        requirement_revision: f.intent.binding.requirement_revision,
        content_hash: "c".repeat(64),
        routing_snapshot_id: f.intent.binding.routing_snapshot_id,
        namespace: command.namespace,
        forge_instance_id: command.resource.instance_id,
        repository_id: f.intent.repository_id,
        base_ref: "refs/heads/main".into(),
        base_commit: f.intent.source_commit.clone(),
        candidate_ref: format!("refs/heads/sdlc/{}", f.intent.binding.task_id),
        source_revision: 1,
        policy_hash: "d".repeat(64),
        source_hash: String::new(),
        owner_subject: "qa-owner".into(),
        created_at: Utc::now(),
    };
    let mut sealed = serde_json::to_value(&source).unwrap();
    sealed.as_object_mut().unwrap().remove("source_hash");
    source.source_hash = sha(&serde_json::to_vec(&canonical(sealed)).unwrap());
    let context = GitContext {
        version: 1,
        project_id: f.project,
        repository: repository.clone(),
        source,
        intent: f.intent.clone(),
        run_id: serde_json::from_value(current["receipt"]["binding"]["run_id"].clone()).unwrap(),
        chat_id: serde_json::from_value(current["receipt"]["binding"]["chat_id"].clone()).unwrap(),
        machine_subject: "qa-role".into(),
        old_oid: "0".repeat(40),
        new_oid: f.intent.source_commit.clone(),
    };
    let grant = context.grant().unwrap();
    let token = format!(
        "sdlc_pat_{}",
        grant.strip_prefix("ci-cd:sdlc:git:v1:").unwrap()
    );
    let context_header = serde_json::to_string(&context).unwrap();
    f.tracker.lock().await["git_source"] = serde_json::to_value(&context.source).unwrap();
    let storage: String =
        sqlx::query_scalar("SELECT storage_name FROM repository_catalog WHERE id=$1")
            .bind(f.intent.repository_id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    let bare = f.config.git.root.join(format!("{storage}.git"));
    let refs_before = git(&f.temp, &["--git-dir", bare.to_str().unwrap(), "show-ref"]);
    // The legacy repository INSERT creates only its storage alias. Grouping
    // that row does not publish the canonical group/slug alias used by HTTP.
    for mapped in [false, true] {
        if mapped {
            sqlx::query("INSERT INTO repository_aliases(alias,repository_id) VALUES($1,$2)")
                .bind(&repository)
                .bind(f.intent.repository_id)
                .execute(&f.pool)
                .await
                .unwrap();
        }
        let response = f
            .http
            .get(format!(
                "{}/git/{repository}.git/info/refs?service=git-receive-pack",
                f.api_url
            ))
            .bearer_auth(&token)
            .header(cicd::task_git::HEADER, &context_header)
            .send()
            .await
            .expect("owned fixture discovery transport");
        let status = response.status();
        let reads = f.tracker.lock().await["git_reads"].as_u64().unwrap_or(0);
        assert_eq!(
            status,
            if mapped {
                StatusCode::OK
            } else {
                StatusCode::NOT_FOUND
            },
            "Task Git discovery HTTP {status}, canonical_alias={mapped}, source_reads={reads}"
        );
        assert_eq!(
            reads,
            if mapped { 2 } else { 1 },
            "discovery must pass real source admission"
        );
        assert_eq!(
            git(&f.temp, &["--git-dir", bare.to_str().unwrap(), "show-ref"]),
            refs_before
        );
    }
    let reads_before_push = f.tracker.lock().await["git_reads"].as_u64().unwrap();
    let sync_guard = MutationGuard::acquire(&f.config.git.root, f.intent.repository_id)
        .await
        .unwrap();
    let mut command = tokio::process::Command::new("git");
    command
        .current_dir(f.temp.join("source"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .kill_on_drop(true)
        .args([
            "-c",
            &format!("http.extraHeader=Authorization: Bearer {token}"),
            "-c",
            &format!(
                "http.extraHeader={}: {}",
                cicd::task_git::HEADER,
                context_header
            ),
            "push",
            &format!("{}/git/{repository}.git", f.api_url),
            &format!("HEAD:{}", context.source.candidate_ref),
        ]);
    let mut push = Box::pin(command.output());
    // Both real admissions reached Tracker; an auth rejection cannot satisfy this test.
    tokio::time::timeout(StdDuration::from_secs(5), async {
        loop {
            tokio::select! {
                output = &mut push => {
                    let output = output.expect("owned Git client transport");
                    let stderr: String = String::from_utf8_lossy(&output.stderr)
                        .replace(&token, "[fixture-child]")
                        .replace(&context_header, "[fixture-context]")
                        .chars().take(2048).collect();
                    let reads = f.tracker.lock().await["git_reads"].as_u64().unwrap_or(0);
                    panic!("Task Git exited before busy-lock wait: success={}, code={:?}, source_reads_since_discovery={}, stderr={stderr}",
                        output.status.success(), output.status.code(), reads.saturating_sub(reads_before_push));
                },
                _ = tokio::time::sleep(StdDuration::from_millis(10)) => {
                    if f.tracker.lock().await["git_reads"].as_u64().unwrap_or(0) >= reads_before_push + 2 { break; }
                }
            }
        }
    }).await.expect("Task Git discovery and receive must reach source admission");
    assert!(
        tokio::time::timeout(StdDuration::from_millis(250), &mut push)
            .await
            .is_err()
    );
    assert_eq!(
        git(&f.temp, &["--git-dir", bare.to_str().unwrap(), "show-ref"]),
        refs_before
    );
    drop(sync_guard);
    let output = tokio::time::timeout(StdDuration::from_secs(20), &mut push)
        .await
        .unwrap()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        git(
            &f.temp,
            &[
                "--git-dir",
                bare.to_str().unwrap(),
                "rev-parse",
                &context.source.candidate_ref
            ]
        ),
        context.new_oid
    );
    assert_eq!(
        git(
            &f.temp,
            &[
                "--git-dir",
                bare.to_str().unwrap(),
                "rev-parse",
                "refs/heads/main"
            ]
        ),
        context.source.base_commit
    );
    assert!(f.tracker.lock().await["git_reads"].as_u64().unwrap() >= reads_before_push + 4);
    assert!(
        MutationGuard::try_acquire(&f.config.git.root, f.intent.repository_id)
            .unwrap()
            .is_some()
    );
    let pipelines: i64 = sqlx::query_scalar("SELECT count(*) FROM pipelines WHERE project_id=$1")
        .bind(f.project)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    let intents: i64 =
        sqlx::query_scalar("SELECT count(*) FROM task_pipeline_intents WHERE project_id=$1")
            .bind(f.project)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!((pipelines, intents), (0, 0));
}

async fn case(f: &TaskPipeline) {
    // No caller-fabricated Forge pipeline/job/attempt/lease or receipt fixtures.
    f.tracker.lock().await["receipt"]["lease"]["version"] = serde_json::json!(1);
    assert_eq!(f.trigger(&f.intent).await.0, StatusCode::CONFLICT);
    f.tracker.lock().await["receipt"]["lease"]["version"] = serde_json::json!(2);
    let mut foreign = f.intent.clone();
    foreign.binding.tracker_project_id = Uuid::new_v4();
    assert_eq!(f.trigger(&foreign).await.0, StatusCode::CONFLICT);
    let counts: (i64,i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM pipelines),(SELECT count(*) FROM job_leases),(SELECT count(*) FROM task_pipeline_intents)").fetch_one(&f.pool).await.unwrap();
    assert_eq!(counts, (0, 0, 0));
    let (status, created) = f.trigger(&f.intent).await;
    assert_eq!(status, StatusCode::OK, "{created}");
    assert_eq!(created["plan"]["config_source"], "repository");
    assert_eq!(
        created["plan"]["resolved_commit_sha"],
        f.intent.source_commit
    );
    let pipeline: Uuid = serde_json::from_value(created["pipeline"]["id"].clone()).unwrap();
    let job: Uuid = sqlx::query_scalar(
        "SELECT j.id FROM jobs j JOIN stages s ON s.id=j.stage_id WHERE s.pipeline_id=$1",
    )
    .bind(pipeline)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    let queued: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM job_queue q JOIN execution_attempts a ON a.id=q.attempt_id WHERE q.job_id=$1 AND q.state='queued' AND a.status='queued')").bind(job).fetch_one(&f.pool).await.unwrap();
    assert!(queued);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM job_leases")
            .fetch_one(&f.pool)
            .await
            .unwrap(),
        0
    );
    let locator = format!(
        "/api/v1/projects/{}/pipelines/{pipeline}/task-workspace",
        f.project
    );
    let (status, pending) = f.request("GET", &locator, serde_json::Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert!(pending.is_null());
    let running = f.running.clone();
    *f.worker.lock().await = Some(tokio::spawn(cicd::runner::run_job_with_config(
        f.pool.clone(),
        job,
        running.clone(),
        cicd::runner::RuntimeRunnerConfig::from_config(&f.config),
    )));
    let deadline = std::time::Instant::now() + StdDuration::from_secs(15);
    let original = loop {
        let (status, value) = f.request("GET", &locator, serde_json::Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        if !value.is_null() {
            break value;
        }
        assert!(
            !f.worker.lock().await.as_ref().unwrap().is_finished(),
            "real embedded runner stopped before receipt"
        );
        assert!(
            std::time::Instant::now() < deadline,
            "embedded preparation timed out"
        );
        tokio::time::sleep(StdDuration::from_millis(20)).await;
    };
    let receipt: WorkspaceOperationReceipt = serde_json::from_value(original.clone()).unwrap();
    assert!(receipt.physical_source_observed);
    assert!(!receipt.dispatch_allowed);
    assert_eq!(receipt.request.binding, f.intent.binding);
    assert_eq!(receipt.request.source_commit, f.intent.source_commit);
    let r = &receipt.request;
    let identity: (Uuid,Uuid,i64,String,bool) = sqlx::query_as("SELECT l.id,l.attempt_id,l.generation,l.runner_name,l.acknowledged_at IS NOT NULL FROM job_leases l WHERE l.job_id=$1").bind(job).fetch_one(&f.pool).await.unwrap();
    assert_eq!(
        identity,
        (
            r.lease_id,
            r.attempt_id,
            r.workspace_generation,
            "embedded".into(),
            true
        )
    );
    let workspace = std::env::temp_dir().join(&r.workspace_id);
    assert_eq!(
        git(&workspace.join("workspace"), &["rev-parse", "HEAD"]),
        f.intent.source_commit
    );
    while running.lock().await.get(&job).is_none_or(|pid| *pid == 0) {
        assert!(
            !f.worker.lock().await.as_ref().unwrap().is_finished(),
            "shell stopped before barrier release"
        );
        assert!(std::time::Instant::now() < deadline, "shell did not start");
        tokio::time::sleep(StdDuration::from_millis(20)).await;
    }
    let nonterminal: bool = sqlx::query_scalar("SELECT l.completed_at IS NULL AND l.completion_received_at IS NULL AND a.finished_at IS NULL AND a.status='running' FROM job_leases l JOIN execution_attempts a ON a.id=l.attempt_id WHERE l.id=$1")
        .bind(r.lease_id).fetch_one(&f.pool).await.unwrap();
    assert!(nonterminal, "preparation must not invent terminal ACK");
    let candidate = format!(
        "/api/v1/projects/{}/sdlc/workspace-operations/{}/candidate-evidence?requestHash={}&taskId={}&rootTaskId={}&assignmentId={}&executionId={}&fencingToken={}",
        f.project,
        r.operation_key,
        receipt.request_hash,
        r.binding.task_id,
        r.binding.root_task_id,
        r.binding.assignment_id,
        r.binding.execution_id,
        r.binding.fencing_token
    );
    let (status, before) = f.request("GET", &candidate, serde_json::Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        before["candidateObserved"], false,
        "no terminal fabrication from preparation"
    );
    std::fs::write(&f.barrier, b"release\n").unwrap();
    let completed = {
        let mut worker = f.worker.lock().await;
        let completed =
            tokio::time::timeout(StdDuration::from_secs(25), worker.as_mut().unwrap()).await;
        if completed.is_ok() {
            worker.take();
        }
        completed
    };
    completed.expect("bounded embedded runner").unwrap();
    assert!(running.lock().await.is_empty());
    let terminal: (String,String,Option<i32>,String,bool,bool) = sqlx::query_as("SELECT p.status,a.status,a.exit_code,l.terminal_status,l.completion_received_at IS NOT NULL,l.completed_at IS NOT NULL FROM execution_attempts a JOIN job_leases l ON l.attempt_id=a.id JOIN jobs j ON j.id=a.job_id JOIN stages s ON s.id=j.stage_id JOIN pipelines p ON p.id=s.pipeline_id WHERE a.id=$1").bind(r.attempt_id).fetch_one(&f.pool).await.unwrap();
    assert_eq!(
        terminal,
        (
            "success".into(),
            "success".into(),
            Some(0),
            "success".into(),
            true,
            true
        )
    );
    assert!(
        !workspace.exists(),
        "real terminal ACK must precede runner cleanup"
    );
    let (status, after) = f.request("GET", &locator, serde_json::Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(after, original, "original receipt survives real cleanup");
    let (status, evidence) = f.request("GET", &candidate, serde_json::Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{evidence}");
    assert_eq!(evidence["candidateObserved"], true, "{evidence}");
    assert_eq!(evidence["operationReceipt"], original);
    assert_eq!(evidence["pipelineId"], pipeline.to_string());
    assert_eq!(evidence["configSha256"], sha(f.yaml.as_bytes()));
    let artifacts = evidence["artifacts"].as_array().unwrap();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0]["attemptId"], r.attempt_id.to_string());
    assert_eq!(artifacts[0]["sha256"], sha(b"actual task artifact\n"));
    for flag in [
        "dispatchAllowed",
        "deploymentVerified",
        "acceptanceVerified",
    ] {
        assert_eq!(evidence[flag], false);
    }
    let storage: String =
        sqlx::query_scalar("SELECT storage_path FROM artifacts WHERE attempt_id=$1")
            .bind(r.attempt_id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(std::fs::read(&storage).unwrap(), b"actual task artifact\n");
    f.tracker.lock().await["runtime_state"] = serde_json::json!("unknown");
    let (status, replayed) = f.trigger(&f.intent).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replayed["pipeline"]["id"], pipeline.to_string());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM job_leases WHERE job_id=$1")
            .bind(job)
            .fetch_one(&f.pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        f.request("GET", &locator, serde_json::Value::Null).await.1,
        original
    );
    cicd::runner::run_job_with_config(
        f.pool.clone(),
        job,
        running.clone(),
        cicd::runner::RuntimeRunnerConfig::from_config(&f.config),
    )
    .await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM job_leases WHERE job_id=$1")
            .bind(job)
            .fetch_one(&f.pool)
            .await
            .unwrap(),
        1
    );
    std::fs::write(&storage, b"tampered artifact\n").unwrap();
    let (_, tampered) = f.request("GET", &candidate, serde_json::Value::Null).await;
    assert_eq!(tampered["candidateObserved"], false);
    assert!(tampered["artifacts"].as_array().unwrap().is_empty());
    assert!(!evidence.to_string().contains(&f.token));
    assert!(!evidence.to_string().contains("qa-tracker-reader"));
}
