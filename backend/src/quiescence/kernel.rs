//! Forge-local effect admission for a coherent backup. Kernel leases, never expiry, prove quiescence.
//! The installation owner must pin the complete participant set and its runtime identities.
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use uuid::Uuid;
#[path = "generation.rs"]
mod generation;
pub use generation::{
    AdoptionHandoff, CustodyFile, RetiredCompose, RootMapping, adoption_root_mappings,
    adoption_status, custody_files, inherited_compose, load_adoption_handoff, retired_compose_all,
    retired_compose_binding, wait_for_enrollment,
};

#[derive(Clone)]
pub struct Gate {
    root: PathBuf,
}
pub struct Lease {
    _file: Option<File>,
    record: Option<PathBuf>,
    tracked: bool,
}
impl Lease {
    pub(crate) fn workspace_effect_id(&self) -> Option<Uuid> {
        self.record.as_ref()?.file_stem()?.to_str()?.parse().ok()
    }
    /// The private workspace operator and every native descendant retain the
    /// same kernel effect lease if their backend waiter disappears.
    pub fn inherit_workspace_operator(
        &self,
        command: &mut std::process::Command,
    ) -> anyhow::Result<()> {
        #[cfg(unix)]
        if let Some(file) = &self._file {
            use std::os::{
                fd::{AsRawFd, FromRawFd, OwnedFd},
                unix::process::CommandExt,
            };
            let duplicate = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 200) };
            ensure!(duplicate >= 0, "effect descriptor duplication unavailable");
            let descriptor = unsafe { OwnedFd::from_raw_fd(duplicate) };
            unsafe {
                command.pre_exec(move || {
                    if libc::dup2(descriptor.as_raw_fd(), 196) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            command.env("PDLC_FORGE_EFFECT_FD", "196");
            if let Some(record) = &self.record {
                command.env(
                    "PDLC_FORGE_EFFECT_ID",
                    record.file_stem().context("effect identity missing")?,
                );
            }
        }
        #[cfg(not(unix))]
        {
            let _ = command;
            anyhow::bail!("workspace operator requires Linux custody");
        }
        Ok(())
    }
}

/// Called only after the matching private operation journal's inherited kernel
/// lock is acquired. This retires that original effect record, never its lock.
pub(crate) fn retire_workspace_effect(id: Uuid) -> anyhow::Result<()> {
    let Some(gate) = Gate::from_env()? else {
        return Ok(());
    };
    ensure!(!id.is_nil(), "effect identity required");
    let path = gate.root.join("effects").join(format!("{id}.json"));
    if !path.try_exists()? {
        return Ok(());
    }
    private(&path, false)?;
    let record: serde_json::Value = read(&path)?;
    ensure!(
        record["effectId"] == id.to_string(),
        "original effect identity mismatch"
    );
    fs::remove_file(path)?;
    Ok(())
}
impl Drop for Lease {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            if let Some(path) = &self.record {
                let _ = fs::remove_file(path);
            }
        }
        if self.tracked {
            ACTIVE.fetch_sub(1, Ordering::SeqCst);
        }
    }
}
pub struct Participant {
    _file: File,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustodyRoot {
    pub kind: String,
    pub path: PathBuf,
}
impl CustodyRoot {
    pub fn workspace(path: PathBuf) -> Self {
        Self {
            kind: "workspace".into(),
            path,
        }
    }
    pub fn delivery(path: PathBuf) -> Self {
        Self {
            kind: "local-delivery".into(),
            path,
        }
    }
    pub fn git(path: PathBuf) -> Self {
        Self {
            kind: "git".into(),
            path,
        }
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Identity {
    pub schema_version: u32,
    pub participant: String,
    pub boot_id: Uuid,
    pub kind: String,
    pub process_id: u32,
    pub process_start: String,
    pub custody_roots: Vec<CustodyRoot>,
}
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub operation_id: Uuid,
}
pub struct Coordinator {
    gate: Gate,
    _owner: File,
    operation: Uuid,
    nonce: Uuid,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ControlOwner {
    schema_version: u32,
    operation_id: Uuid,
    coordinator_id: Uuid,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Held {
    schema_version: u32,
    operation_id: Uuid,
    coordinator_id: Uuid,
    acknowledgement_id: Uuid,
    participants: Vec<Identity>,
}
pub struct Fence {
    _lease: File,
}
pub fn target_idle(path: &Path) -> anyhow::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        meta.is_file() && !meta.file_type().is_symlink(),
        "target lock identity unavailable"
    );
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        ensure!(
            meta.uid() == unsafe { libc::geteuid() },
            "target lock owner differs"
        );
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let f = options.open(path)?;
    ensure!(lock(&f, true)?, "native target owner still running");
    Ok(())
}
static PARTICIPANT: OnceLock<Participant> = OnceLock::new();
static IDENTITY: OnceLock<Identity> = OnceLock::new();
static ENTRY: Mutex<()> = Mutex::new(());
static EXITING: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

fn safe_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 80
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
#[cfg(unix)]
fn private(path: &Path, directory: bool) -> anyhow::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let m = fs::symlink_metadata(path)?;
    ensure!(
        !m.file_type().is_symlink() && if directory { m.is_dir() } else { m.is_file() },
        "quiescence path type/link rejected"
    );
    ensure!(
        m.uid() == unsafe { libc::geteuid() } && m.mode() & 0o077 == 0,
        "quiescence path must be owner-only"
    );
    Ok(())
}
#[cfg(not(unix))]
fn private(_: &Path, _: bool) -> anyhow::Result<()> {
    anyhow::bail!("quiescence requires Unix kernel custody")
}
fn file(path: &Path) -> anyhow::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let f = options.open(path)?;
    private(path, false)?;
    Ok(f)
}
fn lock(file: &File, exclusive: bool) -> anyhow::Result<bool> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let mode = if exclusive {
            libc::LOCK_EX
        } else {
            libc::LOCK_SH
        };
        if unsafe { libc::flock(file.as_raw_fd(), mode | libc::LOCK_NB) } == 0 {
            return Ok(true);
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::WouldBlock {
            return Ok(false);
        }
        Err(error.into())
    }
    #[cfg(not(unix))]
    {
        let _ = (file, exclusive);
        anyhow::bail!("quiescence requires flock")
    }
}
fn write_new(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut f = options.open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    File::open(path.parent().context("parent missing")?)?.sync_all()?;
    Ok(())
}
fn replace(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if path.try_exists()? {
        private(path, false)?;
    }
    let temp = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
    write_new(&temp, bytes)?;
    fs::rename(&temp, path)?;
    File::open(path.parent().context("control parent missing")?)?.sync_all()?;
    Ok(())
}
fn read<T: serde::de::DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    private(path, false)?;
    ensure!(
        fs::metadata(path)?.len() <= 32768,
        "quiescence record oversized"
    );
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
impl Gate {
    pub fn open(root: &Path) -> anyhow::Result<Self> {
        ensure!(root.is_absolute(), "absolute quiescence directory required");
        for parent in root.ancestors() {
            if parent.exists() {
                ensure!(
                    !fs::symlink_metadata(parent)?.file_type().is_symlink(),
                    "linked quiescence ancestor"
                );
            }
        }
        fs::create_dir_all(root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if fs::read_dir(root)?.next().is_none() {
                fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
            }
        }
        private(root, true)?;
        let peers = root.join("participants");
        if !peers.exists() {
            fs::create_dir(&peers)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&peers, fs::Permissions::from_mode(0o700))?;
            }
        }
        private(&peers, true)?;
        Ok(Self {
            root: root.canonicalize()?,
        })
    }
    pub fn from_env() -> anyhow::Result<Option<Self>> {
        std::env::var_os("CICD_QUIESCENCE_DIR")
            .map(|p| Self::open(Path::new(&p)))
            .transpose()
    }
    pub fn git_hook_owner(&self, storage: &str) -> anyhow::Result<Lease> {
        let f = file(&hook_path(self, storage)?)?;
        ensure!(lock(&f, false)?, "Git callback custody unavailable");
        Ok(Lease {
            _file: Some(f),
            record: None,
            tracked: false,
        })
    }
    pub fn require_git_hook_owner(&self, storage: &str) -> anyhow::Result<()> {
        if self.requested()? {
            let path = hook_path(self, storage)?;
            ensure!(path.try_exists()?, "no admitted Git owner");
            let f = file(&path)?;
            ensure!(!lock(&f, true)?, "no live Git owner");
        }
        Ok(())
    }
    pub fn requested(&self) -> anyhow::Result<bool> {
        Ok(self.requested_operation()?.is_some())
    }
    fn requested_operation(&self) -> anyhow::Result<Option<Uuid>> {
        match fs::symlink_metadata(self.root.join("request.json")) {
            Ok(_) => {
                let r: Request = read(&self.root.join("request.json"))?;
                ensure!(
                    r.schema_version == 1 && !r.operation_id.is_nil(),
                    "invalid drain request"
                );
                Ok(Some(r.operation_id))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    // Only process initialization may continue under its explicitly configured
    // operation. The shared kernel lease still excludes a held backup fence.
    fn enter_startup(&self, operation: Uuid) -> anyhow::Result<Lease> {
        ensure!(!operation.is_nil(), "startup operation identity required");
        match self.requested_operation()? {
            None => self.enter(true),
            Some(request) => {
                ensure!(
                    request == operation,
                    "Forge startup belongs to another drain operation"
                );
                let lease = self.enter(false)?;
                ensure!(
                    self.requested_operation()?
                        .is_none_or(|request| request == operation),
                    "Forge startup drain operation changed"
                );
                Ok(lease)
            }
        }
    }
    pub fn enter(&self, new_work: bool) -> anyhow::Result<Lease> {
        generation::admission(self, IDENTITY.get())?;
        if new_work {
            ensure!(!self.requested()?, "Forge draining: new work closed")
        }
        let f = file(&self.root.join("effects.lock"))?;
        ensure!(lock(&f, false)?, "Forge backup fence held");
        // The request can win between the first read and the kernel lease.
        if new_work {
            ensure!(!self.requested()?, "Forge draining: new work closed")
        }
        let directory = self.root.join("effects");
        if !directory.exists() {
            fs::create_dir(&directory)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
            }
        }
        private(&directory, true)?;
        let id = Uuid::new_v4();
        let record = directory.join(format!("{id}.json"));
        write_new(
            &record,
            &serde_json::to_vec(
                &serde_json::json!({"schemaVersion":1,"effectId":id,"processId":std::process::id()}),
            )?,
        )?;
        Ok(Lease {
            _file: Some(f),
            record: Some(record),
            tracked: false,
        })
    }
    pub fn verify_no_effect_records(&self, _fence: &Fence) -> anyhow::Result<()> {
        let directory = self.root.join("effects");
        if directory.try_exists()? {
            private(&directory, true)?;
            ensure!(
                fs::read_dir(directory)?.next().is_none(),
                "orphaned/panicked effect custody remains; original-owner reconciliation required"
            );
        }
        Ok(())
    }
    pub fn register(
        &self,
        id: &str,
        kind: &str,
        custody_roots: Vec<CustodyRoot>,
    ) -> anyhow::Result<(Participant, Identity)> {
        // A restarted container must not replace participant boot evidence
        // while the owner holds the exclusive backup/exit fence.
        let registration = file(&self.root.join("effects.lock"))?;
        ensure!(lock(&registration, false)?, "Forge backup fence held during registration");
        generation::validate_registration(self, id, kind, &custody_roots)?;
        ensure!(
            safe_name(id) && safe_name(kind),
            "invalid participant identity"
        );
        let live = file(&self.root.join("participants").join(format!("{id}.lock")))?;
        ensure!(lock(&live, true)?, "duplicate live Forge participant");
        let stat = fs::read_to_string("/proc/self/stat")?;
        let start = stat
            .rsplit_once(") ")
            .context("process identity unavailable")?
            .1
            .split_whitespace()
            .nth(19)
            .context("process start unavailable")?
            .to_owned();
        let identity = Identity {
            schema_version: 1,
            participant: id.into(),
            boot_id: Uuid::new_v4(),
            kind: kind.into(),
            process_id: std::process::id(),
            process_start: start,
            custody_roots,
        };
        let path = self.root.join("participants").join(format!("{id}.json"));
        if path.exists() {
            private(&path, false)?;
            fs::remove_file(&path)?;
        }
        write_new(&path, &serde_json::to_vec(&identity)?)?;
        Ok((Participant { _file: live }, identity))
    }
    pub fn verify_participants(&self, expected: &[Identity]) -> anyhow::Result<()> {
        ensure!(
            !expected.is_empty() && expected.len() <= 64,
            "complete bounded participant manifest required"
        );
        let mut names = std::collections::BTreeSet::new();
        for identity in expected {
            ensure!(
                identity.schema_version == 1
                    && safe_name(&identity.participant)
                    && names.insert(identity.participant.clone()),
                "invalid/duplicate expected participant"
            );
            let stem = self.root.join("participants").join(&identity.participant);
            let actual: Identity = read(&stem.with_extension("json"))?;
            ensure!(&actual == identity, "participant boot identity changed");
            let live = file(&stem.with_extension("lock"))?;
            ensure!(
                !lock(&live, true)?,
                "participant registration is stale; no live kernel owner"
            );
        }
        let mut count = 0;
        for entry in fs::read_dir(self.root.join("participants"))? {
            let entry = entry?;
            count += 1;
            ensure!(count <= 128, "participant inventory overflow");
            let p = entry.path();
            if p.extension().is_some_and(|v| v == "lock") {
                let id = p
                    .file_stem()
                    .and_then(|v| v.to_str())
                    .context("invalid participant file")?;
                let live = file(&p)?;
                if !lock(&live, true)? {
                    ensure!(
                        names.contains(id),
                        "unexpected live Forge writer participant"
                    );
                }
            }
        }
        Ok(())
    }
}
pub fn initialize(kind: &str, custody_roots: Vec<CustodyRoot>) -> anyhow::Result<()> {
    if let Some(gate) = Gate::from_env()? {
        let id = std::env::var("CICD_QUIESCENCE_PARTICIPANT")
            .context("configured quiescence requires pinned participant name")?;
        let (participant, identity) = gate.register(&id, kind, custody_roots)?;
        ensure!(
            IDENTITY.set(identity).is_ok(),
            "Forge identity already initialized"
        );
        ensure!(
            PARTICIPANT.set(participant).is_ok(),
            "Forge participant already initialized"
        );
    }
    Ok(())
}
pub fn enter(new_work: bool) -> anyhow::Result<Lease> {
    enter_effect(new_work, None)
}
/// Initialize a server/runner under an operator-owned closed admission latch.
/// Ordinary work must continue to use enter(true), including delivery commands.
pub fn enter_startup() -> anyhow::Result<Lease> {
    let operation = match std::env::var("CICD_QUIESCENCE_STARTUP_OPERATION") {
        Ok(value) => Some(startup_operation(&value)?),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => return Err(error.into()),
    };
    enter_effect(true, operation)
}
fn startup_operation(value: &str) -> anyhow::Result<Uuid> {
    let operation = Uuid::parse_str(value).context("invalid Forge startup operation")?;
    ensure!(
        !operation.is_nil() && operation.to_string() == value,
        "canonical nonnil Forge startup operation required"
    );
    Ok(operation)
}
fn enter_effect(new_work: bool, startup: Option<Uuid>) -> anyhow::Result<Lease> {
    let _entry = ENTRY
        .lock()
        .map_err(|_| anyhow::anyhow!("effect entry unavailable"))?;
    ensure!(
        !EXITING.load(Ordering::SeqCst),
        "Forge controlled idle exit in progress"
    );
    match Gate::from_env()? {
        Some(gate) => {
            ensure!(
                PARTICIPANT.get().is_some(),
                "Forge writer has no live participant registration"
            );
            let mut lease = match startup {
                Some(operation) => gate.enter_startup(operation)?,
                None => gate.enter(new_work)?,
            };
            ACTIVE.fetch_add(1, Ordering::SeqCst);
            lease.tracked = true;
            Ok(lease)
        }
        None => {
            ensure!(
                startup.is_none(),
                "Forge startup operation requires a quiescence directory"
            );
            Ok(Lease {
                _file: None,
                record: None,
                tracked: false,
            })
        }
    }
}
pub fn accepting() -> bool {
    Gate::from_env().is_ok_and(|gate| gate.is_none_or(|gate| gate.requested().is_ok_and(|r| !r)))
}
impl Coordinator {
    pub fn request(gate: Gate, operation: Uuid) -> anyhow::Result<Self> {
        ensure!(!operation.is_nil(), "operation identity required");
        let owner = file(&gate.root.join("coordinator.lock"))?;
        ensure!(
            lock(&owner, true)?,
            "another coordinator owns release authority"
        );
        let path = gate.root.join("request.json");
        if path.exists() {
            let r: Request = read(&path)?;
            ensure!(
                r.schema_version == 1 && r.operation_id == operation,
                "drain operation differs; refusing takeover/release"
            );
        } else {
            write_new(
                &path,
                &serde_json::to_vec(&Request {
                    schema_version: 1,
                    operation_id: operation,
                })?,
            )?;
        }
        let nonce = Uuid::new_v4();
        replace(
            &gate.root.join("coordinator-current.json"),
            &serde_json::to_vec(&ControlOwner {
                schema_version: 1,
                operation_id: operation,
                coordinator_id: nonce,
            })?,
        )?;
        Ok(Self {
            gate,
            _owner: owner,
            operation,
            nonce,
        })
    }
    pub fn try_fence(&self) -> anyhow::Result<Option<Fence>> {
        let r: Request = read(&self.gate.root.join("request.json"))?;
        ensure!(r.operation_id == self.operation, "drain operation changed");
        let f = file(&self.gate.root.join("effects.lock"))?;
        Ok(lock(&f, true)?.then_some(Fence { _lease: f }))
    }

    /// Caller has just verified all DB/native custody while this exact fence remains held.
    pub fn publish_held(&self, _fence: &Fence, expected: &[Identity]) -> anyhow::Result<()> {
        self.gate.verify_participants(expected)?;
        self.gate.verify_no_effect_records(_fence)?;
        replace(
            &self.gate.root.join("held.json"),
            &serde_json::to_vec(&Held {
                schema_version: 1,
                operation_id: self.operation,
                coordinator_id: self.nonce,
                acknowledgement_id: Uuid::new_v4(),
                participants: expected.to_vec(),
            })?,
        )?;
        Ok(())
    }
    pub fn request_exit(&self, _fence: &Fence, expected: &[Identity]) -> anyhow::Result<()> {
        self.gate.verify_participants(expected)?;
        self.gate.verify_no_effect_records(_fence)?;
        ensure!(
            expected.iter().all(|i| matches!(
                i.kind.as_str(),
                "server" | "external-runner" | "local-delivery" | "migration"
            )),
            "participant lacks controlled idle exit support; HOLD"
        );
        let held: Held = read(&self.gate.root.join("held.json"))?;
        ensure!(
            held.schema_version == 1
                && held.operation_id == self.operation
                && held.coordinator_id == self.nonce
                && held.participants == expected,
            "full held acknowledgement is stale or foreign"
        );
        replace(
            &self.gate.root.join("exit.json"),
            &serde_json::to_vec(&held)?,
        )?;
        Ok(())
    }
    pub fn gate(&self) -> &Gate {
        &self.gate
    }
    /// Only the live coordinator, after dropping its fence, can explicitly resume its own operation.
    pub fn resume(self) -> anyhow::Result<()> {
        generation::standalone_resume(&self.gate)?;
        self.remove_request()
    }
    fn remove_request(self) -> anyhow::Result<()> {
        let r: Request = read(&self.gate.root.join("request.json"))?;
        ensure!(
            r.operation_id == self.operation,
            "foreign drain release rejected"
        );
        fs::remove_file(self.gate.root.join("request.json"))?;
        File::open(&self.gate.root)?.sync_all()?;
        Ok(())
    }
}

pub fn git_hook_owner(storage: &str) -> anyhow::Result<Lease> {
    let Some(gate) = Gate::from_env()? else {
        return Ok(Lease {
            _file: None,
            record: None,
            tracked: false,
        });
    };
    gate.git_hook_owner(storage)
}
fn hook_path(gate: &Gate, storage: &str) -> anyhow::Result<PathBuf> {
    use sha2::{Digest, Sha256};
    ensure!(
        !storage.is_empty() && storage.len() <= 256,
        "invalid Git storage identity"
    );
    let dir = gate.root.join("git-hooks");
    if !dir.exists() {
        fs::create_dir(&dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
        }
    }
    private(&dir, true)?;
    Ok(dir.join(format!("{:x}.lock", Sha256::digest(storage.as_bytes()))))
}
pub fn require_git_hook_owner(storage: &str) -> anyhow::Result<()> {
    if let Some(gate) = Gate::from_env()? {
        gate.require_git_hook_owner(storage)?;
    }
    Ok(())
}

impl Gate {
    pub fn exit_for(&self, identity: &Identity) -> anyhow::Result<bool> {
        if !self.root.join("exit.json").try_exists()? {
            return Ok(false);
        }
        let request: Request = read(&self.root.join("request.json"))?;
        let owner: ControlOwner = read(&self.root.join("coordinator-current.json"))?;
        let held: Held = read(&self.root.join("held.json"))?;
        let exit: Held = read(&self.root.join("exit.json"))?;
        ensure!(
            request.schema_version == 1
                && owner.schema_version == 1
                && held.schema_version == 1
                && exit.schema_version == 1
                && !request.operation_id.is_nil()
                && !held.coordinator_id.is_nil()
                && !held.acknowledgement_id.is_nil()
                && request.operation_id == held.operation_id
                && exit.operation_id == held.operation_id
                && owner.operation_id == held.operation_id
                && exit.coordinator_id == held.coordinator_id
                && owner.coordinator_id == held.coordinator_id
                && exit.acknowledgement_id == held.acknowledgement_id
                && exit.participants == held.participants,
            "stale/foreign controlled exit intent"
        );
        ensure!(
            held.participants.iter().any(|i| i == identity),
            "controlled exit targets a different process boot"
        );
        let coordinator = file(&self.root.join("coordinator.lock"))?;
        ensure!(
            !lock(&coordinator, true)?,
            "controlled exit coordinator disappeared"
        );
        let effects = file(&self.root.join("effects.lock"))?;
        ensure!(
            !lock(&effects, false)?,
            "controlled exit lacks live exclusive backup fence"
        );
        Ok(true)
    }
    /// Kernel ownership disappearance is checked only for exactly pinned, unchanged boot receipts.
    pub fn all_exited(&self, expected: &[Identity]) -> anyhow::Result<bool> {
        ensure!(
            !expected.is_empty() && expected.len() <= 64,
            "bounded exact exit participants required"
        );
        let mut names = std::collections::BTreeSet::new();
        let mut alive = 0;
        for identity in expected {
            ensure!(
                safe_name(&identity.participant) && names.insert(identity.participant.clone()),
                "invalid/duplicate exit identity"
            );
            let stem = self.root.join("participants").join(&identity.participant);
            let actual: Identity = read(&stem.with_extension("json"))?;
            ensure!(
                &actual == identity,
                "exit participant boot replaced or receipt changed"
            );
            let f = file(&stem.with_extension("lock"))?;
            if !lock(&f, true)? {
                alive += 1;
            }
        }
        let mut count = 0;
        for entry in fs::read_dir(self.root.join("participants"))? {
            count += 1;
            ensure!(count <= 128, "exit inventory overflow");
            let path = entry?.path();
            if path.extension().is_some_and(|v| v == "lock") {
                let id = path
                    .file_stem()
                    .and_then(|v| v.to_str())
                    .context("exit participant file malformed")?;
                let f = file(&path)?;
                if !lock(&f, true)? {
                    ensure!(
                        names.contains(id),
                        "new writer appeared during controlled exit"
                    );
                }
            }
        }
        Ok(alive == 0)
    }
}
impl Drop for Coordinator {
    fn drop(&mut self) {
        let path = self.gate.root.join("held.json");
        if let Ok(held) = read::<Held>(&path) {
            if held.coordinator_id == self.nonce {
                let _ = fs::remove_file(path);
            }
        }
    }
}
/// No signal or timeout fallback. The admission latch and empty local owner set are checked atomically.
pub async fn wait_for_exit() -> anyhow::Result<()> {
    loop {
        let should_exit = (|| -> anyhow::Result<bool> {
            let _entry = ENTRY
                .lock()
                .map_err(|_| anyhow::anyhow!("effect entry unavailable"))?;
            if EXITING.load(Ordering::SeqCst) {
                return Ok(true);
            }
            let Some(identity) = IDENTITY.get() else {
                return Ok(false);
            };
            if ACTIVE.load(Ordering::SeqCst) != 0 {
                return Ok(false);
            }
            let Some(gate) = Gate::from_env()? else {
                return Ok(false);
            };
            if gate.exit_for(identity)? {
                EXITING.store(true, Ordering::SeqCst);
                return Ok(true);
            }
            Ok(false)
        })();
        if matches!(should_exit, Ok(true)) {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

#[cfg(test)]
mod closed_startup_tests {
    use super::*;

    fn gate() -> (tempfile::TempDir, Gate) {
        let temp = tempfile::tempdir().unwrap();
        let gate = Gate::open(&temp.path().join("control")).unwrap();
        (temp, gate)
    }
    fn request(gate: &Gate, operation_id: Uuid) {
        replace(
            &gate.root.join("request.json"),
            &serde_json::to_vec(&Request {
                schema_version: 1,
                operation_id,
            })
            .unwrap(),
        )
        .unwrap();
    }
    #[test]
    fn matching_startup_keeps_new_work_closed() {
        let (_temp, gate) = gate();
        let operation = Uuid::new_v4();
        request(&gate, operation);
        let startup = gate.enter_startup(operation).unwrap();
        assert!(gate.enter(true).is_err());
        let effects = file(&gate.root.join("effects.lock")).unwrap();
        assert!(!lock(&effects, true).unwrap());
        drop(startup);
        assert!(lock(&effects, true).unwrap());
    }
    #[test]
    fn different_startup_and_existing_backup_fence_are_rejected() {
        let (_temp, gate) = gate();
        let operation = Uuid::new_v4();
        request(&gate, operation);
        assert!(gate.enter_startup(Uuid::new_v4()).is_err());
        let fence = file(&gate.root.join("effects.lock")).unwrap();
        assert!(lock(&fence, true).unwrap());
        assert!(gate.enter_startup(operation).is_err());
    }
    #[test]
    fn absent_request_preserves_ordinary_startup_and_future_drain_blocks_it() {
        let (_temp, gate) = gate();
        let operation = Uuid::new_v4();
        drop(gate.enter_startup(operation).unwrap());
        request(&gate, Uuid::new_v4());
        assert!(gate.enter_startup(operation).is_err());
        fs::remove_file(gate.root.join("request.json")).unwrap();
        drop(gate.enter_startup(operation).unwrap());
    }
    #[test]
    fn held_fence_rejects_registration_before_boot_receipt_is_replaced() {
        let (_temp, gate) = gate();
        let fence = file(&gate.root.join("effects.lock")).unwrap();
        assert!(lock(&fence, true).unwrap());
        assert!(gate.register("restarted", "server", vec![]).is_err());
        assert!(!gate.root.join("participants/restarted.json").exists());
    }

    #[test]
    fn startup_operation_requires_exact_nonnil_uuid() {
        let operation = Uuid::new_v4();
        assert_eq!(
            startup_operation(&operation.to_string()).unwrap(),
            operation
        );
        for value in [
            String::new(),
            operation.simple().to_string(),
            operation.to_string().to_uppercase(),
            Uuid::nil().to_string(),
            format!(" {operation}"),
        ] {
            assert!(startup_operation(&value).is_err());
        }
    }
}
