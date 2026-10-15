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

#[derive(Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schema(as = OciDeliveryReceipt)]
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

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = OciDeliveryReadback)]
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

struct TaskAuthority<'a> {
    state: &'a AppState,
    claims: &'a crate::auth::AccessClaims,
    connection: &'a mut sqlx::PgConnection,
    permit: tracker::Permit,
}

impl TaskAuthority<'_> {
    async fn refresh(
        &mut self,
        owner: &Owner,
        command: &DeliveryCommand,
        original: &WorkspaceOperationReceipt,
    ) -> anyhow::Result<()> {
        let policy = owner
            .policy
            .checks
            .task_execution
            .as_ref()
            .context("task policy absent")?;
        sdlc_workspace::authorize(
            self.connection,
            self.state,
            self.claims,
            owner.project,
            "api:write",
        )
        .await
        .map_err(|_| anyhow::anyhow!("owner write authorization changed"))?;
        task_environment(self.connection, owner.project, policy)
            .await
            .map_err(|_| anyhow::anyhow!("nonproduction task target changed"))?;
        crate::repository_catalog::require_project_writable(
            self.state.pool.as_ref().context("owner database absent")?,
            owner.project,
        )
        .await
        .map_err(|_| anyhow::anyhow!("project namespace became read-only"))?;
        self.permit = tracker::read(
            self.state,
            policy,
            original,
            command.task_fence.as_ref().context("task fence absent")?,
            true,
        )
        .await
        .map_err(|_| anyhow::anyhow!("Tracker authority changed before effect"))?;
        sdlc_workspace::authorize(
            self.connection,
            self.state,
            self.claims,
            owner.project,
            "api:write",
        )
        .await
        .map_err(|_| anyhow::anyhow!("owner write authorization changed"))?;
        self.permit.check()
    }
}

fn receipt_scope(command: &DeliveryCommand) -> &'static str {
    if command.task_fence.is_some() {
        "task_nonproduction_delivery"
    } else {
        "owner_local_verification"
    }
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

pub fn rejection_document(error: &anyhow::Error) -> serde_json::Value {
    let rejection = serde_json::json!({"schema":"forge/local-oci-rejection/v1","status":rejection_status(error),"reason":rejection_code(error),"dispatchAllowed":false,"sdlcAcceptanceVerified":false});
    #[cfg(feature = "oci-integration")]
    let rejection = {
        let mut rejection = rejection;
        if let Some(class) = child_error_class(error) {
            rejection["childErrorClass"] = class.into();
        }
        rejection
    };
    rejection
}

#[cfg(feature = "oci-integration")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChildErrorClass {
    ValueError,
    RuntimeError,
    OSError,
    TimeoutError,
    FileNotFoundError,
    PermissionError,
    AssertionError,
    JSONDecodeError,
    KeyError,
    TypeError,
    TimeoutExpired,
    DiskSpaceGuard,
    DaemonIdentityGuard,
    ExistingResourcesGuard,
    Unknown,
}

#[cfg(feature = "oci-integration")]
impl ChildErrorClass {
    fn from_stderr(stderr: &[u8]) -> Self {
        // Whole completed stderr only; this is an observation, not bridge authority.
        if stderr.len() > 128 {
            return Self::Unknown;
        }
        match stderr {
            b"NATIVE_QA_COMPOSE_BRIDGE_FAILED:ValueError\n" => Self::ValueError,
            b"NATIVE_QA_COMPOSE_BRIDGE_FAILED:RuntimeError\n" => Self::RuntimeError,
            b"NATIVE_QA_COMPOSE_BRIDGE_FAILED:OSError\n" => Self::OSError,
            b"NATIVE_QA_COMPOSE_BRIDGE_FAILED:TimeoutError\n" => Self::TimeoutError,
            b"NATIVE_QA_COMPOSE_BRIDGE_FAILED:FileNotFoundError\n" => Self::FileNotFoundError,
            b"NATIVE_QA_COMPOSE_BRIDGE_FAILED:PermissionError\n" => Self::PermissionError,
            b"NATIVE_QA_COMPOSE_BRIDGE_FAILED:AssertionError\n" => Self::AssertionError,
            b"NATIVE_QA_COMPOSE_BRIDGE_FAILED:JSONDecodeError\n" => Self::JSONDecodeError,
            b"NATIVE_QA_COMPOSE_BRIDGE_FAILED:KeyError\n" => Self::KeyError,
            b"NATIVE_QA_COMPOSE_BRIDGE_FAILED:TypeError\n" => Self::TypeError,
            b"NATIVE_QA_COMPOSE_BRIDGE_FAILED:TimeoutExpired\n" => Self::TimeoutExpired,
            b"NATIVE_QA_COMPOSE_BRIDGE_FAILED:DiskSpaceGuard\n" => Self::DiskSpaceGuard,
            b"NATIVE_QA_COMPOSE_BRIDGE_FAILED:DaemonIdentityGuard\n" => Self::DaemonIdentityGuard,
            b"NATIVE_QA_COMPOSE_BRIDGE_FAILED:ExistingResourcesGuard\n" => {
                Self::ExistingResourcesGuard
            }
            _ => Self::Unknown,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::ValueError => "ValueError",
            Self::RuntimeError => "RuntimeError",
            Self::OSError => "OSError",
            Self::TimeoutError => "TimeoutError",
            Self::FileNotFoundError => "FileNotFoundError",
            Self::PermissionError => "PermissionError",
            Self::AssertionError => "AssertionError",
            Self::JSONDecodeError => "JSONDecodeError",
            Self::KeyError => "KeyError",
            Self::TypeError => "TypeError",
            Self::TimeoutExpired => "TimeoutExpired",
            Self::DiskSpaceGuard => "DiskSpaceGuard",
            Self::DaemonIdentityGuard => "DaemonIdentityGuard",
            Self::ExistingResourcesGuard => "ExistingResourcesGuard",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug)]
struct ChildFailure {
    #[cfg(feature = "oci-integration")]
    class: ChildErrorClass,
}
impl std::fmt::Display for ChildFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("owner engine command failed")
    }
}
impl std::error::Error for ChildFailure {}

#[cfg(feature = "oci-integration")]
fn child_error_class(error: &anyhow::Error) -> Option<&'static str> {
    error
        .downcast_ref::<ChildFailure>()
        .map(|e| e.class.as_str())
}

fn child_outcome(success: bool, effect: bool, _stderr: &[u8]) -> anyhow::Result<()> {
    if !success && effect {
        return Err(ChildFailure {
            #[cfg(feature = "oci-integration")]
            class: ChildErrorClass::from_stderr(_stderr),
        }
        .into());
    }
    ensure!(success, "owner engine command failed");
    Ok(())
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
        Self::open_at(
            project,
            create,
            &PathBuf::from(std::env::var("CICD_LOCAL_OCI_ROOT")?),
            &PathBuf::from(std::env::var("CICD_LOCAL_OCI_POLICY")?),
        )
    }

    fn open_at(
        project: Uuid,
        create: bool,
        root: &Path,
        policy_path: &Path,
    ) -> anyhow::Result<Self> {
        ensure!(
            cfg!(target_os = "linux"),
            "OCI executor requires Linux process/filesystem semantics"
        );
        ensure!(
            root.is_absolute()
                && root.components().all(|c| !matches!(
                    c,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )),
            "isolated normalized OCI root required"
        );
        let policy: Policy = read(policy_path)?;
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
            let (status, out, err) = tokio::try_join!(
                async { Ok::<_, anyhow::Error>(child.wait().await?) },
                read(Box::new(stdout)),
                read(Box::new(stderr))
            )?;
            child_outcome(status.success(), effect.is_some(), &err)?;
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
                && serde_json::to_vec(&intent.receipt.command)? == serde_json::to_vec(command)?
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
            self.validate_receipt(r, &intent)?;
        }
        if let Some(proof) = optional::<Receipt>(&dir.join("verified-checks.json"))? {
            ensure!(
                proof.status == DeliveryStatus::Verified,
                "OCI historical checks are not verified"
            );
            self.validate_receipt(&proof, &intent)?;
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

    fn validate_receipt(&self, r: &Receipt, intent: &Intent) -> anyhow::Result<()> {
        ensure!(
            r.schema == "forge/local-oci-operation/v1"
                && r.scope == receipt_scope(&r.command)
                && r.dispatch_allowed == r.command.task_fence.is_some()
                && !r.sdlc_acceptance_verified
                && r.command_sha256 == intent.receipt.command_sha256
                && serde_json::to_vec(&r.command)? == serde_json::to_vec(&intent.receipt.command)?
                && r.manifest_sha256 == intent.receipt.manifest_sha256
                && r.previous_manifest_sha256 == intent.receipt.previous_manifest_sha256
                && serde_json::to_vec(&r.original_operation)?
                    == serde_json::to_vec(&intent.receipt.original_operation)?,
            "OCI receipt does not match original intent"
        );
        if r.status == DeliveryStatus::Verified {
            let m = intent
                .manifest
                .as_ref()
                .context("verified OCI manifest missing")?;
            let hash = sha(&serde_json::to_vec(m)?);
            ensure!(
                r.reason == "owner_oci_identity_data_application_checks_verified"
                    && r.manifest_sha256.as_ref() == Some(&hash)
                    && r.image_id.as_ref() == Some(&m.descriptor.image_id)
                    && r.container_id.as_ref().is_some_and(|id| digest(id))
                    && r.data_sha256.as_ref() == Some(&m.data_sha256),
                "verified OCI identity missing"
            );
            for (probe, expected) in [
                (&r.version, &hash),
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
        Ok(())
    }

    fn retain_verified_checks(
        &self,
        dir: &Path,
        intent: &Intent,
        fresh: &Receipt,
    ) -> anyhow::Result<()> {
        ensure!(
            fresh.status == DeliveryStatus::Verified,
            "fresh OCI checks are not verified"
        );
        self.validate_receipt(fresh, intent)?;
        let path = dir.join("verified-checks.json");
        if let Some(proof) = optional::<Receipt>(&path)? {
            ensure!(
                proof.status == DeliveryStatus::Verified,
                "OCI historical checks are not verified"
            );
            // Historical probe times differ from recovery; retain only exact intent-bound proof.
            self.validate_receipt(&proof, intent)?;
            Ok(())
        } else {
            immutable(&path, fresh)
        }
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
        mut task: Option<TaskAuthority<'_>>,
    ) -> anyhow::Result<Readback> {
        ensure!(
            command.task_fence.is_some() == task.is_some(),
            "task effect requires live owner authorization"
        );
        if let Some(task) = &task {
            task.permit.check()?;
        }
        let _lock = Lock::acquire(&self.root)?;
        let dir = operation_dir(&self.root, &command.operation_key);
        if dir.exists() {
            let old = self.readback(command, &original)?;
            if reconcile && old.reconciliation_needed {
                if let Some(task) = &mut task {
                    task.refresh(self, command, &original).await?;
                }
                self.child_stopped(&dir)?;
                let intent: Intent = read(&dir.join("intent.json"))?;
                if let Some(manifest) = &intent.manifest {
                    let hash = intent.receipt.manifest_sha256.clone().unwrap();
                    if pointer(&self.root, "current.json")?.as_ref() == Some(&hash) {
                        let receipt = self.observe(intent.receipt.clone(), manifest).await;
                        if receipt.status != DeliveryStatus::Unknown {
                            if let Some(task) = &mut task {
                                task.refresh(self, command, &original).await?;
                            }
                            if receipt.status == DeliveryStatus::Verified {
                                self.retain_verified_checks(&dir, &intent, &receipt)?;
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
            scope: receipt_scope(command).into(),
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
            dispatch_allowed: command.task_fence.is_some(),
            sdlc_acceptance_verified: false,
        };
        if let Some(task) = &task {
            task.permit.check()?;
        }
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
        if let Some(task) = &mut task {
            task.refresh(self, command, &original).await?;
        }
        replace_pointer(&self.root, "current.json", &hash)?;
        if let Some(task) = &mut task {
            task.refresh(self, command, &original).await?;
        }
        self.run(&self.policy.compose_bin, &args, Some(&dir))
            .await?;
        let receipt = self.observe(receipt, &manifest).await;
        if let Some(task) = &mut task {
            task.refresh(self, command, &original).await?;
        }
        if receipt.status == DeliveryStatus::Verified {
            immutable(&dir.join("verified-checks.json"), &receipt)?;
            #[cfg(feature = "oci-integration")]
            if std::env::var("CICD_TEST_OCI_PAUSE_AFTER_VERIFIED_CHECKS").as_deref() == Ok("1") {
                // Only the opt-in native QA binary exposes this bounded crash window.
                immutable(&dir.join("qa-verified-checks-ready.json"), &true)?;
                tokio::time::sleep(Duration::from_secs(20)).await;
                anyhow::bail!("native QA verified-checks checkpoint was not interrupted");
            }
            if let Some(task) = &mut task {
                task.refresh(self, command, &original).await?;
            }
            replace_pointer(&self.root, "confirmed.json", &hash)?;
        }
        immutable(&dir.join("result.json"), &receipt)?;
        self.readback(command, &original)
    }
}

fn check_owner_root(state: &AppState, root: &Path) -> anyhow::Result<()> {
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
    Ok(())
}

fn task_owner(state: &AppState, project: Uuid) -> anyhow::Result<Owner> {
    let config = &state.config.sdlc_workspace;
    let root = config
        .local_delivery_root
        .as_deref()
        .context("OCI target root absent")?;
    let policy = config
        .local_delivery_policy
        .as_deref()
        .context("OCI target policy absent")?;
    check_owner_root(state, root)?;
    Owner::open_at(project, false, root, policy)
}

pub(super) async fn execute_task(
    state: Arc<AppState>,
    claims: &crate::auth::AccessClaims,
    connection: &mut sqlx::PgConnection,
    project: Uuid,
    command: &DeliveryCommand,
    original: WorkspaceOperationReceipt,
) -> Result<Readback, crate::api::ApiError> {
    use crate::api::ApiError;
    let owner = task_owner(&state, project)
        .map_err(|_| ApiError::service_unavailable("Опубликованный OCI owner target недоступен"))?;
    let policy =
        owner.policy.checks.task_execution.as_ref().ok_or_else(|| {
            ApiError::service_unavailable("Nonproduction OCI task policy недоступна")
        })?;
    task_environment(connection, project, policy).await?;
    let fence = command
        .task_fence
        .as_ref()
        .ok_or_else(|| ApiError::service_unavailable("Нужен текущий task delivery fence"))?;
    let existing = operation_dir(&owner.root, &command.operation_key).exists();
    if existing {
        let saved = owner
            .readback(command, &original)
            .map_err(|_| ApiError::conflict("Исходный OCI delivery journal не совпадает"))?;
        if !saved.reconciliation_needed {
            return Ok(saved);
        }
    }
    crate::repository_catalog::require_project_writable(
        state
            .pool
            .as_ref()
            .ok_or_else(|| ApiError::service_unavailable("owner database unavailable"))?,
        project,
    )
    .await?;
    let candidate = if !existing && command.action == DeliveryAction::Deploy {
        super::candidate(
            state.clone(),
            claims.clone(),
            project,
            command,
            owner.policy_hash.clone(),
        )
        .await
        .map_err(|_| ApiError::conflict("Readback точного OCI pipeline artifact отклонён"))?
    } else {
        None
    };
    let permit = tracker::read(&state, policy, &original, fence, existing).await?;
    sdlc_workspace::authorize(connection, &state, claims, project, "api:write").await?;
    owner
        .execute(
            command,
            original,
            candidate,
            existing,
            Some(TaskAuthority {
                state: &state,
                claims,
                connection,
                permit,
            }),
        )
        .await
        .map_err(|_| ApiError::conflict("OCI delivery удерживается; прочитайте original key"))
}

pub(super) fn read_task(
    state: &AppState,
    project: Uuid,
    workspace_key: &str,
    delivery_key: &str,
    original: &WorkspaceOperationReceipt,
) -> Result<Readback, crate::api::ApiError> {
    use crate::api::ApiError;
    let owner = task_owner(state, project)
        .map_err(|_| ApiError::service_unavailable("Опубликованный OCI owner target недоступен"))?;
    let intent = optional::<Intent>(&operation_dir(&owner.root, delivery_key).join("intent.json"))
        .map_err(|_| ApiError::conflict("OCI delivery journal недоступен или изменён"))?
        .ok_or_else(|| ApiError::not_found_named("OCI delivery operation"))?;
    if intent.receipt.command.workspace_operation_key != workspace_key
        || intent.receipt.command.operation_key != delivery_key
        || intent.receipt.command.task_fence.is_none()
    {
        return Err(ApiError::conflict(
            "Исходный task OCI delivery key не совпадает",
        ));
    }
    intent
        .receipt
        .command
        .validate()
        .map_err(|_| ApiError::conflict("Исходный OCI command невалиден"))?;
    owner
        .readback(&intent.receipt.command, original)
        .map_err(|_| ApiError::conflict("Исходный OCI delivery journal не совпадает"))
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
        command.task_fence.is_none(),
        "task delivery uses the authenticated owner API"
    );
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
    check_owner_root(
        &state,
        &PathBuf::from(std::env::var("CICD_LOCAL_OCI_ROOT")?),
    )?;
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
            .execute(&command, original, candidate, reconcile, None)
            .await?
    };
    tx.commit().await?;
    Ok(result)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    struct RecoveryFixture {
        owner: Owner,
        dir: PathBuf,
        intent: Intent,
    }

    impl RecoveryFixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("forge-oci-recovery-{}", Uuid::new_v4()));
            directory(&root).unwrap();
            directory(&root.join("operations")).unwrap();
            let project = Uuid::new_v4();
            let policy = Policy {
                project_name: "sdlc-qa-forge-oci-0123456789ab".into(),
                network_name: "sdlc-qa-forge-0123456789ab_qa".into(),
                daemon_id: "test-daemon".into(),
                docker_bin: root.join("unavailable-docker"),
                compose_bin: root.join("unavailable-compose"),
                compose_root: root.clone(),
                volume_name: "sdlc-qa-forge-0123456789ab_delivery-qa".into(),
                volume_root: root.clone(),
                data_file: root.join("data.json"),
                data_sha256: sha(b"data"),
                checks: DeliveryPolicy {
                    origin: "http://127.0.0.1:8000".into(),
                    health_path: "/health".into(),
                    health_body_sha256: sha(b"ok\n"),
                    acceptance_path: "/acceptance".into(),
                    acceptance_body_sha256: sha(b"accepted\n"),
                    task_execution: None,
                },
            };
            let owner = Owner {
                root,
                project,
                policy_hash: sha(&serde_json::to_vec(&policy).unwrap()),
                policy,
            };
            let attempt_id = Uuid::new_v4();
            let request = WorkspaceOperationRequest {
                contract_version: 1,
                operation_key: "workspace:oci-recovery".into(),
                binding: WorkspaceTaskBinding {
                    tracker_instance_id: "test-tracker".into(),
                    tracker_project_id: Uuid::new_v4(),
                    task_id: Uuid::new_v4(),
                    root_task_id: Uuid::new_v4(),
                    assignment_id: Uuid::new_v4(),
                    execution_id: Uuid::new_v4(),
                    routing_snapshot_id: Uuid::new_v4(),
                    requirement_revision: 1,
                    fencing_token: 1,
                    assignment_hash: sha(b"assignment"),
                    workflow_task_ref: "SDLC-1".into(),
                },
                repository_id: Uuid::new_v4(),
                source_commit: "a".repeat(40),
                lease_id: Uuid::new_v4(),
                attempt_id,
                workspace_generation: 1,
                workspace_id: format!("attempt-{attempt_id}-1-{}", Uuid::new_v4().simple()),
                role: WorkspaceRole::Developer,
                access: WorkspaceAccess::ReadWrite,
            };
            request.validate().unwrap();
            let original = WorkspaceOperationReceipt {
                schema: "forge/workspace-operation-receipt/v1".into(),
                operation_id: Uuid::new_v4(),
                project_id: project,
                request_hash: sha(&serde_json::to_vec(&request).unwrap()),
                request,
                recorded_at: "2026-10-07T00:00:00Z".parse().unwrap(),
                status: WorkspaceOperationStatus::Blocked,
                blockers: vec![WorkspaceOperationBlocker::TrackerAdmissionUnavailable],
                physical_source_observed: false,
                dispatch_allowed: false,
            };
            let artifact_id = Uuid::new_v4();
            let command = DeliveryCommand {
                operation_key: "oci:recovery".into(),
                workspace_operation_key: original.request.operation_key.clone(),
                original: WorkspaceOperationLookup {
                    request_hash: original.request_hash.clone(),
                    task_id: original.request.binding.task_id,
                    root_task_id: original.request.binding.root_task_id,
                    assignment_id: original.request.binding.assignment_id,
                    execution_id: original.request.binding.execution_id,
                    fencing_token: original.request.binding.fencing_token,
                },
                action: DeliveryAction::Deploy,
                artifact_id: Some(artifact_id),
                expected_manifest_sha256: None,
                task_fence: None,
            };
            command.validate().unwrap();
            let manifest = Manifest {
                schema: "forge/local-oci-manifest/v1".into(),
                candidate: DeliveryManifest {
                    schema: "forge/local-static-manifest/v1".into(),
                    operation_receipt: original.clone(),
                    pipeline_id: Uuid::new_v4(),
                    artifact_id,
                    artifact_attempt_id: Uuid::new_v4(),
                    artifact_sha256: sha(b"descriptor"),
                    artifact_size_bytes: 10,
                    config_sha256: sha(b"config"),
                    plan_sha256: sha(b"plan"),
                    target_policy_sha256: owner.policy_hash.clone(),
                },
                descriptor: Descriptor {
                    schema: "forge/oci-candidate/v1".into(),
                    image_id: format!("sha256:{}", sha(b"image")),
                    source_commit: original.request.source_commit.clone(),
                    data_protocol: "readonly_snapshot_v1".into(),
                    readable_schema_versions: vec![1],
                    migrations: vec![],
                },
                data_schema_version: 1,
                data_sha256: owner.policy.data_sha256.clone(),
            };
            let hash = sha(&serde_json::to_vec(&manifest).unwrap());
            let intent = Intent {
                receipt: Receipt {
                    schema: "forge/local-oci-operation/v1".into(),
                    scope: "owner_local_verification".into(),
                    command_sha256: sha(&serde_json::to_vec(&command).unwrap()),
                    command,
                    original_operation: original,
                    manifest_sha256: Some(hash.clone()),
                    previous_manifest_sha256: None,
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
                },
                manifest: Some(manifest),
            };
            let dir = operation_dir(&owner.root, &intent.receipt.command.operation_key);
            directory(&dir).unwrap();
            immutable(&dir.join("intent.json"), &intent).unwrap();
            replace_pointer(&owner.root, "current.json", &hash).unwrap();
            Self { owner, dir, intent }
        }

        fn verified(&self, timestamp: &str) -> Receipt {
            let mut receipt = self.intent.receipt.clone();
            let manifest = self.intent.manifest.as_ref().unwrap();
            let probe = |hash: &str| {
                Some(DeliveryProbe {
                    status: DeliveryStatus::Verified,
                    http_status: Some(200),
                    body_sha256: Some(hash.into()),
                    observed_at: timestamp.parse().unwrap(),
                })
            };
            receipt.status = DeliveryStatus::Verified;
            receipt.reason = "owner_oci_identity_data_application_checks_verified".into();
            receipt.version = probe(receipt.manifest_sha256.as_ref().unwrap());
            receipt.health = probe(&self.owner.policy.checks.health_body_sha256);
            receipt.acceptance = probe(&self.owner.policy.checks.acceptance_body_sha256);
            receipt.compatibility = probe(&manifest.data_sha256);
            receipt.container_id = Some(sha(b"container"));
            receipt.image_id = Some(manifest.descriptor.image_id.clone());
            receipt.data_sha256 = Some(manifest.data_sha256.clone());
            receipt
        }

        fn readback(&self) -> anyhow::Result<Readback> {
            self.owner.readback(
                &self.intent.receipt.command,
                &self.intent.receipt.original_operation,
            )
        }
    }

    impl Drop for RecoveryFixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.owner.root).unwrap();
        }
    }

    #[test]
    fn recovery_retains_verified_checks_with_different_probe_times() {
        let fixture = RecoveryFixture::new();
        let proof = fixture.verified("2026-10-07T00:01:00Z");
        let fresh = fixture.verified("2026-10-08T00:01:00Z");
        let path = fixture.dir.join("verified-checks.json");
        immutable(&path, &proof).unwrap();
        let historical_bytes = std::fs::read(&path).unwrap();
        let intent_bytes = std::fs::read(fixture.dir.join("intent.json")).unwrap();
        assert_ne!(historical_bytes, serde_json::to_vec(&fresh).unwrap());
        assert!(fixture.readback().unwrap().reconciliation_needed);
        fixture
            .owner
            .retain_verified_checks(&fixture.dir, &fixture.intent, &fresh)
            .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), historical_bytes);
        immutable(&fixture.dir.join("reconciled.json"), &fresh).unwrap();
        let readback = fixture.readback().unwrap();
        assert_eq!(readback.latest_status(), DeliveryStatus::Verified);
        assert_eq!(
            serde_json::to_vec(&readback.reconciled_receipt.unwrap()).unwrap(),
            serde_json::to_vec(&fresh).unwrap()
        );
        assert_eq!(std::fs::read(&path).unwrap(), historical_bytes);
        assert_eq!(
            std::fs::read(fixture.dir.join("intent.json")).unwrap(),
            intent_bytes
        );
    }

    #[test]
    fn recovery_without_historical_checks_writes_fresh_proof() {
        let fixture = RecoveryFixture::new();
        let fresh = fixture.verified("2026-10-08T00:01:00Z");
        let path = fixture.dir.join("verified-checks.json");
        assert!(!path.exists());
        fixture
            .owner
            .retain_verified_checks(&fixture.dir, &fixture.intent, &fresh)
            .unwrap();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            serde_json::to_vec(&fresh).unwrap()
        );
        fixture
            .owner
            .retain_verified_checks(&fixture.dir, &fixture.intent, &fresh)
            .unwrap();
        assert!(fixture.readback().unwrap().reconciliation_needed);
    }

    #[test]
    fn recovery_rejects_foreign_or_tampered_historical_checks() {
        use serde_json::json;
        let mutations = [
            ("/schema", json!("foreign/v1")),
            ("/scope", json!("foreign")),
            ("/commandSha256", json!(sha(b"foreign-command"))),
            ("/command/operationKey", json!("oci:foreign")),
            ("/command/workspaceOperationKey", json!("workspace:foreign")),
            ("/command/artifactId", json!(Uuid::new_v4())),
            ("/command/original/fencingToken", json!(2)),
            ("/originalOperation/operationId", json!(Uuid::new_v4())),
            ("/originalOperation/projectId", json!(Uuid::new_v4())),
            (
                "/originalOperation/request/sourceCommit",
                json!("b".repeat(40)),
            ),
            ("/manifestSha256", json!(sha(b"foreign-manifest"))),
            ("/previousManifestSha256", json!(sha(b"foreign-previous"))),
            ("/status", json!("unknown")),
            ("/reason", json!("foreign_success")),
            (
                "/imageId",
                json!(format!("sha256:{}", sha(b"foreign-image"))),
            ),
            ("/containerId", json!("invalid")),
            ("/dataSha256", json!(sha(b"foreign-data"))),
            ("/version", json!(null)),
            ("/version/bodySha256", json!(sha(b"foreign-version"))),
            ("/health/bodySha256", json!(sha(b"foreign-health"))),
            ("/health/httpStatus", json!(503)),
            ("/acceptance/status", json!("failed")),
            ("/acceptance/bodySha256", json!(sha(b"foreign-acceptance"))),
            (
                "/compatibility/bodySha256",
                json!(sha(b"foreign-compatibility")),
            ),
            ("/dispatchAllowed", json!(true)),
            ("/sdlcAcceptanceVerified", json!(true)),
        ];
        for (field, value) in mutations {
            let fixture = RecoveryFixture::new();
            let mut proof = serde_json::to_value(fixture.verified("2026-10-07T00:01:00Z")).unwrap();
            *proof.pointer_mut(field).unwrap() = value;
            let path = fixture.dir.join("verified-checks.json");
            immutable(&path, &proof).unwrap();
            let historical_bytes = std::fs::read(&path).unwrap();
            let fresh = fixture.verified("2026-10-08T00:01:00Z");
            assert!(
                fixture
                    .owner
                    .retain_verified_checks(&fixture.dir, &fixture.intent, &fresh)
                    .is_err(),
                "accepted tampered proof: {field}"
            );
            assert!(fixture.readback().is_err(), "read foreign proof: {field}");
            assert_eq!(std::fs::read(&path).unwrap(), historical_bytes);
            assert!(!fixture.dir.join("reconciled.json").exists());
            assert!(
                pointer(&fixture.owner.root, "confirmed.json")
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[tokio::test]
    async fn recovery_requires_fresh_observation_despite_historical_verified_checks() {
        let fixture = RecoveryFixture::new();
        let proof = fixture.verified("2026-10-07T00:01:00Z");
        let path = fixture.dir.join("verified-checks.json");
        immutable(&path, &proof).unwrap();
        immutable(&fixture.dir.join("child-stopped.json"), &true).unwrap();
        let historical_bytes = std::fs::read(&path).unwrap();
        // The unavailable engine makes fresh observation Unknown without running Docker/Compose.
        let readback = fixture
            .owner
            .execute(
                &fixture.intent.receipt.command,
                fixture.intent.receipt.original_operation.clone(),
                None,
                true,
                None,
            )
            .await
            .unwrap();
        assert!(readback.reconciliation_needed);
        assert_eq!(readback.latest_status(), DeliveryStatus::Unknown);
        assert!(readback.reconciled_receipt.is_none());
        assert!(readback.confirmed_manifest_sha256.is_none());
        assert!(!fixture.dir.join("compose.json").exists());
        assert!(!fixture.dir.join("child.json").exists());
        assert_eq!(std::fs::read(&path).unwrap(), historical_bytes);
    }

    fn task_fence() -> crate::domain::task_delivery::TaskDeliveryFence {
        crate::domain::task_delivery::TaskDeliveryFence {
            development_result_id: Uuid::new_v4(),
            assignment_id: Uuid::new_v4(),
            execution_id: Uuid::new_v4(),
            agent_id: Uuid::new_v4(),
            lease_id: Uuid::new_v4(),
            fencing_token: 1,
            expected_lease_version: 1,
            assignment_hash: "a".repeat(64),
        }
    }

    #[test]
    fn task_http_untagged_response_preserves_both_existing_wire_shapes() {
        let fixture = RecoveryFixture::new();
        let oci = fixture.readback().unwrap();
        let expected = serde_json::to_vec(&oci).unwrap();
        assert_eq!(
            serde_json::to_vec(&TaskDeliveryReadback::Oci(oci)).unwrap(),
            expected
        );
        let static_value = DeliveryReadback {
            receipt: DeliveryReceipt {
                schema: "forge/local-delivery-operation/v1".into(),
                scope: "owner_local_verification".into(),
                command: fixture.intent.receipt.command.clone(),
                command_sha256: fixture.intent.receipt.command_sha256.clone(),
                original_operation: fixture.intent.receipt.original_operation.clone(),
                manifest_sha256: None,
                previous_manifest_sha256: None,
                status: DeliveryStatus::Unknown,
                reason: "local_effect_not_finalized".into(),
                version: None,
                served_artifact: None,
                health: None,
                acceptance: None,
                recorded_at: Utc::now(),
                dispatch_allowed: false,
                sdlc_acceptance_verified: false,
            },
            reconciled_receipt: None,
            current_manifest_sha256: None,
            confirmed_manifest_sha256: None,
            reconciliation_needed: true,
        };
        let expected = serde_json::to_vec(&static_value).unwrap();
        assert_eq!(
            serde_json::to_vec(&TaskDeliveryReadback::Static(static_value)).unwrap(),
            expected
        );
    }

    #[test]
    fn task_oci_receipt_cannot_relabel_historical_local_authority() {
        let fixture = RecoveryFixture::new();
        let legacy = fixture.intent.receipt.clone();
        fixture
            .owner
            .validate_receipt(&legacy, &fixture.intent)
            .unwrap();
        let mut forged = legacy.clone();
        forged.scope = "task_nonproduction_delivery".into();
        forged.dispatch_allowed = true;
        assert!(
            fixture
                .owner
                .validate_receipt(&forged, &fixture.intent)
                .is_err()
        );
        let mut intent = Intent {
            receipt: legacy,
            manifest: fixture.intent.manifest.clone(),
        };
        intent.receipt.command.task_fence = Some(task_fence());
        intent.receipt.command_sha256 = sha(&serde_json::to_vec(&intent.receipt.command).unwrap());
        assert!(
            fixture
                .owner
                .validate_receipt(&intent.receipt, &intent)
                .is_err()
        );
        intent.receipt.scope = "task_nonproduction_delivery".into();
        intent.receipt.dispatch_allowed = true;
        fixture
            .owner
            .validate_receipt(&intent.receipt, &intent)
            .unwrap();
        assert!(!fixture.intent.receipt.dispatch_allowed);
        assert_eq!(fixture.intent.receipt.scope, "owner_local_verification");
    }

    #[tokio::test]
    async fn task_oci_effect_without_live_authority_never_enters_engine_or_reconciliation() {
        let fixture = RecoveryFixture::new();
        let original_bytes = std::fs::read(fixture.dir.join("intent.json")).unwrap();
        let mut command = fixture.intent.receipt.command.clone();
        command.task_fence = Some(task_fence());
        for reconcile in [false, true] {
            let error = fixture
                .owner
                .execute(
                    &command,
                    fixture.intent.receipt.original_operation.clone(),
                    None,
                    reconcile,
                    None,
                )
                .await
                .err()
                .unwrap();
            assert!(
                error
                    .to_string()
                    .contains("task effect requires live owner authorization")
            );
        }
        assert!(!fixture.dir.join("child.json").exists());
        assert!(!fixture.dir.join("compose.json").exists());
        assert!(!fixture.dir.join("reconciled.json").exists());
        assert_eq!(
            std::fs::read(fixture.dir.join("intent.json")).unwrap(),
            original_bytes
        );
    }
}

#[cfg(test)]
mod child_diagnostic_tests {
    use super::*;

    #[test]
    fn child_outcome_preserves_failure_and_rejection_contract() {
        for effect in [false, true] {
            assert!(child_outcome(true, effect, b"private-token-sql-path").is_ok());
            let error = child_outcome(false, effect, b"private-token-sql-path")
                .unwrap_err()
                .context("owner engine deadline; outcome requires readback");
            assert_eq!(rejection_status(&error), "unknown_or_rejected");
            assert_eq!(
                rejection_code(&error),
                "oci_command_rejected_or_unavailable"
            );
            assert_eq!(
                error.root_cause().to_string(),
                "owner engine command failed"
            );
            assert_eq!(error.downcast_ref::<ChildFailure>().is_some(), effect);
            assert!(!format!("{error:?}").contains("private"));
        }
        for reason in [
            "unsupported_mutable_data_or_migration",
            "owner_data_snapshot_drift",
            "actual_data_schema_incompatible",
        ] {
            let error = anyhow::Error::new(Blocked(reason));
            let document = rejection_document(&error);
            assert_eq!(document["status"], "blocked");
            assert_eq!(document["reason"], reason);
            assert!(document.get("childErrorClass").is_none());
        }
    }

    #[cfg(not(feature = "oci-integration"))]
    #[test]
    fn ordinary_feature_excludes_child_observation() {
        let error = child_outcome(false, true, b"NATIVE_QA_COMPOSE_BRIDGE_FAILED:ValueError\n")
            .unwrap_err();
        assert_eq!(
            rejection_document(&error),
            serde_json::json!({
            "schema":"forge/local-oci-rejection/v1", "status":"unknown_or_rejected",
            "reason":"oci_command_rejected_or_unavailable", "dispatchAllowed":false,
            "sdlcAcceptanceVerified":false})
        );
        assert!(!format!("{:?}", error.root_cause()).contains("ValueError"));
    }

    #[cfg(feature = "oci-integration")]
    #[test]
    fn all_closed_child_classes_reach_cli_document_without_authority() {
        for class in [
            "ValueError",
            "RuntimeError",
            "OSError",
            "TimeoutError",
            "FileNotFoundError",
            "PermissionError",
            "AssertionError",
            "JSONDecodeError",
            "KeyError",
            "TypeError",
            "TimeoutExpired",
            "DiskSpaceGuard",
            "DaemonIdentityGuard",
            "ExistingResourcesGuard",
        ] {
            let stderr = format!("NATIVE_QA_COMPOSE_BRIDGE_FAILED:{class}\n");
            let error = child_outcome(false, true, stderr.as_bytes())
                .unwrap_err()
                .context("owner engine deadline; outcome requires readback");
            assert_eq!(child_error_class(&error), Some(class));
            let document = rejection_document(&error);
            assert_eq!(
                document,
                serde_json::json!({"schema":"forge/local-oci-rejection/v1",
                "status":"unknown_or_rejected", "reason":"oci_command_rejected_or_unavailable",
                "dispatchAllowed":false, "sdlcAcceptanceVerified":false, "childErrorClass":class})
            );
            assert_eq!(
                error.root_cause().to_string(),
                "owner engine command failed"
            );
            assert!(
                child_error_class(&child_outcome(false, false, stderr.as_bytes()).unwrap_err())
                    .is_none()
            );
            assert!(child_error_class(&anyhow::anyhow!("{stderr}")).is_none());
            assert!(child_outcome(true, true, stderr.as_bytes()).is_ok());
        }
    }

    #[cfg(feature = "oci-integration")]
    #[test]
    fn malformed_mixed_duplicate_truncated_oversized_stderr_is_unknown() {
        let marker = b"NATIVE_QA_COMPOSE_BRIDGE_FAILED:ValueError\n";
        let mut inputs = vec![
            Vec::new(),
            b"private-token-sql-path".to_vec(),
            b"NATIVE_QA_COMPOSE_BRIDGE_FAILED:None\n".to_vec(),
            b"NATIVE_QA_COMPOSE_BRIDGE_FAILED:private-token-sql-path\n".to_vec(),
            b"NATIVE_QA_COMPOSE_BRIDGE_FAILED:ValueError\r\n".to_vec(),
            b"NATIVE_QA_COMPOSE_BRIDGE_FAILED:ValueError\xff\n".to_vec(),
            [marker.as_slice(), marker.as_slice()].concat(),
            [b"private-prefix".as_slice(), marker.as_slice()].concat(),
            [marker.as_slice(), b"private-suffix".as_slice()].concat(),
            vec![b'x'; 129],
            vec![b'x'; 1024 * 1024 + 1],
        ];
        inputs.extend((0..marker.len()).map(|length| marker[..length].to_vec()));
        for class in [
            "DiskSpaceGuard",
            "DaemonIdentityGuard",
            "ExistingResourcesGuard",
        ] {
            let marker = format!("NATIVE_QA_COMPOSE_BRIDGE_FAILED:{class}\n").into_bytes();
            inputs.extend((0..marker.len()).map(|length| marker[..length].to_vec()));
            inputs.extend([
                [marker.as_slice(), marker.as_slice()].concat(),
                [b"private-prefix".as_slice(), marker.as_slice()].concat(),
                [marker.as_slice(), b"private-suffix".as_slice()].concat(),
            ]);
        }
        for stderr in inputs {
            let error = child_outcome(false, true, &stderr).unwrap_err();
            assert_eq!(child_error_class(&error), Some("unknown"));
            assert_eq!(rejection_document(&error)["childErrorClass"], "unknown");
            assert_eq!(rejection_status(&error), "unknown_or_rejected");
            assert!(!format!("{error:?}").contains("private"));
            assert!(!rejection_document(&error).to_string().contains("private"));
        }
    }
}
