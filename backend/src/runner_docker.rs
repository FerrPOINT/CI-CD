//! Compose-owned job execution and file transfer. Journals never mount into jobs.
use serde_json::{Value, json};
use std::{
    io,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::process::Command;
use uuid::Uuid;

#[derive(Clone)]
pub(crate) struct Client {
    host: String,
    cert_path: String,
    daemon_id: String,
}
fn invalid(message: &str) -> io::Error {
    io::Error::other(message)
}

impl Client {
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
        Ok(Self {
            host,
            cert_path,
            daemon_id,
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

impl RemoteJob {
    pub(crate) async fn prepare(id: Uuid, attempt: Uuid, workspace: &Path) -> io::Result<Self> {
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
        let helper = format!("forge-transfer-{attempt}");
        let labels = json!({"sdlc.task": format!("job-{id}"), "sdlc.purpose":"cicd-job", "sdlc.job":id.to_string(), "sdlc.attempt":attempt.to_string(),
            "sdlc.control-volume":std::env::var("CICD_RUNNER_WORKSPACE_VOLUME").unwrap_or_default()});
        let helper_image = client.image("alpine:3.21").await?;
        let manifest = json!({"name":project, "services":{"transfer":{
            "image":helper_image, "container_name":helper, "pull_policy":"never", "restart":"no",
            "entrypoint":["/bin/sh"], "command":["-c","sleep 86400"], "network_mode":"none",
            "read_only":true, "tmpfs":["/tmp:mode=700"], "security_opt":["no-new-privileges:true"],
            "labels":labels, "volumes":[{"type":"volume","source":"workspace","target":"/workspace"},{"type":"volume","source":"cargo","target":"/cache"}]
        }}, "volumes":{"workspace":{"labels":labels}, "cargo":{"labels":labels}}, "networks":{"job":{"labels":labels}}});
        let mut job = Self {
            client,
            helper,
            project,
            directory,
            manifest,
            active: true,
            stopped: false,
        };
        job.record("prepared")?;
        let prepared = async {
            job.save_manifest()?;
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
    fn record(&self, phase: &str) -> io::Result<()> {
        write_private(
            &self.directory.join("journal.json"),
            &json!({"project":self.project,
            "daemon_id":self.client.daemon_id, "phase":phase, "owner":process_owner(), "stopped":self.stopped, "manifest":self.directory.join("compose.json")}),
        )
    }
    fn save_manifest(&self) -> io::Result<()> {
        let mut value = self.manifest.clone();
        literal(&mut value);
        write_private(&self.directory.join("compose.json"), &value)
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
        if revision.len() != 40
            || !matches!(workspace.as_str(), "sdlc1" | "sdlc2")
            || volume[0]["Labels"]["sdlc.base-revision"] != revision
            || volume[0]["Labels"]["sdlc.workspace"] != workspace
        {
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
        self.manifest["services"]["job"] = service;
        self.manifest["volumes"]["sources"] = json!({"external":true,"name":source});
        self.save_manifest()?;
        self.compose(&["config", "--quiet"]).await?;
        self.record("execution-started")?;
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
                }
            }
        }
        Ok(containers)
    }

    pub(crate) async fn confirm_stopped(&mut self) -> io::Result<()> {
        let containers = self.owned_resources().await?;
        for container in containers {
            if container["Config"]["Labels"]["com.docker.compose.service"] == "job"
                && container["State"]["Running"].as_bool() != Some(false)
            {
                return Err(invalid("job termination is unconfirmed"));
            }
        }
        self.stopped = true;
        self.record("execution-stopped")
    }

    pub(crate) async fn stop_execution(&mut self) -> io::Result<()> {
        for container in self.owned_resources().await? {
            if container["Config"]["Labels"]["com.docker.compose.service"] == "job" {
                let id = container["Id"]
                    .as_str()
                    .ok_or_else(|| invalid("container ID missing"))?;
                self.client.run(&["stop", "-t", "2", id]).await?;
            }
        }
        self.confirm_stopped().await
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
        self.owned_resources().await?;
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
        self.record("cleaned")?;
        self.active = false;
        // Erase per-job environment only after successful cleanup. Keep safe recovery journal.
        for service in self.manifest["services"]
            .as_object_mut()
            .unwrap()
            .values_mut()
        {
            service.as_object_mut().unwrap().remove("environment");
        }
        self.save_manifest()?;
        Ok(())
    }
}
impl Drop for RemoteJob {
    fn drop(&mut self) {
        if self.active {
            let _ = self.record("cleanup-required");
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
