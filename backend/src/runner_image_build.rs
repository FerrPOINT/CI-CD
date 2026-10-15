//! Backend-owned build primitive. The existing runner owns admission, spawn and monitoring.
//! Started/partial custody is never permission to rebuild or clean up a possibly live build.
use crate::{
    domain::image_build::{
        ImageBuildSpec, OciCandidateDescriptor, valid_commit, valid_relative_path,
    },
    runner_docker::{Client, ImageBuildCommand},
    runner_workspace::preparation::{self, PreparationSource},
};
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::fs::File;
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Command,
};
use uuid::Uuid;

const MAX_TREE: usize = 32 * 1024 * 1024;
const MAX_FILES: usize = 100_000;
const MAX_FILE: usize = 128 * 1024 * 1024;
const MAX_TOTAL: usize = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ImageBuildScope {
    pub project_id: Uuid,
    pub pipeline_id: Uuid,
    pub job_id: Uuid,
    pub attempt_id: Uuid,
    pub lease_id: Uuid,
    pub generation: i64,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    schema: String,
    scope: ImageBuildScope,
    repository_id: Uuid,
    source_commit: String,
    spec: ImageBuildSpec,
    daemon_id: String,
}

#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Completion {
    request_sha256: String,
    daemon_id: String,
    descriptor: OciCandidateDescriptor,
}

/// These are monitor observations, not evidence that a remote BuildKit solver stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BuildExit {
    Success,
    Failed,
    TimedOut,
    Cancelled,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BuildHoldReason {
    CliFailed,
    TimedOut,
    Cancelled,
    Unknown,
    VerificationUnavailable,
}

#[derive(Debug)]
pub(crate) struct CompletedImageBuild {
    pub descriptor: OciCandidateDescriptor,
    pub artifact_root: PathBuf,
    pub artifact_path: PathBuf,
}

#[derive(Debug)]
pub(crate) enum BuildCompletion {
    Complete(CompletedImageBuild),
    Held(BuildHoldReason),
}

pub(crate) struct PreparedImageBuild {
    client: Client,
    directory: PathBuf,
    intent: Intent,
    request_sha256: String,
    started: bool,
}

impl ImageBuildScope {
    fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            [
                self.project_id,
                self.pipeline_id,
                self.job_id,
                self.attempt_id,
                self.lease_id
            ]
            .iter()
            .all(|id| !id.is_nil())
                && self.generation > 0,
            "invalid server-owned build scope"
        );
        Ok(())
    }
}

/// `source` is the existing owner-authorized object-store binding at the exact task SHA.
/// No mutable checkout, artifacts, secret resolution, task permit or job lease is created here.
pub(crate) async fn prepare(
    control_root: &Path,
    source: &PreparationSource,
    scope: ImageBuildScope,
    spec: ImageBuildSpec,
) -> anyhow::Result<PreparedImageBuild> {
    ensure!(
        cfg!(unix),
        "backend image build requires the protected Unix filesystem"
    );
    scope.validate()?;
    spec.validate().map_err(anyhow::Error::msg)?;
    ensure!(
        scope.project_id == source.project_id
            && !source.repository_id.is_nil()
            && valid_commit(&source.source_commit),
        "build source differs from task project/pin"
    );
    ensure!(
        control_root.is_absolute()
            && control_root
                .file_name()
                .is_some_and(|name| name == ".compose-control"),
        "build custody requires the existing backend control root"
    );
    if missing(control_root)? {
        anchored_directory(
            control_root
                .parent()
                .context("control root parent missing")?,
        )?;
        create_directory(control_root)?;
    }
    private_directory(control_root)?;
    ensure!(
        control_root.canonicalize()? == control_root
            && control_root
                .file_name()
                .is_some_and(|name| name == ".compose-control"),
        "build custody must use the backend control root outside job mounts"
    );
    anchored_directory(&source.repository)?;
    ensure!(
        source.repository.canonicalize()? == source.repository
            && !source.repository.starts_with(control_root)
            && !control_root.starts_with(&source.repository),
        "invalid build source/control separation"
    );
    // Reuse task preparation's actual bare/config/exact-pin guards (no hooks, helpers,
    // includes, filters or partial-clone configuration), not a second repository policy.
    preparation::repository_pipeline_config(&source.repository, &source.source_commit).await?;
    let entries = tree_entries(
        &git_read(
            &source.repository,
            &["ls-tree", "-r", "-z", "--full-tree", &source.source_commit],
            MAX_TREE,
        )
        .await?,
    )?;
    require_dockerfile(&entries, &spec)?;

    let directory = control_root.join(format!("image-build-{}", scope.attempt_id.simple()));
    // Any retained/partial attempt is Held by the caller, never a second build command.
    create_directory(&directory)
        .context("image build custody exists or could not be created; Held")?;
    let docker_config = directory.join("docker-config");
    create_directory(&docker_config)?;
    write_new(&docker_config.join("config.json"), b"{\"auths\":{}}", 0o600)?;
    let client = Client::for_image_build(&docker_config).await?;
    let intent = Intent {
        schema: "forge/backend-image-build/v1".into(),
        scope,
        repository_id: source.repository_id,
        source_commit: source.source_commit.clone(),
        spec,
        daemon_id: client.image_build_daemon_id().to_owned(),
    };
    let bytes = serde_json::to_vec(&intent)?;
    ensure!(bytes.len() <= 8192, "build intent exceeds bound");
    let request_sha256 = digest(&bytes);
    write_new(&directory.join("intent.json"), &bytes, 0o600)?;
    let tree = directory.join("tree");
    create_directory(&tree)?;
    export_objects(&source.repository, &tree, &entries).await?;
    seal_directories(&tree)?;
    // Export may take time; re-observe the actual builder before handing a Command to the runner.
    client.verify_image_build(&docker_config).await?;
    write_new(
        &directory.join("exported.json"),
        request_sha256.as_bytes(),
        0o600,
    )?;
    Ok(PreparedImageBuild {
        client,
        directory,
        intent,
        request_sha256,
        started: false,
    })
}

/// MAIN calls this from the existing embedded retry guard before its Compose-only branch.
/// Even a partial directory is retained custody. Process/CLI exit is never a stop proof.
pub(crate) fn retry_unconfirmed(
    control_root: &Path,
    job: Uuid,
    attempt: Uuid,
) -> anyhow::Result<bool> {
    ensure!(
        !job.is_nil() && !attempt.is_nil(),
        "invalid image build retry identity"
    );
    if missing(control_root)? {
        return Ok(false);
    }
    ensure!(cfg!(unix), "image build custody filesystem is unsupported");
    private_directory(control_root)?;
    let directory = control_root.join(format!("image-build-{}", attempt.simple()));
    if missing(&directory)? {
        return Ok(false);
    }
    private_directory(&directory)?;
    let path = directory.join("intent.json");
    if !missing(&path)? {
        let intent: Intent = serde_json::from_slice(&read_private(&path, 8192)?)?;
        intent.scope.validate()?;
        intent.spec.validate().map_err(anyhow::Error::msg)?;
        ensure!(
            intent.schema == "forge/backend-image-build/v1"
                && intent.scope.job_id == job
                && intent.scope.attempt_id == attempt,
            "image build retry custody differs from job/attempt"
        );
    }
    Ok(true)
}

impl PreparedImageBuild {
    fn verify_intent(&self) -> anyhow::Result<()> {
        private_directory(&self.directory)?;
        let bytes = read_private(&self.directory.join("intent.json"), 8192)?;
        let saved: Intent = serde_json::from_slice(&bytes)?;
        ensure!(
            saved == self.intent && digest(&bytes) == self.request_sha256,
            "build intent changed; Held"
        );
        ensure!(
            read_private(&self.directory.join("exported.json"), 64)?
                == self.request_sha256.as_bytes(),
            "build source export is not sealed; Held"
        );
        Ok(())
    }

    /// Construct only. MAIN must retain its existing task/lease CAS and deadline before spawn.
    pub(crate) fn command(&self) -> anyhow::Result<Command> {
        self.verify_intent()?;
        ensure!(
            !self.started
                && missing(&self.directory.join("started.json"))?
                && missing(&self.directory.join("image-id"))?,
            "build already started; Held"
        );
        let context = context_path(&self.directory, &self.intent.spec);
        let dockerfile = context.join(&self.intent.spec.dockerfile);
        Ok(self.client.image_build_command(ImageBuildCommand {
            spec: &self.intent.spec,
            context: &context,
            dockerfile: &dockerfile,
            iid_file: &self.directory.join("image-id"),
            docker_config: &self.directory.join("docker-config"),
            source_commit: &self.intent.source_commit,
            attempt: self.intent.scope.attempt_id,
            request_sha256: &self.request_sha256,
        })?)
    }

    /// Call AFTER `task_pipeline::before_spawn` and live lease checks, immediately BEFORE spawn.
    /// A spawn error/crash after this fsync is Unknown/Held, not permission to retry.
    pub(crate) fn mark_started(&mut self) -> anyhow::Result<()> {
        self.verify_intent()?;
        ensure!(!self.started, "build already started; Held");
        write_new(
            &self.directory.join("started.json"),
            self.request_sha256.as_bytes(),
            0o600,
        )?;
        self.started = true;
        Ok(())
    }

    /// The returned proof is not a job completion/dispatch permit. MAIN rechecks current task,
    /// active attempt and cancellation before existing artifact publication and DB completion.
    pub(crate) async fn complete(&self, exit: BuildExit) -> BuildCompletion {
        if let Some(reason) = hold_reason(exit) {
            return BuildCompletion::Held(reason);
        }
        match self.complete_success().await {
            Ok(completed) => BuildCompletion::Complete(completed),
            Err(_) => BuildCompletion::Held(BuildHoldReason::VerificationUnavailable),
        }
    }

    pub(crate) fn hold_unknown(&self) -> BuildCompletion {
        BuildCompletion::Held(BuildHoldReason::Unknown)
    }

    async fn complete_success(&self) -> anyhow::Result<CompletedImageBuild> {
        self.verify_intent()?;
        ensure!(
            self.started
                && read_private(&self.directory.join("started.json"), 64)?
                    == self.request_sha256.as_bytes(),
            "build has no started custody"
        );
        // Docker writes iidfile using its own umask; the containing control directory is private.
        let image = String::from_utf8(read_owned(&self.directory.join("image-id"), 80, false)?)?;
        let image = image.trim();
        self.client
            .inspect_built_image(
                &self.directory.join("docker-config"),
                image,
                &self.intent.source_commit,
                self.intent.scope.attempt_id,
                &self.request_sha256,
            )
            .await?;
        let descriptor = OciCandidateDescriptor::new(
            image.into(),
            self.intent.source_commit.clone(),
            self.intent.spec.readable_schema_versions.clone(),
        )
        .map_err(anyhow::Error::msg)?;
        let output = self.directory.join("output");
        if missing(&output)? {
            create_directory(&output)?;
        }
        private_directory(&output)?;
        let artifact_path = output.join(&self.intent.spec.descriptor_artifact);
        create_parents(&output, Path::new(&self.intent.spec.descriptor_artifact))?;
        // Create-new only. A repeated completion may read identical saved bytes, never replace them.
        let bytes = serde_json::to_vec(&descriptor)?;
        if missing(&artifact_path)? {
            write_new(&artifact_path, &bytes, 0o600)?;
        } else {
            ensure!(
                read_private(&artifact_path, 4096)? == bytes,
                "descriptor publication differs; Held"
            );
        }
        let proof = serde_json::to_vec(&Completion {
            request_sha256: self.request_sha256.clone(),
            daemon_id: self.intent.daemon_id.clone(),
            descriptor: descriptor.clone(),
        })?;
        let marker = self.directory.join("completed.json");
        if missing(&marker)? {
            write_new(&marker, &proof, 0o600)?;
        } else {
            ensure!(
                read_private(&marker, 8192)? == proof,
                "build completion custody differs; Held"
            );
        }
        Ok(CompletedImageBuild {
            descriptor,
            artifact_root: output,
            artifact_path,
        })
    }
}

fn hold_reason(exit: BuildExit) -> Option<BuildHoldReason> {
    match exit {
        BuildExit::Success => None,
        BuildExit::Failed => Some(BuildHoldReason::CliFailed),
        BuildExit::TimedOut => Some(BuildHoldReason::TimedOut),
        BuildExit::Cancelled => Some(BuildHoldReason::Cancelled),
        BuildExit::Unknown => Some(BuildHoldReason::Unknown),
    }
}

fn context_path(directory: &Path, spec: &ImageBuildSpec) -> PathBuf {
    let tree = directory.join("tree");
    if spec.context == "." {
        tree
    } else {
        tree.join(&spec.context)
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

struct TreeEntry {
    path: String,
    oid: String,
    executable: bool,
}

fn excluded_source_path(path: &str) -> bool {
    path.split('/').any(|part| {
        let name = part.to_ascii_lowercase();
        matches!(
            name.as_str(),
            ".git"
                | ".local"
                | "target"
                | "node_modules"
                | ".venv"
                | "venv"
                | ".cache"
                | "__pycache__"
                | "backups"
                | "credentials"
                | "secrets"
                | ".ssh"
                | ".aws"
                | ".kube"
                | ".env"
                | "id_rsa"
                | "id_ed25519"
        ) || (name.starts_with(".env.")
            && !matches!(
                name.as_str(),
                ".env.example" | ".env.sample" | ".env.template"
            ))
            || name.ends_with(".key")
            || name.ends_with(".p12")
            || name.ends_with(".pfx")
    })
}

fn tree_entries(bytes: &[u8]) -> anyhow::Result<Vec<TreeEntry>> {
    ensure!(
        !bytes.is_empty() && bytes.last() == Some(&0) && bytes.len() <= MAX_TREE,
        "missing/truncated source inventory"
    );
    let mut entries = Vec::new();
    let mut paths = BTreeSet::new();
    for raw in bytes[..bytes.len() - 1].split(|b| *b == 0) {
        let (header, path) = std::str::from_utf8(raw)?
            .split_once('\t')
            .context("invalid Git tree record")?;
        let fields: Vec<_> = header.split(' ').collect();
        ensure!(
            fields.len() == 3
                && matches!(fields[0], "100644" | "100755")
                && fields[1] == "blob"
                && valid_commit(fields[2]),
            "symlink/gitlink/invalid source object denied"
        );
        ensure!(
            valid_relative_path(path, false)
                && path.split('/').count() <= 64
                && !excluded_source_path(path)
                && paths.insert(path.to_owned()),
            "unsafe/secret/cache source path denied"
        );
        entries.push(TreeEntry {
            path: path.into(),
            oid: fields[2].into(),
            executable: fields[0] == "100755",
        });
        ensure!(
            entries.len() <= MAX_FILES,
            "source inventory exceeds file bound"
        );
    }
    Ok(entries)
}

fn require_dockerfile(entries: &[TreeEntry], spec: &ImageBuildSpec) -> anyhow::Result<()> {
    let path = if spec.context == "." {
        spec.dockerfile.clone()
    } else {
        format!("{}/{}", spec.context, spec.dockerfile)
    };
    ensure!(
        entries.iter().any(|entry| entry.path == path),
        "Dockerfile is absent from sealed context"
    );
    Ok(())
}

fn git_command(repository: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args([
            "--no-replace-objects",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "credential.helper=",
            "-c",
            "protocol.allow=never",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .arg("--git-dir")
        .arg(repository)
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", root);
    }
    command
}

async fn git_read(repository: &Path, args: &[&str], bound: usize) -> anyhow::Result<Vec<u8>> {
    let mut child = git_command(repository)
        .args(args)
        .stdin(Stdio::null())
        .spawn()?;
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        let mut bytes = Vec::new();
        child
            .stdout
            .take()
            .context("Git stdout missing")?
            .take(bound as u64 + 1)
            .read_to_end(&mut bytes)
            .await?;
        ensure!(
            bytes.len() <= bound && child.wait().await?.success(),
            "sealed Git read failed"
        );
        Ok::<_, anyhow::Error>(bytes)
    })
    .await;
    match result {
        Ok(Ok(bytes)) => Ok(bytes),
        _ => {
            let _ = child.kill().await;
            anyhow::bail!("sealed Git read unavailable")
        }
    }
}

async fn export_objects(
    repository: &Path,
    root: &Path,
    entries: &[TreeEntry],
) -> anyhow::Result<()> {
    let mut child = git_command(repository)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .spawn()?;
    let result = tokio::time::timeout(Duration::from_secs(300), async {
        let mut input = child.stdin.take().context("Git object input missing")?;
        let mut output = BufReader::new(child.stdout.take().context("Git object output missing")?);
        let mut total = 0usize;
        for entry in entries {
            input
                .write_all(format!("{}\n", entry.oid).as_bytes())
                .await?;
            let mut header = Vec::new();
            output.read_until(b'\n', &mut header).await?;
            let length = blob_length(&header, &entry.oid)?;
            total = total.checked_add(length).context("source size overflow")?;
            ensure!(total <= MAX_TOTAL, "source export exceeds total bound");
            let mut bytes = vec![0; length];
            output.read_exact(&mut bytes).await?;
            ensure!(
                output.read_u8().await? == b'\n',
                "truncated Git object boundary"
            );
            verify_blob(&bytes, &entry.oid)?;
            create_parents(root, Path::new(&entry.path))?;
            write_new(
                &root.join(&entry.path),
                &bytes,
                if entry.executable { 0o755 } else { 0o644 },
            )?;
        }
        drop(input);
        ensure!(child.wait().await?.success(), "Git export did not complete");
        Ok::<(), anyhow::Error>(())
    })
    .await;
    match result {
        Ok(Ok(())) => Ok(()),
        _ => {
            let _ = child.kill().await;
            anyhow::bail!("source export incomplete; custody retained")
        }
    }
}

fn blob_length(header: &[u8], oid: &str) -> anyhow::Result<usize> {
    ensure!(
        header.len() <= 128 && header.last() == Some(&b'\n'),
        "invalid Git blob header"
    );
    let fields: Vec<_> = std::str::from_utf8(&header[..header.len() - 1])?
        .split(' ')
        .collect();
    ensure!(
        fields.len() == 3 && fields[0] == oid && fields[1] == "blob",
        "Git blob identity differs"
    );
    let length: usize = fields[2].parse()?;
    ensure!(
        length <= MAX_FILE && length.to_string() == fields[2],
        "invalid/bounded Git blob size"
    );
    Ok(length)
}

fn verify_blob(bytes: &[u8], oid: &str) -> anyhow::Result<()> {
    let mut hash = Sha1::new();
    hash.update(format!("blob {}\0", bytes.len()).as_bytes());
    hash.update(bytes);
    ensure!(
        format!("{:x}", hash.finalize()) == oid,
        "source blob differs from exact Git object"
    );
    Ok(())
}

fn anchored_directory(path: &Path) -> anyhow::Result<()> {
    ensure!(path.is_absolute(), "absolute protected path required");
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor)?;
        ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "linked/non-directory ancestor denied"
        );
    }
    Ok(())
}

fn private_directory(path: &Path) -> anyhow::Result<()> {
    anchored_directory(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::metadata(path)?;
        ensure!(
            metadata.uid() == unsafe { libc::geteuid() } && metadata.mode() & 0o077 == 0,
            "build control directory is not private"
        );
    }
    Ok(())
}

fn create_directory(path: &Path) -> anyhow::Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    sync_directory(path.parent().context("directory parent missing")?)?;
    Ok(())
}

fn missing(path: &Path) -> anyhow::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error.into()),
    }
}

fn create_parents(root: &Path, path: &Path) -> anyhow::Result<()> {
    let mut directory = root.to_path_buf();
    for component in path
        .parent()
        .context("relative file parent missing")?
        .components()
    {
        ensure!(
            matches!(component, std::path::Component::Normal(_)),
            "unsafe output parent"
        );
        directory.push(component);
        if missing(&directory)? {
            create_directory(&directory)?;
        }
        private_directory(&directory)?;
    }
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8], mode: u32) -> anyhow::Result<()> {
    private_directory(path.parent().context("file parent missing")?)?;
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode).custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(not(unix))]
    let _ = mode;
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(mode))?;
    }
    file.sync_all()?;
    sync_directory(path.parent().context("file parent missing")?)?;
    Ok(())
}

fn read_private(path: &Path, bound: usize) -> anyhow::Result<Vec<u8>> {
    read_owned(path, bound, true)
}

fn read_owned(path: &Path, bound: usize, private_mode: bool) -> anyhow::Result<Vec<u8>> {
    private_directory(path.parent().context("file parent missing")?)?;
    let before = fs::symlink_metadata(path)?;
    ensure!(
        before.is_file() && !before.file_type().is_symlink() && before.len() <= bound as u64,
        "invalid build custody file"
    );
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path)?;
    let after = file.metadata()?;
    ensure!(
        after.is_file() && after.len() <= bound as u64,
        "invalid opened custody file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            after.dev() == before.dev()
                && after.ino() == before.ino()
                && after.nlink() == 1
                && after.uid() == unsafe { libc::geteuid() }
                && (!private_mode || after.mode() & 0o077 == 0),
            "linked/non-private build custody file"
        );
    }
    let mut bytes = Vec::new();
    #[cfg(not(unix))]
    let _ = private_mode;
    Read::by_ref(&mut file)
        .take(bound as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= bound, "build custody exceeds bound");
    Ok(bytes)
}

fn sync_directory(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn seal_directories(path: &Path) -> anyhow::Result<()> {
    for item in fs::read_dir(path)? {
        let item = item?;
        if item.file_type()?.is_dir() {
            seal_directories(&item.path())?;
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Preserve normal Git checkout directory modes for Docker COPY/non-root application users.
        // The containing attempt/control directories remain 0700 and are never mounted into jobs.
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
    sync_directory(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(path: &str, mode: &str) -> Vec<u8> {
        format!("{mode} blob {}\t{path}\0", "a".repeat(40)).into_bytes()
    }

    #[cfg(unix)]
    fn loose_object(repository: &Path, kind: &str, bytes: &[u8]) -> anyhow::Result<String> {
        let mut object = format!("{kind} {}\0", bytes.len()).into_bytes();
        object.extend_from_slice(bytes);
        let oid = format!("{:x}", Sha1::digest(&object));
        let directory = repository.join("objects").join(&oid[..2]);
        if missing(&directory)? {
            create_directory(&directory)?;
        }
        let mut compressor =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        compressor.write_all(&object)?;
        write_new(&directory.join(&oid[2..]), &compressor.finish()?, 0o600)?;
        Ok(oid)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn image_build_exports_exact_git_objects_not_mutable_checkout_or_artifacts() {
        let root =
            std::env::temp_dir().join(format!("forge-image-build-export-{}", Uuid::new_v4()));
        create_directory(&root).unwrap();
        let result = async {
            let repository = root.join("source.git");
            create_directory(&repository)?;
            for name in ["objects", "refs"] { create_directory(&repository.join(name))?; }
            write_new(&repository.join("HEAD"), b"ref: refs/heads/main\n", 0o600)?;
            write_new(&repository.join("config"), b"[core]\nrepositoryformatversion = 0\nfilemode = true\nbare = true\n", 0o600)?;
            let dockerfile = b"FROM scratch\n";
            let pipeline = b"version: 1\njobs: {}\n";
            let dockerfile_oid = loose_object(&repository, "blob", dockerfile)?;
            let config_oid = loose_object(&repository, "blob", pipeline)?;
            let mut raw_tree = Vec::new();
            for (name, oid) in [(".forge-ci.yml", config_oid), ("Dockerfile", dockerfile_oid)] {
                raw_tree.extend_from_slice(format!("100644 {name}\0").as_bytes());
                for index in (0..40).step_by(2) { raw_tree.push(u8::from_str_radix(&oid[index..index+2], 16)?); }
            }
            let tree_oid = loose_object(&repository, "tree", &raw_tree)?;
            let commit = loose_object(&repository, "commit", format!("tree {tree_oid}\nauthor Fixture <fixture@example.invalid> 0 +0000\ncommitter Fixture <fixture@example.invalid> 0 +0000\n\nsealed\n").as_bytes())?;
            let mutable = root.join("mutable-worktree");
            create_directory(&mutable)?;
            write_new(&mutable.join("Dockerfile"), b"not the pinned source", 0o600)?;
            write_new(&mutable.join(".env"), b"synthetic-not-exported", 0o600)?;
            assert_eq!(preparation::repository_pipeline_config(&repository, &commit).await?, pipeline);
            let entries = tree_entries(&git_read(&repository, &["ls-tree", "-r", "-z", "--full-tree", &commit], MAX_TREE).await?)?;
            let export = root.join("export");
            create_directory(&export)?;
            export_objects(&repository, &export, &entries).await?;
            seal_directories(&export)?;
            assert_eq!(fs::read(export.join("Dockerfile"))?, dockerfile);
            assert_eq!(fs::read(export.join(".forge-ci.yml"))?, pipeline);
            assert_eq!(fs::read_dir(&export)?.count(), 2);
            assert!(missing(&export.join(".env"))? && missing(&export.join(".git"))?);
            Ok::<(), anyhow::Error>(())
        }.await;
        fs::remove_dir_all(&root).unwrap();
        result.unwrap();
    }

    #[test]
    fn image_build_source_inventory_rejects_links_traversal_and_secret_cache() {
        for path in [
            "../Dockerfile",
            "a/../../b",
            "/Dockerfile",
            ".git/config",
            ".env",
            "x/.env.production",
            "node_modules/a",
            "target/a",
            ".local/key",
            "secrets/a",
            "x/server.key",
        ] {
            assert!(tree_entries(&tree(path, "100644")).is_err());
        }
        for mode in ["120000", "160000", "040000"] {
            assert!(tree_entries(&tree("Dockerfile", mode)).is_err());
        }
        assert!(tree_entries(&tree(".env.example", "100644")).is_ok());
        assert!(tree_entries(&tree("src/main.rs", "100755")).unwrap()[0].executable);
        let mut duplicate = tree("Dockerfile", "100644");
        duplicate.extend(tree("Dockerfile", "100644"));
        assert!(tree_entries(&duplicate).is_err());
        assert!(tree_entries(b"truncated").is_err());
    }

    #[test]
    fn image_build_export_checks_actual_git_blob_bytes_and_header() {
        let oid = "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391";
        assert!(verify_blob(b"", oid).is_ok());
        assert!(verify_blob(b"changed", oid).is_err());
        assert_eq!(
            blob_length(format!("{oid} blob 0\n").as_bytes(), oid).unwrap(),
            0
        );
        for suffix in [
            "tree 0\n",
            "blob 00\n",
            "blob -1\n",
            "blob 134217729\n",
            "missing\n",
        ] {
            assert!(blob_length(format!("{oid} {suffix}").as_bytes(), oid).is_err());
        }
        assert!(blob_length(format!("{} blob 0\n", "b".repeat(40)).as_bytes(), oid).is_err());
    }

    #[test]
    fn image_build_unknown_cancel_timeout_and_cli_failure_are_not_cleanable_failure() {
        for (exit, reason) in [
            (BuildExit::Unknown, BuildHoldReason::Unknown),
            (BuildExit::Cancelled, BuildHoldReason::Cancelled),
            (BuildExit::TimedOut, BuildHoldReason::TimedOut),
            (BuildExit::Failed, BuildHoldReason::CliFailed),
        ] {
            assert_eq!(hold_reason(exit), Some(reason));
        }
        assert_eq!(hold_reason(BuildExit::Success), None);
    }

    #[test]
    fn image_build_dockerfile_is_relative_inside_sealed_context() {
        let spec = ImageBuildSpec {
            context: "app".into(),
            dockerfile: "docker/Dockerfile".into(),
            target: Some("release".into()),
            descriptor_artifact: "delivery/candidate.json".into(),
            readable_schema_versions: vec![1],
        };
        assert!(
            require_dockerfile(
                &tree_entries(&tree("app/docker/Dockerfile", "100644")).unwrap(),
                &spec
            )
            .is_ok()
        );
        assert!(
            require_dockerfile(
                &tree_entries(&tree("docker/Dockerfile", "100644")).unwrap(),
                &spec
            )
            .is_err()
        );
        assert_eq!(
            context_path(Path::new("/control"), &spec),
            Path::new("/control/tree/app")
        );
    }

    #[test]
    fn image_build_server_scope_has_no_nil_or_unfenced_attempt() {
        let valid = ImageBuildScope {
            project_id: Uuid::new_v4(),
            pipeline_id: Uuid::new_v4(),
            job_id: Uuid::new_v4(),
            attempt_id: Uuid::new_v4(),
            lease_id: Uuid::new_v4(),
            generation: 1,
        };
        assert!(valid.validate().is_ok());
        for field in [
            "project_id",
            "pipeline_id",
            "job_id",
            "attempt_id",
            "lease_id",
        ] {
            let mut value = serde_json::to_value(&valid).unwrap();
            value[field] = serde_json::json!(Uuid::nil());
            assert!(
                serde_json::from_value::<ImageBuildScope>(value)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        let mut wrong = valid;
        wrong.generation = 0;
        assert!(wrong.validate().is_err());
    }

    #[cfg(unix)]
    #[test]
    fn image_build_export_preserves_copy_modes_but_parent_control_stays_private() {
        use std::os::unix::fs::MetadataExt;
        let root = std::env::temp_dir().join(format!("forge-image-build-mode-{}", Uuid::new_v4()));
        create_directory(&root).unwrap();
        let result = (|| -> anyhow::Result<()> {
            let tree = root.join("tree");
            create_directory(&tree)?;
            write_new(&tree.join("Dockerfile"), b"FROM scratch", 0o644)?;
            write_new(&tree.join("entrypoint"), b"#!/bin/sh\n", 0o755)?;
            seal_directories(&tree)?;
            assert_eq!(fs::metadata(&tree)?.mode() & 0o777, 0o755);
            assert_eq!(fs::metadata(tree.join("Dockerfile"))?.mode() & 0o777, 0o644);
            assert_eq!(fs::metadata(tree.join("entrypoint"))?.mode() & 0o777, 0o755);
            assert_eq!(fs::metadata(&root)?.mode() & 0o777, 0o700);
            // A Docker-created iidfile may be 0644 inside this private parent, but never a hardlink.
            write_new(&root.join("image-id"), b"sha256:nonsecret", 0o644)?;
            assert!(read_private(&root.join("image-id"), 80).is_err());
            assert_eq!(
                read_owned(&root.join("image-id"), 80, false)?,
                b"sha256:nonsecret"
            );
            Ok(())
        })();
        fs::remove_dir_all(&root).unwrap();
        result.unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn image_build_retry_retains_partial_started_completed_and_foreign_custody() {
        let root = std::env::temp_dir().join(format!("forge-image-build-retry-{}", Uuid::new_v4()));
        create_directory(&root).unwrap();
        let job = Uuid::new_v4();
        let attempt = Uuid::new_v4();
        let result = (|| -> anyhow::Result<()> {
            assert!(!retry_unconfirmed(&root, job, attempt)?);
            let directory = root.join(format!("image-build-{}", attempt.simple()));
            create_directory(&directory)?;
            assert!(retry_unconfirmed(&root, job, attempt)?);
            let intent = Intent {
                schema: "forge/backend-image-build/v1".into(),
                scope: ImageBuildScope {
                    project_id: Uuid::new_v4(),
                    pipeline_id: Uuid::new_v4(),
                    job_id: job,
                    attempt_id: attempt,
                    lease_id: Uuid::new_v4(),
                    generation: 1,
                },
                repository_id: Uuid::new_v4(),
                source_commit: "a".repeat(40),
                spec: ImageBuildSpec {
                    context: ".".into(),
                    dockerfile: "Dockerfile".into(),
                    target: None,
                    descriptor_artifact: "candidate.json".into(),
                    readable_schema_versions: vec![1],
                },
                daemon_id: "same-daemon".into(),
            };
            let bytes = serde_json::to_vec(&intent)?;
            write_new(&directory.join("intent.json"), &bytes, 0o600)?;
            assert!(retry_unconfirmed(&root, job, attempt)?);
            assert!(retry_unconfirmed(&root, Uuid::new_v4(), attempt).is_err());
            write_new(
                &directory.join("started.json"),
                digest(&bytes).as_bytes(),
                0o600,
            )?;
            assert!(
                write_new(
                    &directory.join("started.json"),
                    digest(&bytes).as_bytes(),
                    0o600
                )
                .is_err()
            );
            assert!(retry_unconfirmed(&root, job, attempt)?);
            write_new(
                &directory.join("completed.json"),
                b"not a DB completion/stop acknowledgement",
                0o600,
            )?;
            assert!(retry_unconfirmed(&root, job, attempt)?);
            Ok(())
        })();
        fs::remove_dir_all(&root).unwrap();
        result.unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn image_build_private_custody_is_create_new_and_partial_attempt_never_reopens() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let root = std::env::temp_dir().join(format!("forge-image-build-test-{}", Uuid::new_v4()));
        create_directory(&root).unwrap();
        let result = (|| -> anyhow::Result<()> {
            let path = root.join("intent.json");
            write_new(&path, b"original", 0o600)?;
            assert!(write_new(&path, b"replacement", 0o600).is_err());
            assert_eq!(read_private(&path, 32)?, b"original");
            let alias = root.join("alias");
            symlink(&path, &alias)?;
            assert!(read_private(&alias, 32).is_err());
            fs::remove_file(&alias)?;
            fs::hard_link(&path, &alias)?;
            assert!(read_private(&path, 32).is_err());
            fs::remove_file(&alias)?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;
            assert!(read_private(&path, 32).is_err());
            assert!(create_directory(&root).is_err());
            Ok(())
        })();
        fs::remove_dir_all(&root).unwrap();
        result.unwrap();
    }
}
