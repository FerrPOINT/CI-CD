#![cfg(all(unix, feature = "integration"))]
//! Explicit owner-run native qualification. No mock transport and no server lease impersonation.
use anyhow::{Context, Result, ensure};
use cicd::execution::kubernetes::{self, Config, Execution, Scope, Terminal};
use cicd::runner_workspace::OwnedWorkspace;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::time::{sleep, timeout};
use uuid::Uuid;

const ROOT: &str = "/workspaces";
const SERVICE: &str = "/var/run/secrets/kubernetes.io/serviceaccount";
const MAX_JSON: usize = 1024 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FilePin {
    path: PathBuf,
    sha256: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    schema_version: u32,
    kind: String,
    operation_id: Uuid,
    expected_source_head: String,
    expected_executor_sha256: String,
    runner_config: FilePin,
    expected_config: Config,
    image: String,
    source_commit: String,
    evidence_directory: PathBuf,
    case_timeout_seconds: u64,
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn hex(value: &str, size: usize) -> bool {
    value.len() == size
        && value
            .bytes()
            .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
}
fn validate_input(input: &Input, build_head: Option<&str>, executor_hash: &str) -> Result<()> {
    ensure!(
        input.schema_version == 1 && input.kind == "forge/native-kubernetes-executor/v1",
        "unsupported fixture ABI"
    );
    ensure!(!input.operation_id.is_nil(), "operation identity missing");
    ensure!(
        hex(&input.expected_source_head, 40)
            && build_head == Some(input.expected_source_head.as_str()),
        "immutable compiled source head mismatch"
    );
    ensure!(
        input.expected_executor_sha256 == executor_hash,
        "compiled executor bytes mismatch"
    );
    ensure!(
        hex(&input.source_commit, 40) && kubernetes::digest_image(&input.image),
        "exact fixture source/image required"
    );
    ensure!(
        (60..=180).contains(&input.case_timeout_seconds),
        "case deadline outside fixture bound"
    );
    input.expected_config.validate()?;
    ensure!(
        !input.expected_config.builder_enabled
            && input.expected_config.deployment_recipes.is_empty(),
        "ordinary executor qualification only"
    );
    ensure!(
        input.runner_config.path.is_absolute() && hex(&input.runner_config.sha256, 64),
        "config file pin required"
    );
    ensure!(
        input.evidence_directory.is_absolute() && !input.evidence_directory.starts_with(ROOT),
        "evidence must be outside job PVC"
    );
    Ok(())
}
fn private_read(path: &Path, limit: u64) -> Result<Vec<u8>> {
    ensure!(
        path.is_absolute() && path.canonicalize()? == path,
        "private input path aliases forbidden"
    );
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        meta.is_file()
            && meta.nlink() == 1
            && meta.uid() == unsafe { libc::geteuid() }
            && meta.mode() & 0o077 == 0
            && meta.len() <= limit,
        "private input ownership/mode/bound mismatch"
    );
    Ok(fs::read(path)?)
}
fn write_evidence(directory: &Path, name: &str, value: &Value) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(directory.join(name))?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    fs::File::open(directory)?.sync_all()?;
    Ok(())
}

// This observer only reads native objects. All effects belong to the public Execution adapter.
struct Observer {
    client: Client,
    server: String,
    namespace: String,
}
impl Observer {
    fn new(config: &Config) -> Result<Self> {
        let host = std::env::var("KUBERNETES_SERVICE_HOST")?;
        let port = std::env::var("KUBERNETES_SERVICE_PORT_HTTPS").unwrap_or_else(|_| "443".into());
        let server = format!("https://{host}:{port}");
        let url = reqwest::Url::parse(&server)?;
        ensure!(
            url.host_str().is_some()
                && url.path() == "/"
                && url.username().is_empty()
                && url.password().is_none()
                && port.parse::<u16>().is_ok(),
            "invalid native API origin"
        );
        let ca = fs::read(Path::new(SERVICE).join("ca.crt"))?;
        ensure!(
            hash(&ca) == config.cluster_ca_sha256,
            "native CA pin changed"
        );
        let client = Client::builder()
            .tls_built_in_root_certs(false)
            .add_root_certificate(reqwest::Certificate::from_pem(&ca)?)
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self {
            client,
            server,
            namespace: config.namespace.clone(),
        })
    }
    fn get(&self, path: &str) -> Result<reqwest::RequestBuilder> {
        let token = fs::read_to_string(Path::new(SERVICE).join("token"))?;
        Ok(self
            .client
            .get(format!("{}{path}", self.server))
            .bearer_auth(token.trim()))
    }
    fn pods_path(&self) -> String {
        format!("/api/v1/namespaces/{}/pods", self.namespace)
    }
    async fn json(&self, path: &str) -> Result<Option<Value>> {
        let mut response = self.get(path)?.send().await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        ensure!(
            response.status().is_success(),
            "native observer request rejected"
        );
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            ensure!(
                bytes.len() + chunk.len() <= MAX_JSON,
                "native observer response exceeds bound"
            );
            bytes.extend_from_slice(&chunk);
        }
        Ok(Some(serde_json::from_slice(&bytes)?))
    }
    async fn pod(&self, name: &str) -> Result<Option<Value>> {
        self.json(&format!("{}/{name}", self.pods_path())).await
    }
    async fn resource_version(&self, name: &str) -> Result<String> {
        let path = format!("{}?fieldSelector=metadata.name%3D{name}", self.pods_path());
        let list = self.json(&path).await?.context("native Pod list absent")?;
        ensure!(
            list["items"].as_array().is_some_and(|v| v.is_empty()),
            "unexpected preexisting Pod name"
        );
        Ok(list["metadata"]["resourceVersion"]
            .as_str()
            .context("Pod list resourceVersion missing")?
            .into())
    }
    async fn added(&self, name: &str, resource_version: &str) -> Result<Value> {
        let mut response = self
            .get(&self.pods_path())?
            .query(&[
                ("watch", "true"),
                ("timeoutSeconds", "30"),
                ("fieldSelector", &format!("metadata.name={name}")),
                ("resourceVersion", resource_version),
            ])
            .send()
            .await?;
        ensure!(response.status().is_success(), "native watch rejected");
        let mut line = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            for byte in chunk {
                if byte == b'\n' {
                    if line.is_empty() {
                        continue;
                    }
                    let event: Value = serde_json::from_slice(&line)?;
                    line.clear();
                    ensure!(event["type"] != "ERROR", "native watch history unavailable");
                    if event["type"] == "ADDED" {
                        let pod = &event["object"];
                        ensure!(
                            pod["metadata"]["name"] == name
                                && pod["metadata"]["namespace"] == self.namespace,
                            "native watch identity mismatch"
                        );
                        return Ok(pod.clone());
                    }
                } else {
                    ensure!(line.len() < MAX_JSON, "native watch event exceeds bound");
                    line.push(byte);
                }
            }
        }
        anyhow::bail!("native watch ended without exact ADDED event")
    }
    async fn preflight(&self, input: &Input) -> Result<()> {
        let c = &input.expected_config;
        let ns = self
            .json(&format!("/api/v1/namespaces/{}", c.namespace))
            .await?
            .context("namespace missing")?;
        ensure!(
            ns["metadata"]["uid"] == c.namespace_uid,
            "namespace UID mismatch"
        );
        let pvc = self
            .json(&format!(
                "/api/v1/namespaces/{}/persistentvolumeclaims/{}",
                c.namespace, c.workspace_claim
            ))
            .await?
            .context("workspace claim missing")?;
        ensure!(
            pvc["metadata"]["uid"] == c.workspace_claim_uid && pvc["status"]["phase"] == "Bound",
            "workspace PVC identity/state mismatch"
        );
        let pods = self
            .json(&format!(
                "{}?labelSelector=app.kubernetes.io%2Fmanaged-by%3Dforge-runner",
                self.pods_path()
            ))
            .await?
            .context("Pod inventory unavailable")?;
        ensure!(
            pods["items"].as_array().is_some_and(|v| v.is_empty()),
            "existing Forge execution Pods; fixture must not share active namespace"
        );
        ensure!(
            kubernetes::inventory(Path::new(ROOT))?.is_empty()
                && OwnedWorkspace::inventory(Path::new(ROOT))?.is_empty(),
            "existing workspace/execution custody; no fixture effects allowed"
        );
        Ok(())
    }
}
// Kubernetes canonicalizes quantities returned by GET; compare exact units, not spelling.
fn quantity(value: &Value, cpu: bool) -> Result<u64> {
    let text = value.as_str().context("resource quantity missing")?;
    if cpu {
        if let Some(value) = text.strip_suffix('m') {
            return Ok(value.parse()?);
        }
        return text
            .parse::<u64>()?
            .checked_mul(1000)
            .context("CPU quantity overflow");
    }
    for (suffix, multiplier) in [
        ("Ki", 1024),
        ("Mi", 1024_u64.pow(2)),
        ("Gi", 1024_u64.pow(3)),
        ("k", 1000),
        ("M", 1000_u64.pow(2)),
        ("G", 1000_u64.pow(3)),
    ] {
        if let Some(value) = text.strip_suffix(suffix) {
            return value
                .parse::<u64>()?
                .checked_mul(multiplier)
                .context("storage quantity overflow");
        }
    }
    Ok(text.parse()?)
}
#[derive(Clone, Copy, Debug)]
enum Case {
    Success,
    Timeout,
    Cancel,
    Reopen,
    LostCreateAck,
}
impl Case {
    fn name(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Timeout => "timeout",
            Self::Cancel => "cancel",
            Self::Reopen => "reopen-reconcile",
            Self::LostCreateAck => "lost-create-ack-before-custody-commit",
        }
    }
}
struct Attempt {
    execution: Execution,
    scope: Scope,
    workspace_id: String,
    checkout: PathBuf,
    control: PathBuf,
    name: String,
    marker: String,
    command: Vec<String>,
    evidence: PathBuf,
}
impl Attempt {
    fn prepare(input: &Input, case: Case) -> Result<Self> {
        let scope = Scope {
            job_id: Uuid::new_v4(),
            attempt_id: Uuid::new_v4(),
            lease_id: Uuid::new_v4(),
            generation: 1,
            source_commit: input.source_commit.clone(),
        };
        let workspace = OwnedWorkspace::create(
            Path::new(ROOT),
            scope.attempt_id,
            scope.lease_id,
            scope.generation,
        )?;
        let checkout = workspace.checkout().to_path_buf();
        fs::create_dir(&checkout)?;
        fs::set_permissions(&checkout, fs::Permissions::from_mode(0o700))?;
        let workspace_id = checkout
            .parent()
            .context("workspace wrapper missing")?
            .file_name()
            .context("workspace name missing")?
            .to_str()
            .context("workspace name encoding")?
            .to_owned();
        let evidence = input.evidence_directory.join(case.name());
        fs::create_dir(&evidence)?;
        fs::set_permissions(&evidence, fs::Permissions::from_mode(0o700))?;
        let marker = format!("{}-{}", input.operation_id, scope.attempt_id);
        let wait = match case {
            Case::Success => "",
            Case::Reopen | Case::LostCreateAck => "; exec sleep 8",
            _ => "; exec sleep 90",
        };
        let script = format!(
            "mkdir -p /workspace/artifacts; printf '%s\\n' \"$FORGE_NATIVE_MARKER\" > /workspace/artifacts/result.txt; printf 'forge-native:%s\\n' \"$FORGE_NATIVE_MARKER\"{wait}"
        );
        let command = vec!["/bin/sh".into(), "-ec".into(), script];
        let deadline = if matches!(case, Case::Timeout) { 5 } else { 60 };
        let execution = Execution::prepare(
            input.expected_config.clone(),
            Path::new(ROOT),
            scope.clone(),
            &checkout,
            &input.image,
            &command,
            &[("FORGE_NATIVE_MARKER".into(), marker.clone())],
            deadline,
        )?;
        let name = format!("forge-{}-g{}", scope.attempt_id.simple(), scope.generation);
        let control = Path::new(ROOT).join(".kubernetes-control").join(&name);
        write_evidence(
            &evidence,
            "scope.json",
            &json!({"scope":scope,"podName":name,"workspaceId":workspace_id,"namespaceUid":input.expected_config.namespace_uid,"workspaceClaimUid":input.expected_config.workspace_claim_uid,"operationId":input.operation_id}),
        )?;
        Ok(Self {
            execution,
            scope,
            workspace_id,
            checkout,
            control,
            name,
            marker,
            command,
            evidence,
        })
    }
    fn replace_with_reopened(&mut self) -> Result<()> {
        let workspace = OwnedWorkspace::reopen(Path::new(ROOT), &self.workspace_id)?;
        workspace.verify_identity(
            self.scope.attempt_id,
            self.scope.lease_id,
            self.scope.generation,
        )?;
        self.execution = Execution::reopen(
            Path::new(ROOT),
            self.scope.attempt_id,
            self.scope.generation,
        )?;
        Ok(())
    }
    async fn started(&self) -> Result<()> {
        loop {
            if self.checkout.join("artifacts/result.txt").exists() {
                self.artifact()?;
                return Ok(());
            }
            ensure!(
                self.execution.observe().await?.is_none(),
                "execution stopped before start marker"
            );
            sleep(Duration::from_millis(100)).await;
        }
    }
    fn artifact(&self) -> Result<Vec<u8>> {
        let path = self.checkout.join("artifacts/result.txt");
        let meta = fs::symlink_metadata(&path)?;
        ensure!(
            path.canonicalize()? == path
                && meta.is_file()
                && meta.nlink() == 1
                && meta.len() <= 256,
            "artifact is not bounded owned regular file"
        );
        let bytes = fs::read(path)?;
        ensure!(
            bytes == format!("{}\n", self.marker).as_bytes(),
            "actual mounted artifact bytes mismatch"
        );
        Ok(bytes)
    }
    async fn stopped(&self) -> Result<Terminal> {
        loop {
            if let Some(proof) = self.execution.observe().await? {
                return Ok(proof);
            }
            sleep(Duration::from_millis(200)).await;
        }
    }
    fn assert_pod(&self, pod: &Value, input: &Input) -> Result<String> {
        let c = &input.expected_config;
        let subpath = format!("{}/workspace", self.workspace_id);
        let metadata = &pod["metadata"];
        let spec = &pod["spec"];
        let containers = spec["containers"]
            .as_array()
            .context("native containers missing")?;
        ensure!(
            metadata["name"] == self.name
                && metadata["namespace"] == c.namespace
                && metadata["labels"]["pdlc.installation"] == c.installation
                && metadata["labels"]["pdlc.forge-job"] == self.scope.job_id.to_string()
                && metadata["labels"]["pdlc.forge-attempt"] == self.scope.attempt_id.to_string(),
            "native Pod scope mismatch"
        );
        ensure!(
            metadata["annotations"]["pdlc.operation-id"] == input.operation_id.to_string()
                && metadata["annotations"]["pdlc.workspace-subpath"] == subpath
                && metadata["annotations"]["pdlc.workspace-claim-uid"] == c.workspace_claim_uid,
            "native admission/workspace binding mismatch"
        );
        ensure!(
            spec["restartPolicy"] == "Never"
                && spec["automountServiceAccountToken"] == false
                && spec["serviceAccountName"] == c.job_service_account
                && !spec["hostNetwork"].as_bool().unwrap_or(false)
                && !spec["hostPID"].as_bool().unwrap_or(false)
                && !spec["hostIPC"].as_bool().unwrap_or(false),
            "native process/security boundary mismatch"
        );
        ensure!(
            spec["imagePullSecrets"]
                .as_array()
                .is_none_or(|v| v.is_empty())
                && spec["initContainers"]
                    .as_array()
                    .is_none_or(|v| v.is_empty())
                && containers.len() == 1,
            "unexpected credential/init/companion container"
        );
        let main = &containers[0];
        ensure!(
            main["name"] == "execution"
                && main["command"] == json!(self.command)
                && main["image"] == input.image
                && main["imagePullPolicy"] == "IfNotPresent"
                && main["securityContext"]["allowPrivilegeEscalation"] == false
                && main["securityContext"]["readOnlyRootFilesystem"] == true
                && main["securityContext"]["capabilities"]["drop"] == json!(["ALL"]),
            "native execution image/security mismatch"
        );
        ensure!(
            spec["securityContext"]["runAsNonRoot"] == true
                && spec["securityContext"]["runAsUser"] == c.security.run_as_user
                && spec["securityContext"]["runAsGroup"] == c.security.run_as_group
                && spec["securityContext"]["fsGroup"] == c.security.fs_group
                && spec["securityContext"]["seccompProfile"]["type"] == "RuntimeDefault",
            "native identity/seccomp mismatch"
        );
        for kind in ["requests", "limits"] {
            let resources = &main["resources"][kind];
            ensure!(
                quantity(&resources["cpu"], true)? == u64::from(c.resources.cpu_millis)
                    && quantity(&resources["memory"], false)? == c.resources.memory_bytes
                    && quantity(&resources["ephemeral-storage"], false)?
                        == c.resources.ephemeral_storage_bytes,
                "native resource allocation differs from exact config"
            );
        }
        let mounts = main["volumeMounts"].as_array().context("mounts missing")?;
        ensure!(
            mounts.len() == 2
                && mounts.iter().any(|v| v["name"] == "workspace"
                    && v["mountPath"] == "/workspace"
                    && v["subPath"] == subpath)
                && mounts
                    .iter()
                    .any(|v| v["name"] == "tmp" && v["mountPath"] == "/tmp"),
            "native mount boundary mismatch"
        );
        let volumes = spec["volumes"].as_array().context("volumes missing")?;
        ensure!(
            volumes.len() == 2
                && volumes.iter().any(|v| v["name"] == "workspace"
                    && v["persistentVolumeClaim"]["claimName"] == c.workspace_claim)
                && volumes.iter().any(|v| v["name"] == "tmp"
                    && v["emptyDir"]["medium"] == "Memory"
                    && v["emptyDir"]["sizeLimit"] == "64Mi"),
            "native volume boundary mismatch"
        );
        ensure!(
            main["env"] == json!([{"name":"FORGE_NATIVE_MARKER","value":self.marker}]),
            "unexpected execution environment"
        );
        Ok(metadata["uid"]
            .as_str()
            .filter(|v| !v.is_empty())
            .context("native Pod UID missing")?
            .into())
    }
}
async fn exercise(
    attempt: &mut Attempt,
    input: &Input,
    observer: &Observer,
    case: Case,
) -> Result<Value> {
    let mut race_observed = false;
    let mut watched_uid = None;
    if matches!(case, Case::LostCreateAck) {
        let resource_version = observer.resource_version(&attempt.name).await?;
        {
            let start = attempt.execution.start();
            tokio::pin!(start);
            tokio::select! {
                biased;
                event = observer.added(&attempt.name, &resource_version) => {
                    let pod = event?;
                    watched_uid = Some(attempt.assert_pod(&pod, input)?);
                    // Dropping only the actual caller future is intentional; the Pod continues.
                    race_observed = attempt.control.join("issued.json").exists() && !attempt.control.join("created.json").exists();
                }
                result = &mut start => { result?; }
            }
        }
        attempt.replace_with_reopened()?;
        attempt.execution.reconcile().await?;
    } else {
        attempt.execution.start().await?;
    }
    let pod = observer
        .pod(&attempt.name)
        .await?
        .context("created Pod missing; absence is not stop proof")?;
    let uid = attempt.assert_pod(&pod, input)?;
    if let Some(watched) = watched_uid {
        ensure!(uid == watched, "watch/reconcile Pod UID changed");
    }
    write_evidence(
        &attempt.evidence,
        "native-identity.json",
        &json!({"podUid":uid,"namespaceUid":input.expected_config.namespace_uid,"workspaceClaimUid":input.expected_config.workspace_claim_uid,"restartPolicy":"Never","image":input.image,"imagePullPolicy":"IfNotPresent"}),
    )?;
    attempt.started().await?;
    if matches!(case, Case::Cancel) {
        ensure!(
            attempt.execution.observe().await?.is_none(),
            "cancel fixture was already terminal"
        );
        attempt.execution.cancel().await?;
    }
    if matches!(case, Case::Reopen) {
        ensure!(
            attempt.execution.observe().await?.is_none(),
            "reopen fixture was already terminal"
        );
        attempt.replace_with_reopened()?;
        attempt.execution.reconcile().await?;
        let reopened = observer
            .pod(&attempt.name)
            .await?
            .context("reopened Pod absent")?;
        ensure!(
            attempt.assert_pod(&reopened, input)? == uid,
            "reopen changed physical execution"
        );
    }
    let terminal = attempt.stopped().await?;
    ensure!(
        terminal.pod_uid == uid,
        "terminal physical identity changed"
    );
    if matches!(case, Case::Timeout | Case::Cancel) {
        ensure!(
            terminal.exit_code != 0,
            "timeout/cancel did not stop running execution"
        );
        if matches!(case, Case::Timeout) {
            let pod = observer
                .pod(&attempt.name)
                .await?
                .context("timeout Pod absent")?;
            ensure!(
                pod["status"]["reason"] == "DeadlineExceeded",
                "native timeout was not activeDeadlineSeconds"
            );
        }
    } else {
        ensure!(terminal.exit_code == 0, "ordinary execution failed");
    }
    let artifact = attempt.artifact()?;
    let mut artifact_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(attempt.evidence.join("result.txt"))?;
    artifact_file.write_all(&artifact)?;
    artifact_file.sync_all()?;
    let mut lines = Vec::new();
    attempt
        .execution
        .stream_logs(|line| {
            if lines.len() < 32 && line.len() <= 1024 {
                lines.push(line);
            }
            std::future::ready(Ok(()))
        })
        .await?;
    ensure!(
        lines
            .iter()
            .any(|line| line == &format!("forge-native:{}", attempt.marker)),
        "actual executor log marker missing"
    );
    write_evidence(
        &attempt.evidence,
        "output.json",
        &json!({"artifactSha256":hash(&artifact),"artifactBytes":artifact.len(),"logMarkerObserved":true,"logs":lines,"terminal":terminal,"lostCreateAckWindowObserved":race_observed}),
    )?;
    Ok(
        json!({"lostCreateAckWindowObserved":race_observed,"qualified":!matches!(case, Case::LostCreateAck) || race_observed}),
    )
}
async fn cleanup(attempt: &Attempt, input: &Input, observer: &Observer) -> Result<()> {
    // No effect may be inferred stopped from a missing object or a dropped future.
    if attempt.execution.observe().await?.is_none() {
        attempt.execution.cancel().await?;
    }
    let terminal = timeout(Duration::from_secs(90), attempt.stopped())
        .await
        .context("cleanup stop deadline; retain custody")??;
    let pod = observer
        .pod(&attempt.name)
        .await?
        .context("terminal Pod missing before fixture ACK")?;
    ensure!(
        attempt.assert_pod(&pod, input)? == terminal.pod_uid,
        "cleanup Pod UID changed"
    );
    let proof = attempt.execution.termination_proof()?;
    write_evidence(
        &attempt.evidence,
        "fixture-owner-terminal-ack.json",
        &json!({"kind":"forge/native-fixture-owner-ack/v1","operationId":input.operation_id,"scope":attempt.scope,"terminationProof":proof,"terminal":terminal,"serverLeaseProtocolQualified":false}),
    )?;
    attempt.execution.acknowledge().await?;
    timeout(Duration::from_secs(30), async {
        loop {
            match observer.pod(&attempt.name).await? {
                None => return Ok::<_, anyhow::Error>(()),
                Some(pod) => {
                    ensure!(
                        attempt.assert_pod(&pod, input)? == terminal.pod_uid,
                        "cleanup replacement Pod; HOLD"
                    );
                }
            }
            sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .context("acknowledged Pod cleanup not confirmed")??;
    let workspace = OwnedWorkspace::reopen(Path::new(ROOT), &attempt.workspace_id)?;
    workspace.verify_identity(
        attempt.scope.attempt_id,
        attempt.scope.lease_id,
        attempt.scope.generation,
    )?;
    let status = if terminal.exit_code == 0 {
        "success"
    } else if attempt.execution.cancelled() {
        "canceled"
    } else {
        "failed"
    };
    workspace.record_completion(status)?;
    workspace.acknowledge_completion(status)?;
    workspace.cleanup_after_ack().await?;
    attempt.execution.finish_workspace_cleanup()?;
    ensure!(
        !attempt.checkout.exists()
            && !kubernetes::inventory(Path::new(ROOT))?
                .iter()
                .any(|scope| scope.attempt_id == attempt.scope.attempt_id),
        "owned cleanup inventory remains"
    );
    Ok(())
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CaseResult {
    name: String,
    status: &'static str,
    cleanup_complete: bool,
    detail: Value,
}
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires exact owner-created native Kubernetes fixture, never a default CI test"]
async fn actual_kubernetes_executor_native() -> Result<()> {
    let input_path = PathBuf::from(
        std::env::var("FORGE_NATIVE_EXECUTOR_INPUT").context("private native input required")?,
    );
    let bytes = private_read(&input_path, 64 * 1024)?;
    ensure!(
        hash(&bytes) == std::env::var("FORGE_NATIVE_EXECUTOR_INPUT_SHA256")?,
        "native input SHA256 mismatch"
    );
    let input: Input = serde_json::from_slice(&bytes)?;
    validate_input(
        &input,
        option_env!("FORGE_NATIVE_EXECUTOR_BUILD_HEAD"),
        &hash(include_bytes!("../src/execution/kubernetes.rs")),
    )?;
    ensure!(
        std::env::var_os("CICD_QUIESCENCE_DIR").is_none(),
        "fixture must not enroll into application quiescence control"
    );
    ensure!(
        PathBuf::from(std::env::var("CICD_KUBERNETES_RUNNER_CONFIG")?) == input.runner_config.path,
        "config env/path mismatch"
    );
    ensure!(
        hash(&fs::read(&input.runner_config.path)?) == input.runner_config.sha256
            && Config::from_env()? == input.expected_config,
        "actual runner config differs from pinned expected config"
    );
    ensure!(
        unsafe { libc::geteuid() } == input.expected_config.security.run_as_user
            && unsafe { libc::getegid() } == input.expected_config.security.run_as_group,
        "controller and job identity must match for private checkout"
    );
    let evidence_parent = input
        .evidence_directory
        .parent()
        .context("evidence parent required")?;
    ensure!(
        evidence_parent.canonicalize()? == evidence_parent && !evidence_parent.starts_with(ROOT),
        "evidence parent aliases forbidden"
    );
    fs::create_dir(&input.evidence_directory)?;
    fs::set_permissions(&input.evidence_directory, fs::Permissions::from_mode(0o700))?;
    let observer = Observer::new(&input.expected_config)?;
    observer.preflight(&input).await?;
    write_evidence(
        &input.evidence_directory,
        "preflight.json",
        &json!({"schemaVersion":1,"operationId":input.operation_id,"sourceHead":input.expected_source_head,"executorSha256":input.expected_executor_sha256,"namespace":input.expected_config.namespace,"namespaceUid":input.expected_config.namespace_uid,"workspaceClaimUid":input.expected_config.workspace_claim_uid,"serverLeaseProtocolQualified":false,"sourcePreparationQualified":false,"imageGraphQualification":"external-owner-receipt","cleanPullProven":false}),
    )?;
    let mut results = Vec::new();
    for case in [
        Case::Success,
        Case::Timeout,
        Case::Cancel,
        Case::Reopen,
        Case::LostCreateAck,
    ] {
        let mut attempt = match Attempt::prepare(&input, case) {
            Ok(attempt) => attempt,
            Err(error) => {
                results.push(CaseResult {
                    name: case.name().into(),
                    status: "HOLD",
                    cleanup_complete: false,
                    detail: json!({"reason":error.to_string()}),
                });
                break;
            }
        };
        let result = timeout(
            Duration::from_secs(input.case_timeout_seconds),
            exercise(&mut attempt, &input, &observer, case),
        )
        .await;
        let cleanup_result = timeout(
            Duration::from_secs(150),
            cleanup(&attempt, &input, &observer),
        )
        .await;
        let cleanup_complete = matches!(&cleanup_result, Ok(Ok(())));
        let (mut status, mut detail) = match result {
            Ok(Ok(facts)) => (
                if facts["qualified"] == true {
                    "PASS"
                } else {
                    "HOLD"
                },
                facts,
            ),
            Ok(Err(error)) => ("FAIL", json!({"reason":error.to_string()})),
            Err(_) => (
                "HOLD",
                json!({"reason":"case observation deadline; only caller future stopped"}),
            ),
        };
        if !cleanup_complete {
            status = "HOLD";
            detail["cleanup"] =
                json!("unknown or incomplete; preserve exact custody and stop further cases");
            detail["cleanupReason"] = match &cleanup_result {
                Ok(Err(error)) => json!(error.to_string()),
                Err(_) => json!("cleanup observation deadline; process stop not inferred"),
                Ok(Ok(())) => Value::Null,
            };
        }
        let result = CaseResult {
            name: case.name().into(),
            status,
            cleanup_complete,
            detail,
        };
        write_evidence(
            &attempt.evidence,
            "result.json",
            &serde_json::to_value(&result)?,
        )?;
        results.push(result);
        if !cleanup_complete {
            break;
        }
    }
    for case in [
        Case::Success,
        Case::Timeout,
        Case::Cancel,
        Case::Reopen,
        Case::LostCreateAck,
    ] {
        if !results.iter().any(|result| result.name == case.name()) {
            results.push(CaseResult {
                name: case.name().into(),
                status: "NOT_RUN",
                cleanup_complete: false,
                detail: json!({"reason":"prior unknown custody; no further effects attempted"}),
            });
        }
    }
    let qualified = results.len() == 5
        && results
            .iter()
            .all(|result| result.status == "PASS" && result.cleanup_complete);
    write_evidence(
        &input.evidence_directory,
        "result.json",
        &json!({"schemaVersion":1,"kind":"forge/native-kubernetes-executor-result/v1","operationId":input.operation_id,"status":if qualified {"PASS"} else if results.iter().any(|r| r.status == "HOLD" || r.status == "NOT_RUN") {"HOLD"} else {"FAIL"},"cases":results,"serverLeaseProtocolQualified":false,"sourcePreparationQualified":false,"cleanPullProven":false}),
    )?;
    ensure!(
        qualified,
        "native qualification incomplete; inspect individual private case results, retain unknown custody"
    );
    Ok(())
}

#[test]
fn private_abi_rejects_wrong_build_and_builder_scope() {
    let config: Config = serde_json::from_value(json!({
        "schemaVersion":1,"installationSha256":"a".repeat(64),"installation":"qa-forge",
        "clusterCaSha256":"b".repeat(64),"namespace":"qa-forge","namespaceUid":"11111111-1111-4111-8111-111111111111",
        "workspaceClaim":"workspace","workspaceClaimUid":"22222222-2222-4222-8222-222222222222","jobServiceAccount":"job",
        "resources":{"cpuMillis":100,"memoryBytes":134217728,"ephemeralStorageBytes":134217728},
        "security":{"runAsUser":10001,"runAsGroup":10001,"fsGroup":10001}
    }))
    .unwrap();
    config
        .validate()
        .expect("native fixture Config must satisfy the Kubernetes UID contract");
    let mut input = Input {
        schema_version: 1,
        kind: "forge/native-kubernetes-executor/v1".into(),
        operation_id: Uuid::new_v4(),
        expected_source_head: "c".repeat(40),
        expected_executor_sha256: "d".repeat(64),
        runner_config: FilePin {
            path: "/config/runner.json".into(),
            sha256: "e".repeat(64),
        },
        expected_config: config,
        image: format!("docker.io/library/postgres@sha256:{}", "f".repeat(64)),
        source_commit: "1".repeat(40),
        evidence_directory: "/evidence/new".into(),
        case_timeout_seconds: 120,
    };
    assert!(validate_input(&input, Some(&"c".repeat(40)), &"d".repeat(64)).is_ok());
    assert!(validate_input(&input, Some(&"0".repeat(40)), &"d".repeat(64)).is_err());
    assert!(validate_input(&input, Some(&"c".repeat(40)), &"0".repeat(64)).is_err());
    input.evidence_directory = "/workspaces/leak".into();
    assert!(validate_input(&input, Some(&"c".repeat(40)), &"d".repeat(64)).is_err());
    input.evidence_directory = "/evidence/new".into();
    input.expected_config.builder_enabled = true;
    assert!(validate_input(&input, Some(&"c".repeat(40)), &"d".repeat(64)).is_err());
}

#[test]
fn exact_native_quantities_accept_canonical_neighbors_reject_mismatch() {
    assert_eq!(quantity(&json!("1"), true).unwrap(), 1000);
    assert_eq!(quantity(&json!("100m"), true).unwrap(), 100);
    assert_eq!(quantity(&json!("128Mi"), false).unwrap(), 134217728);
    assert_eq!(quantity(&json!("134217728"), false).unwrap(), 134217728);
    assert_ne!(quantity(&json!("127Mi"), false).unwrap(), 134217728);
    assert!(quantity(&json!("18446744073709551615Gi"), false).is_err());
    assert!(quantity(&Value::Null, false).is_err());
}
