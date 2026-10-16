//! Opt-in native backend component proof. Tracker authority is a controlled HTTP peer.
//! MAIN owns the enclosing Base ComposeHelper v2 and exact TLS-resource cleanup.
use super::*;
use cicd::domain::{
    image_build::OciCandidateDescriptor,
    sdlc_workspace::WorkspaceOperationLookup,
    task_delivery::{DeliveryAction, DeliveryCommand, TaskDeliveryFence},
};
use serde_json::{Value, json};
use std::{
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    process::Stdio,
};
use tokio::io::AsyncReadExt;

struct NativeInputs {
    docker: PathBuf,
    compose: PathBuf,
    daemon: String,
    backend: String,
    candidate_image: String,
    reachability_container: String,
    reachability_url: reqwest::Url,
    reachability_sha256: String,
    parent: String,
    project: String,
    network: String,
    volume: String,
    workspace_volume: String,
    volume_root: PathBuf,
    compose_root: PathBuf,
    base: String,
}

fn required(name: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| panic!("explicit native fixture input required: {name}"))
}

fn anchored(path: &Path) {
    assert!(
        path.is_absolute(),
        "native fixture requires an absolute path"
    );
    for parent in path.ancestors() {
        let metadata = std::fs::symlink_metadata(parent).expect("native fixture path unavailable");
        assert!(
            !metadata.file_type().is_symlink(),
            "native fixture path is linked"
        );
    }
    assert!(
        path.canonicalize().is_ok_and(|p| p == path),
        "native fixture path is not canonical"
    );
}

impl NativeInputs {
    fn read() -> Self {
        assert_eq!(
            std::env::consts::OS,
            "linux",
            "native backend requires Linux"
        );
        assert_eq!(required("CICD_RUNNER_MODE"), "docker");
        assert_ne!(
            std::env::var("CICD_TEST_OCI_PAUSE_AFTER_VERIFIED_CHECKS").as_deref(),
            Ok("1"),
            "positive aggregate cannot use the crash-window fixture"
        );
        assert!(
            required("DOCKER_HOST").starts_with("tcp://"),
            "native fixture requires existing TLS authority"
        );
        assert!(
            required("DOCKER_TLS_VERIFY") == "1",
            "TLS verification required"
        );
        let endpoint = reqwest::Url::parse(&required("DOCKER_HOST")).unwrap();
        assert!(
            endpoint.host_str().is_some()
                && endpoint.port().is_some()
                && endpoint.username().is_empty()
                && endpoint.password().is_none()
                && endpoint.query().is_none()
                && endpoint.fragment().is_none()
                && matches!(endpoint.path(), "" | "/"),
            "explicit credential-free TLS engine endpoint required"
        );
        let certificates = PathBuf::from(required("DOCKER_CERT_PATH"));
        for name in ["ca.pem", "cert.pem", "key.pem"] {
            assert!(
                certificates.join(name).is_file(),
                "existing TLS mount unavailable"
            );
        }
        // Both the original Docker preparation and image-build custody use this production path.
        assert!(
            required("CICD_RUNNER_WORKSPACE_ROOT") == "/workspaces",
            "fixture must match production Docker workspace path"
        );
        let project = required("CICD_TEST_OCI_PROJECT");
        let suffix = project
            .strip_prefix("sdlc-qa-forge-oci-")
            .expect("own OCI QA project required");
        assert!(
            (12..=32).contains(&suffix.len()) && suffix.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid QA project suffix"
        );
        let parent = format!("sdlc-qa-forge-{suffix}");
        let network = required("CICD_TEST_OCI_NETWORK");
        let volume = required("CICD_TEST_OCI_VOLUME");
        assert!(network == format!("{parent}_qa"), "network owner mismatch");
        assert!(
            volume == format!("{parent}_delivery-qa"),
            "data volume owner mismatch"
        );
        let workspace_volume = required("CICD_RUNNER_WORKSPACE_VOLUME");
        assert!(
            workspace_volume == format!("{parent}_workspaces"),
            "own workspace volume required"
        );
        let docker = PathBuf::from(required("CICD_TEST_OCI_DOCKER_BIN"));
        let compose = PathBuf::from(required("CICD_TEST_OCI_COMPOSE_BIN"));
        let volume_root = PathBuf::from(required("CICD_TEST_OCI_VOLUME_ROOT"));
        let compose_root = PathBuf::from(required("CICD_TEST_OCI_COMPOSE_ROOT"));
        for path in [
            &docker,
            &compose,
            &volume_root,
            &compose_root,
            Path::new("/workspaces"),
        ] {
            anchored(path);
        }
        assert!(
            docker.is_file() && compose.is_file(),
            "qualified Docker/Compose binaries required"
        );
        assert!(
            volume_root.is_dir() && compose_root.is_dir(),
            "own fixture directories required"
        );
        assert_eq!(volume_root, Path::new("/delivery-qa"));
        assert_eq!(required("CICD_RUNNER_DOCKER_NETWORK"), network);
        assert!(
            compose_root.starts_with(&volume_root) && compose_root != volume_root,
            "compose files must remain in the exact owned data volume"
        );
        assert!(
            std::fs::read_dir(&compose_root).unwrap().next().is_none(),
            "fresh compose directory required"
        );
        let path = std::env::split_paths(&std::env::var_os("PATH").expect("backend PATH required"))
            .map(|p| p.join("docker"))
            .find(|p| p.is_file())
            .expect("backend Docker CLI unavailable");
        assert!(
            path.canonicalize().unwrap() == docker,
            "fixture and backend must use the same Docker CLI"
        );
        let base = required("CICD_TEST_IMAGE_BUILD_BASE");
        let (name, digest) = base
            .split_once("@sha256:")
            .expect("explicit immutable Python base required");
        assert!(
            !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"/._:-".contains(&b)),
            "invalid pinned base name"
        );
        assert!(
            cicd::domain::task_delivery::digest(digest),
            "full base digest required"
        );
        let candidate_image = required("CICD_TEST_NATIVE_CANDIDATE_IMAGE");
        assert!(cicd::domain::image_build::valid_image_id(&candidate_image));
        let binary_sha256 = required("CICD_TEST_NATIVE_BINARY_SHA256");
        assert!(cicd::domain::task_delivery::digest(&binary_sha256));
        let executable = std::env::current_exe().unwrap();
        anchored(&executable);
        let mut binary = std::fs::File::open(executable).unwrap();
        let mut digest = Sha256::new();
        std::io::copy(&mut binary, &mut digest).unwrap();
        assert_eq!(format!("{:x}", digest.finalize()), binary_sha256);
        let reachability_url =
            reqwest::Url::parse(&required("CICD_TEST_NATIVE_REACHABILITY_URL")).unwrap();
        assert!(
            reachability_url.scheme() == "http"
                && reachability_url.host_str().is_some()
                && reachability_url.port() == Some(8000)
                && reachability_url.username().is_empty()
                && reachability_url.password().is_none()
                && reachability_url.query().is_none()
                && reachability_url.fragment().is_none(),
            "explicit direct container-IP:8000 reachability URL required"
        );
        let reachability_sha256 = required("CICD_TEST_NATIVE_REACHABILITY_SHA256");
        assert!(cicd::domain::task_delivery::digest(&reachability_sha256));
        Self {
            docker,
            compose,
            daemon: required("CICD_RUNNER_DOCKER_DAEMON_ID"),
            backend: required("CICD_TEST_NATIVE_BACKEND_CONTAINER"),
            candidate_image,
            reachability_container: required("CICD_TEST_NATIVE_REACHABILITY_CONTAINER"),
            reachability_url,
            reachability_sha256,
            parent,
            project,
            network,
            volume,
            workspace_volume,
            volume_root,
            compose_root,
            base,
        }
    }

    async fn metadata(&self, args: &[&str]) -> Vec<u8> {
        self.metadata_with(&self.docker, args).await
    }

    async fn metadata_with(&self, binary: &Path, args: &[&str]) -> Vec<u8> {
        let mut command = tokio::process::Command::new(binary);
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap())
            .env("DOCKER_CONFIG", &self.compose_root)
            .env("DOCKER_HOST", required("DOCKER_HOST"))
            .env("DOCKER_TLS_VERIFY", "1")
            .env("DOCKER_CERT_PATH", required("DOCKER_CERT_PATH"))
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = command
            .spawn()
            .unwrap_or_else(|_| panic!("native metadata process unavailable"));
        let observed = tokio::time::timeout(StdDuration::from_secs(30), async {
            let mut bytes = Vec::new();
            child
                .stdout
                .take()
                .unwrap()
                .take(65537)
                .read_to_end(&mut bytes)
                .await?;
            let status = child.wait().await?;
            Ok::<_, std::io::Error>((status.success(), bytes))
        })
        .await;
        if let Ok(Ok((true, bytes))) = observed {
            assert!(bytes.len() <= 65536, "native metadata exceeds bound");
            bytes
        } else {
            let _ = child.kill().await;
            panic!("bounded native metadata unavailable");
        }
    }

    async fn json(&self, args: &[&str]) -> Value {
        serde_json::from_slice(&self.metadata(args).await)
            .expect("native metadata JSON unavailable")
    }

    async fn verify_ownership(&self) {
        let id = self.metadata(&["info", "--format", "{{.ID}}"]).await;
        assert!(
            std::str::from_utf8(&id).unwrap().trim() == self.daemon,
            "actual TLS daemon mismatch"
        );
        let backend = self.json(&["container", "inspect", &self.backend]).await;
        let backend = &backend[0];
        let labels = &backend["Config"]["Labels"];
        assert!(
            labels["com.docker.compose.project"] == self.parent
                && labels["sdlc.task"] == "forge-task-delivery"
                && labels["com.docker.compose.service"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty())
                && labels["sdlc.purpose"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty()),
            "native process must be in the own backend QA service"
        );
        assert_eq!(backend["State"]["Running"], true);
        assert_eq!(backend["Image"], self.candidate_image);
        assert_eq!(labels["sdlc.lifecycle"], "disposable");
        assert!(
            labels["sdlc.cleanup-id"]
                .as_str()
                .is_some_and(|s| !s.is_empty())
        );
        for version in [
            self.metadata(&["compose", "version", "--short"]).await,
            self.metadata_with(&self.compose, &["version", "--short"])
                .await,
        ] {
            assert_eq!(
                std::str::from_utf8(&version)
                    .unwrap()
                    .trim()
                    .trim_start_matches('v'),
                "5.5.1"
            );
        }
        assert!(
            backend["Config"]["Hostname"].as_str().unwrap()
                == std::fs::read_to_string("/etc/hostname").unwrap().trim(),
            "native test must execute in that backend container"
        );
        let mounts = &backend["Mounts"];
        for (destination, volume) in [
            ("/workspaces", &self.workspace_volume),
            (self.volume_root.to_str().unwrap(), &self.volume),
        ] {
            assert!(
                mounts
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|m| m["Type"] == "volume"
                        && m["Name"] == *volume
                        && m["Destination"] == destination
                        && m["RW"] == true),
                "own same-engine backend volume mapping required"
            );
        }
        let networks = &backend["NetworkSettings"]["Networks"];
        assert!(
            networks.get(&self.network).is_some(),
            "backend must share the actual internal OCI network for probes"
        );
        let network = self.json(&["network", "inspect", &self.network]).await;
        assert!(
            network[0]["Internal"] == true,
            "isolated OCI QA network required"
        );
        for object in [
            network[0].clone(),
            self.json(&["volume", "inspect", &self.volume]).await[0].clone(),
            self.json(&["volume", "inspect", &self.workspace_volume])
                .await[0]
                .clone(),
        ] {
            assert!(
                object["Labels"]["com.docker.compose.project"] == self.parent
                    && object["Labels"]["sdlc.task"] == "forge-task-delivery"
                    && object["Labels"]["sdlc.purpose"]
                        .as_str()
                        .is_some_and(|s| !s.is_empty())
                    && object["Labels"]["sdlc.cleanup-id"]
                        .as_str()
                        .is_some_and(|s| !s.is_empty())
                    && object["Labels"]["sdlc.cleanup-id"] == labels["sdlc.cleanup-id"]
                    && object["Labels"]["sdlc.lifecycle"] == "disposable",
                "resource owner labels mismatch"
            );
        }
        // Production prepare independently verifies exact Buildx/driver/endpoint before any build.
        let base = self.json(&["image", "inspect", &self.base]).await;
        assert!(
            base[0]["Os"] == "linux",
            "pinned native application base must already exist on this engine"
        );
        let probe = self
            .json(&["container", "inspect", &self.reachability_container])
            .await;
        assert_eq!(probe[0]["State"]["Running"], true);
        assert_eq!(
            probe[0]["Config"]["Labels"]["com.docker.compose.project"],
            self.parent
        );
        assert_eq!(
            probe[0]["Config"]["Labels"]["sdlc.task"],
            "forge-task-delivery"
        );
        let ip: std::net::IpAddr =
            probe[0]["NetworkSettings"]["Networks"][&self.network]["IPAddress"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap();
        let host: std::net::IpAddr = self.reachability_url.host_str().unwrap().parse().unwrap();
        assert_eq!(
            ip, host,
            "reachability must use the inspected same-engine container IP"
        );
        assert_eq!(
            sha(&http_bytes(self.reachability_url.clone()).await),
            self.reachability_sha256
        );
        assert!(
            self.metadata(&[
                "ps",
                "-a",
                "--filter",
                &format!("label=com.docker.compose.project={}", self.project),
                "--format",
                "{{.ID}}"
            ])
            .await
            .iter()
            .all(u8::is_ascii_whitespace),
            "application project must be empty before first HTTP publication"
        );
    }
}

async fn http_bytes(url: reqwest::Url) -> Vec<u8> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(StdDuration::from_secs(3))
        .build()
        .unwrap();
    let mut response = client
        .get(url)
        .send()
        .await
        .expect("direct same-engine HTTP reachability required");
    assert_eq!(response.status(), StatusCode::OK);
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.unwrap() {
        assert!(bytes.len() + chunk.len() <= 128 * 1024);
        bytes.extend_from_slice(&chunk);
    }
    bytes
}

fn private_dir(path: &Path) {
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(path)
        .expect("create-new owned fixture directory");
}

fn private_file(path: &Path, bytes: &[u8]) {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .expect("create-new fixture file");
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}

// Test-only owner installation config, in the exact existing Policy serialization order.
// It is not a receipt or a substitute for native observation.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PublishedPolicy {
    project_name: String,
    network_name: String,
    daemon_id: String,
    docker_bin: PathBuf,
    compose_bin: PathBuf,
    compose_root: PathBuf,
    volume_name: String,
    volume_root: PathBuf,
    data_file: PathBuf,
    data_sha256: String,
    checks: cicd::task_delivery::DeliveryPolicy,
}

fn configure_native(
    config: &mut cicd::config::RuntimeConfig,
    project: Uuid,
    intent: &TaskWorkspaceIntent,
    temp: &Path,
) {
    let inputs = NativeInputs::read();
    std::fs::set_permissions(temp, std::fs::Permissions::from_mode(0o700)).unwrap();
    private_dir(&config.artifacts.root);
    let root = inputs
        .volume_root
        .join(format!("oci-root-{}", project.simple()));
    private_dir(&root);
    for name in ["operations", "manifests", "docker-config"] {
        private_dir(&root.join(name));
    }
    private_file(&root.join("docker-config/config.json"), b"{}");
    let data_file = inputs
        .volume_root
        .join(format!("oci-data-{}.json", project.simple()));
    let data = br#"{"schemaVersion":1,"records":[{"id":"task-1","state":"queued"}]}"#;
    private_file(&data_file, data);
    std::fs::set_permissions(&data_file, std::fs::Permissions::from_mode(0o444)).unwrap();
    let checks: cicd::task_delivery::DeliveryPolicy = serde_json::from_value(json!({
        "origin":"http://127.0.0.1:8000", "healthPath":"/health", "healthBodySha256":sha(b"ok\n"),
        "acceptancePath":"/acceptance", "acceptanceBodySha256":sha(b"accepted:task-1:queued\n"),
        "taskExecution":{"environment":"nonproduction","environmentId":project,
            "repositoryId":intent.repository_id,"trackerInstanceId":intent.binding.tracker_instance_id,
            "trackerProjectId":intent.binding.tracker_project_id,"holderSubject":"qa-scheduler"}
    })).unwrap();
    checks.validate().unwrap();
    let policy = PublishedPolicy {
        project_name: inputs.project,
        network_name: inputs.network,
        daemon_id: inputs.daemon,
        docker_bin: inputs.docker,
        compose_bin: inputs.compose,
        compose_root: inputs.compose_root,
        volume_name: inputs.volume,
        volume_root: inputs.volume_root,
        data_file,
        data_sha256: sha(data),
        checks,
    };
    let bytes = serde_json::to_vec(&policy).unwrap();
    let policy_file = temp.join("oci-policy.json");
    private_file(&policy_file, &bytes);
    private_file(&root.join("owner.json"), &serde_json::to_vec(&json!({"schema":"forge/local-oci-target/v1","projectId":project,"policySha256":sha(&bytes)})).unwrap());
    config.runner.mode = cicd::config::RunnerMode::Docker;
    config.runner.workspace_volume = inputs.workspace_volume;
    config.runner.docker_network = policy.network_name;
    config.sdlc_workspace.local_delivery_kind = cicd::config::LocalDeliveryKind::Oci;
    config.sdlc_workspace.local_delivery_root = Some(root);
    config.sdlc_workspace.local_delivery_policy = Some(policy_file);
}

struct Built {
    pipeline: Uuid,
    job: Uuid,
    artifact: Uuid,
    storage: PathBuf,
    original: WorkspaceOperationReceipt,
    descriptor: OciCandidateDescriptor,
}

async fn build(f: &TaskPipeline, inputs: &NativeInputs) -> Built {
    let (status, created) = f.trigger(&f.intent).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "native task pipeline admission failed"
    );
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
    let queued: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM job_queue q JOIN execution_attempts a ON a.id=q.attempt_id WHERE q.job_id=$1 AND q.state='queued' AND a.status='queued')")
        .bind(job).fetch_one(&f.pool).await.unwrap();
    assert!(queued);
    let leases: i64 = sqlx::query_scalar("SELECT count(*) FROM job_leases WHERE job_id=$1")
        .bind(job)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(leases, 0);
    let locator = format!(
        "/api/v1/projects/{}/pipelines/{pipeline}/task-workspace",
        f.project
    );
    assert_eq!(
        f.request("GET", &locator, Value::Null).await,
        (StatusCode::OK, Value::Null)
    );
    *f.worker.lock().await = Some(tokio::spawn(cicd::runner::run_job_with_config(
        f.pool.clone(),
        job,
        f.running.clone(),
        cicd::runner::RuntimeRunnerConfig::from_config(&f.config),
    )));
    let deadline = std::time::Instant::now() + StdDuration::from_secs(30);
    let prepared = loop {
        let (status, value) = f.request("GET", &locator, Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        if !value.is_null() {
            break value;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "native preparation receipt deadline"
        );
        assert!(
            !f.worker.lock().await.as_ref().unwrap().is_finished(),
            "native worker stopped before preparation receipt"
        );
        tokio::time::sleep(StdDuration::from_millis(20)).await;
    };
    {
        let mut worker = f.worker.lock().await;
        tokio::time::timeout(StdDuration::from_secs(360), worker.as_mut().unwrap())
            .await
            .expect("native build deadline; retain unknown BuildKit custody")
            .unwrap();
        worker.take();
    }
    assert!(
        f.running.lock().await.is_empty(),
        "native build must have known terminal custody"
    );
    let terminal: (String,String,Option<i32>,String,bool,bool) = sqlx::query_as("SELECT p.status,a.status,a.exit_code,l.terminal_status,l.completion_received_at IS NOT NULL,l.completed_at IS NOT NULL FROM execution_attempts a JOIN job_leases l ON l.attempt_id=a.id JOIN jobs j ON j.id=a.job_id JOIN stages s ON s.id=j.stage_id JOIN pipelines p ON p.id=s.pipeline_id WHERE j.id=$1").bind(job).fetch_one(&f.pool).await.unwrap();
    assert_eq!(
        terminal,
        (
            "success".into(),
            "success".into(),
            Some(0),
            "success".into(),
            true,
            true
        ),
        "native terminal ACK required"
    );
    let (status, value) = f.request("GET", &locator, Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        value, prepared,
        "original receipt must survive build completion and cleanup"
    );
    let original: WorkspaceOperationReceipt = serde_json::from_value(value).unwrap();
    let r = &original.request;
    assert_eq!(r.binding, f.intent.binding);
    assert_eq!(r.source_commit, f.intent.source_commit);
    assert_eq!(r.role, WorkspaceRole::Developer);
    assert_eq!(r.binding.task_id, r.binding.root_task_id);
    let identity: (Uuid,Uuid,i64,String,bool) = sqlx::query_as("SELECT l.id,l.attempt_id,l.generation,l.runner_name,l.acknowledged_at IS NOT NULL FROM job_leases l WHERE l.job_id=$1")
        .bind(job).fetch_one(&f.pool).await.unwrap();
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
    assert!(original.physical_source_observed && !original.dispatch_allowed);
    assert!(!Path::new("/workspaces").join(&r.workspace_id).exists());
    let candidate = format!(
        "/api/v1/projects/{}/sdlc/workspace-operations/{}/candidate-evidence?requestHash={}&taskId={}&rootTaskId={}&assignmentId={}&executionId={}&fencingToken={}",
        f.project,
        r.operation_key,
        original.request_hash,
        r.binding.task_id,
        r.binding.root_task_id,
        r.binding.assignment_id,
        r.binding.execution_id,
        r.binding.fencing_token
    );
    let (status, evidence) = f.request("GET", &candidate, Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(evidence["candidateObserved"], true);
    assert_eq!(evidence["configSha256"], sha(f.yaml.as_bytes()));
    assert_eq!(
        evidence["operationReceipt"],
        serde_json::to_value(&original).unwrap()
    );
    for flag in [
        "dispatchAllowed",
        "deploymentVerified",
        "acceptanceVerified",
    ] {
        assert_eq!(evidence[flag], false);
    }
    let artifacts = evidence["artifacts"].as_array().unwrap();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0]["attemptId"], r.attempt_id.to_string());
    let artifact: Uuid = serde_json::from_value(artifacts[0]["artifactId"].clone()).unwrap();
    let storage: String =
        sqlx::query_scalar("SELECT storage_path FROM artifacts WHERE id=$1 AND attempt_id=$2")
            .bind(artifact)
            .bind(r.attempt_id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    let storage = PathBuf::from(storage);
    anchored(&storage);
    assert!(storage.starts_with(f.config.artifacts.root.canonicalize().unwrap()));
    assert_eq!(
        std::fs::metadata(&f.config.artifacts.root)
            .unwrap()
            .permissions()
            .mode()
            & 0o077,
        0
    );
    let bytes = std::fs::read(&storage).unwrap();
    assert_eq!(artifacts[0]["sha256"], sha(&bytes));
    assert_eq!(artifacts[0]["sizeBytes"], bytes.len() as u64);
    let descriptor: OciCandidateDescriptor = serde_json::from_slice(&bytes).unwrap();
    descriptor.validate().unwrap();
    assert_eq!(descriptor.source_commit, f.intent.source_commit);
    let control = Path::new("/workspaces/.compose-control")
        .join(format!("image-build-{}", r.attempt_id.simple()));
    let intent = std::fs::read(control.join("intent.json")).unwrap();
    let build_intent: Value = serde_json::from_slice(&intent).unwrap();
    assert_eq!(build_intent["schema"], "forge/backend-image-build/v1");
    assert_eq!(
        build_intent["scope"],
        json!({"project_id":f.project,"pipeline_id":pipeline,"job_id":job,"attempt_id":r.attempt_id,"lease_id":r.lease_id,"generation":r.workspace_generation})
    );
    assert_eq!(
        build_intent["repository_id"],
        f.intent.repository_id.to_string()
    );
    assert_eq!(build_intent["source_commit"], f.intent.source_commit);
    assert_eq!(build_intent["daemon_id"], inputs.daemon);
    for name in ["exported.json", "started.json"] {
        assert_eq!(
            std::fs::read(control.join(name)).unwrap(),
            sha(&intent).as_bytes()
        );
    }
    let completed_bytes = std::fs::read(control.join("completed.json")).unwrap();
    let completed: Value = serde_json::from_slice(&completed_bytes).unwrap();
    assert_eq!(completed["request_sha256"], sha(&intent));
    assert_eq!(completed["daemon_id"], inputs.daemon);
    assert_eq!(
        completed["descriptor"],
        serde_json::to_value(&descriptor).unwrap()
    );
    assert_eq!(
        std::fs::read(control.join("acknowledged.json")).unwrap(),
        std::fs::read(control.join("cleaned.json")).unwrap()
    );
    let acknowledgement: Value =
        serde_json::from_slice(&std::fs::read(control.join("acknowledged.json")).unwrap()).unwrap();
    assert_eq!(
        acknowledgement,
        json!({"request_sha256":sha(&intent),"completion_sha256":sha(&completed_bytes)})
    );
    assert_eq!(
        std::fs::read_to_string(control.join("image-id"))
            .unwrap()
            .trim(),
        descriptor.image_id
    );
    assert!(!control.join("tree").exists() && !control.join("output").exists());
    assert!(
        !Path::new("/workspaces/.compose-control")
            .join(format!("sdlc-build-job-{}", r.attempt_id.simple()))
            .exists()
    );
    let image = inputs
        .json(&["image", "inspect", &descriptor.image_id])
        .await;
    assert_eq!(image[0]["Id"], descriptor.image_id);
    assert_eq!(
        image[0]["Config"]["Labels"]["org.opencontainers.image.revision"],
        f.intent.source_commit
    );
    assert_eq!(
        image[0]["Config"]["Labels"]["sdlc.attempt"],
        r.attempt_id.to_string()
    );
    assert_eq!(
        image[0]["Config"]["Labels"]["forge.image-build-request"],
        sha(&intent)
    );
    let jobs = inputs
        .metadata(&[
            "ps",
            "-a",
            "--filter",
            &format!("label=sdlc.attempt={}", r.attempt_id),
            "--format",
            "{{.ID}}",
        ])
        .await;
    assert!(
        jobs.iter().all(u8::is_ascii_whitespace),
        "typed image build must not create a generic job/transfer container"
    );
    Built {
        pipeline,
        job,
        artifact,
        storage,
        original,
        descriptor,
    }
}

async fn install_devops_peer(f: &TaskPipeline, built: &Built) -> DeliveryCommand {
    let b = &f.intent.binding;
    let mut current = f.tracker.lock().await;
    let routing = current["receipt"]["task"]["routing"].clone();
    let agent = routing["policy"]["routes"]["devops"]["agent_id"].clone();
    let mut assignment = json!({"assignment_id":Uuid::new_v4(),"execution_id":Uuid::new_v4(),"task_id":b.task_id,"owner_subject":"qa-owner","role":"devops","stage":"Deployment","cycle":0,"attempt":1,"agent_id":agent,"routing":routing,"previous_result_id":Uuid::new_v4(),"fencing_token":4,"lease_id":Uuid::new_v4()});
    let hash = sha(&serde_json::to_vec(&canonical(assignment.clone())).unwrap());
    assignment["assignment_hash"] = json!(hash);
    let result_id = Uuid::new_v4();
    let source_result = json!({"source":"delivery","receipt":{"result_id":result_id,"assignment_id":b.assignment_id,"task_id":b.task_id,"cycle":0,"output":{"role":"developer","commit_sha":f.intent.source_commit},"next_stage":"Review"}});
    let now = Utc::now();
    *current = json!({"observed_at":now,"runtime_state":"running","receipt":{"task":{"task_id":b.task_id,"owner_subject":"qa-owner","version":2,"stage":"Deployment","status":"InProgress","runtime":"running","cycle":0,"waiting_reason":null,"routing":routing},"assignment":assignment,"lease":{"lease_id":assignment["lease_id"],"version":1,"holder_subject":"qa-scheduler","claimed_at":now,"heartbeat_at":now,"expires_at":now+Duration::seconds(30)},"binding":{"chat_id":Uuid::new_v4(),"run_id":Uuid::new_v4(),"machine_subject":"qa-devops"}}});
    current["native_result"] = source_result;
    let fence = TaskDeliveryFence {
        development_result_id: result_id,
        assignment_id: serde_json::from_value(assignment["assignment_id"].clone()).unwrap(),
        execution_id: serde_json::from_value(assignment["execution_id"].clone()).unwrap(),
        agent_id: serde_json::from_value(agent).unwrap(),
        lease_id: serde_json::from_value(assignment["lease_id"].clone()).unwrap(),
        fencing_token: 4,
        expected_lease_version: 1,
        assignment_hash: hash,
    };
    let command = DeliveryCommand {
        operation_key: "native:deploy".into(),
        workspace_operation_key: built.original.request.operation_key.clone(),
        original: WorkspaceOperationLookup {
            request_hash: built.original.request_hash.clone(),
            task_id: b.task_id,
            root_task_id: b.root_task_id,
            assignment_id: b.assignment_id,
            execution_id: b.execution_id,
            fencing_token: b.fencing_token,
        },
        action: DeliveryAction::Deploy,
        artifact_id: Some(built.artifact),
        expected_manifest_sha256: None,
        task_fence: Some(fence),
    };
    command.validate().unwrap();
    command
}

async fn deliver(f: &TaskPipeline, inputs: &NativeInputs, built: &Built) {
    let command = install_devops_peer(f, built).await;
    let policy: Value = serde_json::from_slice(
        &std::fs::read(
            f.config
                .sdlc_workspace
                .local_delivery_policy
                .as_ref()
                .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let data_file = Path::new(policy["dataFile"].as_str().unwrap());
    let data_before = std::fs::read(data_file).unwrap();
    let path = format!(
        "/api/v1/projects/{}/sdlc/workspace-operations/{}/delivery-operations",
        f.project, command.workspace_operation_key
    );
    // A wrong current holder is denied before any original delivery journal/Compose effect.
    f.tracker.lock().await["receipt"]["lease"]["holder_subject"] = json!("foreign");
    assert_eq!(
        f.request("POST", &path, serde_json::to_value(&command).unwrap())
            .await
            .0,
        StatusCode::CONFLICT
    );
    let root = f
        .config
        .sdlc_workspace
        .local_delivery_root
        .as_ref()
        .unwrap();
    let directory = root
        .join("operations")
        .join(sha(command.operation_key.as_bytes()));
    assert!(!directory.exists());
    f.tracker.lock().await["receipt"]["lease"]["holder_subject"] = json!("qa-scheduler");
    let (status, delivered) = f
        .request("POST", &path, serde_json::to_value(&command).unwrap())
        .await;
    assert_eq!(status, StatusCode::OK, "native OCI HTTP delivery failed");
    let latest = if delivered["reconciledReceipt"].is_null() {
        &delivered["receipt"]
    } else {
        &delivered["reconciledReceipt"]
    };
    assert_eq!(latest["scope"], "task_nonproduction_delivery");
    assert_eq!(latest["status"], "verified");
    assert_eq!(latest["dispatchAllowed"], true);
    assert_eq!(latest["sdlcAcceptanceVerified"], false);
    assert_eq!(
        latest["originalOperation"],
        serde_json::to_value(&built.original).unwrap()
    );
    assert_eq!(latest["imageId"], built.descriptor.image_id);
    for (key, digest) in [
        ("health", sha(b"ok\n")),
        ("acceptance", sha(b"accepted:task-1:queued\n")),
    ] {
        assert_eq!(latest[key]["httpStatus"], 200);
        assert_eq!(latest[key]["status"], "verified");
        assert_eq!(latest[key]["bodySha256"], digest);
    }
    assert_eq!(
        latest["version"]["bodySha256"],
        delivered["confirmedManifestSha256"]
    );
    assert_eq!(latest["compatibility"]["status"], "verified");
    assert_eq!(latest["compatibility"]["httpStatus"], 200);
    assert_eq!(latest["compatibility"]["bodySha256"], sha(&data_before));
    assert_eq!(latest["dataSha256"], sha(&data_before));
    assert_eq!(
        delivered["currentManifestSha256"],
        delivered["confirmedManifestSha256"]
    );
    assert_eq!(delivered["reconciliationNeeded"], false);
    let manifest_hash = delivered["confirmedManifestSha256"].as_str().unwrap();
    assert!(cicd::domain::task_delivery::digest(manifest_hash));
    let manifest_file = root.join("manifests").join(format!("{manifest_hash}.json"));
    let manifest_bytes = std::fs::read(&manifest_file).unwrap();
    assert_eq!(sha(&manifest_bytes), manifest_hash);
    let manifest: Value = serde_json::from_slice(&manifest_bytes).unwrap();
    assert_eq!(
        manifest["descriptor"],
        serde_json::to_value(&built.descriptor).unwrap()
    );
    assert_eq!(manifest["dataSchemaVersion"], 1);
    assert_eq!(manifest["dataSha256"], sha(&data_before));
    let container = latest["containerId"].as_str().unwrap();
    let actual = inputs.json(&["container", "inspect", container]).await;
    assert_eq!(actual[0]["Image"], built.descriptor.image_id);
    assert_eq!(actual[0]["State"]["Running"], true);
    assert_eq!(
        actual[0]["Config"]["Labels"]["com.docker.compose.project"],
        inputs.project
    );
    assert_eq!(actual[0]["HostConfig"]["ReadonlyRootfs"], true);
    assert_eq!(actual[0]["HostConfig"]["Privileged"], false);
    assert!(
        actual[0]["Config"]["Env"]
            .as_array()
            .unwrap()
            .iter()
            .all(|value| {
                let value = value.as_str().unwrap();
                !value.starts_with("DOCKER_") && !value.starts_with("CICD_")
            })
    );
    let mounts = &actual[0]["Mounts"];
    assert_eq!(mounts.as_array().unwrap().len(), 2);
    assert!(mounts.as_array().unwrap().iter().all(|m| m["RW"] == false));
    let volume = inputs.json(&["volume", "inspect", &inputs.volume]).await;
    let mountpoint = Path::new(volume[0]["Mountpoint"].as_str().unwrap());
    for (local, destination) in [
        (manifest_file.as_path(), "/forge/manifest.json"),
        (data_file, "/forge/data.json"),
    ] {
        let expected = mountpoint.join(local.strip_prefix(&inputs.volume_root).unwrap());
        assert!(mounts.as_array().unwrap().iter().any(|mount| {
            mount["Destination"] == destination && mount["Source"] == expected.to_str().unwrap()
        }));
    }
    let networks = actual[0]["NetworkSettings"]["Networks"]
        .as_object()
        .unwrap();
    assert_eq!(networks.len(), 1);
    let ip: std::net::IpAddr = networks[&inputs.network]["IPAddress"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    for (path, expected) in [
        ("/.forge/version", manifest_bytes.as_slice()),
        ("/health", b"ok\n".as_slice()),
        ("/acceptance", b"accepted:task-1:queued\n".as_slice()),
        ("/compatibility", data_before.as_slice()),
    ] {
        let url = reqwest::Url::parse(&format!("http://{ip}:8000{path}")).unwrap();
        assert_eq!(http_bytes(url).await, expected);
    }
    assert_eq!(std::fs::read(data_file).unwrap(), data_before);
    let lookup = &command.original;
    let get = format!(
        "{path}/{}?requestHash={}&taskId={}&rootTaskId={}&assignmentId={}&executionId={}&fencingToken={}",
        command.operation_key,
        lookup.request_hash,
        lookup.task_id,
        lookup.root_task_id,
        lookup.assignment_id,
        lookup.execution_id,
        lookup.fencing_token
    );
    f.tracker.lock().await["runtime_state"] = json!("unknown");
    assert_eq!(
        f.request("GET", &get, Value::Null).await,
        (StatusCode::OK, delivered.clone())
    );
    assert_eq!(
        f.request("POST", &path, serde_json::to_value(&command).unwrap())
            .await,
        (StatusCode::OK, delivered.clone())
    );
    let mut changed = command.clone();
    changed.expected_manifest_sha256 = delivered["confirmedManifestSha256"]
        .as_str()
        .map(str::to_owned);
    assert_eq!(
        f.request("POST", &path, serde_json::to_value(&changed).unwrap())
            .await
            .0,
        StatusCode::CONFLICT
    );
    f.tracker.lock().await["runtime_state"] = json!("running");
    let compose_file = inputs
        .compose_root
        .join(format!("{}.json", sha(command.operation_key.as_bytes())));
    let compose_before = std::fs::read(&compose_file).unwrap();
    let mut denied = command.clone();
    denied.operation_key = "native:stale-fence".into();
    denied.expected_manifest_sha256 = Some(manifest_hash.into());
    denied.task_fence.as_mut().unwrap().expected_lease_version += 1;
    assert_eq!(
        f.request("POST", &path, serde_json::to_value(&denied).unwrap())
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert!(
        !root
            .join("operations")
            .join(sha(denied.operation_key.as_bytes()))
            .exists()
    );
    assert!(
        !inputs
            .compose_root
            .join(format!("{}.json", sha(denied.operation_key.as_bytes())))
            .exists()
    );
    denied.operation_key = "native:tampered-artifact".into();
    denied.task_fence = command.task_fence.clone();
    let artifact_bytes = std::fs::read(&built.storage).unwrap();
    std::fs::write(&built.storage, b"tampered native artifact\n").unwrap();
    let (status, body) = f
        .request("POST", &path, serde_json::to_value(&denied).unwrap())
        .await;
    // Restore only the owned test mutation, even when the denial assertion fails.
    std::fs::write(&built.storage, &artifact_bytes).unwrap();
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        !root
            .join("operations")
            .join(sha(denied.operation_key.as_bytes()))
            .exists()
    );
    assert!(
        !inputs
            .compose_root
            .join(format!("{}.json", sha(denied.operation_key.as_bytes())))
            .exists()
    );
    assert_eq!(std::fs::read(&compose_file).unwrap(), compose_before);
    assert_eq!(std::fs::read_dir(&inputs.compose_root).unwrap().count(), 1);
    assert_eq!(
        std::fs::read_dir(root.join("operations")).unwrap().count(),
        1
    );
    assert_eq!(
        f.request("GET", &get, Value::Null).await,
        (StatusCode::OK, delivered.clone())
    );
    let after = inputs.json(&["container", "inspect", container]).await;
    assert_eq!(after[0]["Id"], actual[0]["Id"]);
    assert_eq!(
        after[0]["State"]["StartedAt"],
        actual[0]["State"]["StartedAt"]
    );
    assert_eq!(after[0]["Image"], actual[0]["Image"]);
    assert_eq!(std::fs::read(data_file).unwrap(), data_before);
    // Same pipeline/worker replay cannot create a second build attempt.
    assert_eq!(
        f.trigger(&f.intent).await.1["pipeline"]["id"],
        built.pipeline.to_string()
    );
    cicd::runner::run_job_with_config(
        f.pool.clone(),
        built.job,
        f.running.clone(),
        cicd::runner::RuntimeRunnerConfig::from_config(&f.config),
    )
    .await;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM job_leases WHERE job_id=$1")
        .bind(built.job)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let locator = format!(
        "/api/v1/projects/{}/pipelines/{}/task-workspace",
        f.project, built.pipeline
    );
    assert_eq!(
        f.request("GET", &locator, Value::Null).await,
        (
            StatusCode::OK,
            serde_json::to_value(&built.original).unwrap()
        )
    );
    assert!(
        !delivered.to_string().contains(&f.token)
            && !delivered.to_string().contains("qa-tracker-reader")
    );
    assert!(
        built.storage.is_file(),
        "private artifact survives disposable build cleanup"
    );
}

#[tokio::test]
#[ignore = "requires explicit owned TLS backend/Compose v2 resources, pinned base, fresh PG and namespace; never silently skips"]
async fn task_http_backend_image_build_tls_artifact_oci_health_and_acceptance() {
    let inputs = NativeInputs::read();
    inputs.verify_ownership().await;
    let dockerfile = format!(
        "FROM {}\nCOPY app.py /app.py\nCOPY app-version /app-version\nEXPOSE 8000\nENTRYPOINT [\"python\",\"-B\",\"/app.py\"]\n",
        inputs.base
    );
    let yaml = serde_yaml::to_string(&json!({"version":1,"jobs":{"build":{"image_build":{"context":".","dockerfile":"Dockerfile","target":null,"descriptor_artifact":"candidate.json","readable_schema_versions":[1]},"timeout":"300s"}}})).unwrap();
    let files: &[(&str, &[u8])] = &[
        ("Dockerfile", dockerfile.as_bytes()),
        ("app.py", include_bytes!("../helpers/oci_application.py")),
        ("app-version", b"A\n"),
    ];
    let f =
        Arc::new(TaskPipeline::new_with_source(Some(yaml), files, Some(configure_native)).await);
    sqlx::query("INSERT INTO environments(id,project_id,name,protected,required_approvals) VALUES($1,$1,'native-task-oci',false,0)").bind(f.project).execute(&f.pool).await.unwrap();
    let case = f.clone();
    let result = tokio::spawn(async move {
        let built = build(&case, &inputs).await;
        deliver(&case, &inputs, &built).await;
    })
    .await;
    let mut f = Arc::try_unwrap(f).unwrap_or_else(|_| panic!("native fixture still borrowed"));
    if let Err(error) = result {
        // Killing the CLI is not BuildKit stop proof. Leave control, exports and owner data
        // to MAIN's retained v2 journal; no generic shell cleanup or unconditional deletion.
        eprintln!("NATIVE_BUILD_OR_DELIVERY_UNCONFIRMED_RETAIN_OWNED_RESOURCES");
        if error.is_panic() {
            std::panic::resume_unwind(error.into_panic());
        }
        panic!("native case cancelled; original custody retained");
    }
    f.cleanup()
        .await
        .expect("known terminal native fixture cleanup");
    // MAIN finally closes the exact native:deploy Compose file on the SAME TLS daemon,
    // before deleting its QA volumes. The image and retained receipts are not pruned here.
}
