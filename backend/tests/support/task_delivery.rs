use super::*;
use cicd::domain::task_delivery::*;
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

struct ActualDelivery {
    f: Fixture,
    temp: PathBuf,
    source: PathBuf,
    bare: PathBuf,
    git_root: PathBuf,
    artifact_root: PathBuf,
    target_root: PathBuf,
    policy_file: PathBuf,
    address: std::net::SocketAddr,
    server: tokio::task::JoinHandle<()>,
    _target: Option<tokio::process::Child>,
    tag: String,
    database_url: String,
}

impl Drop for ActualDelivery {
    fn drop(&mut self) {
        self.server.abort();
        let _ = std::fs::remove_dir_all(&self.temp);
    }
}

impl ActualDelivery {
    async fn new() -> Self {
        let mut f = Fixture::new().await;
        let schema: String = sqlx::query_scalar("SELECT current_setting('search_path')")
            .fetch_one(&f.pool)
            .await
            .unwrap();
        let mut database_url =
            reqwest::Url::parse(&std::env::var("CICD_TEST_DATABASE_URL").unwrap()).unwrap();
        database_url
            .query_pairs_mut()
            .append_pair("options", &format!("-csearch_path={schema}"));
        let temp = std::env::temp_dir().join(format!("forge-task-delivery-{}", Uuid::new_v4()));
        let source = temp.join("source");
        let git_root = temp.join("git");
        let artifact_root = temp.join("artifacts");
        for path in [&source, &git_root, &artifact_root] {
            std::fs::create_dir_all(path).unwrap();
        }
        let (target_root, origin, target) =
            if let Ok(origin) = std::env::var("CICD_TEST_DELIVERY_ORIGIN") {
                assert_eq!(origin, "http://delivery-target:8080");
                (PathBuf::from("/delivery-qa/target-root"), origin, None)
            } else {
                let target_root = temp.join("target-root");
                let binary = temp.join("manifest-target");
                assert!(
                    std::process::Command::new("rustc")
                        .args([
                            "--edition=2024",
                            concat!(
                                env!("CARGO_MANIFEST_DIR"),
                                "/tests/helpers/manifest_target.rs"
                            ),
                            "-o"
                        ])
                        .arg(&binary)
                        .status()
                        .unwrap()
                        .success()
                );
                let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
                let address = listener.local_addr().unwrap();
                drop(listener);
                let target = tokio::process::Command::new(binary)
                    .arg(&target_root)
                    .arg(address.to_string())
                    .kill_on_drop(true)
                    .spawn()
                    .unwrap();
                (target_root, format!("http://{address}"), Some(target))
            };
        let policy_file = temp.join("policy.json");
        std::fs::write(
            &policy_file,
            serde_json::to_vec(&cicd::task_delivery::DeliveryPolicy {
                origin,
                health_path: "/health".into(),
                health_body_sha256: hash(b"ok\n"),
                acceptance_path: "/acceptance".into(),
                acceptance_body_sha256: hash(b"accepted\n"),
            })
            .unwrap(),
        )
        .unwrap();
        physical_git(&source, &["init", "--initial-branch=main"]);
        std::fs::write(source.join("README.md"), "actual delivery source\n").unwrap();
        physical_git(&source, &["add", "."]);
        physical_git(
            &source,
            &[
                "-c",
                "user.name=QA",
                "-c",
                "user.email=qa@example.invalid",
                "commit",
                "-m",
                "initial source",
            ],
        );
        let name: String = sqlx::query_scalar("SELECT name FROM repositories WHERE id=$1")
            .bind(f.request.repository_id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
        let bare = git_root.join(format!("{name}.git"));
        physical_git(
            &temp,
            &[
                "clone",
                "--bare",
                source.to_str().unwrap(),
                bare.to_str().unwrap(),
            ],
        );
        sqlx::query("UPDATE projects SET repository_url=$1 WHERE id=$2")
            .bind(format!("file://{}", bare.display()))
            .bind(f.project)
            .execute(&f.pool)
            .await
            .unwrap();
        let mut config = cicd::config::RuntimeConfig::test_default()
            .with_auth_secret(Some("workspace-test-secret".into()));
        config.sdlc_workspace.operation_subject = Some(f.subject);
        config.sdlc_workspace.local_delivery_root = Some(target_root.clone());
        config.sdlc_workspace.local_delivery_policy = Some(policy_file.clone());
        config.artifacts.root = artifact_root.clone();
        f.app = cicd::api::app_with_git_and_config(
            Some(f.pool.clone()),
            cicd::git_host::GitConfig {
                root: git_root.clone(),
                token: None,
                internal_token: None,
            },
            None,
            config,
        )
        .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = f.app.clone();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            f,
            temp,
            source,
            bare,
            git_root,
            artifact_root,
            target_root,
            policy_file,
            address,
            server,
            _target: target,
            tag: format!("delivery-{}", Uuid::new_v4().simple()),
            database_url: database_url.to_string(),
        }
    }

    async fn build(
        &mut self,
        version: &str,
        payload: &str,
    ) -> (DeliveryCommand, serde_json::Value) {
        assert!(
            payload
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"=\n-".contains(&b))
        );
        let script = format!("printf '{}' > product.txt", payload.replace('\n', "\\n"));
        let result = self.build_script(version, &script).await;
        assert_eq!(result.1["sha256"], hash(payload.as_bytes()));
        (result.0, result.2)
    }

    async fn build_script(
        &mut self,
        version: &str,
        script: &str,
    ) -> (DeliveryCommand, serde_json::Value, serde_json::Value) {
        self.build_script_with_timeout(version, script, Duration::from_secs(25))
            .await
    }

    async fn build_script_with_timeout(
        &mut self,
        version: &str,
        script: &str,
        completion_timeout: Duration,
    ) -> (DeliveryCommand, serde_json::Value, serde_json::Value) {
        let barrier = self.temp.join(format!("release-{version}"));
        let command = format!(
            "while test ! -f '{}'; do sleep 0.1; done; {script}",
            barrier.display()
        );
        let yaml=serde_yaml::to_string(&serde_json::json!({"version":1,"jobs":{"build":{"tags":[self.tag],"commands":[command],"artifacts":{"paths":["product.txt"]}}}})).unwrap();
        std::fs::write(self.source.join(".forge-ci.yml"), yaml).unwrap();
        physical_git(&self.source, &["add", "."]);
        physical_git(
            &self.source,
            &[
                "-c",
                "user.name=QA",
                "-c",
                "user.email=qa@example.invalid",
                "commit",
                "-m",
                version,
            ],
        );
        physical_git(
            &self.source,
            &["push", self.bare.to_str().unwrap(), "HEAD:refs/heads/main"],
        );
        let pin = physical_git(&self.source, &["rev-parse", "HEAD"]);
        let (status, pipeline) = call(
            &self.f.app,
            "POST",
            &format!("/api/v1/projects/{}/pipelines", self.f.project),
            &self.f.token,
            serde_json::json!({"git_ref":pin}).to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{pipeline}");
        assert_eq!(pipeline["plan"]["config_source"], "repository");
        let pipeline_id: Uuid = serde_json::from_value(pipeline["pipeline"]["id"].clone()).unwrap();
        let runner_id = Uuid::new_v4();
        let credential = format!("cicd_runner_{}", runner_id.simple());
        sqlx::query("INSERT INTO runners(id,name,tags,status,last_seen_at,credential_hash,credential_expires_at,capabilities) VALUES($1,$2,$3,'online',now(),$4,now()+interval '1 hour','{}')").bind(runner_id).bind(format!("delivery-{runner_id}")).bind(vec![self.tag.clone()]).bind(cicd::auth::hash_token(&credential)).execute(&self.f.pool).await.unwrap();
        let child = tokio::process::Command::new(env!("CARGO_BIN_EXE_forge-runner"))
            .kill_on_drop(true)
            .args([
                "--api-url",
                &format!("http://{}", self.address),
                "--name",
                &format!("delivery-{runner_id}"),
                "--tags",
                &self.tag,
                "--once",
                "--keep-workspace",
                "--work-dir",
            ])
            .arg(self.temp.join(format!("workspaces-{version}")))
            .env("CICD_RUNNER_CREDENTIAL", credential)
            .env_remove("CICD_RUNNER_REGISTRATION_TOKEN")
            .env_remove("CICD_RUNNER_NO_CHECKOUT")
            .env_remove("CICD_RUNNER_INSPECT_WORKSPACES")
            .env_remove("CICD_RUNNER_RECONCILE_WORKSPACES")
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        let (lease, attempt, generation) = loop {
            let row:Option<(Uuid,Uuid,i64)>=sqlx::query_as("SELECT l.id,l.attempt_id,l.generation FROM job_leases l JOIN jobs j ON j.id=l.job_id JOIN stages s ON s.id=j.stage_id WHERE s.pipeline_id=$1 AND l.acknowledged_at IS NOT NULL AND l.lease_status='active'").bind(pipeline_id).fetch_optional(&self.f.pool).await.unwrap();
            if let Some(row) = row {
                break row;
            }
            assert!(Instant::now() < deadline, "runner ACK deadline");
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        self.f.request.operation_key = format!("prepare:{version}");
        self.f.request.source_commit = pin;
        self.f.request.lease_id = lease;
        self.f.request.attempt_id = attempt;
        self.f.request.workspace_generation = generation;
        self.f.request.workspace_id =
            format!("attempt-{attempt}-{generation}-{}", Uuid::new_v4().simple());
        let (status, original) = self.f.post(&self.f.request).await;
        assert_eq!(status, StatusCode::OK, "{original}");
        std::fs::write(barrier, b"release").unwrap();
        let output = tokio::time::timeout(completion_timeout, child.wait_with_output())
            .await
            .unwrap()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let (status, evidence) = call(
            &self.f.app,
            "GET",
            &self
                .f
                .lookup_url(&original)
                .replace("?", "/candidate-evidence?"),
            &self.f.token,
            String::new(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{evidence}");
        assert_eq!(evidence["candidateObserved"], true, "{evidence}");
        assert_eq!(evidence["dispatchAllowed"], false);
        let b = &self.f.request.binding;
        (
            DeliveryCommand {
                operation_key: format!("deploy:{version}"),
                workspace_operation_key: self.f.request.operation_key.clone(),
                original: cicd::domain::sdlc_workspace::WorkspaceOperationLookup {
                    request_hash: original["requestHash"].as_str().unwrap().into(),
                    task_id: b.task_id,
                    root_task_id: b.root_task_id,
                    assignment_id: b.assignment_id,
                    execution_id: b.execution_id,
                    fencing_token: b.fencing_token,
                },
                action: DeliveryAction::Deploy,
                artifact_id: Some(
                    serde_json::from_value(evidence["artifacts"][0]["artifactId"].clone()).unwrap(),
                ),
                expected_manifest_sha256: None,
            },
            evidence["artifacts"][0].clone(),
            original,
        )
    }

    fn process(&self, command: &DeliveryCommand) -> tokio::process::Command {
        let file = self.temp.join(format!(
            "command-{}.json",
            hash(command.operation_key.as_bytes())
        ));
        std::fs::write(&file, serde_json::to_vec(command).unwrap()).unwrap();
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_forge-delivery"));
        child
            .kill_on_drop(true)
            .args([
                "--project-id",
                &self.f.project.to_string(),
                "--command-file",
            ])
            .arg(file)
            .env("CICD_DATABASE_URL", &self.database_url)
            .env("CICD_AUTH_SECRET", "workspace-test-secret")
            .env("CICD_SDLC_WORKSPACE_SUBJECT", self.f.subject.to_string())
            .env("CICD_GIT_ROOT", &self.git_root)
            .env("CICD_ARTIFACTS_DIR", &self.artifact_root)
            .env("CICD_LOCAL_DELIVERY_ROOT", &self.target_root)
            .env("CICD_LOCAL_DELIVERY_POLICY", &self.policy_file)
            .env("CICD_LOCAL_DELIVERY_MODE", "local-verification")
            .env("CICD_LOCAL_DELIVERY_TOKEN", &self.f.token)
            .env_remove("CICD_AUTH__CENTRAL_JWKS_URI")
            .env_remove("CICD_GIT_INTERNAL_TOKEN")
            .env_remove("CICD_SECRETS_KEY");
        child
    }

    async fn execute(
        &self,
        command: &DeliveryCommand,
        mode: Option<&str>,
    ) -> (i32, Option<serde_json::Value>) {
        let mut child = self.process(command);
        if let Some(mode) = mode {
            child.arg(mode);
        }
        let process = child
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut wait = tokio::spawn(async move { process.wait_with_output().await.unwrap() });
        let output = match tokio::time::timeout(Duration::from_secs(25), &mut wait).await {
            Ok(result) => result.unwrap(),
            Err(_) => {
                type LockDiagnostic = (i32, String, Option<String>, Vec<i32>, String);
                let locks: Vec<LockDiagnostic> = sqlx::query_as("SELECT pid,state,wait_event,pg_blocking_pids(pid),left(query,160) FROM pg_stat_activity WHERE datname=current_database() AND pid<>pg_backend_pid() ORDER BY pid LIMIT 20").fetch_all(&self.f.pool).await.unwrap();
                eprintln!(
                    "DELIVERY_TIMEOUT key={} locks={locks:?}",
                    command.operation_key
                );
                let files = std::fs::read_dir(&self.target_root)
                    .ok()
                    .map(|entries| entries.flatten().map(|e| e.file_name()).collect::<Vec<_>>());
                eprintln!("DELIVERY_TARGET_FILES={files:?}");
                wait.abort();
                panic!("owner local CLI exceeded deadline");
            }
        };
        let json = serde_json::from_slice(&output.stdout).ok();
        (output.status.code().unwrap(), json)
    }

    async fn curl(
        &self,
        method: &str,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> (u16, serde_json::Value) {
        use tokio::io::AsyncWriteExt;
        let mut config = format!(
            "url = \"http://{}{}\"\nheader = \"Authorization: Bearer {}\"\nrequest = \"{}\"\n",
            self.address, path, self.f.token, method
        );
        if let Some(body) = body {
            let file = self.temp.join("curl-request.json");
            std::fs::write(&file, serde_json::to_vec(&body).unwrap()).unwrap();
            config.push_str(&format!(
                "header = \"Content-Type: application/json\"\ndata-binary = \"@{}\"\n",
                file.display()
            ));
        }
        let mut child = tokio::process::Command::new("curl")
            .args([
                "--silent",
                "--show-error",
                "--max-time",
                "3",
                "--config",
                "-",
                "--write-out",
                "\n%{http_code}",
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        input.write_all(config.as_bytes()).await.unwrap();
        input.shutdown().await.unwrap();
        drop(input);
        let output = child.wait_with_output().await.unwrap();
        assert!(output.status.success(), "{output:?}");
        let text = String::from_utf8(output.stdout).unwrap();
        let (body, status) = text.rsplit_once('\n').unwrap();
        (status.parse().unwrap(), serde_json::from_str(body).unwrap())
    }

    async fn read_http(
        &self,
        command: &DeliveryCommand,
        original: &serde_json::Value,
    ) -> serde_json::Value {
        let url = self.f.lookup_url(original).replace(
            &format!("/{}?", self.f.request.operation_key),
            &format!(
                "/{}/delivery-operations/{}?",
                command.workspace_operation_key, command.operation_key
            ),
        );
        let (status, result) = call(&self.f.app, "GET", &url, &self.f.token, String::new()).await;
        assert_eq!(status, StatusCode::OK, "{result}");
        result
    }
}

#[cfg(feature = "oci-integration")]
#[path = "oci_delivery.rs"]
mod oci_delivery;

#[cfg(feature = "postgres-integration")]
#[path = "postgres_delivery.rs"]
mod postgres_delivery;

fn latest(readback: &serde_json::Value) -> &serde_json::Value {
    if readback["reconciledReceipt"].is_object() {
        &readback["reconciledReceipt"]
    } else {
        &readback["receipt"]
    }
}

#[cfg(unix)]
#[tokio::test]
async fn sdlc_task_delivery_actual_versions_health_acceptance_rollback_and_crash_recovery() {
    let mut t = ActualDelivery::new().await;
    let (a, original_a) = t.build("A", "version=A\nhealth=ok\nacceptance=ok\n").await;
    let mut missing = a.clone();
    missing.operation_key = "deploy:artifact-unavailable".into();
    missing.artifact_id = Some(Uuid::new_v4());
    let (code, unavailable) = t.execute(&missing, None).await;
    assert_eq!(code, 2);
    let unavailable = unavailable.unwrap();
    assert_eq!(latest(&unavailable)["status"], "unavailable");
    assert_eq!(
        unavailable["currentManifestSha256"],
        serde_json::Value::Null
    );
    // Missing confirmed history is unavailable, and no caller-selected rollback artifact is accepted.
    let mut empty = a.clone();
    empty.operation_key = "rollback:empty".into();
    empty.action = DeliveryAction::Rollback;
    empty.artifact_id = None;
    empty.expected_manifest_sha256 = Some("a".repeat(64));
    assert_eq!(t.execute(&empty, None).await.0, 1); // Empty target CAS cannot match a fabricated current hash.
    let (code, first) = t.execute(&a, None).await;
    assert_eq!(code, 0);
    let first = first.unwrap();
    assert_eq!(latest(&first)["status"], "verified");
    let manifest_a = first["currentManifestSha256"].as_str().unwrap().to_string();
    assert_eq!(first["confirmedManifestSha256"], manifest_a);
    let pointer_time = std::fs::metadata(t.target_root.join("current.json"))
        .unwrap()
        .modified()
        .unwrap();
    let replay = t.execute(&a, None).await.1.unwrap();
    assert_eq!(replay["receipt"], first["receipt"]);
    assert_eq!(
        std::fs::metadata(t.target_root.join("current.json"))
            .unwrap()
            .modified()
            .unwrap(),
        pointer_time
    );
    let mut changed = a.clone();
    changed.original.fencing_token += 1;
    assert_eq!(t.execute(&changed, None).await.0, 1);
    let mut changed = a.clone();
    changed.artifact_id = Some(Uuid::new_v4());
    assert_eq!(t.execute(&changed, None).await.0, 1);
    assert_eq!(
        t.read_http(&a, &original_a).await["receipt"],
        first["receipt"]
    );
    let original_url = t.f.lookup_url(&original_a);
    let (status, replay) = t
        .curl(
            "POST",
            &format!("/api/v1/projects/{}/sdlc/workspace-operations", t.f.project),
            Some(serde_json::to_value(&t.f.request).unwrap()),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(replay, original_a);
    assert_eq!(t.curl("GET", &original_url, None).await.0, 200);
    assert_eq!(
        t.curl(
            "GET",
            &original_url.replace("?", "/candidate-evidence?"),
            None
        )
        .await
        .1["candidateObserved"],
        true
    );
    let delivery_url =
        original_url.replace("?", &format!("/delivery-operations/{}?", a.operation_key));
    assert_eq!(
        t.curl("GET", &delivery_url, None).await.1["receipt"],
        first["receipt"]
    );
    assert_eq!(
        t.curl(
            "POST",
            &format!(
                "/api/v1/projects/{}/sdlc/workspace-operations/{}/delivery-operations",
                t.f.project, a.workspace_operation_key
            ),
            Some(serde_json::to_value(&a).unwrap())
        )
        .await
        .0,
        503
    );
    let record = t
        .target_root
        .join("operations")
        .join(hash(a.operation_key.as_bytes()))
        .join("result.json");
    let saved = std::fs::read(&record).unwrap();
    let mut forged: serde_json::Value = serde_json::from_slice(&saved).unwrap();
    forged["dispatchAllowed"] = serde_json::json!(true);
    std::fs::write(&record, serde_json::to_vec(&forged).unwrap()).unwrap();
    let get =
        t.f.lookup_url(&original_a)
            .replace("?", &format!("/delivery-operations/{}?", a.operation_key));
    assert_eq!(
        call(&t.f.app, "GET", &get, &t.f.token, String::new())
            .await
            .0,
        StatusCode::CONFLICT
    );
    std::fs::write(record, saved).unwrap();
    let mut no_mode = t.process(&a);
    no_mode.env_remove("CICD_LOCAL_DELIVERY_MODE");
    assert_eq!(no_mode.output().await.unwrap().status.code(), Some(1));
    let mut readonly = t.process(&a);
    readonly.env("CICD_LOCAL_DELIVERY_TOKEN", &t.f.read_token);
    assert_eq!(readonly.output().await.unwrap().status.code(), Some(1));
    let mut readonly = t.process(&a);
    readonly
        .env("CICD_LOCAL_DELIVERY_TOKEN", &t.f.read_token)
        .arg("--readback");
    assert_eq!(readonly.output().await.unwrap().status.code(), Some(0));
    let (status, _) = call(
        &t.f.app,
        "POST",
        &format!(
            "/api/v1/projects/{}/sdlc/workspace-operations/{}/delivery-operations",
            t.f.project, a.workspace_operation_key
        ),
        &t.f.token,
        serde_json::to_string(&a).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);

    let (mut b, original_b) = t
        .build("B", "version=B\nhealth=failed\nacceptance=ok\n")
        .await;
    b.expected_manifest_sha256 = Some(manifest_a.clone());
    let (code, bad) = t.execute(&b, None).await;
    assert_eq!(code, 2);
    let bad = bad.unwrap();
    assert_eq!(latest(&bad)["status"], "failed");
    assert_eq!(latest(&bad)["health"]["httpStatus"], 503);
    assert_eq!(bad["confirmedManifestSha256"], manifest_a);
    let manifest_b = bad["currentManifestSha256"].as_str().unwrap().to_string();
    assert_ne!(manifest_b, manifest_a);
    let mut rollback = b.clone();
    rollback.operation_key = "rollback:B".into();
    rollback.action = DeliveryAction::Rollback;
    rollback.artifact_id = None;
    rollback.expected_manifest_sha256 = Some(manifest_b);
    let (code, restored) = t.execute(&rollback, None).await;
    assert_eq!(code, 0);
    let restored = restored.unwrap();
    assert_eq!(restored["currentManifestSha256"], manifest_a);
    assert_eq!(latest(&restored)["originalOperation"], original_b);
    assert_eq!(
        latest(&restored)["servedArtifact"]["bodySha256"],
        hash(b"version=A\nhealth=ok\nacceptance=ok\n")
    );
    assert_eq!(
        t.execute(&b, None).await.1.unwrap()["receipt"],
        bad["receipt"]
    ); // Historical failed result stays failed after rollback.

    let (mut c, _) = t
        .build("C", "version=C\nhealth=ok\nacceptance=failed\n")
        .await;
    c.expected_manifest_sha256 = Some(manifest_a.clone());
    let bad = t.execute(&c, None).await.1.unwrap();
    assert_eq!(latest(&bad)["status"], "failed");
    assert_eq!(latest(&bad)["health"]["status"], "verified");
    assert_eq!(latest(&bad)["acceptance"]["httpStatus"], 422);
    assert_eq!(bad["confirmedManifestSha256"], manifest_a);
    let mut rollback = c.clone();
    rollback.operation_key = "rollback:C".into();
    rollback.action = DeliveryAction::Rollback;
    rollback.artifact_id = None;
    rollback.expected_manifest_sha256 = bad["currentManifestSha256"].as_str().map(str::to_owned);
    assert_eq!(t.execute(&rollback, None).await.0, 0);

    let (mut d, original_d) = t.build("D", "version=D\nhealth=ok\nacceptance=ok\n").await;
    d.expected_manifest_sha256 = Some(manifest_a.clone());
    let controls = t.target_root.parent().unwrap();
    let block = controls.join("block-health");
    let entered = controls.join("entered-block-health");
    std::fs::write(&block, b"hold").unwrap();
    let mut child = t.process(&d).spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !entered.exists() {
        assert!(
            Instant::now() < deadline,
            "delivery did not enter health probe"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    child.start_kill().unwrap();
    let killed = child.wait().await.unwrap();
    assert!(!killed.success());
    std::fs::remove_file(block).unwrap();
    let unknown = t.read_http(&d, &original_d).await;
    assert_eq!(latest(&unknown)["status"], "unknown");
    assert_eq!(unknown["reconciliationNeeded"], true);
    assert_eq!(unknown["confirmedManifestSha256"], manifest_a);
    let after_crash = std::fs::metadata(t.target_root.join("current.json"))
        .unwrap()
        .modified()
        .unwrap();
    let mut other = d.clone();
    other.operation_key = "deploy:blocked-by-unknown".into();
    other.expected_manifest_sha256 = unknown["currentManifestSha256"].as_str().map(str::to_owned);
    assert_eq!(t.execute(&other, None).await.0, 1);
    let (code, recovered) = t.execute(&d, Some("--reconcile")).await;
    assert_eq!(code, 0);
    let recovered = recovered.unwrap();
    assert_eq!(recovered["receipt"], unknown["receipt"]);
    assert_eq!(latest(&recovered)["status"], "verified");
    assert_eq!(recovered["reconciliationNeeded"], false);
    assert_eq!(
        std::fs::metadata(t.target_root.join("current.json"))
            .unwrap()
            .modified()
            .unwrap(),
        after_crash
    );
    assert_eq!(latest(&recovered)["dispatchAllowed"], false);
    assert_eq!(latest(&recovered)["sdlcAcceptanceVerified"], false);
    let mut network = d.clone();
    network.operation_key = "deploy:version-unavailable".into();
    network.expected_manifest_sha256 = recovered["currentManifestSha256"]
        .as_str()
        .map(str::to_owned);
    let block = t.target_root.parent().unwrap().join("block-version");
    std::fs::write(&block, b"hold").unwrap();
    let (code, lost) = t.execute(&network, None).await;
    assert_eq!(code, 2);
    let lost = lost.unwrap();
    assert_eq!(latest(&lost)["status"], "unknown");
    assert_eq!(lost["reconciliationNeeded"], true);
    std::fs::remove_file(block).unwrap();
    let recovered = t.execute(&network, Some("--reconcile")).await.1.unwrap();
    assert_eq!(latest(&recovered)["status"], "verified");
    assert_eq!(recovered["receipt"], lost["receipt"]);
    sqlx::query("UPDATE api_tokens SET revoked_at=clock_timestamp() WHERE id=$1")
        .bind(t.f.token_id)
        .execute(&t.f.pool)
        .await
        .unwrap();
    assert_eq!(t.execute(&network, Some("--readback")).await.0, 1);
    println!(
        "ACTUAL_DELIVERY_SCENARIOS=runner-artifact,unavailable,A-verified,B-health-failed,rollback-A,C-acceptance-failed,rollback-A,D-SIGKILL,readback-only-recovery,version-unavailable-recovery"
    );
}

#[tokio::test]
async fn sdlc_task_delivery_http_dispatch_remains_closed_and_lookup_is_scoped() {
    let f = Fixture::new().await;
    let (status, receipt) = f.post(&f.request).await;
    assert_eq!(status, StatusCode::OK);
    let b = &f.request.binding;
    let command = DeliveryCommand {
        operation_key: "deploy:blocked".into(),
        workspace_operation_key: f.request.operation_key.clone(),
        original: cicd::domain::sdlc_workspace::WorkspaceOperationLookup {
            request_hash: receipt["requestHash"].as_str().unwrap().into(),
            task_id: b.task_id,
            root_task_id: b.root_task_id,
            assignment_id: b.assignment_id,
            execution_id: b.execution_id,
            fencing_token: b.fencing_token,
        },
        action: DeliveryAction::Deploy,
        artifact_id: Some(Uuid::new_v4()),
        expected_manifest_sha256: None,
    };
    let url = format!(
        "/api/v1/projects/{}/sdlc/workspace-operations/{}/delivery-operations",
        f.project, command.workspace_operation_key
    );
    assert_eq!(
        call(
            &f.app,
            "POST",
            &url,
            &f.token,
            serde_json::to_string(&command).unwrap()
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        call(
            &f.app,
            "POST",
            &url,
            &f.read_token,
            serde_json::to_string(&command).unwrap()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let mut fake = serde_json::to_value(&command).unwrap();
    fake["accepted"] = serde_json::json!(true);
    assert_eq!(
        call(&f.app, "POST", &url, &f.token, fake.to_string())
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let mut changed = command.clone();
    changed.original.fencing_token += 1;
    assert_eq!(
        call(
            &f.app,
            "POST",
            &url,
            &f.token,
            serde_json::to_string(&changed).unwrap()
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let get = f
        .lookup_url(&receipt)
        .replace("?", "/delivery-operations/deploy:blocked?");
    assert_eq!(
        call(&f.app, "GET", &get, &f.token, String::new()).await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    let mut fabricated = serde_json::to_value(command).unwrap();
    fabricated["action"] = serde_json::json!("rollback");
    assert_eq!(
        call(&f.app, "POST", &url, &f.token, fabricated.to_string())
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
}
