//! Compose-owned job execution and file transfer. Journals never mount into jobs.
use serde_json::{Value, json};
use std::{
    io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use uuid::Uuid;

const IMAGE_BUILD_INSPECT_FORMAT: &str = r#"{"id":{{json .Id}},"revision":{{json (index .Config.Labels "org.opencontainers.image.revision")}},"attempt":{{json (index .Config.Labels "sdlc.attempt")}},"request":{{json (index .Config.Labels "forge.image-build-request")}},"os":{{json .Os}},"volumes":{{json (index .Config "Volumes")}}}"#;
const RUNTIME_IMAGE_FORMAT: &str = r#"{"image_id":{{json .Id}},"os":{{json .Os}},"architecture":{{json .Architecture}},"default_user":{{json (index .Config "User")}},"volumes":{{json (index .Config "Volumes")}}}"#;

#[derive(Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct RuntimeImageFacts {
    image_id: String,
    os: String,
    architecture: String,
    default_user: Option<String>,
    volumes: std::collections::BTreeSet<String>,
}

fn architecture(value: &str) -> Option<&'static str> {
    match value {
        "amd64" | "x86_64" => Some("amd64"),
        "arm64" | "aarch64" => Some("arm64"),
        _ => None,
    }
}

fn runtime_image_facts(
    value: &Value,
    image: &str,
    platform: &str,
) -> io::Result<RuntimeImageFacts> {
    let platform =
        architecture(platform).ok_or_else(|| invalid("unsupported dependency platform"))?;
    if !crate::domain::image_build::valid_image_id(image)
        || value["image_id"] != image
        || value["os"] != "linux"
        || value["architecture"].as_str().and_then(architecture) != Some(platform)
    {
        return Err(invalid("runtime image identity/platform mismatch"));
    }
    let user = match &value["default_user"] {
        Value::Null => None,
        Value::String(user) if user.len() <= 128 && !user.chars().any(char::is_control) => {
            Some(user.clone())
        }
        _ => return Err(invalid("invalid image Config.User")),
    };
    let mut volumes = std::collections::BTreeSet::new();
    if !value["volumes"].is_null() {
        let declared = value["volumes"]
            .as_object()
            .ok_or_else(|| invalid("invalid image Config.Volumes"))?;
        if declared.len() > 8 {
            return Err(invalid("image volume bound"));
        }
        for (path, options) in declared {
            if path.len() > 256
                || !path.starts_with('/')
                || path == "/"
                || path.contains(['\\', ':', '$'])
                || path.chars().any(char::is_control)
                || !path[1..]
                    .split('/')
                    .all(|part| !part.is_empty() && part != "." && part != "..")
                || !options.as_object().is_some_and(|v| v.is_empty())
            {
                return Err(invalid("invalid image volume declaration"));
            }
            volumes.insert(path.clone());
        }
    }
    Ok(RuntimeImageFacts {
        image_id: image.into(),
        os: "linux".into(),
        architecture: platform.into(),
        default_user: user,
        volumes,
    })
}

async fn bounded_image_read(mut command: Command, args: &[&str]) -> io::Result<String> {
    let mut child = command
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        let mut bytes = Vec::new();
        child
            .stdout
            .take()
            .ok_or_else(|| invalid("missing Docker readback"))?
            .take(65537)
            .read_to_end(&mut bytes)
            .await?;
        if bytes.len() > 65536 || !child.wait().await?.success() {
            return Err(invalid("bounded Docker image-build readback failed"));
        }
        String::from_utf8(bytes)
            .map(|v| v.trim().to_owned())
            .map_err(|_| invalid("invalid Docker image-build readback encoding"))
    })
    .await;
    match result {
        Ok(Ok(value)) => Ok(value),
        _ => {
            let _ = child.kill().await;
            Err(invalid("Docker image-build readback unavailable"))
        }
    }
}

#[derive(Clone)]
pub(crate) struct Client {
    host: String,
    cert_path: String,
    daemon_id: String,
    cgroup_parent: Option<String>,
}

pub(crate) struct ImageBuildCommand<'a> {
    pub spec: &'a crate::domain::image_build::ImageBuildSpec,
    pub context: &'a Path,
    pub dockerfile: &'a Path,
    pub iid_file: &'a Path,
    pub docker_config: &'a Path,
    pub source_commit: &'a str,
    pub attempt: Uuid,
    pub request_sha256: &'a str,
}

fn image_build_inspection_matches(
    value: &Value,
    image: &str,
    source_commit: &str,
    attempt: Uuid,
    request_sha256: &str,
) -> bool {
    value["id"] == image
        && value["revision"] == source_commit
        && value["attempt"] == attempt.to_string()
        && value["request"] == request_sha256
        && value["os"] == "linux"
        && (value["volumes"].is_null()
            || value["volumes"].as_object().is_some_and(|v| v.is_empty()))
}

fn image_build_builder_matches(value: &Value, host: &str, default_endpoint: &str) -> bool {
    default_endpoint == host
        && value["Name"] == "default"
        && value["Driver"] == "docker"
        && value.get("Err").is_none()
        && value["Nodes"].as_array().is_some_and(|nodes| {
            nodes.len() == 1
                && nodes[0]["Endpoint"] == "default"
                && nodes[0]["Status"] == "running"
                && nodes[0].get("Err").is_none()
        })
}

fn image_build_version_matches(value: &str) -> bool {
    let mut fields = value.split_whitespace();
    fields.next() == Some("github.com/docker/buildx") && fields.next() == Some("v0.37.2")
}
// The operator owns the host budget; jobs cannot choose another cgroup.
fn parse_cgroup_parent(value: Option<&str>) -> io::Result<Option<String>> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let stem = value.strip_suffix(".slice").unwrap_or_default();
    if stem.is_empty()
        || value.len() > 128
        || !stem.as_bytes()[0].is_ascii_alphanumeric()
        || !stem
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
    {
        return Err(invalid(
            "runner cgroup parent must be a named systemd slice",
        ));
    }
    Ok(Some(value.to_owned()))
}

fn apply_cgroup_parent(manifest: &mut Value, parent: Option<&str>) -> io::Result<()> {
    let Some(parent) = parent else { return Ok(()) };
    let services = manifest["services"]
        .as_object_mut()
        .ok_or_else(|| invalid("job services missing"))?;
    for service in services.values_mut() {
        if service
            .get("cgroup_parent")
            .is_some_and(|value| value != parent)
        {
            return Err(invalid("job cgroup differs from operator budget"));
        }
        service["cgroup_parent"] = json!(parent);
    }
    Ok(())
}

fn cgroup_parent_matches(spec: &Value, host: &Value) -> bool {
    match spec.get("cgroup_parent") {
        Some(Value::String(parent)) => host["CgroupParent"].as_str() == Some(parent),
        None => true,
        _ => false,
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::other(message)
}

/// Retry admission reads existing recovery evidence; it never stops or cleans.
pub(crate) async fn retry_unconfirmed(job: Uuid, attempt: Uuid) -> io::Result<bool> {
    let root = PathBuf::from(
        std::env::var("CICD_RUNNER_WORKSPACE_ROOT").unwrap_or_else(|_| "/workspaces".into()),
    )
    .join(".compose-control");
    retry_unconfirmed_at(
        &root,
        job,
        attempt,
        &std::env::var("CICD_RUNNER_DOCKER_DAEMON_ID").unwrap_or_default(),
        &std::env::var("CICD_RUNNER_WORKSPACE_VOLUME").unwrap_or_default(),
    )
    .await
}

async fn retry_unconfirmed_at(
    root: &Path,
    job: Uuid,
    attempt: Uuid,
    daemon: &str,
    control_volume: &str,
) -> io::Result<bool> {
    if crate::runner_image_build::retry_unconfirmed(root, job, attempt)
        .map_err(|_| invalid("image build recovery custody is unavailable"))?
    {
        return Ok(true);
    }
    let project = format!("sdlc-build-job-{}", attempt.simple());
    for path in [root.to_path_buf(), root.join(&project)] {
        match tokio::fs::symlink_metadata(&path).await {
            // Legacy/shell and preparation failures before any Docker effect
            // have no Compose journal. Existing directories must be complete.
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(invalid("recovery directory ownership is unavailable"));
            }
            Ok(_) => {}
        }
    }
    let directory = root.join(&project);
    let manifest_path = directory.join("compose.json");
    let journal = recovery_json(&directory.join("journal.json"), 64 * 1024).await?;
    let manifest = recovery_json(&manifest_path, 1024 * 1024).await?;
    let labels = &manifest["services"]["transfer"]["labels"];
    if daemon.is_empty()
        || control_volume.is_empty()
        || journal["project"] != project
        || journal["daemon_id"] != daemon
        || journal["manifest"].as_str() != manifest_path.to_str()
        || manifest["name"] != project
        || labels["sdlc.job"] != job.to_string()
        || labels["sdlc.attempt"] != attempt.to_string()
        || labels["sdlc.task"] != format!("job-{job}")
        || labels["sdlc.purpose"] != "cicd-job"
        || labels["sdlc.control-volume"] != control_volume
    {
        return Err(invalid("recovery journal scope differs from retry"));
    }
    if journal["phase"] == "cleaned" {
        if journal["stopped"] != true {
            return Err(invalid("cleaned recovery journal has no stop confirmation"));
        }
        return Ok(false);
    }
    // Owner death and a terminal DB projection do not prove Engine termination.
    // Only the existing owner reconciliation may publish the cleaned journal.
    Ok(true)
}

async fn recovery_json(path: &Path, limit: u64) -> io::Result<Value> {
    let metadata = tokio::fs::symlink_metadata(path).await?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > limit {
        return Err(invalid("recovery evidence is not a bounded regular file"));
    }
    let mut bytes = Vec::new();
    tokio::fs::File::open(path)
        .await?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() as u64 > limit {
        return Err(invalid("recovery evidence exceeds its limit"));
    }
    serde_json::from_slice(&bytes).map_err(|_| invalid("invalid recovery evidence"))
}

fn source_provenance_matches(volume: &Value, revision: &str, workspace: &str) -> bool {
    revision.len() == 40
        && revision
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && matches!(
            workspace,
            "sdlc1" | "sdlc2" | "pdlc1" | "pdlc2" | "pdlc3" | "pdlc4"
        )
        && volume[0]["Labels"]["sdlc.base-revision"] == revision
        && volume[0]["Labels"]["sdlc.workspace"] == workspace
}

impl Client {
    /// Shared authenticated task actions use the already-configured backend Engine authority.
    /// Legacy callers keep their existing paths; this never selects a host/socket fallback.
    pub(crate) async fn configured_verified() -> io::Result<Self> {
        let client = Self::from_env()?;
        client.verify().await?;
        Ok(client)
    }

    /// Existing backend authority only; the caller cannot supply an endpoint or credential.
    pub(crate) async fn for_image_build(docker_config: &Path) -> io::Result<Self> {
        let client = Self::from_env()?;
        client.verify_image_build(docker_config).await?;
        Ok(client)
    }

    fn image_build_control_command(&self, docker_config: &Path) -> Command {
        let mut command = Command::new("docker");
        // No inherited job secrets, Buildx remote builder, contexts or credential helpers.
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("DOCKER_CONFIG", docker_config)
            .kill_on_drop(true);
        #[cfg(windows)]
        if let Some(root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", root);
        }
        self.apply(&mut command);
        command
    }

    async fn image_build_read(&self, docker_config: &Path, args: &[&str]) -> io::Result<String> {
        bounded_image_read(self.image_build_control_command(docker_config), args).await
    }

    /// An immutable selector is not proof: resolve through the existing owner inventory,
    /// then inspect actual Config on the same verified daemon. Never pull dependencies.
    pub(crate) async fn runtime_image(&self, image: &str) -> io::Result<RuntimeImageFacts> {
        if !crate::domain::image_build::valid_image_id(image) {
            return Err(invalid("runtime dependency requires an immutable image ID"));
        }
        let format =
            r#"{"id":{{json .ID}},"os":{{json .OSType}},"architecture":{{json .Architecture}}}"#;
        let before: Value = serde_json::from_str(
            &bounded_image_read(self.command(), &["info", "--format", format]).await?,
        )?;
        if before["id"] != self.daemon_id
            || before["os"] != "linux"
            || self.image(image).await? != image
        {
            return Err(invalid("dependency owner inventory differs"));
        }
        let value: Value = serde_json::from_str(
            &bounded_image_read(
                self.command(),
                &["image", "inspect", image, "--format", RUNTIME_IMAGE_FORMAT],
            )
            .await?,
        )?;
        let facts = runtime_image_facts(
            &value,
            image,
            before["architecture"]
                .as_str()
                .ok_or_else(|| invalid("daemon platform missing"))?,
        )?;
        let after: Value = serde_json::from_str(
            &bounded_image_read(self.command(), &["info", "--format", format]).await?,
        )?;
        if after != before {
            return Err(invalid("dependency daemon changed during readback"));
        }
        Ok(facts)
    }

    async fn verify_image_build_daemon(&self, docker_config: &Path) -> io::Result<()> {
        if self
            .image_build_read(docker_config, &["info", "--format", "{{.ID}}"])
            .await?
            != self.daemon_id
        {
            return Err(invalid(
                "image build daemon differs from registered workspace",
            ));
        }
        Ok(())
    }

    pub(crate) async fn verify_image_build(&self, docker_config: &Path) -> io::Result<()> {
        self.verify_image_build_daemon(docker_config).await?;
        if !image_build_version_matches(
            &self
                .image_build_read(docker_config, &["buildx", "version"])
                .await?,
        ) {
            return Err(invalid("verified Buildx v0.37.2 is required"));
        }
        let default_endpoint: String = serde_json::from_str(
            &self
                .image_build_read(
                    docker_config,
                    &[
                        "context",
                        "inspect",
                        "default",
                        "--format",
                        "{{json .Endpoints.docker.Host}}",
                    ],
                )
                .await?,
        )?;
        // v0.37.2 inspect has no --format. ls emits one JSON object per builder.
        let listing = self
            .image_build_read(docker_config, &["buildx", "ls", "--format", "json"])
            .await?;
        let builders = serde_json::Deserializer::from_str(&listing)
            .into_iter::<Value>()
            .collect::<Result<Vec<_>, _>>()?;
        if builders.len() != 1
            || !image_build_builder_matches(&builders[0], &self.host, &default_endpoint)
        {
            return Err(invalid(
                "image build requires the existing same-daemon Docker builder",
            ));
        }
        Ok(())
    }

    pub(crate) fn image_build_command(&self, build: ImageBuildCommand<'_>) -> io::Result<Command> {
        use crate::domain::image_build::{ImageBuildSpec, valid_commit};
        let ImageBuildSpec { target, .. } = build.spec;
        if build.spec.validate().is_err()
            || !valid_commit(build.source_commit)
            || build.attempt.is_nil()
            || build.request_sha256.len() != 64
            || !build
                .request_sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(invalid("invalid typed image build command"));
        }
        let mut command = self.image_build_control_command(build.docker_config);
        command
            .args([
                "buildx",
                "build",
                "--builder",
                "default",
                "--load",
                "--network",
                "default",
                "--provenance=false",
                "--sbom=false",
                "--progress",
                "plain",
                "--iidfile",
            ])
            .arg(build.iid_file)
            .arg("--file")
            .arg(build.dockerfile)
            .arg("--label")
            .arg(format!(
                "org.opencontainers.image.revision={}",
                build.source_commit
            ))
            .arg("--label")
            .arg(format!("sdlc.attempt={}", build.attempt))
            .arg("--label")
            .arg(format!(
                "forge.image-build-request={}",
                build.request_sha256
            ));
        if let Some(target) = target {
            command.arg("--target").arg(target);
        }
        command
            .arg(build.context)
            .stdin(std::process::Stdio::null());
        Ok(command)
    }

    pub(crate) async fn inspect_built_image(
        &self,
        docker_config: &Path,
        image: &str,
        source_commit: &str,
        attempt: Uuid,
        request_sha256: &str,
    ) -> io::Result<()> {
        if !crate::domain::image_build::valid_image_id(image) {
            return Err(invalid("build did not return an immutable image ID"));
        }
        self.verify_image_build_daemon(docker_config).await?;
        let value: Value = serde_json::from_str(
            &self
                .image_build_read(
                    docker_config,
                    &[
                        "image",
                        "inspect",
                        image,
                        "--format",
                        IMAGE_BUILD_INSPECT_FORMAT,
                    ],
                )
                .await?,
        )?;
        if !image_build_inspection_matches(&value, image, source_commit, attempt, request_sha256) {
            return Err(invalid(
                "built image differs from exact source/attempt or OCI requirements",
            ));
        }
        Ok(())
    }

    pub(crate) fn image_build_daemon_id(&self) -> &str {
        &self.daemon_id
    }

    fn from_env() -> io::Result<Self> {
        let host = std::env::var("DOCKER_HOST").unwrap_or_default();
        let cert_path = std::env::var("DOCKER_CERT_PATH").unwrap_or_default();
        let daemon_id = std::env::var("CICD_RUNNER_DOCKER_DAEMON_ID").unwrap_or_default();
        if daemon_id.is_empty() || !(host.starts_with("unix://") || host.starts_with("tcp://")) {
            return Err(invalid(
                "explicit Docker endpoint and expected daemon ID are required",
            ));
        }
        if host.starts_with("tcp://")
            && (std::env::var("DOCKER_TLS_VERIFY").as_deref() != Ok("1")
                || ["ca.pem", "cert.pem", "key.pem"]
                    .iter()
                    .any(|n| !Path::new(&cert_path).join(n).is_file()))
        {
            return Err(invalid(
                "TCP runner requires existing TLS certificates; fallback is forbidden",
            ));
        }
        let cgroup_parent = match std::env::var("CICD_RUNNER_CGROUP_PARENT") {
            Ok(value) => parse_cgroup_parent(Some(&value))?,
            Err(std::env::VarError::NotPresent) => None,
            Err(_) => return Err(invalid("invalid runner cgroup parent encoding")),
        };
        Ok(Self {
            host,
            cert_path,
            daemon_id,
            cgroup_parent,
        })
    }
    pub(crate) fn apply(&self, command: &mut Command) {
        command
            .env_remove("DOCKER_CONTEXT")
            .env("DOCKER_HOST", &self.host);
        if self.host.starts_with("tcp://") {
            command
                .env("DOCKER_CERT_PATH", &self.cert_path)
                .env("DOCKER_TLS_VERIFY", "1");
        } else {
            command
                .env_remove("DOCKER_CERT_PATH")
                .env_remove("DOCKER_TLS_VERIFY");
        }
    }
    fn command(&self) -> Command {
        let mut c = Command::new("docker");
        self.apply(&mut c);
        c.kill_on_drop(true);
        c
    }
    async fn run(&self, args: &[&str]) -> io::Result<String> {
        let result =
            tokio::time::timeout(Duration::from_secs(180), self.command().args(args).output())
                .await
                .map_err(|_| {
                    invalid("Docker operation timed out; outcome requires reconciliation")
                })??;
        if !result.status.success() {
            return Err(invalid(
                "Docker operation failed; protected journal retained",
            ));
        }
        Ok(String::from_utf8_lossy(&result.stdout).trim().to_string())
    }
    async fn verify(&self) -> io::Result<()> {
        if self.run(&["info", "--format", "{{.ID}}"]).await? != self.daemon_id {
            return Err(invalid("Docker daemon differs from registered workspace"));
        }
        if self
            .run(&["compose", "version", "--short"])
            .await?
            .trim_start_matches('v')
            != "5.5.1"
        {
            return Err(invalid("verified Compose plugin 5.5.1 is required"));
        }
        Ok(())
    }
    async fn image(&self, image: &str) -> io::Result<String> {
        if let Ok(id) = self
            .run(&["image", "inspect", image, "--format", "{{.Id}}"])
            .await
        {
            return Ok(id);
        }
        // Pinned private/local images must already exist on the selected daemon.
        if image.starts_with("sha256:") {
            return Err(invalid("pinned job image is absent on registered daemon"));
        }
        self.run(&["pull", image]).await?;
        self.run(&["image", "inspect", image, "--format", "{{.Id}}"])
            .await
    }
}

pub(crate) async fn stop_job(job_id: Uuid) -> io::Result<()> {
    let client = Client::from_env()?;
    client.verify().await?;
    let inspected: Value = serde_json::from_str(
        &client
            .run(&["inspect", &format!("forge-job-{job_id}")])
            .await?,
    )?;
    let container = &inspected[0];
    let labels = &container["Config"]["Labels"];
    let attempt = labels["sdlc.attempt"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| invalid("job container has no valid attempt owner"))?;
    if labels["sdlc.job"] != job_id.to_string()
        || labels["com.docker.compose.project"] != format!("sdlc-build-job-{}", attempt.simple())
        || labels["com.docker.compose.service"] != "job"
        || labels["sdlc.purpose"] != "cicd-job"
    {
        return Err(invalid("refusing to stop a foreign job resource"));
    }
    client
        .run(&[
            "stop",
            "-t",
            "2",
            container["Id"]
                .as_str()
                .ok_or_else(|| invalid("container ID missing"))?,
        ])
        .await?;
    Ok(())
}

pub(crate) struct RemoteJob {
    client: Client,
    helper: String,
    project: String,
    directory: PathBuf,
    manifest: Value,
    active: bool,
    stopped: bool,
    writes: Arc<PrivateWrites>,
    deployment_output: Option<PathBuf>,
    deployment_witness: Option<(Value, bool)>,
}

#[derive(Default)]
struct PrivateWrites {
    requested: AtomicU64,
    closed: Mutex<bool>,
}

impl PrivateWrites {
    fn write(&self, path: &Path, value: &Value, sequence: u64) -> io::Result<()> {
        let closed = self
            .closed
            .lock()
            .map_err(|_| invalid("private journal lock poisoned"))?;
        if *closed || self.requested.load(Ordering::SeqCst) != sequence {
            return Err(invalid("private journal write superseded or owner dropped"));
        }
        write_private(path, value)
    }
}

fn literal(value: &mut Value) {
    match value {
        Value::String(text) => *text = text.replace('$', "$$"),
        Value::Array(items) => items.iter_mut().for_each(literal),
        Value::Object(items) => items.values_mut().for_each(literal),
        _ => (),
    }
}
fn write_private(path: &Path, value: &Value) -> io::Result<()> {
    use std::io::Write;
    let temp = path.with_extension("tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp)?;
    file.write_all(serde_json::to_string_pretty(value)?.as_bytes())?;
    file.sync_all()?;
    std::fs::rename(temp, path)
}

async fn write_private_async(
    path: PathBuf,
    value: Value,
    writes: Arc<PrivateWrites>,
) -> io::Result<()> {
    let sequence = writes
        .requested
        .fetch_add(1, Ordering::SeqCst)
        .wrapping_add(1);
    tokio::task::spawn_blocking(move || writes.write(&path, &value, sequence))
        .await
        .map_err(|_| invalid("private journal write worker failed"))?
}

fn process_owner() -> String {
    let pid = std::process::id();
    format!("{pid}:{}", process_start(pid).unwrap_or_default())
}

fn process_start(pid: u32) -> Option<String> {
    // PID plus Linux process start ticks distinguishes a reused PID after a container restart.
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(") ")?
        .1
        .split_whitespace()
        .nth(19)
        .map(str::to_owned)
}

fn owner_alive(owner: &str) -> bool {
    let Some((pid, start)) = owner.split_once(':') else {
        return false;
    };
    pid.parse::<u32>().ok().and_then(process_start).as_deref() == Some(start)
}

fn resource_owned(
    kind: &str,
    resource: &Value,
    manifest: &Value,
    project: &str,
    file: &Path,
) -> bool {
    let expected = &manifest["services"]["transfer"]["labels"];
    let Some(attempt) = expected["sdlc.attempt"]
        .as_str()
        .and_then(|v| Uuid::parse_str(v).ok())
    else {
        return false;
    };
    let Some(job) = expected["sdlc.job"]
        .as_str()
        .and_then(|v| Uuid::parse_str(v).ok())
    else {
        return false;
    };
    if project != format!("sdlc-build-job-{}", attempt.simple())
        || expected["sdlc.task"] != format!("job-{job}")
        || expected["sdlc.purpose"] != "cicd-job"
    {
        return false;
    }
    let labels = if kind == "container" {
        &resource["Config"]["Labels"]
    } else {
        &resource["Labels"]
    };
    if labels["com.docker.compose.project"] != project
        || [
            "sdlc.task",
            "sdlc.purpose",
            "sdlc.job",
            "sdlc.attempt",
            "sdlc.control-volume",
        ]
        .iter()
        .any(|key| labels[*key] != expected[*key])
    {
        return false;
    }
    if kind == "container" {
        let Some(service) = labels["com.docker.compose.service"].as_str() else {
            return false;
        };
        return manifest["services"][service].is_object()
            && labels["com.docker.compose.project.config_files"].as_str() == file.to_str()
            && resource["Name"].as_str().is_some_and(|name| {
                manifest["services"][service]["container_name"]
                    .as_str()
                    .is_some_and(|expected| name.trim_start_matches('/') == expected)
            });
    }
    let collection = match kind {
        "volume" => "volumes",
        "network" => "networks",
        _ => return false,
    };
    let Some(logical) = labels[format!("com.docker.compose.{kind}")].as_str() else {
        return false;
    };
    let definition = &manifest[collection][logical];
    definition.is_object()
        && definition["external"] != true
        && resource["Name"] == format!("{project}_{logical}")
}

fn storage_users_owned(users: &[&str], containers: &[Value]) -> bool {
    users.len() <= 16
        && users.iter().all(|id| {
            id.len() == 64
                && id.bytes().all(|byte| byte.is_ascii_hexdigit())
                && containers
                    .iter()
                    .any(|container| container["Id"].as_str() == Some(*id))
        })
}

/// Ownership inspection precedes this selection. Only the transfer helper is exempt.
fn execution_containers<'a>(
    manifest: &Value,
    containers: &'a [Value],
) -> io::Result<Vec<&'a Value>> {
    let services = manifest["services"]
        .as_object()
        .ok_or_else(|| invalid("services missing"))?;
    if !services.get("transfer").is_some_and(Value::is_object) {
        return Err(invalid("transfer service missing"));
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut execution = Vec::new();
    for container in containers {
        let service = container["Config"]["Labels"]["com.docker.compose.service"]
            .as_str()
            .ok_or_else(|| invalid("container service missing"))?;
        if !services.get(service).is_some_and(Value::is_object) || !seen.insert(service) {
            return Err(invalid("unknown or duplicate execution service"));
        }
        if service != "transfer" {
            let id = container["Id"]
                .as_str()
                .ok_or_else(|| invalid("container ID missing"))?;
            if id.len() != 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(invalid("invalid execution container ID"));
            }
            execution.push(container);
        }
    }
    // Missing containers are allowed after partial creation/removal; no absent service is replayed.
    Ok(execution)
}

fn execution_stopped(manifest: &Value, containers: &[Value]) -> io::Result<()> {
    for container in execution_containers(manifest, containers)? {
        if container["State"]["Running"].as_bool() != Some(false)
            || container["State"]["Paused"].as_bool() != Some(false)
            || container["State"]["Restarting"].as_bool() != Some(false)
        {
            return Err(invalid("execution service termination is unconfirmed"));
        }
    }
    Ok(())
}

const VOLUME_INITIALIZE: &str = r#"test "$#" -eq 2
test -d "$1" && test ! -L "$1"
test -z "$(find "$1" -mindepth 1 -maxdepth 1 -print -quit)"
chown "$2" "$1"
chmod 700 "$1"
test "$(stat -c '%u:%g:%a' "$1")" = "$2:700"
"#;

fn merge_deployment(
    manifest: &mut Value,
    fragment: &Value,
    users: &std::collections::BTreeMap<String, String>,
) -> io::Result<()> {
    let project = manifest["name"]
        .as_str()
        .ok_or_else(|| invalid("project missing"))?
        .to_owned();
    let labels = manifest["services"]["transfer"]["labels"].clone();
    if fragment["name"] != project
        || fragment["networks"]["job"]["name"] != format!("{project}_job")
        || fragment["networks"]["job"]["internal"] != false
        || fragment["networks"]["job"]["labels"] != labels
        || fragment["networks"]["job"]
            .as_object()
            .is_none_or(|v| v.len() != 3)
    {
        return Err(invalid(
            "deployment cannot replace the existing owner network",
        ));
    }
    let services = fragment["services"]
        .as_object()
        .ok_or_else(|| invalid("services missing"))?;
    let volumes = fragment["volumes"]
        .as_object()
        .ok_or_else(|| invalid("volumes missing"))?;
    if services.is_empty()
        || services.len() > 8
        || volumes.len() > 8
        || volumes.len() != users.len()
    {
        return Err(invalid("deployment resource bound"));
    }
    for (name, service) in services {
        if name == "job"
            || name == "transfer"
            || service["labels"] != labels
            || service["container_name"] != format!("{project}-{name}")
            || !service["image"]
                .as_str()
                .is_some_and(crate::domain::image_build::valid_image_id)
            || service.get("x-forge-image-build").is_some()
        {
            return Err(invalid("unsealed deployment service"));
        }
        manifest["services"][name] = service.clone();
    }
    for (name, volume) in volumes {
        let user = users
            .get(name)
            .ok_or_else(|| invalid("volume user missing"))?;
        if ["workspace", "cargo", "sources"].contains(&name.as_str())
            || volume["labels"] != labels
            || volume["name"] != format!("{project}_{name}")
            || !user.split_once(':').is_some_and(|(uid, gid)| {
                [uid, gid].into_iter().all(|id| {
                    id.parse::<u32>()
                        .is_ok_and(|n| n > 0 && n <= i32::MAX as u32)
                })
            })
        {
            return Err(invalid("unsealed disposable volume"));
        }
        manifest["volumes"][name] = volume.clone();
        manifest["services"]["transfer"]["volumes"].as_array_mut().ok_or_else(||invalid("helper mounts missing"))?
            .push(json!({"type":"volume","source":name,"target":format!("/forge-data/{name}"),"volume":{"nocopy":true}}));
    }
    Ok(())
}

fn mounts_match(manifest: &Value, service: &str, container: &Value) -> bool {
    let expected = manifest["services"][service]["volumes"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let Some(actual) = container["Mounts"].as_array() else {
        return false;
    };
    let tmpfs: Vec<_> = manifest["services"][service]["tmpfs"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .filter_map(|v| {
            v.as_str()
                .and_then(|v| v.split_once(':'))
                .map(|(path, _)| path)
        })
        .collect();
    actual.iter().all(|mount| {
        mount["Type"] == "volume"
            || (mount["Type"] == "tmpfs"
                && mount["Destination"]
                    .as_str()
                    .is_some_and(|path| tmpfs.contains(&path)))
    }) && expected.len()
        == actual
            .iter()
            .filter(|mount| mount["Type"] == "volume")
            .count()
        && expected.iter().all(|mount| {
            let Some(logical) = mount["source"].as_str() else {
                return false;
            };
            let name = manifest["volumes"][logical]["name"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    format!(
                        "{}_{}",
                        manifest["name"].as_str().unwrap_or_default(),
                        logical
                    )
                });
            actual
                .iter()
                .filter(|observed| {
                    observed["Type"] == "volume"
                        && observed["Name"] == name
                        && observed["Destination"] == mount["target"]
                        && observed["RW"] == !mount["read_only"].as_bool().unwrap_or(false)
                })
                .count()
                == 1
        })
}

#[derive(Clone, Copy)]
enum DeploymentState {
    Created,
    Running,
    Stopped,
    BeforeStop,
}

fn deployment_containers(
    manifest: &Value,
    containers: &[Value],
    state: DeploymentState,
) -> io::Result<Value> {
    let mut result = serde_json::Map::new();
    let services = manifest["services"]
        .as_object()
        .ok_or_else(|| invalid("services missing"))?;
    for (name, spec) in services
        .iter()
        .filter(|(name, _)| !matches!(name.as_str(), "job" | "transfer"))
    {
        let found: Vec<_> = containers
            .iter()
            .filter(|c| c["Config"]["Labels"]["com.docker.compose.service"] == *name)
            .collect();
        if found.len() != 1 {
            return Err(invalid("exact deployment service readback missing"));
        }
        let container = found[0];
        let running = if matches!(state, DeploymentState::BeforeStop) {
            container["State"]["Running"]
                .as_bool()
                .ok_or_else(|| invalid("running state missing"))?
        } else {
            matches!(state, DeploymentState::Running)
        };
        let host = &container["HostConfig"];
        if container["Image"] != spec["image"]
            || !container["Id"]
                .as_str()
                .is_some_and(|id| id.len() == 64 && id.bytes().all(|c| c.is_ascii_hexdigit()))
            || container["Config"]["User"] != spec["user"]
            || container["State"]["Running"] != running
            || container["State"]["Paused"] != false
            || container["State"]["Restarting"] != false
            || container["State"]["Dead"] != false
            || !container["State"]["Pid"]
                .as_i64()
                .is_some_and(|pid| if running { pid > 0 } else { pid == 0 })
            || (running && container["State"]["Status"] != "running")
            || (running
                && !container["State"]["StartedAt"]
                    .as_str()
                    .is_some_and(|start| !start.is_empty() && !start.starts_with("0001-")))
            || (matches!(state, DeploymentState::Created)
                && container["State"]["Status"] != "created")
            || host["Privileged"] != false
            || host["PidMode"] != ""
            || !host["IpcMode"]
                .as_str()
                .is_some_and(|mode| matches!(mode, "" | "private"))
            || host["RestartPolicy"]["Name"] != "no"
            || !host["SecurityOpt"].as_array().is_some_and(|opts| {
                opts.len() == 1
                    && matches!(
                        opts[0].as_str(),
                        Some("no-new-privileges:true" | "no-new-privileges")
                    )
            })
            || host["ReadonlyRootfs"] != true
            || host["PublishAllPorts"] != false
            || host["NetworkMode"]
                != format!("{}_job", manifest["name"].as_str().unwrap_or_default())
            || !cgroup_parent_matches(spec, host)
            || host["Memory"] != spec["mem_limit"]
            || host["PidsLimit"] != spec["pids_limit"]
            || host["NanoCpus"].as_i64()
                != spec["cpus"]
                    .as_f64()
                    .map(|cpu| (cpu * 1_000_000_000.0).round() as i64)
            || ![
                "Binds",
                "Devices",
                "DeviceRequests",
                "ExtraHosts",
                "PortBindings",
                "CapAdd",
            ]
            .iter()
            .all(|key| {
                host[*key].is_null()
                    || host[*key].as_array().is_some_and(|v| v.is_empty())
                    || host[*key].as_object().is_some_and(|v| v.is_empty())
            })
            || host["CapDrop"] != json!(["ALL"])
            || !mounts_match(manifest, name, container)
        {
            return Err(invalid(
                "actual deployment container differs from sealed owner manifest",
            ));
        }
        result.insert(name.clone(),json!({"id":container["Id"],"image":container["Image"],
            "state":container["State"]["Status"],"started_at":container["State"]["StartedAt"],
            "pid":container["State"]["Pid"],"mounts":container["Mounts"].as_array().unwrap().iter().map(|mount|
                json!({"name":mount["Name"],"destination":mount["Destination"],"rw":mount["RW"]})).collect::<Vec<_>>()}));
    }
    if result.is_empty() {
        return Err(invalid("deployment service set empty"));
    }
    Ok(result.into())
}

impl RemoteJob {
    pub(crate) async fn prepare(id: Uuid, attempt: Uuid, workspace: &Path) -> io::Result<Self> {
        Self::prepare_inner(id, attempt, workspace, None).await
    }

    pub(crate) async fn prepare_deployment(
        id: Uuid,
        attempt: Uuid,
        workspace: &Path,
        deployment: &crate::runner_image_build::workspace_deploy::PreparedDeployment,
        pool: &sqlx::PgPool,
        config: &crate::runner::RuntimeRunnerConfig,
    ) -> io::Result<Self> {
        let until = deployment
            .effect_deadline(pool, config)
            .await
            .map_err(|_| invalid("task preparation authority unavailable"))?;
        tokio::time::timeout_at(
            tokio::time::Instant::from_std(until),
            Self::prepare_inner(id, attempt, workspace, Some(deployment)),
        )
        .await
        .map_err(|_| invalid("deployment preparation outcome unknown; original journal retained"))?
    }

    async fn prepare_inner(
        id: Uuid,
        attempt: Uuid,
        workspace: &Path,
        deployment: Option<&crate::runner_image_build::workspace_deploy::PreparedDeployment>,
    ) -> io::Result<Self> {
        let client = Client::from_env()?;
        client.verify().await?;
        let root = PathBuf::from(
            std::env::var("CICD_RUNNER_WORKSPACE_ROOT").unwrap_or_else(|_| "/workspaces".into()),
        )
        .join(".compose-control");
        std::fs::create_dir_all(&root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
        }
        let project = format!("sdlc-build-job-{}", attempt.simple());
        let directory = root.join(&project);
        // A persisted invocation is never replayed, even if no container is currently visible.
        std::fs::create_dir(&directory)
            .map_err(|_| invalid("attempt journal exists; reconcile instead of reexecuting"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
        }
        let helper = format!("forge-transfer-{attempt}");
        let labels = json!({"sdlc.task": format!("job-{id}"), "sdlc.purpose":"cicd-job", "sdlc.job":id.to_string(), "sdlc.attempt":attempt.to_string(),
            "sdlc.control-volume":std::env::var("CICD_RUNNER_WORKSPACE_VOLUME").unwrap_or_default()});
        let helper_image = client.image("alpine:3.21").await?;
        let mut manifest = json!({"name":project, "services":{"transfer":{
            "image":helper_image, "container_name":helper, "pull_policy":"never", "restart":"no",
            "entrypoint":["/bin/sh"], "command":["-c","sleep 86400"], "network_mode":"none",
            "read_only":true, "tmpfs":["/tmp:mode=700"], "security_opt":["no-new-privileges:true"],
            "labels":labels, "volumes":[{"type":"volume","source":"workspace","target":"/workspace"},{"type":"volume","source":"cargo","target":"/cache"}]
        }}, "volumes":{"workspace":{"labels":labels}, "cargo":{"labels":labels}}, "networks":{"job":{"labels":labels}}});
        let volume_users = if let Some(deployment) = deployment {
            let (fragment, users) = deployment
                .services_for(id, attempt)
                .map_err(|_| invalid("deployment consumer differs"))?;
            merge_deployment(&mut manifest, fragment, users)?;
            users.clone()
        } else {
            std::collections::BTreeMap::new()
        };
        apply_cgroup_parent(&mut manifest, client.cgroup_parent.as_deref())?;
        let job = Self {
            client,
            helper,
            project,
            directory,
            manifest,
            active: true,
            stopped: false,
            writes: Arc::default(),
            deployment_output: None,
            deployment_witness: None,
        };
        job.record("prepared").await?;
        let prepared = async {
            if deployment.is_some() {
                let names = bounded_image_read(
                    job.client.command(),
                    &["volume", "ls", "--format", "{{.Name}}"],
                )
                .await?;
                let planned: Vec<_> = job.manifest["volumes"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .map(|name| format!("{}_{}", job.project, name))
                    .collect();
                if names
                    .lines()
                    .any(|name| planned.iter().any(|planned| planned == name))
                {
                    return Err(invalid(
                        "disposable deployment volume already exists; no adoption permitted",
                    ));
                }
            }
            job.save_manifest().await?;
            job.compose(&["up", "-d", "--no-build", "--pull", "never", "transfer"])
                .await?;
            let parent = workspace
                .parent()
                .ok_or_else(|| invalid("job checkout parent missing"))?;
            job.client
                .run(&[
                    "cp",
                    &format!("{}/.", parent.display()),
                    &format!("{}:/workspace", job.helper),
                ])
                .await?;
            job.initialize_data_volumes(&volume_users).await?;
            job.client
                .run(&[
                    "exec",
                    &job.helper,
                    "chown",
                    "-R",
                    "10001:10001",
                    "/workspace",
                    "/cache",
                ])
                .await?;
            Ok::<_, io::Error>(())
        }
        .await;
        prepared?;
        Ok(job)
    }

    async fn initialize_data_volumes(
        &self,
        users: &std::collections::BTreeMap<String, String>,
    ) -> io::Result<()> {
        if users.is_empty() {
            return Ok(());
        }
        let containers = self.owned_resources().await?;
        let helper = containers
            .iter()
            .find(|item| item["Name"] == format!("/{}", self.helper))
            .ok_or_else(|| invalid("trusted transfer helper missing"))?;
        if helper["Image"] != self.manifest["services"]["transfer"]["image"]
            || !mounts_match(&self.manifest, "transfer", helper)
        {
            return Err(invalid("trusted helper image/mount readback differs"));
        }
        for (volume, user) in users {
            let inspected: Value = serde_json::from_str(
                &self
                    .client
                    .run(&["volume", "inspect", &format!("{}_{}", self.project, volume)])
                    .await?,
            )?;
            let actual = inspected
                .as_array()
                .filter(|v| v.len() == 1)
                .and_then(|v| v.first())
                .ok_or_else(|| invalid("data volume readback missing"))?;
            if !resource_owned(
                "volume",
                actual,
                &self.manifest,
                &self.project,
                &self.directory.join("compose.json"),
            ) {
                return Err(invalid("foreign data volume blocks initialization"));
            }
            self.client
                .run(&[
                    "exec",
                    &self.helper,
                    "sh",
                    "-ec",
                    VOLUME_INITIALIZE,
                    "forge-volume-init",
                    &format!("/forge-data/{volume}"),
                    user,
                ])
                .await?;
        }
        Ok(())
    }

    pub(crate) async fn start_deployment(
        &mut self,
        deployment: &crate::runner_image_build::workspace_deploy::PreparedDeployment,
        pool: &sqlx::PgPool,
        config: &crate::runner::RuntimeRunnerConfig,
    ) -> io::Result<()> {
        let (fragment, _) = deployment
            .services_for(self.job_id()?, self.attempt_id()?)
            .map_err(|_| invalid("deployment consumer differs"))?;
        let names: Vec<&str> = fragment["services"]
            .as_object()
            .ok_or_else(|| invalid("services missing"))?
            .keys()
            .map(String::as_str)
            .collect();
        let manifest = self.manifest_bytes().await?;
        deployment
            .record_started(&self.directory, manifest)
            .await
            .map_err(|_| invalid("original deployment intent unavailable"))?;
        for action in ["create", "start"] {
            let until = deployment
                .effect_deadline(pool, config)
                .await
                .map_err(|_| invalid("task start authority unavailable"))?;
            let containers = self.owned_resources().await?;
            self.verify_deployment_custody(&containers, DeploymentState::BeforeStop)?;
            let mut arguments = if action == "create" {
                vec![
                    "up",
                    "--no-start",
                    "--no-deps",
                    "--no-build",
                    "--pull",
                    "never",
                ]
            } else {
                vec!["start"]
            };
            arguments.extend(names.iter().copied());
            tokio::time::timeout_at(
                tokio::time::Instant::from_std(until),
                self.compose(&arguments),
            )
            .await
            .map_err(|_| {
                invalid("deployment action outcome unknown; original journal retained")
            })??;
            let observation = self
                .observe_deployment(if action == "create" {
                    DeploymentState::Created
                } else {
                    DeploymentState::Running
                })
                .await?;
            deployment
                .record_observation(
                    &self.directory,
                    if action == "create" {
                        "deployment-created.json"
                    } else {
                        "deployment-running.json"
                    },
                    observation.clone(),
                )
                .await
                .map_err(|_| invalid("original deployment observation cannot be recorded"))?;
            self.deployment_witness = Some((observation, action == "start"));
        }
        Ok(())
    }

    pub(crate) async fn complete_deployment(
        &mut self,
        deployment: &crate::runner_image_build::workspace_deploy::PreparedDeployment,
        status: std::process::ExitStatus,
    ) -> io::Result<()> {
        deployment
            .services_for(self.job_id()?, self.attempt_id()?)
            .map_err(|_| invalid("deployment completion consumer differs"))?;
        self.stop_execution().await?;
        let observed = self.observe_deployment(DeploymentState::Stopped).await?;
        let output = deployment
            .record_validation(
                &self.directory,
                self.manifest_bytes().await?,
                observed,
                status,
            )
            .await
            .map_err(|_| invalid("historical deployment receipt unavailable"))?;
        self.deployment_output = Some(output);
        Ok(())
    }

    pub(crate) fn deployment_artifacts(&self) -> Option<&Path> {
        self.deployment_output.as_deref()
    }

    fn job_id(&self) -> io::Result<Uuid> {
        self.manifest["services"]["transfer"]["labels"]["sdlc.job"]
            .as_str()
            .and_then(|id| Uuid::parse_str(id).ok())
            .ok_or_else(|| invalid("job identity missing"))
    }
    fn attempt_id(&self) -> io::Result<Uuid> {
        self.manifest["services"]["transfer"]["labels"]["sdlc.attempt"]
            .as_str()
            .and_then(|id| Uuid::parse_str(id).ok())
            .ok_or_else(|| invalid("attempt identity missing"))
    }
    async fn manifest_bytes(&self) -> io::Result<Vec<u8>> {
        let path = self.directory.join("compose.json");
        tokio::task::spawn_blocking(move || std::fs::read(path))
            .await
            .map_err(|_| invalid("manifest read failed"))?
    }

    async fn observe_deployment(&self, state: DeploymentState) -> io::Result<Value> {
        let containers = self.owned_resources().await?;
        let observed = deployment_containers(&self.manifest, &containers, state)?;
        let network: Value = serde_json::from_str(
            &self
                .client
                .run(&["network", "inspect", &format!("{}_job", self.project)])
                .await?,
        )?;
        let network = network
            .as_array()
            .filter(|v| v.len() == 1)
            .and_then(|v| v.first())
            .ok_or_else(|| invalid("deployment network readback missing"))?;
        if !resource_owned(
            "network",
            network,
            &self.manifest,
            &self.project,
            &self.directory.join("compose.json"),
        ) || network["Driver"] != "bridge"
            || !network["Id"]
                .as_str()
                .is_some_and(|id| id.len() == 64 && id.bytes().all(|c| c.is_ascii_hexdigit()))
            || network["Internal"] != false
            || network["IPAM"]["Config"]
                .as_array()
                .is_none_or(|v| v.is_empty())
        {
            return Err(invalid("owned deployment network differs"));
        }
        Ok(
            json!({"containers":observed,"network":{"id":network["Id"],"name":network["Name"],
            "ipam":network["IPAM"]["Config"]}}),
        )
    }
    fn journal(&self, phase: &str) -> Value {
        json!({"project":self.project,
            "daemon_id":self.client.daemon_id, "phase":phase, "owner":process_owner(), "stopped":self.stopped, "manifest":self.directory.join("compose.json")})
    }
    async fn record(&self, phase: &str) -> io::Result<()> {
        write_private_async(
            self.directory.join("journal.json"),
            self.journal(phase),
            Arc::clone(&self.writes),
        )
        .await
    }
    async fn save_manifest(&self) -> io::Result<()> {
        let mut value = self.manifest.clone();
        literal(&mut value);
        write_private_async(
            self.directory.join("compose.json"),
            value,
            Arc::clone(&self.writes),
        )
        .await
    }
    async fn compose(&self, tail: &[&str]) -> io::Result<String> {
        let file = self.directory.join("compose.json");
        let mut args = vec![
            "compose",
            "-p",
            &self.project,
            "-f",
            file.to_str()
                .ok_or_else(|| invalid("invalid manifest path"))?,
        ];
        args.extend_from_slice(tail);
        self.client.run(&args).await
    }
    pub(crate) async fn job_command(
        &mut self,
        args: &[String],
        envs: &[(String, String)],
    ) -> io::Result<Command> {
        let source = std::env::var("CICD_RUNNER_SHARED_SOURCES_VOLUME").unwrap_or_default();
        if source.is_empty() {
            return Err(invalid("immutable source volume is required"));
        }
        let volume: Value =
            serde_json::from_str(&self.client.run(&["volume", "inspect", &source]).await?)?;
        let revision = std::env::var("CICD_RUNNER_BASE_REVISION").unwrap_or_default();
        let workspace = std::env::var("CICD_RUNNER_WORKSPACE_PROJECT").unwrap_or_default();
        if !source_provenance_matches(&volume, &revision, &workspace) {
            return Err(invalid(
                "source volume provenance differs from registered Base revision/workspace",
            ));
        }
        let mut service = compose_job(args, envs)?;
        service["image"] = json!(
            self.client
                .image(service["image"].as_str().unwrap())
                .await?
        );
        service["labels"] = self.manifest["services"]["transfer"]["labels"].clone();
        if let Some(parent) = &self.client.cgroup_parent {
            service["cgroup_parent"] = json!(parent);
        }
        self.manifest["services"]["job"] = service;
        self.manifest["volumes"]["sources"] = json!({"external":true,"name":source});
        self.save_manifest().await?;
        self.compose(&["config", "--quiet"]).await?;
        self.record("execution-started").await?;
        let mut cmd = self.client.command();
        cmd.args([
            "compose",
            "-p",
            &self.project,
            "-f",
            self.directory.join("compose.json").to_str().unwrap(),
            "run",
            "--no-deps",
            "-T",
            "--name",
            self.manifest["services"]["job"]["container_name"]
                .as_str()
                .unwrap(),
            "job",
        ]);
        Ok(cmd)
    }
    pub(crate) async fn copy_back(&self, workspace: &Path) -> io::Result<()> {
        self.client
            .run(&[
                "cp",
                &format!("{}:/workspace/workspace/.", self.helper),
                &workspace.to_string_lossy(),
            ])
            .await?;
        Ok(())
    }
    pub(crate) async fn stage_artifacts(&self, directory: &Path) -> io::Result<()> {
        self.client
            .run(&[
                "exec",
                &self.helper,
                "mkdir",
                "-p",
                "/workspace/artifacts",
                "/workspace/pipeline-artifacts",
            ])
            .await?;
        self.client
            .run(&[
                "cp",
                &format!("{}/.", directory.display()),
                &format!("{}:/workspace/pipeline-artifacts", self.helper),
            ])
            .await?;
        self.client
            .run(&[
                "exec",
                &self.helper,
                "chown",
                "-R",
                "10001:10001",
                "/workspace",
                "/cache",
            ])
            .await?;
        Ok(())
    }
    pub(crate) async fn return_artifacts(&self, job: &Path, pipeline: &Path) -> io::Result<()> {
        for (name, path) in [("artifacts", job), ("pipeline-artifacts", pipeline)] {
            self.client
                .run(&[
                    "cp",
                    &format!("{}:/workspace/{name}/.", self.helper),
                    &path.to_string_lossy(),
                ])
                .await?;
        }
        Ok(())
    }
    async fn owned_resources(&self) -> io::Result<Vec<Value>> {
        self.client.verify().await?;
        let mut containers = Vec::new();
        for kind in ["container", "volume", "network"] {
            let names = self
                .client
                .run(&[
                    kind,
                    "ls",
                    if kind == "container" { "-aq" } else { "-q" },
                    "--filter",
                    &format!("label=com.docker.compose.project={}", self.project),
                ])
                .await?;
            let names: Vec<&str> = names.lines().collect();
            if names.len() > 16 {
                return Err(invalid("unexpected resource inventory; cleanup blocked"));
            }
            for name in names {
                let inspected: Value =
                    serde_json::from_str(&self.client.run(&[kind, "inspect", name]).await?)?;
                let resource = inspected
                    .as_array()
                    .and_then(|items| items.first())
                    .ok_or_else(|| invalid("resource ownership response missing"))?;
                if !resource_owned(
                    kind,
                    resource,
                    &self.manifest,
                    &self.project,
                    &self.directory.join("compose.json"),
                ) {
                    return Err(invalid("foreign resource blocks execution and cleanup"));
                }
                if kind == "container" {
                    containers.push(resource.clone());
                } else {
                    let users = if kind == "volume" {
                        self.client
                            .run(&[
                                "container",
                                "ls",
                                "-aq",
                                "--no-trunc",
                                "--filter",
                                &format!("volume={name}"),
                            ])
                            .await?
                    } else {
                        let members = resource["Containers"]
                            .as_object()
                            .ok_or_else(|| invalid("network membership response missing"))?;
                        members.keys().cloned().collect::<Vec<_>>().join("\n")
                    };
                    if !storage_users_owned(&users.lines().collect::<Vec<_>>(), &containers) {
                        return Err(invalid("foreign storage user blocks execution and cleanup"));
                    }
                }
            }
        }
        Ok(containers)
    }

    pub(crate) async fn confirm_stopped(&mut self) -> io::Result<()> {
        self.stopped = false;
        let containers = self.owned_resources().await?;
        self.verify_deployment_custody(&containers, DeploymentState::Stopped)?;
        execution_stopped(&self.manifest, &containers)?;
        if let Some((witness, started)) = &self.deployment_witness {
            let current = self.observe_deployment(DeploymentState::Stopped).await?;
            crate::runner_image_build::workspace_deploy::verify_observation(
                witness, &current, *started,
            )
            .map_err(|_| invalid("deployment identity changed before cleanup"))?;
        }
        self.stopped = true;
        self.record("execution-stopped").await
    }

    pub(crate) async fn stop_execution(&mut self) -> io::Result<()> {
        self.stopped = false;
        let containers = self.owned_resources().await?;
        self.verify_deployment_custody(&containers, DeploymentState::BeforeStop)?;
        for container in execution_containers(&self.manifest, &containers)? {
            let id = container["Id"]
                .as_str()
                .ok_or_else(|| invalid("container ID missing"))?;
            self.client.run(&["stop", "-t", "2", id]).await?;
        }
        self.confirm_stopped().await
    }

    fn verify_deployment_custody(
        &self,
        containers: &[Value],
        state: DeploymentState,
    ) -> io::Result<()> {
        if let Some((witness, started)) = &self.deployment_witness {
            let current =
                json!({"containers":deployment_containers(&self.manifest,containers,state)?});
            crate::runner_image_build::workspace_deploy::verify_container_custody(
                witness, &current, *started,
            )
            .map_err(|_| invalid("deployment container replaced; original journal retained"))?;
        } else if containers.iter().any(|c| {
            c["Config"]["Labels"]["com.docker.compose.service"]
                .as_str()
                .is_some_and(|s| !matches!(s, "job" | "transfer"))
        }) {
            return Err(invalid(
                "deployment has no original create observation; hold",
            ));
        }
        Ok(())
    }

    pub(crate) async fn cleanup(&mut self, pool: &sqlx::PgPool) -> io::Result<()> {
        self.confirm_stopped().await?;
        let labels = &self.manifest["services"]["transfer"]["labels"];
        let attempt = labels["sdlc.attempt"]
            .as_str()
            .and_then(|v| Uuid::parse_str(v).ok())
            .ok_or_else(|| invalid("attempt owner missing"))?;
        let job = labels["sdlc.job"]
            .as_str()
            .and_then(|v| Uuid::parse_str(v).ok())
            .ok_or_else(|| invalid("job owner missing"))?;
        let acknowledged: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM job_leases l JOIN execution_attempts a ON a.id=l.attempt_id \
             WHERE a.id=$1 AND a.job_id=$2 AND l.job_id=$2 AND l.completed_at IS NOT NULL \
             AND l.lease_status IN ('completed','canceled') AND a.finished_at IS NOT NULL \
             AND a.status=l.terminal_status AND l.terminal_status IN ('success','failed','canceled'))")
            .bind(attempt).bind(job).fetch_one(pool).await
            .map_err(|_| invalid("terminal receipt readback failed; resources retained"))?;
        if !acknowledged {
            return Err(invalid("terminal receipt missing; resources retained"));
        }
        self.confirm_stopped().await?;
        self.compose(&["down", "--remove-orphans", "--volumes"])
            .await?;
        if !self.owned_resources().await?.is_empty() {
            return Err(invalid("Compose containers remain after cleanup"));
        }
        for kind in ["volume", "network"] {
            if !self
                .client
                .run(&[
                    kind,
                    "ls",
                    "-q",
                    "--filter",
                    &format!("label=com.docker.compose.project={}", self.project),
                ])
                .await?
                .is_empty()
            {
                return Err(invalid("Compose storage remains after cleanup"));
            }
        }
        self.record("cleaned").await?;
        self.active = false;
        // Erase per-job environment only after successful cleanup. Keep safe recovery journal.
        for service in self.manifest["services"]
            .as_object_mut()
            .unwrap()
            .values_mut()
        {
            service.as_object_mut().unwrap().remove("environment");
        }
        self.save_manifest().await?;
        Ok(())
    }
}
impl Drop for RemoteJob {
    fn drop(&mut self) {
        if self.active {
            // No detached write can overwrite this marker after Drop. Keep the original
            // synchronous failure fallback; normal lifecycle writes are awaited/offloaded.
            if let Ok(mut closed) = self.writes.closed.lock() {
                *closed = true;
                let _ = write_private(
                    &self.directory.join("journal.json"),
                    &self.journal("cleanup-required"),
                );
            }
            // Unwinding is not termination or completion authority. Recovery must
            // read fresh Engine state and the terminal lease before deletion.
        }
    }
}

/// Reconcile persisted attempts before dispatch. Never replay an uncertain execution.
pub(crate) async fn reconcile(pool: &sqlx::PgPool) -> Result<(), sqlx::Error> {
    let root = PathBuf::from(
        std::env::var("CICD_RUNNER_WORKSPACE_ROOT").unwrap_or_else(|_| "/workspaces".into()),
    )
    .join(".compose-control");
    if !root.is_dir() {
        return Ok(());
    }
    let entries = std::fs::read_dir(&root).map_err(sqlx::Error::Io)?;
    for entry in entries {
        let directory = entry.map_err(sqlx::Error::Io)?.path();
        if !directory.is_dir() || directory.is_symlink() {
            continue;
        }
        let journal_path = directory.join("journal.json");
        if !journal_path.is_file() {
            continue;
        }
        let journal: Value =
            serde_json::from_slice(&std::fs::read(&journal_path).map_err(sqlx::Error::Io)?)
                .map_err(|_| sqlx::Error::Io(invalid("invalid protected Compose journal")))?;
        if journal["phase"] == "cleaned" {
            continue;
        }
        if journal["owner"].as_str().is_some_and(owner_alive) {
            if journal["phase"] != "cleanup-required" {
                continue;
            }
            if journal["stopped"] != true {
                return Err(sqlx::Error::Io(invalid(
                    "live owner has unconfirmed execution; dispatch held",
                )));
            }
        }
        let manifest_path = directory.join("compose.json");
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(&manifest_path).map_err(sqlx::Error::Io)?)
                .map_err(|_| sqlx::Error::Io(invalid("invalid protected Compose manifest")))?;
        let labels = &manifest["services"]["transfer"]["labels"];
        let attempt = labels["sdlc.attempt"]
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or_else(|| sqlx::Error::Io(invalid("journal attempt missing")))?;
        let id = labels["sdlc.job"]
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or_else(|| sqlx::Error::Io(invalid("journal job missing")))?;
        let project = format!("sdlc-build-job-{}", attempt.simple());
        let client = Client::from_env().map_err(sqlx::Error::Io)?;
        client.verify().await.map_err(sqlx::Error::Io)?;
        if journal["daemon_id"] != client.daemon_id
            || journal["project"] != project
            || manifest["name"] != project
            || directory.file_name().and_then(|n| n.to_str()) != Some(project.as_str())
        {
            return Err(sqlx::Error::Io(invalid(
                "journal endpoint/project ownership differs",
            )));
        }
        let deployment_witness =
            crate::runner_image_build::workspace_deploy::read_observation(&directory)
                .await
                .map_err(|_| {
                    sqlx::Error::Io(invalid("deployment observation unavailable; dispatch held"))
                })?;
        let mut job = RemoteJob {
            helper: manifest["services"]["transfer"]["container_name"]
                .as_str()
                .unwrap_or_default()
                .into(),
            client,
            project,
            directory,
            manifest,
            active: true,
            stopped: false,
            writes: Arc::default(),
            deployment_output: None,
            deployment_witness,
        };
        // Fresh Engine termination precedes releasing the project's lease.
        job.stop_execution().await.map_err(sqlx::Error::Io)?;
        // Mark only the old attempt terminal; an explicit newer retry is untouched.
        let mut tx = pool.begin().await?;
        sqlx::query("UPDATE execution_attempts SET status='failed', finished_at=COALESCE(finished_at,now()), error_tail=COALESCE(error_tail,'runner interrupted; execution outcome uncertain; explicit retry required') WHERE id=$1 AND job_id=$2 AND status IN ('queued','running')")
            .bind(attempt).bind(id).execute(&mut *tx).await?;
        let stage: Option<Uuid> = sqlx::query_scalar("UPDATE jobs SET status='failed', finished_at=COALESCE(finished_at,now()) WHERE id=$1 AND status IN ('queued','running') AND EXISTS (SELECT 1 FROM job_queue WHERE job_id=$1 AND attempt_id=$2) RETURNING stage_id")
            .bind(id).bind(attempt).fetch_optional(&mut *tx).await?;
        sqlx::query("UPDATE job_leases SET lease_status='completed', terminal_status='failed', completed_at=COALESCE(completed_at,now()), error_tail=COALESCE(error_tail,'runner interrupted; explicit retry required') WHERE attempt_id=$1 AND job_id=$2 AND lease_status='active'")
            .bind(attempt).bind(id).execute(&mut *tx).await?;
        sqlx::query("UPDATE job_queue SET state='completed', completed_at=COALESCE(completed_at,now()), updated_at=now() WHERE attempt_id=$1 AND state IN ('queued','leased')")
            .bind(attempt).execute(&mut *tx).await?;
        tx.commit().await?;
        if let Some(stage) = stage {
            crate::api::refresh_statuses(pool, stage)
                .await
                .map_err(|e| {
                    sqlx::Error::Io(invalid(&format!("status reconciliation failed: {e:?}")))
                })?;
        }
        job.cleanup(pool).await.map_err(sqlx::Error::Io)?;
        tracing::warn!(job_id=%id, attempt_id=%attempt, "interrupted Compose attempt reconciled without replay");
    }
    Ok(())
}

/// The old argument builder is a declarative adapter only; never passed to Docker.
fn compose_job(args: &[String], envs: &[(String, String)]) -> io::Result<Value> {
    let mut service = json!({"pull_policy":"never", "restart":"no", "networks":["job"], "volumes":[
        {"type":"volume","source":"workspace","target":"/workspace"},
        {"type":"volume","source":"cargo","target":"/usr/local/cargo/registry"},
        {"type":"volume","source":"sources","target":"/runner-sources","read_only":true}]});
    let mut index = 1;
    while index < args.len() && args[index].starts_with('-') {
        let flag = args[index].as_str();
        index += 1;
        if flag == "--rm" {
            continue;
        }
        if flag == "--read-only" {
            service["read_only"] = json!(true);
            continue;
        }
        let value = args
            .get(index)
            .ok_or_else(|| invalid("incomplete job adapter option"))?;
        index += 1;
        match flag {
            "--name" => service["container_name"] = json!(value),
            "--network" | "--mount" => (),
            "--cap-drop" => service["cap_drop"] = json!([value]),
            "--security-opt" => {
                if service.get("security_opt").is_none() {
                    service["security_opt"] = json!([]);
                }
                service["security_opt"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!(value));
            }
            "--tmpfs" => service["tmpfs"] = json!([value]),
            "--memory" => service["mem_limit"] = json!(value),
            "--memory-swap" => service["memswap_limit"] = json!(value),
            "--pids-limit" => {
                service["pids_limit"] = json!(
                    value
                        .parse::<u32>()
                        .map_err(|_| invalid("invalid pids limit"))?
                )
            }
            "--workdir" => service["working_dir"] = json!(value),
            "--user" => service["user"] = json!(value),
            _ => return Err(invalid("unsupported declarative job adapter option")),
        }
    }
    service["image"] = json!(
        args.get(index)
            .ok_or_else(|| invalid("job image missing"))?
    );
    service["entrypoint"] = json!(args[index + 1..].to_vec());
    let environment: serde_json::Map<String, Value> = envs
        .iter()
        .filter(|(k, _)| !k.starts_with("DOCKER_") && !k.starts_with("CICD_RUNNER_"))
        .map(|(k, v)| (k.clone(), json!(v)))
        .collect();
    service["environment"] = Value::Object(environment);
    Ok(service)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_budget_parent_is_optional_and_accepts_independent_workspaces() {
        assert_eq!(parse_cgroup_parent(None).unwrap(), None);
        assert_eq!(parse_cgroup_parent(Some("")).unwrap(), None);
        for name in ["pdlc4.slice", "another-workspace.slice"] {
            assert_eq!(
                parse_cgroup_parent(Some(name)).unwrap().as_deref(),
                Some(name)
            );
        }
        for name in [
            "../pdlc4.slice",
            "pdlc4",
            ".slice",
            "x/y.slice",
            "pdlc4.slice;id",
        ] {
            assert!(parse_cgroup_parent(Some(name)).is_err());
        }
    }

    #[test]
    fn host_budget_covers_transfer_job_and_deployment_without_changing_legacy() {
        let original = json!({"services":{"transfer":{},"job":{},"database":{}}});
        let mut legacy = original.clone();
        apply_cgroup_parent(&mut legacy, None).unwrap();
        assert_eq!(legacy, original);
        let mut budgeted = original;
        apply_cgroup_parent(&mut budgeted, Some("pdlc4.slice")).unwrap();
        for service in budgeted["services"].as_object().unwrap().values() {
            assert_eq!(service["cgroup_parent"], "pdlc4.slice");
        }
        budgeted["services"]["job"]["cgroup_parent"] = json!("foreign.slice");
        assert!(apply_cgroup_parent(&mut budgeted, Some("pdlc4.slice")).is_err());
    }

    #[test]
    fn host_budget_readback_rejects_escape_to_another_parent() {
        let spec = json!({"cgroup_parent":"pdlc4.slice"});
        assert!(cgroup_parent_matches(
            &spec,
            &json!({"CgroupParent":"pdlc4.slice"})
        ));
        assert!(!cgroup_parent_matches(
            &spec,
            &json!({"CgroupParent":"foreign.slice"})
        ));
        assert!(!cgroup_parent_matches(&spec, &json!({})));
        assert!(cgroup_parent_matches(
            &json!({}),
            &json!({"CgroupParent":"host-default.slice"})
        ));
    }

    fn deployment_fixture() -> (Value, Value, Vec<Value>) {
        let labels = json!({"sdlc.task":"fixture"});
        let project = "sdlc-build-job-fixture";
        let service = json!({"image":format!("sha256:{}","a".repeat(64)),"user":"999:999",
            "mem_limit":16777216,"cpus":0.5,"pids_limit":16,"labels":labels,
            "container_name":format!("{project}-db"),"volumes":[{"type":"volume","source":"data","target":"/data"}]});
        let fragment = json!({"name":project,"services":{"db":service},"volumes":{"data":{"name":format!("{project}_data"),"labels":labels}},
            "networks":{"job":{"name":format!("{project}_job"),"internal":false,"labels":labels}}});
        let mut manifest = json!({"name":project,"services":{"transfer":{"labels":labels,"volumes":[]}},
            "volumes":{},"networks":{"job":{"labels":labels}}});
        merge_deployment(
            &mut manifest,
            &fragment,
            &std::collections::BTreeMap::from([("data".into(), "999:999".into())]),
        )
        .unwrap();
        let container = json!({"Id":"b".repeat(64),"Image":service["image"],
            "Config":{"User":"999:999","Labels":{"com.docker.compose.service":"db"}},
            "State":{"Running":true,"Paused":false,"Restarting":false,"Dead":false,"Pid":42,"Status":"running","StartedAt":"2026-10-17T01:02:03Z"},
            "HostConfig":{"Privileged":false,"PidMode":"","IpcMode":"private","RestartPolicy":{"Name":"no"},
                "SecurityOpt":["no-new-privileges:true"],"ReadonlyRootfs":true,"PublishAllPorts":false,"NetworkMode":format!("{project}_job"),
                "Memory":16777216,"PidsLimit":16,"NanoCpus":500000000,"CapDrop":["ALL"]},
            "Mounts":[{"Type":"volume","Name":format!("{project}_data"),"Destination":"/data","RW":true}]});
        (manifest, fragment, vec![container])
    }

    #[test]
    fn deployment_stop_requires_original_container_and_start_epoch_not_matching_labels() {
        let (manifest, _, containers) = deployment_fixture();
        let observed = json!({"containers":deployment_containers(&manifest,&containers,DeploymentState::Running).unwrap()});
        let mut job = RemoteJob {
            client: Client {
                host: "not-contacted".into(),
                cert_path: "not-opened".into(),
                daemon_id: "fixture-daemon".into(),
                cgroup_parent: None,
            },
            helper: String::new(),
            project: "fixture".into(),
            directory: PathBuf::new(),
            manifest,
            active: false,
            stopped: false,
            writes: Arc::default(),
            deployment_output: None,
            deployment_witness: None,
        };
        assert!(
            job.verify_deployment_custody(&containers, DeploymentState::BeforeStop)
                .is_err(),
            "matching service labels cannot invent the original create ACK"
        );
        job.deployment_witness = Some((observed.clone(), true));
        job.verify_deployment_custody(&containers, DeploymentState::BeforeStop)
            .unwrap();
        let mut replaced = containers.clone();
        replaced[0]["Id"] = json!("c".repeat(64));
        assert!(
            job.verify_deployment_custody(&replaced, DeploymentState::BeforeStop)
                .is_err()
        );
        let mut restarted = containers.clone();
        restarted[0]["State"]["StartedAt"] = json!("2026-10-17T01:02:04Z");
        assert!(
            job.verify_deployment_custody(&restarted, DeploymentState::BeforeStop)
                .is_err()
        );
        // An unknown start ACK permits stopping only the exact previously Created ID;
        // it does not manufacture a Running observation or a validation receipt.
        job.deployment_witness = Some((observed, false));
        job.verify_deployment_custody(&restarted, DeploymentState::BeforeStop)
            .unwrap();
        assert!(
            job.verify_deployment_custody(&replaced, DeploymentState::BeforeStop)
                .is_err()
        );
        assert!(job.deployment_artifacts().is_none());
        assert!(!job.stopped);
    }

    #[test]
    fn deployment_merge_preserves_existing_network_and_rejects_control_resource_shadowing() {
        let (manifest, fragment, _) = deployment_fixture();
        assert!(manifest["networks"]["job"].get("ipam").is_none());
        assert_eq!(
            manifest["services"]["transfer"]["volumes"][0]["target"],
            "/forge-data/data"
        );
        let users = std::collections::BTreeMap::from([("data".into(), "999:999".into())]);
        for key in ["name", "internal", "labels", "ipam"] {
            let mut wrong = fragment.clone();
            wrong["networks"]["job"][key] = json!("foreign");
            assert!(merge_deployment(&mut manifest.clone(), &wrong, &users).is_err());
        }
        let mut wrong = fragment.clone();
        wrong["services"]["job"] = wrong["services"]["db"].clone();
        assert!(merge_deployment(&mut manifest.clone(), &wrong, &users).is_err());
        assert!(
            merge_deployment(
                &mut manifest.clone(),
                &fragment,
                &std::collections::BTreeMap::from([("data".into(), "0:0".into())])
            )
            .is_err()
        );
        assert!(VOLUME_INITIALIZE.contains("-mindepth 1 -maxdepth 1"));
        assert!(VOLUME_INITIALIZE.contains("chown \"$2\" \"$1\""));
        assert!(!VOLUME_INITIALIZE.contains("chown -R"));
    }

    #[test]
    fn deployment_readback_rejects_actual_privilege_mount_image_and_start_drift() {
        let (manifest, _, containers) = deployment_fixture();
        deployment_containers(&manifest, &containers, DeploymentState::Running).unwrap();
        for (key, value) in [
            ("Privileged", json!(true)),
            ("ReadonlyRootfs", json!(false)),
            ("PublishAllPorts", json!(true)),
            ("NetworkMode", json!("host")),
            ("PidMode", json!("host")),
            ("Memory", json!(0)),
            ("CapAdd", json!(["SYS_ADMIN"])),
            ("ExtraHosts", json!(["host:gateway"])),
        ] {
            let mut wrong = containers.clone();
            wrong[0]["HostConfig"][key] = value;
            assert!(
                deployment_containers(&manifest, &wrong, DeploymentState::Running).is_err(),
                "{key}"
            );
        }
        for (key, value) in [
            ("Id", json!("short")),
            ("Image", json!("tag")),
            (
                "Mounts",
                json!([{"Type":"bind","Destination":"/data","RW":true}]),
            ),
        ] {
            let mut wrong = containers.clone();
            wrong[0][key] = value;
            assert!(
                deployment_containers(&manifest, &wrong, DeploymentState::Running).is_err(),
                "{key}"
            );
        }
        for (key, value) in [
            ("Pid", json!(0)),
            ("StartedAt", json!("0001-01-01T00:00:00Z")),
            ("Running", json!(false)),
        ] {
            let mut wrong = containers.clone();
            wrong[0]["State"][key] = value;
            assert!(
                deployment_containers(&manifest, &wrong, DeploymentState::Running).is_err(),
                "{key}"
            );
        }
        let mut stopped = containers.clone();
        stopped[0]["State"]["Running"] = json!(false);
        stopped[0]["State"]["Pid"] = json!(0);
        stopped[0]["State"]["Status"] = json!("exited");
        deployment_containers(&manifest, &stopped, DeploymentState::BeforeStop).unwrap();
        deployment_containers(&manifest, &stopped, DeploymentState::Stopped).unwrap();
        assert!(deployment_containers(&manifest, &containers, DeploymentState::Stopped).is_err());
    }

    #[test]
    fn dependency_inventory_preserves_actual_user_and_implicit_volumes_without_build_labels() {
        let id = format!("sha256:{}", "a".repeat(64));
        let value = json!({"image_id":id,"os":"linux","architecture":"amd64",
            "default_user":"999:999","volumes":{"/var/lib/database":{}}});
        let facts = runtime_image_facts(&value, &id, "x86_64").unwrap();
        let projection = serde_json::to_value(&facts).unwrap();
        assert_eq!(projection["image_id"], id);
        assert_eq!(projection["default_user"], "999:999");
        assert_eq!(projection["volumes"], json!(["/var/lib/database"]));
        assert!(projection.get("revision").is_none());
        assert!(projection.get("attempt").is_none());
    }

    #[test]
    fn dependency_inventory_docker29_optional_user_and_volumes_do_not_invent_a_uid() {
        let id = format!("sha256:{}", "a".repeat(64));
        let value = json!({"image_id":id,"os":"linux","architecture":"arm64"});
        let facts = runtime_image_facts(&value, &id, "aarch64").unwrap();
        let projection = serde_json::to_value(&facts).unwrap();
        assert_eq!(projection["default_user"], Value::Null);
        assert_eq!(projection["volumes"], json!([]));
        assert!(RUNTIME_IMAGE_FORMAT.contains(r#"index .Config "Volumes""#));
        assert!(RUNTIME_IMAGE_FORMAT.contains(r#"index .Config "User""#));
    }

    #[test]
    fn dependency_inventory_rejects_mutable_ids_platform_drift_and_malformed_config() {
        let id = format!("sha256:{}", "a".repeat(64));
        let value = json!({"image_id":id,"os":"linux","architecture":"amd64",
            "default_user":"","volumes":null});
        for (key, changed) in [
            ("image_id", json!("tag:latest")),
            ("os", json!("windows")),
            ("architecture", json!("arm64")),
            ("default_user", json!({"verified":true})),
            ("volumes", json!(["/data"])),
        ] {
            let mut wrong = value.clone();
            wrong[key] = changed;
            assert!(runtime_image_facts(&wrong, &id, "amd64").is_err());
        }
        assert!(runtime_image_facts(&value, "tag:latest", "amd64").is_err());
        assert!(runtime_image_facts(&value, &id, "unknown").is_err());
        for path in ["/", "relative", "/data/../host", "/data//x", "/data\\x"] {
            let mut wrong = value.clone();
            wrong["volumes"] = json!({(path):{}});
            assert!(runtime_image_facts(&wrong, &id, "amd64").is_err());
        }
        let facts = runtime_image_facts(&value, &id, "amd64").unwrap();
        assert_eq!(serde_json::to_value(facts).unwrap()["default_user"], "");
    }

    fn execution_fixture() -> (Value, Vec<Value>) {
        let manifest = json!({"services":{"transfer":{},"job":{},"application":{},"database":{}}});
        let containers = ["transfer", "job", "application", "database"]
            .iter()
            .enumerate()
            .map(|(index, service)| {
                json!({"Id":format!("{index:064x}"),
                "Config":{"Labels":{"com.docker.compose.service":service}},
                "State":{"Running":*service=="transfer","Paused":false,"Restarting":false}})
            })
            .collect();
        (manifest, containers)
    }

    #[test]
    fn termination_covers_all_application_services_but_not_transfer() {
        let (manifest, containers) = execution_fixture();
        assert_eq!(
            execution_containers(&manifest, &containers).unwrap().len(),
            3
        );
        execution_stopped(&manifest, &containers).unwrap();
        for index in 1..containers.len() {
            for field in ["Running", "Paused", "Restarting"] {
                let mut changed = containers.clone();
                changed[index]["State"][field] = json!(true);
                assert!(
                    execution_stopped(&manifest, &changed).is_err(),
                    "{index} {field}"
                );
                changed[index]["State"]
                    .as_object_mut()
                    .unwrap()
                    .remove(field);
                assert!(execution_stopped(&manifest, &changed).is_err());
            }
        }
    }

    #[test]
    fn partial_creation_is_not_replayed_and_unknown_inventory_is_not_ignored() {
        let (manifest, mut containers) = execution_fixture();
        execution_stopped(&manifest, &[]).unwrap();
        execution_stopped(&manifest, &containers[..1]).unwrap();
        containers[3]["Config"]["Labels"]["com.docker.compose.service"] = json!("foreign");
        assert!(execution_containers(&manifest, &containers).is_err());
        containers[3]["Config"]["Labels"]["com.docker.compose.service"] = json!("application");
        assert!(execution_containers(&manifest, &containers).is_err());
        let (manifest, mut containers) = execution_fixture();
        containers[2]["Id"] = json!("short");
        assert!(execution_containers(&manifest, &containers).is_err());
    }

    #[tokio::test]
    async fn private_journal_async_write_is_awaited_and_preserves_mode() {
        let directory =
            std::env::temp_dir().join(format!("forge-private-write-{}", Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("journal.json");
        let writes = Arc::new(PrivateWrites::default());
        for phase in ["prepared", "execution-stopped"] {
            write_private_async(path.clone(), json!({"phase":phase}), Arc::clone(&writes))
                .await
                .unwrap();
            let value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(value["phase"], phase);
            assert!(!path.with_extension("tmp").exists());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn closed_owner_refuses_late_write_without_overwriting_cleanup_required() {
        let directory =
            std::env::temp_dir().join(format!("forge-private-closed-{}", Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("journal.json");
        let writes = Arc::new(PrivateWrites::default());
        let job = RemoteJob {
            client: Client {
                host: "not-contacted".into(),
                cert_path: "not-opened".into(),
                daemon_id: "fixture-daemon".into(),
                cgroup_parent: None,
            },
            helper: String::new(),
            project: "fixture".into(),
            directory: directory.clone(),
            manifest: json!({}),
            active: true,
            stopped: false,
            writes: Arc::clone(&writes),
            deployment_output: None,
            deployment_witness: None,
        };
        let pending =
            write_private_async(path.clone(), json!({"phase":"execution-started"}), writes);
        drop(job);
        assert!(pending.await.is_err());
        let marker: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(marker["phase"], "cleanup-required");
        assert_eq!(marker["stopped"], false);
        assert!(!path.with_extension("tmp").exists());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn async_private_write_failure_does_not_replace_existing_evidence() {
        let directory = std::env::temp_dir().join(format!("forge-private-held-{}", Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("journal.json");
        let marker = json!({"phase":"prepared","stopped":false});
        write_private(&path, &marker).unwrap();
        std::fs::write(path.with_extension("tmp"), b"unknown previous write").unwrap();
        assert!(
            write_private_async(path.clone(), json!({"phase":"cleaned"}), Arc::default())
                .await
                .is_err()
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&std::fs::read(&path).unwrap()).unwrap(),
            marker
        );
        assert_eq!(
            std::fs::read(path.with_extension("tmp")).unwrap(),
            b"unknown previous write"
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn superseded_queued_write_cannot_overwrite_newer_journal_phase() {
        let directory =
            std::env::temp_dir().join(format!("forge-private-order-{}", Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("journal.json");
        let writes = PrivateWrites::default();
        writes.requested.store(2, Ordering::SeqCst);
        let marker = json!({"phase":"execution-stopped"});
        writes.write(&path, &marker, 2).unwrap();
        assert!(
            writes
                .write(&path, &json!({"phase":"execution-started"}), 1)
                .is_err()
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&std::fs::read(&path).unwrap()).unwrap(),
            marker
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn image_build_requires_pinned_buildx_and_same_daemon_default_driver() {
        assert!(image_build_version_matches(
            "github.com/docker/buildx v0.37.2 abcdef"
        ));
        for version in [
            "",
            "github.com/docker/buildx v0.37.1 abc",
            "other v0.37.2",
            "github.com/docker/buildx v0.37.20",
        ] {
            assert!(!image_build_version_matches(version));
        }
        let host = "tcp://runner-daemon:2376";
        let builder = json!({"Name":"default", "Driver":"docker",
            "Nodes":[{"Endpoint":"default","Status":"running"}]});
        assert!(image_build_builder_matches(&builder, host, host));
        assert!(!image_build_builder_matches(
            &builder,
            host,
            "tcp://foreign:2376"
        ));
        for field in ["Name", "Driver"] {
            let mut wrong = builder.clone();
            wrong[field] = json!("remote");
            assert!(!image_build_builder_matches(&wrong, host, host));
        }
        for (field, value) in [("Endpoint", "tcp://foreign:2376"), ("Status", "inactive")] {
            let mut wrong = builder.clone();
            wrong["Nodes"][0][field] = json!(value);
            assert!(!image_build_builder_matches(&wrong, host, host));
        }
        let mut wrong = builder.clone();
        wrong["Nodes"] = json!([]);
        assert!(!image_build_builder_matches(&wrong, host, host));
        wrong["Nodes"] = json!([builder["Nodes"][0].clone(), builder["Nodes"][0].clone()]);
        assert!(!image_build_builder_matches(&wrong, host, host));
        let mut wrong = builder;
        wrong["Err"] = json!("unavailable");
        assert!(!image_build_builder_matches(&wrong, host, host));
    }

    #[test]
    fn image_build_inspection_pins_image_source_attempt_request_and_oci_shape() {
        let attempt = Uuid::new_v4();
        let image = format!("sha256:{}", "a".repeat(64));
        let source = "b".repeat(40);
        let hash = "c".repeat(64);
        let value = json!({"id":image,"revision":source,"attempt":attempt.to_string(),
            "request":hash,"os":"linux","volumes":null});
        assert!(image_build_inspection_matches(
            &value, &image, &source, attempt, &hash
        ));
        for field in ["id", "revision", "attempt", "request", "os"] {
            let mut wrong = value.clone();
            wrong[field] = json!("changed");
            assert!(!image_build_inspection_matches(
                &wrong, &image, &source, attempt, &hash
            ));
        }
        let mut wrong = value;
        wrong["volumes"] = json!({"/implicit":{}});
        assert!(!image_build_inspection_matches(
            &wrong, &image, &source, attempt, &hash
        ));
    }

    #[test]
    fn image_build_inspection_allows_omitted_optional_config_volumes() {
        // Docker 29 omits Config.Volumes; Go template index returns null for it.
        assert!(
            IMAGE_BUILD_INSPECT_FORMAT.contains(r#""volumes":{{json (index .Config "Volumes")}}"#)
        );
        assert!(!IMAGE_BUILD_INSPECT_FORMAT.contains(".Config.Volumes"));
        let config = json!({"Labels": {}});
        assert!(config.get("Volumes").is_none());
        let attempt = Uuid::new_v4();
        let image = format!("sha256:{}", "a".repeat(64));
        let source = "b".repeat(40);
        let hash = "c".repeat(64);
        let value = json!({"id":image,"revision":source,"attempt":attempt.to_string(),
            "request":hash,"os":"linux","volumes":config.get("Volumes")});
        assert!(value["volumes"].is_null());
        assert!(image_build_inspection_matches(
            &value, &image, &source, attempt, &hash
        ));
    }

    #[test]
    fn image_build_command_is_fixed_load_without_job_env_or_arbitrary_options() {
        use crate::domain::image_build::ImageBuildSpec;
        let client = Client {
            host: "tcp://runner-daemon:2376".into(),
            cert_path: "/backend-only/tls".into(),
            daemon_id: "expected-daemon".into(),
            cgroup_parent: None,
        };
        let spec = ImageBuildSpec {
            context: ".".into(),
            dockerfile: "Dockerfile".into(),
            target: Some("release".into()),
            descriptor_artifact: "candidate.json".into(),
            readable_schema_versions: vec![1],
        };
        let source = "a".repeat(40);
        let hash = "b".repeat(64);
        let attempt = Uuid::new_v4();
        let command = client
            .image_build_command(ImageBuildCommand {
                spec: &spec,
                context: Path::new("/control/tree"),
                dockerfile: Path::new("/control/tree/Dockerfile"),
                iid_file: Path::new("/control/image-id"),
                docker_config: Path::new("/control/docker-config"),
                source_commit: &source,
                attempt,
                request_sha256: &hash,
            })
            .unwrap();
        let command = command.as_std();
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_str().unwrap())
            .collect();
        assert_eq!(
            &args[..8],
            &[
                "buildx",
                "build",
                "--builder",
                "default",
                "--load",
                "--network",
                "default",
                "--provenance=false"
            ]
        );
        assert_eq!(
            &args[args.len() - 3..],
            &["--target", "release", "/control/tree"]
        );
        for flag in [
            "--push",
            "--secret",
            "--ssh",
            "--allow",
            "--build-arg",
            "--tag",
            "--output",
        ] {
            assert!(!args.contains(&flag));
        }
        let env: std::collections::BTreeMap<_, _> = command
            .get_envs()
            .filter_map(|(key, value)| {
                value.map(|value| {
                    (
                        key.to_string_lossy().into_owned(),
                        value.to_string_lossy().into_owned(),
                    )
                })
            })
            .collect();
        assert_eq!(env["DOCKER_HOST"], client.host);
        assert_eq!(env["DOCKER_TLS_VERIFY"], "1");
        assert_eq!(env["DOCKER_CONFIG"], "/control/docker-config");
        for key in env.keys() {
            assert!(
                [
                    "PATH",
                    "SystemRoot",
                    "DOCKER_HOST",
                    "DOCKER_CERT_PATH",
                    "DOCKER_TLS_VERIFY",
                    "DOCKER_CONFIG"
                ]
                .contains(&key.as_str())
            );
        }
    }

    #[tokio::test]
    async fn retry_requires_cleaned_owned_journal_even_after_owner_death() {
        let root = std::env::temp_dir().join(format!("forge-retry-journal-{}", Uuid::new_v4()));
        let job = Uuid::new_v4();
        let attempt = Uuid::new_v4();
        let project = format!("sdlc-build-job-{}", attempt.simple());
        let directory = root.join(&project);
        let manifest_path = directory.join("compose.json");
        let journal_path = directory.join("journal.json");
        let daemon = "registered-test-daemon";
        let control = "owned-control";
        assert!(
            !retry_unconfirmed_at(&root, job, attempt, daemon, control)
                .await
                .unwrap()
        );
        std::fs::create_dir_all(&directory).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let manifest = json!({"name":project,"services":{"transfer":{"labels":{
            "sdlc.job":job.to_string(),"sdlc.attempt":attempt.to_string(),
            "sdlc.task":format!("job-{job}"),"sdlc.purpose":"cicd-job","sdlc.control-volume":control}}}});
        write_private(&manifest_path, &manifest).unwrap();
        let mut journal = json!({"project":project,"daemon_id":daemon,"manifest":manifest_path,
                                 "owner":"4294967295:0","stopped":false});
        for phase in [
            "prepared",
            "execution-started",
            "cleanup-required",
            "execution-stopped",
        ] {
            journal["phase"] = json!(phase);
            journal["stopped"] = json!(phase == "execution-stopped");
            write_private(&journal_path, &journal).unwrap();
            assert!(
                retry_unconfirmed_at(&root, job, attempt, daemon, control)
                    .await
                    .unwrap(),
                "owner death or stopped phase {phase} is not completed recovery"
            );
        }
        journal["phase"] = json!("cleaned");
        journal["stopped"] = json!(true);
        write_private(&journal_path, &journal).unwrap();
        assert!(
            !retry_unconfirmed_at(&root, job, attempt, daemon, control)
                .await
                .unwrap()
        );
        assert!(
            retry_unconfirmed_at(&root, Uuid::new_v4(), attempt, daemon, control)
                .await
                .is_err(),
            "foreign job binding must fail closed"
        );
        assert!(
            retry_unconfirmed_at(&root, job, attempt, "foreign-daemon", control)
                .await
                .is_err()
        );
        assert!(
            retry_unconfirmed_at(&root, job, attempt, daemon, "foreign-control")
                .await
                .is_err()
        );
        journal["stopped"] = json!(false);
        write_private(&journal_path, &journal).unwrap();
        assert!(
            retry_unconfirmed_at(&root, job, attempt, daemon, control)
                .await
                .is_err(),
            "cleaned without stopped must fail closed"
        );
        std::fs::write(&journal_path, b"{invalid").unwrap();
        assert!(
            retry_unconfirmed_at(&root, job, attempt, daemon, control)
                .await
                .is_err()
        );
        std::fs::write(&journal_path, vec![b' '; 64 * 1024 + 1]).unwrap();
        assert!(
            retry_unconfirmed_at(&root, job, attempt, daemon, control)
                .await
                .is_err()
        );
        std::fs::remove_file(&journal_path).unwrap();
        assert!(
            retry_unconfirmed_at(&root, job, attempt, daemon, control)
                .await
                .is_err(),
            "existing incomplete directory is not prelaunch absence"
        );
        std::fs::remove_file(&manifest_path).unwrap();
        std::fs::remove_dir(&directory).unwrap();
        #[cfg(unix)]
        {
            let foreign = root.join("foreign");
            std::fs::create_dir(&foreign).unwrap();
            std::fs::write(foreign.join("do-not-touch"), b"foreign").unwrap();
            std::os::unix::fs::symlink(&foreign, &directory).unwrap();
            assert!(
                retry_unconfirmed_at(&root, job, attempt, daemon, control)
                    .await
                    .is_err(),
                "foreign recovery symlink must fail closed"
            );
            assert_eq!(
                std::fs::read(foreign.join("do-not-touch")).unwrap(),
                b"foreign"
            );
            std::fs::remove_file(&directory).unwrap();
            std::fs::remove_file(foreign.join("do-not-touch")).unwrap();
            std::fs::remove_dir(foreign).unwrap();
        }
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn runner_sources_accept_registered_installations_and_reject_foreign_provenance() {
        let revision = "875cac2edf1a18c3a8a59e2f67256d02a8fc04e4";
        for workspace in ["sdlc1", "sdlc2", "pdlc1", "pdlc2", "pdlc3", "pdlc4"] {
            let volume =
                json!([{"Labels":{"sdlc.base-revision":revision,"sdlc.workspace":workspace}}]);
            assert!(source_provenance_matches(&volume, revision, workspace));
            assert!(!source_provenance_matches(&volume, revision, "foreign"));
            assert!(!source_provenance_matches(
                &volume,
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                workspace
            ));
            assert!(!source_provenance_matches(&volume, revision, "pdlc-common"));
        }
        let volume = json!([{"Labels":{"sdlc.base-revision":revision,"sdlc.workspace":"sdlc1"}}]);
        assert!(!source_provenance_matches(&volume, revision, "pdlc1"));
        assert!(!source_provenance_matches(&json!([]), revision, "pdlc1"));
        let malformed = "z".repeat(40);
        let volume = json!([{"Labels":{"sdlc.base-revision":malformed,"sdlc.workspace":"pdlc1"}}]);
        assert!(!source_provenance_matches(&volume, &malformed, "pdlc1"));
    }

    #[test]
    fn foreign_storage_users_block_cleanup_before_any_side_effect() {
        for markers in [("a", "b", "c"), ("d", "e", "f")] {
            let transfer = markers.0.repeat(64);
            let job = markers.1.repeat(64);
            let neighbor = markers.2.repeat(64);
            let containers = vec![json!({"Id":transfer}), json!({"Id":job})];
            assert!(storage_users_owned(&[], &containers));
            assert!(storage_users_owned(&[&transfer, &job], &containers));
            assert!(!storage_users_owned(&[&transfer, &neighbor], &containers));
            assert!(!storage_users_owned(&[&job, ""], &containers));
            assert!(!storage_users_owned(&["malformed-id"], &containers));
            assert!(!storage_users_owned(&[&transfer], &[json!({})]));
        }
    }
    #[test]
    fn cleanup_requires_exact_container_and_storage_ownership() {
        for (attempt, job) in [
            (Uuid::new_v4(), Uuid::new_v4()),
            (Uuid::new_v4(), Uuid::new_v4()),
        ] {
            let project = format!("sdlc-build-job-{}", attempt.simple());
            let file = PathBuf::from("/work/control/compose.json");
            let expected = json!({"sdlc.job":job.to_string(), "sdlc.attempt":attempt.to_string(),
                "sdlc.task":format!("job-{job}"), "sdlc.purpose":"cicd-job", "sdlc.control-volume":"owned-control"});
            let manifest = json!({"services":{"transfer":{"labels":expected, "container_name":"owned-transfer"},
                "job":{"container_name":"owned-job"}},
                "volumes":{"workspace":{},"sources":{"external":true}}, "networks":{"job":{}}});
            for (kind, logical) in [
                ("container", "transfer"),
                ("container", "job"),
                ("volume", "workspace"),
                ("network", "job"),
            ] {
                let mut labels = expected.clone();
                labels["com.docker.compose.project"] = json!(project);
                labels[format!(
                    "com.docker.compose.{}",
                    if kind == "container" { "service" } else { kind }
                )] = json!(logical);
                labels["com.docker.compose.project.config_files"] = json!(file.to_str().unwrap());
                let resource = if kind == "container" {
                    json!({"Name":format!("/owned-{logical}"),"Config":{"Labels":labels}})
                } else {
                    json!({"Name":format!("{project}_{logical}"),"Labels":labels})
                };
                assert!(resource_owned(kind, &resource, &manifest, &project, &file));
                for key in [
                    "sdlc.job",
                    "sdlc.attempt",
                    "sdlc.task",
                    "sdlc.purpose",
                    "com.docker.compose.project",
                ] {
                    let mut foreign = resource.clone();
                    let foreign_labels = if kind == "container" {
                        &mut foreign["Config"]["Labels"]
                    } else {
                        &mut foreign["Labels"]
                    };
                    foreign_labels[key] = json!("different-owner");
                    assert!(!resource_owned(kind, &foreign, &manifest, &project, &file));
                }
                let mut wrong_name = resource.clone();
                wrong_name["Name"] = json!("foreign-name");
                assert!(!resource_owned(
                    kind,
                    &wrong_name,
                    &manifest,
                    &project,
                    &file
                ));
                let mut wrong_logical = resource.clone();
                let foreign_labels = if kind == "container" {
                    &mut wrong_logical["Config"]["Labels"]
                } else {
                    &mut wrong_logical["Labels"]
                };
                foreign_labels[format!(
                    "com.docker.compose.{}",
                    if kind == "container" { "service" } else { kind }
                )] = json!("neighbor");
                assert!(!resource_owned(
                    kind,
                    &wrong_logical,
                    &manifest,
                    &project,
                    &file
                ));
            }
            let mut external_labels = expected.clone();
            external_labels["com.docker.compose.project"] = json!(project);
            external_labels["com.docker.compose.volume"] = json!("sources");
            let external = json!({"Name":format!("{project}_sources"),"Labels":external_labels});
            assert!(!resource_owned(
                "volume", &external, &manifest, &project, &file
            ));
            assert!(!resource_owned(
                "container",
                &json!({}),
                &json!({}),
                &project,
                &file
            ));
        }
    }
    #[test]
    fn recovery_distinguishes_dead_process_and_current_owner() {
        #[cfg(target_os = "linux")]
        assert!(owner_alive(&process_owner()));
        assert!(!owner_alive("4294967295:0"));
        assert!(!owner_alive("unverified"));
    }
    #[test]
    fn job_has_no_control_workspace_or_tls_mount() {
        let args = [
            "run",
            "--rm",
            "--name",
            "forge-job-test",
            "--mount",
            "type=volume,src=control,dst=/workspaces",
            "--user",
            "10001:10001",
            "rust:1.88",
            "sh",
            "-c",
            "echo $CICD_JOB_ID",
        ]
        .map(String::from);
        let value = compose_job(
            &args,
            &[
                ("DOCKER_HOST".into(), "secret".into()),
                ("CICD_JOB_ID".into(), "test".into()),
            ],
        )
        .unwrap();
        assert!(!value.to_string().contains("control"));
        assert!(!value.to_string().contains("secret"));
        assert_eq!(value["entrypoint"][2], "echo $CICD_JOB_ID");
        assert_eq!(value["volumes"].as_array().unwrap().len(), 3);
    }
    #[test]
    fn compose_preserves_literal_dollars() {
        let mut value = json!({"command":"printf '%s' '${TOKEN}'"});
        literal(&mut value);
        assert_eq!(value["command"], "printf '%s' '$${TOKEN}'");
    }
}
