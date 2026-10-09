//! Temporary owner-local Compose deployment of a sealed, locally available OCI image.
//! Only immutable read-only data snapshots are supported. No migration execution.
use super::*;
use std::{process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, process::Command};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Descriptor {
    schema: String,
    image_id: String,
    source_commit: String,
    data_protocol: String,
    readable_schema_versions: Vec<u32>,
    migrations: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Policy {
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
    checks: DeliveryPolicy,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    schema: String,
    candidate: DeliveryManifest,
    descriptor: Descriptor,
    data_schema_version: u32,
    data_sha256: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    schema: String,
    scope: String,
    command: DeliveryCommand,
    command_sha256: String,
    original_operation: WorkspaceOperationReceipt,
    manifest_sha256: Option<String>,
    previous_manifest_sha256: Option<String>,
    status: DeliveryStatus,
    reason: String,
    version: Option<DeliveryProbe>,
    health: Option<DeliveryProbe>,
    acceptance: Option<DeliveryProbe>,
    compatibility: Option<DeliveryProbe>,
    container_id: Option<String>,
    image_id: Option<String>,
    data_sha256: Option<String>,
    dispatch_allowed: bool,
    sdlc_acceptance_verified: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Readback {
    receipt: Receipt,
    reconciled_receipt: Option<Receipt>,
    current_manifest_sha256: Option<String>,
    confirmed_manifest_sha256: Option<String>,
    reconciliation_needed: bool,
}
impl Readback {
    pub fn latest_status(&self) -> DeliveryStatus {
        self.reconciled_receipt
            .as_ref()
            .unwrap_or(&self.receipt)
            .status
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Intent {
    receipt: Receipt,
    manifest: Option<Manifest>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChildIdentity {
    pid: u32,
    start: String,
}

struct Owner {
    root: PathBuf,
    project: Uuid,
    policy: Policy,
    policy_hash: String,
}

#[derive(Debug)]
struct Blocked(&'static str);
impl std::fmt::Display for Blocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for Blocked {}
pub fn rejection_code(error: &anyhow::Error) -> &'static str {
    error
        .downcast_ref::<Blocked>()
        .map_or("oci_command_rejected_or_unavailable", |e| e.0)
}
pub fn rejection_status(error: &anyhow::Error) -> &'static str {
    if error.downcast_ref::<Blocked>().is_some() {
        "blocked"
    } else {
        "unknown_or_rejected"
    }
}

fn child_start(pid: u32) -> anyhow::Result<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    Ok(stat
        .rsplit_once(')')
        .context("process identity unavailable")?
        .1
        .split_whitespace()
        .nth(19)
        .context("process identity unavailable")?
        .to_owned())
}

impl Owner {
    fn open(project: Uuid, create: bool) -> anyhow::Result<Self> {
        ensure!(
            cfg!(target_os = "linux"),
            "OCI executor requires Linux process/filesystem semantics"
        );
        let root = PathBuf::from(std::env::var("CICD_LOCAL_OCI_ROOT")?);
        ensure!(
            root.is_absolute()
                && root.components().all(|c| !matches!(
                    c,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )),
            "isolated normalized OCI root required"
        );
        let policy: Policy = read(&PathBuf::from(std::env::var("CICD_LOCAL_OCI_POLICY")?))?;
        let suffix = policy
            .project_name
            .strip_prefix("sdlc-qa-forge-oci-")
            .context("temporary owned OCI project required")?;
        ensure!(
            (12..=32).contains(&suffix.len()) && suffix.bytes().all(|b| b.is_ascii_hexdigit()),
            "unique explicit OCI project required"
        );
        ensure!(
            policy.network_name.starts_with("sdlc-qa-forge-")
                && policy
                    .network_name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b)),
            "isolated QA network required"
        );
        ensure!(digest(&policy.data_sha256), "exact data snapshot required");
        policy.checks.validate()?;
        for path in [
            &policy.docker_bin,
            &policy.compose_bin,
            &policy.data_file,
            &policy.compose_root,
        ] {
            ensure!(path.is_absolute(), "owner paths must be absolute");
            plain(path)?;
        }
        if create {
            directory(&root)?;
        }
        plain(&root)?;
        let root = root.canonicalize()?;
        ensure!(
            !policy.data_file.canonicalize()?.starts_with(&root),
            "data must remain outside publication root"
        );
        let policy_hash = sha(&serde_json::to_vec(&policy)?);
        let marker = serde_json::json!({"schema":"forge/local-oci-target/v1","projectId":project,"policySha256":policy_hash});
        if create {
            let _lock = Lock::acquire(&root)?;
            if !root.join("owner.json").exists() {
                ensure!(
                    std::fs::read_dir(&root)?.all(|e| e.is_ok_and(|e| e.file_name() == ".lock")),
                    "refusing existing OCI root contents"
                );
                for name in ["operations", "manifests", "docker-config"] {
                    directory(&root.join(name))?;
                }
                immutable(
                    &root.join("docker-config/config.json"),
                    &serde_json::json!({}),
                )?;
                immutable(&root.join("owner.json"), &marker)?;
            }
        }
        ensure!(
            read::<serde_json::Value>(&root.join("owner.json"))? == marker,
            "OCI owner/policy mismatch"
        );
        for name in ["operations", "manifests", "docker-config"] {
            plain(&root.join(name))?;
            ensure!(root.join(name).is_dir(), "OCI target is torn");
        }
        Ok(Self {
            root,
            project,
            policy,
            policy_hash,
        })
    }

    fn data(&self, descriptor: &Descriptor) -> anyhow::Result<(u32, String)> {
        if descriptor.schema != "forge/oci-candidate/v1"
            || descriptor.data_protocol != "readonly_snapshot_v1"
            || !descriptor.migrations.is_empty()
        {
            return Err(Blocked("unsupported_mutable_data_or_migration").into());
        }
        ensure!(
            !descriptor.readable_schema_versions.is_empty()
                && descriptor.readable_schema_versions.len() <= 16,
            "unknown data compatibility"
        );
        let bytes = read_bytes(&self.policy.data_file, 1024 * 1024)?;
        let hash = sha(&bytes);
        if hash != self.policy.data_sha256 {
            return Err(Blocked("owner_data_snapshot_drift").into());
        }
        let data: serde_json::Value = serde_json::from_slice(&bytes)?;
        let version = data["schemaVersion"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .context("unknown actual data schema")?;
        if !descriptor.readable_schema_versions.contains(&version) {
            return Err(Blocked("actual_data_schema_incompatible").into());
        }
        Ok((version, hash))
    }

    fn process(&self, binary: &Path, args: &[String]) -> Command {
        let mut child = Command::new(binary);
        child
            .args(args)
            .env_clear()
            .env("DOCKER_HOST", "unix:///var/run/docker.sock")
            .env("DOCKER_CONFIG", self.root.join("docker-config"))
            .env("PATH", "/usr/local/bin:/usr/bin:/bin")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(target_os = "linux")]
        unsafe {
            child.pre_exec(|| {
                let parent = libc::getppid();
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::getppid() != parent {
                    return Err(std::io::Error::other("owner exited"));
                }
                Ok(())
            });
        }
        child
    }

    async fn run(
        &self,
        binary: &Path,
        args: &[String],
        effect: Option<&Path>,
    ) -> anyhow::Result<Vec<u8>> {
        let mut child = self.process(binary, args).spawn()?;
        if let Some(dir) = effect {
            let pid = child.id().context("compose process missing")?;
            // A crash before this durable identity cannot be auto-reconciled.
            immutable(
                &dir.join("child.json"),
                &ChildIdentity {
                    pid,
                    start: child_start(pid)?,
                },
            )?;
        }
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let task = async {
            let read = |pipe: Box<dyn tokio::io::AsyncRead + Unpin + Send>| async move {
                let mut bytes = Vec::new();
                pipe.take(1024 * 1024 + 1).read_to_end(&mut bytes).await?;
                ensure!(bytes.len() <= 1024 * 1024, "engine output exceeds bound");
                Ok::<_, anyhow::Error>(bytes)
            };
            let (status, out, _err) = tokio::try_join!(
                async { Ok::<_, anyhow::Error>(child.wait().await?) },
                read(Box::new(stdout)),
                read(Box::new(stderr))
            )?;
            ensure!(status.success(), "owner engine command failed");
            Ok::<_, anyhow::Error>(out)
        };
        let result = tokio::time::timeout(Duration::from_secs(20), task).await;
        if !matches!(&result, Ok(Ok(_))) {
            let _ = child.start_kill();
            child
                .wait()
                .await
                .context("Compose writer exit unverified")?;
        }
        if let Some(dir) = effect {
            immutable(&dir.join("child-stopped.json"), &true)?;
        }
        result.context("owner engine deadline; outcome requires readback")?
    }

    async fn docker(&self, args: &[&str]) -> anyhow::Result<serde_json::Value> {
        let args = args.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        Ok(serde_json::from_slice(
            &self.run(&self.policy.docker_bin, &args, None).await?,
        )?)
    }

    async fn engine(&self) -> anyhow::Result<()> {
        let parent = self
            .policy
            .network_name
            .strip_suffix("_qa")
            .context("owned QA network identity missing")?;
        ensure!(
            parent.ends_with(
                self.policy
                    .project_name
                    .strip_prefix("sdlc-qa-forge-oci-")
                    .unwrap()
            ) && self.policy.volume_name == format!("{parent}_delivery-qa"),
            "QA resource owner binding mismatch"
        );
        ensure!(
            self.docker(&["info", "--format", "{{json .ID}}"])
                .await?
                .as_str()
                == Some(&self.policy.daemon_id),
            "engine identity mismatch"
        );
        let network = self
            .docker(&["network", "inspect", &self.policy.network_name])
            .await?;
        ensure!(
            network[0]["Internal"] == true
                && network[0]["Labels"]["sdlc.task"] == "forge-task-delivery",
            "owner network is not isolated"
        );
        ensure!(
            network[0]["Labels"]["com.docker.compose.project"] == parent,
            "network project mismatch"
        );
        Ok(())
    }

    async fn host_path(&self, local: &Path) -> anyhow::Result<PathBuf> {
        plain(&self.policy.volume_root)?;
        let relative = local
            .canonicalize()?
            .strip_prefix(self.policy.volume_root.canonicalize()?)?
            .to_owned();
        ensure!(
            self.policy.volume_name.starts_with("sdlc-qa-forge-"),
            "owned disposable volume required"
        );
        let volume = self
            .docker(&["volume", "inspect", &self.policy.volume_name])
            .await?;
        ensure!(
            volume[0]["Labels"]["sdlc.task"] == "forge-task-delivery",
            "owner data volume mismatch"
        );
        ensure!(
            volume[0]["Labels"]["com.docker.compose.project"]
                == self.policy.network_name.strip_suffix("_qa").unwrap(),
            "volume project mismatch"
        );
        Ok(PathBuf::from(
            volume[0]["Mountpoint"]
                .as_str()
                .context("data mountpoint unavailable")?,
        )
        .join(relative))
    }

    async fn image(&self, descriptor: &Descriptor) -> anyhow::Result<()> {
        ensure!(
            descriptor
                .image_id
                .strip_prefix("sha256:")
                .is_some_and(digest),
            "mutable tag/unknown image identity rejected"
        );
        let value = self
            .docker(&["image", "inspect", &descriptor.image_id])
            .await?;
        ensure!(
            value[0]["Id"] == descriptor.image_id
                && value[0]["Config"]["Labels"]["org.opencontainers.image.revision"]
                    == descriptor.source_commit,
            "actual OCI image/commit mismatch"
        );
        ensure!(
            value[0]["Os"] == "linux"
                && value[0]["Config"]["Volumes"]
                    .as_object()
                    .is_none_or(|v| v.is_empty()),
            "unsupported image platform/implicit volumes"
        );
        Ok(())
    }

    fn manifest(&self, hash: &str) -> anyhow::Result<Manifest> {
        ensure!(digest(hash), "invalid OCI manifest reference");
        let bytes = read_bytes(
            &self.root.join("manifests").join(format!("{hash}.json")),
            128 * 1024,
        )?;
        ensure!(sha(&bytes) == hash, "OCI manifest bytes mismatch");
        let manifest: Manifest = serde_json::from_slice(&bytes)?;
        ensure!(
            manifest.schema == "forge/local-oci-manifest/v1"
                && manifest.candidate.operation_receipt.project_id == self.project
                && manifest.candidate.target_policy_sha256 == self.policy_hash
                && manifest.descriptor.source_commit
                    == manifest.candidate.operation_receipt.request.source_commit,
            "OCI manifest binding mismatch"
        );
        Ok(manifest)
    }

    fn readback(
        &self,
        command: &DeliveryCommand,
        original: &WorkspaceOperationReceipt,
    ) -> anyhow::Result<Readback> {
        let dir = operation_dir(&self.root, &command.operation_key);
        let intent: Intent = read(&dir.join("intent.json"))?;
        ensure!(
            intent.receipt.command_sha256 == sha(&serde_json::to_vec(command)?)
                && serde_json::to_vec(&intent.receipt.original_operation)?
                    == serde_json::to_vec(original)?,
            "original OCI input mismatch"
        );
        if let Some(manifest) = &intent.manifest {
            ensure!(
                intent.receipt.manifest_sha256.as_ref()
                    == Some(&sha(&serde_json::to_vec(manifest)?)),
                "OCI intent mismatch"
            );
        }
        let receipt = optional::<Receipt>(&dir.join("result.json"))?
            .unwrap_or_else(|| intent.receipt.clone());
        let reconciled_receipt = optional::<Receipt>(&dir.join("reconciled.json"))?;
        for r in std::iter::once(&receipt).chain(reconciled_receipt.iter()) {
            ensure!(
                r.schema == "forge/local-oci-operation/v1"
                    && r.scope == "owner_local_verification"
                    && !r.dispatch_allowed
                    && !r.sdlc_acceptance_verified
                    && r.command_sha256 == intent.receipt.command_sha256
                    && serde_json::to_vec(&r.command)? == serde_json::to_vec(command)?
                    && r.manifest_sha256 == intent.receipt.manifest_sha256
                    && r.previous_manifest_sha256 == intent.receipt.previous_manifest_sha256
                    && serde_json::to_vec(&r.original_operation)? == serde_json::to_vec(original)?,
                "OCI receipt does not match original intent"
            );
            if r.status == DeliveryStatus::Verified {
                let m = intent
                    .manifest
                    .as_ref()
                    .context("verified OCI manifest missing")?;
                ensure!(
                    r.image_id.as_ref() == Some(&m.descriptor.image_id)
                        && r.container_id.as_ref().is_some_and(|id| digest(id))
                        && r.data_sha256.as_ref() == Some(&m.data_sha256),
                    "verified OCI identity missing"
                );
                for (probe, expected) in [
                    (&r.version, r.manifest_sha256.as_ref().unwrap()),
                    (&r.health, &self.policy.checks.health_body_sha256),
                    (&r.acceptance, &self.policy.checks.acceptance_body_sha256),
                    (&r.compatibility, &m.data_sha256),
                ] {
                    ensure!(
                        probe
                            .as_ref()
                            .is_some_and(|p| p.status == DeliveryStatus::Verified
                                && p.http_status == Some(200)
                                && p.body_sha256.as_ref() == Some(expected)),
                        "verified OCI check evidence missing"
                    );
                }
            }
        }
        let current_manifest_sha256 = pointer(&self.root, "current.json")?;
        let confirmed_manifest_sha256 = pointer(&self.root, "confirmed.json")?;
        let reconciliation_needed =
            reconciled_receipt.as_ref().unwrap_or(&receipt).status == DeliveryStatus::Unknown;
        Ok(Readback {
            receipt,
            reconciled_receipt,
            current_manifest_sha256,
            confirmed_manifest_sha256,
            reconciliation_needed,
        })
    }

    fn child_stopped(&self, dir: &Path) -> anyhow::Result<()> {
        if optional::<bool>(&dir.join("child-stopped.json"))? == Some(true) {
            return Ok(());
        }
        let child: ChildIdentity = read(&dir.join("child.json"))?;
        match child_start(child.pid) {
            Ok(start) => ensure!(start != child.start, "previous Compose writer still alive"),
            Err(_) => ensure!(
                !PathBuf::from(format!("/proc/{}", child.pid)).exists(),
                "previous writer exit unavailable"
            ),
        }
        immutable(&dir.join("child-stopped.json"), &true)
    }

    async fn observe(&self, mut receipt: Receipt, manifest: &Manifest) -> Receipt {
        let result = async {
            self.engine().await?;
            self.image(&manifest.descriptor).await?;
            let (_, data) = self.data(&manifest.descriptor)?;
            ensure!(
                data == manifest.data_sha256,
                "published data identity changed"
            );
            let name = format!("{}-application-1", self.policy.project_name);
            let containers = self.docker(&["container", "inspect", &name]).await?;
            let c = &containers[0];
            ensure!(
                c["Image"] == manifest.descriptor.image_id
                    && c["State"]["Running"] == true
                    && c["Config"]["Labels"]["com.docker.compose.project"]
                        == self.policy.project_name
                    && c["Config"]["Labels"]["sdlc.task"] == "forge-task-delivery"
                    && c["HostConfig"]["ReadonlyRootfs"] == true
                    && c["HostConfig"]["Privileged"] == false,
                "actual running container/config mismatch"
            );
            let mounts = c["Mounts"]
                .as_array()
                .context("mount readback unavailable")?;
            ensure!(
                mounts.len() == 2 && mounts.iter().all(|m| m["RW"] == false),
                "unexpected writable/persistent mount"
            );
            let expected_manifest = self
                .host_path(&self.root.join("manifests").join(format!(
                    "{}.json",
                    receipt.manifest_sha256.as_ref().unwrap()
                )))
                .await?;
            let expected_data = self.host_path(&self.policy.data_file).await?;
            ensure!(
                mounts
                    .iter()
                    .any(|m| m["Destination"] == "/forge/manifest.json"
                        && m["Source"] == expected_manifest.display().to_string())
                    && mounts.iter().any(|m| m["Destination"] == "/forge/data.json"
                        && m["Source"] == expected_data.display().to_string()),
                "actual data/manifest mount identity mismatch"
            );
            let networks = c["NetworkSettings"]["Networks"]
                .as_object()
                .context("network readback unavailable")?;
            ensure!(
                networks.len() == 1 && networks.contains_key(&self.policy.network_name),
                "unexpected application network"
            );
            let ip = networks[&self.policy.network_name]["IPAddress"]
                .as_str()
                .context("serving address missing")?;
            let parsed: std::net::IpAddr = ip.parse()?;
            let mut checks = self.policy.checks.clone();
            checks.origin = format!("http://{parsed}:8000");
            let hash = receipt.manifest_sha256.as_ref().unwrap();
            let mut version = probes::probe(&checks, "/.forge/version", hash, 128 * 1024).await;
            for _ in 0..10 {
                if version.status != DeliveryStatus::Unavailable {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
                version = probes::probe(&checks, "/.forge/version", hash, 128 * 1024).await;
            }
            let health = probes::probe(
                &checks,
                &checks.health_path,
                &checks.health_body_sha256,
                64 * 1024,
            )
            .await;
            let acceptance = probes::probe(
                &checks,
                &checks.acceptance_path,
                &checks.acceptance_body_sha256,
                64 * 1024,
            )
            .await;
            let compatibility = probes::probe(&checks, "/compatibility", &data, 1024 * 1024).await;
            let end_version = probes::probe(&checks, "/.forge/version", hash, 128 * 1024).await;
            let end = self.docker(&["container", "inspect", &name]).await?;
            ensure!(
                end[0]["Id"] == c["Id"]
                    && end[0]["Image"] == manifest.descriptor.image_id
                    && end[0]["State"]["Running"] == true
                    && self.data(&manifest.descriptor)?.1 == data,
                "container/data changed during checks"
            );
            receipt.status = if version.status == DeliveryStatus::Unavailable
                || end_version.status == DeliveryStatus::Unavailable
            {
                DeliveryStatus::Unknown
            } else if [
                version.status,
                end_version.status,
                health.status,
                acceptance.status,
                compatibility.status,
            ]
            .contains(&DeliveryStatus::Failed)
            {
                DeliveryStatus::Failed
            } else if [health.status, acceptance.status, compatibility.status]
                .contains(&DeliveryStatus::Unavailable)
            {
                DeliveryStatus::Unavailable
            } else {
                DeliveryStatus::Verified
            };
            receipt.version = Some(end_version);
            receipt.health = Some(health);
            receipt.acceptance = Some(acceptance);
            receipt.compatibility = Some(compatibility);
            receipt.container_id = c["Id"].as_str().map(str::to_owned);
            receipt.image_id = Some(manifest.descriptor.image_id.clone());
            receipt.data_sha256 = Some(data);
            Ok::<_, anyhow::Error>(())
        }
        .await;
        if result.is_err() {
            receipt.status = DeliveryStatus::Unknown;
        }
        receipt.reason = match receipt.status {
            DeliveryStatus::Verified => "owner_oci_identity_data_application_checks_verified",
            DeliveryStatus::Failed => "oci_application_check_failed",
            DeliveryStatus::Unavailable => "oci_application_check_unavailable",
            DeliveryStatus::Unknown => "oci_outcome_unknown_reconciliation_required",
        }
        .into();
        receipt
    }

    async fn execute(
        &self,
        command: &DeliveryCommand,
        original: WorkspaceOperationReceipt,
        candidate: Option<VerifiedCandidate>,
        reconcile: bool,
    ) -> anyhow::Result<Readback> {
        let _lock = Lock::acquire(&self.root)?;
        let dir = operation_dir(&self.root, &command.operation_key);
        if dir.exists() {
            let old = self.readback(command, &original)?;
            if reconcile && old.reconciliation_needed {
                self.child_stopped(&dir)?;
                let intent: Intent = read(&dir.join("intent.json"))?;
                if let Some(manifest) = intent.manifest {
                    let hash = intent.receipt.manifest_sha256.clone().unwrap();
                    if pointer(&self.root, "current.json")?.as_ref() == Some(&hash) {
                        let receipt = self.observe(intent.receipt, &manifest).await;
                        if receipt.status != DeliveryStatus::Unknown {
                            if receipt.status == DeliveryStatus::Verified {
                                immutable(&dir.join("verified-checks.json"), &receipt)?;
                                replace_pointer(&self.root, "confirmed.json", &hash)?;
                            }
                            immutable(&dir.join("reconciled.json"), &receipt)?;
                        }
                    }
                }
            }
            return self.readback(command, &original);
        }
        ensure!(!reconcile, "original OCI operation unavailable");
        let mut count = 0;
        for entry in std::fs::read_dir(self.root.join("operations"))? {
            count += 1;
            ensure!(count <= 1024, "OCI operation inventory bound exceeded");
            let path = entry?.path();
            plain(&path)?;
            let _: Intent = read(&path.join("intent.json"))?;
            let result = optional::<Receipt>(&path.join("reconciled.json"))?
                .or(optional::<Receipt>(&path.join("result.json"))?);
            ensure!(
                result.is_some_and(|r| r.status != DeliveryStatus::Unknown),
                "unknown OCI operation holds target"
            );
        }
        let previous = pointer(&self.root, "current.json")?;
        ensure!(
            previous == command.expected_manifest_sha256,
            "OCI current CAS mismatch"
        );
        self.engine().await?;
        let inventory = self
            .run(
                &self.policy.docker_bin,
                &[
                    "container".into(),
                    "ls".into(),
                    "--all".into(),
                    "--quiet".into(),
                    "--filter".into(),
                    format!(
                        "label=com.docker.compose.project={}",
                        self.policy.project_name
                    ),
                ],
                None,
            )
            .await?;
        let ids = std::str::from_utf8(&inventory)?
            .lines()
            .filter(|id| !id.is_empty())
            .collect::<Vec<_>>();
        ensure!(ids.len() <= 1, "unexpected OCI project service inventory");
        if let Some(id) = ids.first() {
            let old_hash = previous
                .as_ref()
                .context("refusing enrollment over existing containers")?;
            let old = self.manifest(old_hash)?;
            let actual = self.docker(&["container", "inspect", id]).await?;
            ensure!(
                actual[0]["Image"] == old.descriptor.image_id
                    && actual[0]["Config"]["Labels"]["com.docker.compose.service"] == "application"
                    && actual[0]["Config"]["Labels"]["sdlc.task"] == "forge-task-delivery"
                    && actual[0]["Config"]["Labels"]["sdlc.purpose"]
                        == "disposable-oci-application",
                "existing container is outside owner intent"
            );
        }
        let manifest = match command.action {
            DeliveryAction::Deploy => {
                let candidate = candidate.context("verified OCI candidate unavailable")?;
                let descriptor: Descriptor =
                    serde_json::from_slice(&read_bytes(&candidate.storage_path, 16 * 1024)?)?;
                ensure!(
                    descriptor.source_commit == original.request.source_commit
                        && sha(&read_bytes(&candidate.storage_path, 16 * 1024)?)
                            == candidate.manifest.artifact_sha256,
                    "OCI descriptor/source mismatch"
                );
                let (data_schema_version, data_sha256) = self.data(&descriptor)?;
                self.image(&descriptor).await?;
                Manifest {
                    schema: "forge/local-oci-manifest/v1".into(),
                    candidate: candidate.manifest,
                    descriptor,
                    data_schema_version,
                    data_sha256,
                }
            }
            DeliveryAction::Rollback => {
                let hash = pointer(&self.root, "confirmed.json")?
                    .context("last-confirmed OCI manifest unavailable")?;
                self.manifest(&hash)?
            }
        };
        let (schema, data) = self.data(&manifest.descriptor)?;
        ensure!(
            schema == manifest.data_schema_version && data == manifest.data_sha256,
            "rollback data compatibility unavailable"
        );
        self.image(&manifest.descriptor).await?;
        let hash = sha(&serde_json::to_vec(&manifest)?);
        let receipt = Receipt {
            schema: "forge/local-oci-operation/v1".into(),
            scope: "owner_local_verification".into(),
            command: command.clone(),
            command_sha256: sha(&serde_json::to_vec(command)?),
            original_operation: original.clone(),
            manifest_sha256: Some(hash.clone()),
            previous_manifest_sha256: previous,
            status: DeliveryStatus::Unknown,
            reason: "oci_effect_not_finalized".into(),
            version: None,
            health: None,
            acceptance: None,
            compatibility: None,
            container_id: None,
            image_id: None,
            data_sha256: None,
            dispatch_allowed: false,
            sdlc_acceptance_verified: false,
        };
        directory(&dir)?;
        immutable(
            &dir.join("intent.json"),
            &Intent {
                receipt: receipt.clone(),
                manifest: Some(manifest.clone()),
            },
        )?;
        let manifest_file = self.root.join("manifests").join(format!("{hash}.json"));
        immutable(&manifest_file, &manifest)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&manifest_file, std::fs::Permissions::from_mode(0o444))?;
        }
        let host_manifest = self.host_path(&manifest_file).await?;
        let host_data = self.host_path(&self.policy.data_file).await?;
        let compose = serde_json::json!({"services":{"application":{"image":manifest.descriptor.image_id,"pull_policy":"never","user":"65532:65532","read_only":true,"cap_drop":["ALL"],"security_opt":["no-new-privileges:true"],"pids_limit":64,"mem_limit":"128m","cpus":0.5,"labels":{"sdlc.task":"forge-task-delivery","sdlc.purpose":"disposable-oci-application"},"volumes":[{"type":"bind","source":host_manifest,"target":"/forge/manifest.json","read_only":true},{"type":"bind","source":host_data,"target":"/forge/data.json","read_only":true}],"networks":["owner"]}},"networks":{"owner":{"external":true,"name":self.policy.network_name}}});
        let compose_file = dir.join("compose.json");
        immutable(&compose_file, &compose)?;
        let execution_file = self
            .policy
            .compose_root
            .join(format!("{}.json", sha(command.operation_key.as_bytes())));
        immutable(&execution_file, &compose)?;
        replace_pointer(&self.root, "current.json", &hash)?;
        let args = vec![
            "-p".into(),
            self.policy.project_name.clone(),
            "-f".into(),
            execution_file.display().to_string(),
            "up".into(),
            "--detach".into(),
            "--no-build".into(),
            "--pull".into(),
            "never".into(),
            "--force-recreate".into(),
            "application".into(),
        ];
        self.run(&self.policy.compose_bin, &args, Some(&dir))
            .await?;
        let receipt = self.observe(receipt, &manifest).await;
        if receipt.status == DeliveryStatus::Verified {
            immutable(&dir.join("verified-checks.json"), &receipt)?;
            replace_pointer(&self.root, "confirmed.json", &hash)?;
        }
        immutable(&dir.join("result.json"), &receipt)?;
        self.readback(command, &original)
    }
}

/// Privileged local entry point; never called by the SDLC HTTP command route.
pub async fn local_command(
    state: Arc<AppState>,
    project: Uuid,
    token: &str,
    command: DeliveryCommand,
    reconcile: bool,
    read_only: bool,
) -> anyhow::Result<Readback> {
    if !read_only {
        ensure!(
            std::env::var("CICD_LOCAL_DELIVERY_MODE").ok().as_deref() == Some("local-verification"),
            "explicit local verification mode required"
        );
    }
    command.validate().map_err(anyhow::Error::msg)?;
    ensure!(
        token.starts_with("forge_sat_"),
        "existing owner machine credential required"
    );
    let db = state.pool.as_ref().context("owner database unavailable")?;
    let claims = crate::api::identity_for_bearer_token(
        db,
        state
            .auth_secret
            .as_deref()
            .context("owner auth unavailable")?,
        token,
    )
    .await
    .map_err(|_| anyhow::anyhow!("owner credential rejected"))?;
    let mut tx = db.begin().await?;
    sdlc_workspace::authorize(&mut tx, &state, &claims, project, "api:read")
        .await
        .map_err(|_| anyhow::anyhow!("owner read rejected"))?;
    if !read_only {
        sdlc_workspace::authorize(&mut tx, &state, &claims, project, "api:write")
            .await
            .map_err(|_| anyhow::anyhow!("owner write rejected"))?;
    }
    let original = ledger::find_receipt(
        &mut tx,
        project,
        claims.sub,
        &command.workspace_operation_key,
    )
    .await?
    .context("original operation unavailable")?;
    sdlc_workspace::verify_lookup(&original, &command.original)
        .map_err(|_| anyhow::anyhow!("original binding mismatch"))?;
    let root = PathBuf::from(std::env::var("CICD_LOCAL_OCI_ROOT")?);
    let resolved = if root.exists() {
        plain(&root)?;
        root.canonicalize()?
    } else {
        let parent = root.parent().context("OCI parent missing")?;
        plain(parent)?;
        parent
            .canonicalize()?
            .join(root.file_name().context("OCI root name missing")?)
    };
    for protected in [&state.git.root, &state.config.artifacts.root] {
        if let Ok(path) = protected.canonicalize() {
            ensure!(
                !resolved.starts_with(&path) && !path.starts_with(&resolved),
                "OCI target overlaps protected root"
            );
        }
    }
    let owner = Owner::open(project, !read_only)?;
    let result = if read_only {
        owner.readback(&command, &original)?
    } else {
        let existing = operation_dir(&owner.root, &command.operation_key).exists();
        let candidate = if !existing && command.action == DeliveryAction::Deploy {
            super::candidate(
                state.clone(),
                claims.clone(),
                project,
                &command,
                owner.policy_hash.clone(),
            )
            .await?
        } else {
            None
        };
        sdlc_workspace::authorize(&mut tx, &state, &claims, project, "api:write")
            .await
            .map_err(|_| anyhow::anyhow!("fresh owner write rejected"))?;
        owner
            .execute(&command, original, candidate, reconcile)
            .await?
    };
    tx.commit().await?;
    Ok(result)
}
