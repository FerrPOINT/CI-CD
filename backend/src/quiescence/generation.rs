//! Explicit owner-authorized adoption. Native CID/namespace retirement is verified by Base, not JSON.
use super::*;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceFile {
    pub path: PathBuf,
    pub sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetiredProcess {
    pub participant: String,
    pub boot_id: Uuid,
    pub container_id: String,
    pub image_config_digest: String,
    pub process_id: u32,
    pub process_start: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DestinationIdentity {
    pub installation_sha256: String,
    pub namespace: String,
    pub namespace_uid: Uuid,
    pub workspace_claim_uid: Uuid,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RootMapping {
    pub source: CustodyRoot,
    pub destination: CustodyRoot,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Enrollment {
    pub source_participants: Vec<String>,
    pub participant: String,
    pub kind: String,
    pub roots: Vec<RootMapping>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PriorGeneration {
    pub generation_id: Uuid,
    pub handoff_sha256: String,
    pub state_sha256: String,
    pub archive_catalog_sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdoptionHandoff {
    pub schema_version: u32,
    pub schema: String,
    pub operation_id: Uuid,
    pub generation_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prior_generation: Option<PriorGeneration>,
    pub source_exited_receipt: EvidenceFile,
    pub retired_processes: Vec<RetiredProcess>,
    pub destination: DestinationIdentity,
    pub participants: Vec<Enrollment>,
    pub source_docker_daemon_id: Option<String>,
    pub source_workspace_volume: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExitedReceipt {
    schema: String,
    operation_id: Uuid,
    participants: Vec<Identity>,
    all_pinned_kernel_owners_exited: bool,
    exclusive_fence_still_held: bool,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustodyFile {
    pub path: PathBuf,
    pub sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetiredCompose {
    pub root: PathBuf,
    pub source_root: PathBuf,
    pub name: String,
    pub files: Vec<CustodyFile>,
    pub daemon_id: String,
    pub workspace_volume: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SnapshotFile {
    path: PathBuf,
    sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Snapshot {
    files: Vec<SnapshotFile>,
    directories: Vec<PathBuf>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Adoption {
    schema_version: u32,
    operation_id: Uuid,
    generation_id: Uuid,
    handoff_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    prior_generation: Option<PriorGeneration>,
    phase: String,
    archive_sha256: String,
    basis_sha256: String,
    control_sha256: String,
    root_inode: u64,
    coordinator_inode: u64,
    effects_inode: u64,
    retired_compose: Vec<RetiredCompose>,
    enrolled: Vec<Identity>,
}
fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn supported(s: &str) -> bool {
    matches!(
        s,
        "server" | "external-runner" | "local-delivery" | "migration"
    )
}
fn generation_root(gate: &Gate, generation: Uuid) -> PathBuf {
    gate.root.join("generations").join(generation.to_string())
}
fn inode(path: &Path) -> anyhow::Result<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(fs::symlink_metadata(path)?.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        anyhow::bail!("Unix inode identity required")
    }
}
fn directory(path: &Path) -> anyhow::Result<()> {
    if !path.try_exists()? {
        fs::create_dir(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        }
        File::open(path.parent().context("archive parent missing")?)?.sync_all()?;
    }
    private(path, true)
}
fn relative(path: &Path) -> anyhow::Result<()> {
    use std::path::Component;
    ensure!(
        !path.as_os_str().is_empty()
            && path.components().all(|c| matches!(c, Component::Normal(_))),
        "noncanonical archived path"
    );
    Ok(())
}
fn bounded_bytes(path: &Path) -> anyhow::Result<Vec<u8>> {
    private(path, false)?;
    let before = fs::symlink_metadata(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(before.nlink() == 1, "linked adoption evidence");
    }
    ensure!(
        before.len() <= 1024 * 1024,
        "adoption evidence exceeds bound"
    );
    let bytes = fs::read(path)?;
    let after = fs::symlink_metadata(path)?;
    ensure!(
        inode(path)? == {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                before.ino()
            }
            #[cfg(not(unix))]
            {
                0
            }
        } && before.len() == after.len()
            && before.modified()? == after.modified()?,
        "adoption evidence changed while reading"
    );
    Ok(bytes)
}
fn collect(root: &Path, at: &Path, snapshot: &mut Snapshot, control: bool) -> anyhow::Result<()> {
    private(at, true)?;
    for entry in fs::read_dir(at)? {
        ensure!(
            snapshot.files.len() + snapshot.directories.len() < 8192,
            "adoption inventory overflow"
        );
        let path = entry?.path();
        let rel = path.strip_prefix(root)?.to_path_buf();
        relative(&rel)?;
        if control && at == root {
            if matches!(
                rel.to_str(),
                Some("adoption.json" | "adoption.next" | "generations")
            ) {
                continue;
            }
            ensure!(
                matches!(
                    rel.to_str(),
                    Some(
                        "participants"
                            | "effects"
                            | "git-hooks"
                            | "request.json"
                            | "coordinator-current.json"
                            | "held.json"
                            | "exit.json"
                            | "coordinator.lock"
                            | "effects.lock"
                    )
                ),
                "unknown control entry; HOLD"
            );
        }
        let meta = fs::symlink_metadata(&path)?;
        ensure!(!meta.file_type().is_symlink(), "linked archived control");
        if meta.is_dir() {
            snapshot.directories.push(rel);
            collect(root, &path, snapshot, false)?;
        } else {
            snapshot.files.push(SnapshotFile {
                path: rel,
                sha256: sha(&bounded_bytes(&path)?),
            });
        }
    }
    Ok(())
}
fn snapshot(root: &Path, control: bool) -> anyhow::Result<Snapshot> {
    let mut value = Snapshot {
        files: vec![],
        directories: vec![],
    };
    collect(root, root, &mut value, control)?;
    value.files.sort_by(|a, b| a.path.cmp(&b.path));
    value.directories.sort();
    Ok(value)
}
fn snapshot_sha(value: &Snapshot) -> anyhow::Result<String> {
    Ok(sha(&serde_json::to_vec(value)?))
}
fn save_exact(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if path.try_exists()? {
        ensure!(
            bounded_bytes(path)? == bytes,
            "archive/CAS bytes changed; HOLD"
        );
    } else {
        write_new(path, bytes)?;
    }
    Ok(())
}
fn store(gate: &Gate, state: &Adoption) -> anyhow::Result<()> {
    let next = gate.root.join("adoption.next");
    let bytes = serde_json::to_vec(state)?;
    ensure!(bytes.len() <= 32768, "adoption journal exceeds bound");
    save_exact(&next, &bytes)?;
    fs::rename(&next, gate.root.join("adoption.json"))?;
    File::open(&gate.root)?.sync_all()?;
    Ok(())
}
fn ensure_no_generation_bypass(gate: &Gate) -> anyhow::Result<()> {
    if gate.root.join("adoption.next").try_exists()? {
        let _: Adoption = read(&gate.root.join("adoption.next"))?;
    }

    if !gate.root.join("adoption.json").try_exists()?
        && gate.root.join("generations").try_exists()?
    {
        ensure!(
            fs::read_dir(gate.root.join("generations"))?
                .next()
                .is_none(),
            "archived adoption lacks its journal; HOLD"
        );
    }
    Ok(())
}
fn catalog(gate: &Gate, exclude: Uuid) -> anyhow::Result<Snapshot> {
    let base = gate.root.join("generations");
    let mut value = Snapshot {
        files: vec![],
        directories: vec![],
    };
    if !base.try_exists()? {
        return Ok(value);
    }
    private(&base, true)?;
    let pending = if gate.root.join("adoption.next").try_exists()? {
        let next: Adoption = read(&gate.root.join("adoption.next"))?;
        if next.phase == "preparing" {
            Some(next.generation_id)
        } else {
            None
        }
    } else {
        None
    };
    for entry in fs::read_dir(&base)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .context("generation archive name invalid")?;
        let generation = Uuid::parse_str(name)?;
        ensure!(
            !generation.is_nil() && generation.to_string() == name,
            "unknown generation archive"
        );
        if generation == exclude || Some(generation) == pending {
            continue;
        }
        value.directories.push(PathBuf::from(name));
        collect(&base, &path, &mut value, false)?;
    }
    value.files.sort_by(|a, b| a.path.cmp(&b.path));
    value.directories.sort();
    Ok(value)
}
fn verify_lineage(gate: &Gate, current: &Adoption) -> anyhow::Result<()> {
    let mut expected = BTreeSet::new();
    expected.insert(current.generation_id);
    let mut prior = current.prior_generation.clone();
    let mut at = current.generation_id;
    while let Some(reference) = prior {
        ensure!(
            expected.len() < 64 && expected.insert(reference.generation_id),
            "cyclic/oversized generation chain"
        );
        let raw = bounded_bytes(&generation_root(gate, at).join("prior-state.json"))?;
        ensure!(
            sha(&raw) == reference.state_sha256,
            "lineage state hash changed"
        );
        let state: Adoption = serde_json::from_slice(&raw)?;
        ensure!(
            state.phase == "active"
                && state.generation_id == reference.generation_id
                && state.handoff_sha256 == reference.handoff_sha256,
            "lineage predecessor differs"
        );
        at = state.generation_id;
        prior = state.prior_generation;
    }
    let pending = if gate.root.join("adoption.next").try_exists()? {
        let next: Adoption = read(&gate.root.join("adoption.next"))?;
        if next.phase == "preparing" {
            Some(next.generation_id)
        } else {
            None
        }
    } else {
        None
    };
    let mut actual = BTreeSet::new();
    for entry in fs::read_dir(gate.root.join("generations"))? {
        let path = entry?.path();
        private(&path, true)?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .context("lineage archive name invalid")?;
        let id = Uuid::parse_str(name)?;
        if Some(id) != pending {
            ensure!(
                id.to_string() == name && actual.insert(id),
                "unknown lineage archive"
            );
        }
    }
    ensure!(
        actual == expected,
        "unlinked/branched generation archive; HOLD"
    );
    Ok(())
}
fn verify_prior(
    gate: &Gate,
    handoff: &AdoptionHandoff,
    old: &[Identity],
    bytes: &[u8],
) -> anyhow::Result<Vec<u8>> {
    let prior = handoff
        .prior_generation
        .as_ref()
        .context("existing generation requires an exact predecessor")?;
    let (state, _) = load_inner(gate, false)?.context("predecessor state missing")?;
    ensure!(
        state.phase == "active"
            && state.generation_id == prior.generation_id
            && state.handoff_sha256 == prior.handoff_sha256
            && sha(bytes) == prior.state_sha256
            && snapshot_sha(&catalog(gate, handoff.generation_id)?)?
                == prior.archive_catalog_sha256,
        "predecessor is not the exact sealed active generation"
    );
    ensure!(
        state.enrolled.len() == old.len() && old.iter().all(|i| state.enrolled.contains(i)),
        "source-exited boots differ from predecessor enrollment"
    );
    Ok(serde_json::to_vec(&catalog(gate, handoff.generation_id)?)?)
}
pub fn adoption_status(gate: &Gate) -> anyhow::Result<serde_json::Value> {
    let (state, _) = load(gate)?.context("directory has no adopted generation")?;
    ensure!(
        state.phase == "active" && !gate.root.join("adoption.next").try_exists()?,
        "generation is not a stable active predecessor"
    );
    Ok(
        serde_json::json!({"schema":"forge/quiescence-generation/v1","generationId":state.generation_id,
        "handoffSha256":state.handoff_sha256,"stateSha256":sha(&bounded_bytes(&gate.root.join("adoption.json"))?),
        "archiveCatalogSha256":snapshot_sha(&catalog(gate,Uuid::nil())?)?,"nativeRetirementVerified":false}),
    )
}
fn basis(state: &Adoption) -> anyhow::Result<Vec<u8>> {
    let mut value = serde_json::json!({"schemaVersion":state.schema_version,"operationId":state.operation_id,"generationId":state.generation_id,
        "handoffSha256":state.handoff_sha256,"archiveSha256":state.archive_sha256,"controlSha256":state.control_sha256,
        "rootInode":state.root_inode,"coordinatorInode":state.coordinator_inode,"effectsInode":state.effects_inode,"retiredCompose":state.retired_compose});
    if let Some(prior) = &state.prior_generation {
        value["priorGeneration"] = serde_json::to_value(prior)?;
    }
    Ok(serde_json::to_vec(&value)?)
}
fn load(gate: &Gate) -> anyhow::Result<Option<(Adoption, AdoptionHandoff)>> {
    load_inner(gate, true)
}
fn load_inner(
    gate: &Gate,
    current_inodes: bool,
) -> anyhow::Result<Option<(Adoption, AdoptionHandoff)>> {
    ensure_no_generation_bypass(gate)?;
    let path = gate.root.join("adoption.json");
    if !path.try_exists()? {
        ensure!(
            !gate.root.join("adoption.next").try_exists()?,
            "unfinished initial adoption CAS; HOLD"
        );
        return Ok(None);
    }
    let state: Adoption = read(&path)?;
    ensure!(
        state.schema_version == 1
            && !state.generation_id.is_nil()
            && !state.operation_id.is_nil()
            && hex(&state.handoff_sha256)
            && matches!(
                state.phase.as_str(),
                "preparing" | "prepared" | "committed" | "active"
            ),
        "unknown adoption journal state"
    );
    if current_inodes {
        ensure!(
            inode(&gate.root)? == state.root_inode
                && inode(&gate.root.join("coordinator.lock"))? == state.coordinator_inode
                && inode(&gate.root.join("effects.lock"))? == state.effects_inode,
            "adoption root/lock inode changed"
        );
    }
    let archive = generation_root(gate, state.generation_id);
    private(&archive, true)?;
    for entry in fs::read_dir(&archive)? {
        ensure!(
            matches!(
                entry?.file_name().to_str(),
                Some(
                    "handoff.json"
                        | "original-control"
                        | "source-exited.json"
                        | "inventory.json"
                        | "basis.json"
                        | "prior-state.json"
                        | "prior-catalog.json"
                )
            ),
            "unknown generation archive state"
        );
    }
    let bytes = bounded_bytes(&archive.join("handoff.json"))?;
    ensure!(
        sha(&bytes) == state.handoff_sha256,
        "archived handoff changed"
    );
    let handoff: AdoptionHandoff = serde_json::from_slice(&bytes)?;
    ensure!(
        handoff.operation_id == state.operation_id
            && handoff.generation_id == state.generation_id
            && handoff.prior_generation == state.prior_generation,
        "adoption handoff identity changed"
    );
    if state.prior_generation.is_none() {
        ensure!(
            !archive.join("prior-state.json").try_exists()?
                && !archive.join("prior-catalog.json").try_exists()?,
            "unplanned predecessor archive"
        );
    }
    if state.phase != "preparing" {
        verify_lineage(gate, &state)?;
        if let Some(prior) = &state.prior_generation {
            let previous_bytes = bounded_bytes(&archive.join("prior-state.json"))?;
            ensure!(
                sha(&previous_bytes) == prior.state_sha256,
                "archived predecessor state changed"
            );
            let previous: Adoption = serde_json::from_slice(&previous_bytes)?;
            ensure!(
                previous.phase == "active"
                    && previous.generation_id == prior.generation_id
                    && previous.handoff_sha256 == prior.handoff_sha256,
                "archived predecessor is unknown/incomplete"
            );
            let prior_catalog = bounded_bytes(&archive.join("prior-catalog.json"))?;
            ensure!(
                sha(&prior_catalog) == prior.archive_catalog_sha256
                    && snapshot_sha(&catalog(gate, state.generation_id)?)?
                        == prior.archive_catalog_sha256,
                "prior archive catalog changed"
            );
        }
        let archived_basis = bounded_bytes(&archive.join("basis.json"))?;
        ensure!(
            sha(&archived_basis) == state.basis_sha256 && archived_basis == basis(&state)?,
            "adoption immutable basis changed"
        );
        let control = archive.join("original-control");
        ensure!(
            snapshot_sha(&snapshot(&control, false)?)? == state.control_sha256,
            "archived old control changed"
        );
        let archive_bytes = bounded_bytes(&archive.join("inventory.json"))?;
        ensure!(
            sha(&archive_bytes) == state.archive_sha256,
            "adoption archive inventory changed"
        );
        let inventory: Snapshot = serde_json::from_slice(&archive_bytes)?;
        ensure!(
            snapshot_sha(&snapshot(&control, false)?)? == snapshot_sha(&inventory)?,
            "archive entries differ from sealed inventory"
        );
        ensure!(
            sha(&bounded_bytes(&archive.join("source-exited.json"))?)
                == handoff.source_exited_receipt.sha256,
            "archived source-exited evidence changed"
        );
    }
    Ok(Some((state, handoff)))
}
pub fn load_adoption_handoff(
    path: &Path,
    expected_sha: &str,
) -> anyhow::Result<(AdoptionHandoff, Vec<u8>, Vec<Identity>)> {
    ensure!(hex(expected_sha), "handoff requires a lowercase SHA256");
    let bytes = bounded_bytes(path)?;
    ensure!(sha(&bytes) == expected_sha, "owner handoff hash changed");
    let handoff: AdoptionHandoff = serde_json::from_slice(&bytes)?;
    ensure!(
        handoff.schema_version == 1
            && handoff.schema == "forge/quiescence-adoption/v1"
            && !handoff.operation_id.is_nil()
            && !handoff.generation_id.is_nil(),
        "unsupported adoption handoff"
    );
    ensure!(
        hex(&handoff.destination.installation_sha256)
            && !handoff.destination.namespace_uid.is_nil()
            && !handoff.destination.workspace_claim_uid.is_nil()
            && !handoff.destination.namespace.is_empty()
            && handoff.destination.namespace.len() <= 63
            && handoff
                .destination
                .namespace
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
        "destination binding incomplete"
    );
    if let Some(prior) = &handoff.prior_generation {
        ensure!(
            !prior.generation_id.is_nil()
                && prior.generation_id != handoff.generation_id
                && hex(&prior.handoff_sha256)
                && hex(&prior.state_sha256)
                && hex(&prior.archive_catalog_sha256),
            "invalid predecessor reference"
        );
    }
    ensure!(
        hex(&handoff.source_exited_receipt.sha256)
            && handoff.source_exited_receipt.path.is_absolute(),
        "source-exited receipt reference incomplete"
    );
    let exited_bytes = bounded_bytes(&handoff.source_exited_receipt.path)?;
    ensure!(
        sha(&exited_bytes) == handoff.source_exited_receipt.sha256,
        "source-exited receipt hash changed"
    );
    let exited: ExitedReceipt = serde_json::from_slice(&exited_bytes)?;
    ensure!(
        exited.schema == "forge/quiescence-exited/v1"
            && exited.operation_id == handoff.operation_id
            && exited.all_pinned_kernel_owners_exited
            && exited.exclusive_fence_still_held,
        "source-exited receipt is not exact original held operation"
    );
    ensure!(
        !exited.participants.is_empty()
            && exited.participants.len() <= 64
            && handoff.retired_processes.len() == exited.participants.len(),
        "complete retired process catalog required"
    );
    let mut old = BTreeMap::new();
    for identity in &exited.participants {
        ensure!(
            identity.schema_version == 1
                && safe_name(&identity.participant)
                && !identity.boot_id.is_nil()
                && old.insert(identity.participant.clone(), identity).is_none(),
            "invalid old participant"
        );
    }
    let mut retired = BTreeSet::new();
    for process in &handoff.retired_processes {
        let identity = old
            .get(&process.participant)
            .context("foreign retired process")?;
        ensure!(
            retired.insert(process.participant.clone())
                && process.boot_id == identity.boot_id
                && process.process_id == identity.process_id
                && process.process_start == identity.process_start
                && hex(&process.container_id)
                && process
                    .image_config_digest
                    .strip_prefix("sha256:")
                    .is_some_and(hex),
            "retired process does not bind original boot/image/CID"
        );
    }
    ensure!(
        !handoff.participants.is_empty() && handoff.participants.len() <= 64,
        "bounded actual destination catalog required"
    );
    let mut names = BTreeSet::new();
    let mut mapped = BTreeSet::new();
    let mut mapped_roots = BTreeSet::new();
    let mut destination_roots = BTreeMap::new();
    for target in &handoff.participants {
        ensure!(
            safe_name(&target.participant)
                && supported(&target.kind)
                && names.insert(target.participant.clone())
                && !target.source_participants.is_empty(),
            "invalid destination participant"
        );
        let mut sources = BTreeSet::new();
        for source in &target.source_participants {
            ensure!(
                old.contains_key(source) && sources.insert(source),
                "unknown/duplicate source participant mapping"
            );
            mapped.insert(source.clone());
        }
        let mut roots = BTreeSet::new();
        for binding in &target.roots {
            ensure!(
                binding.source.kind == binding.destination.kind
                    && matches!(
                        binding.source.kind.as_str(),
                        "workspace" | "git" | "local-delivery"
                    )
                    && binding.source.path.is_absolute()
                    && binding.destination.path.is_absolute()
                    && roots.insert((
                        binding.destination.kind.clone(),
                        binding.destination.path.clone()
                    )),
                "invalid destination root mapping"
            );
            ensure!(
                sources
                    .iter()
                    .any(|id| old[*id].custody_roots.contains(&binding.source)),
                "root mapping lacks its source owner"
            );
            let key = (
                binding.destination.kind.clone(),
                binding.destination.path.clone(),
            );
            if let Some(previous) = destination_roots.insert(key, binding.source.clone()) {
                ensure!(
                    previous == binding.source,
                    "destination root has ambiguous source ownership"
                );
            }
            mapped_roots.insert((binding.source.kind.clone(), binding.source.path.clone()));
        }
    }
    ensure!(
        mapped == old.keys().cloned().collect(),
        "old participant omitted from generation mapping"
    );
    ensure!(
        old.values()
            .flat_map(|i| i.custody_roots.iter())
            .all(|r| mapped_roots.contains(&(r.kind.clone(), r.path.clone()))),
        "old custody root omitted"
    );
    Ok((handoff, bytes, exited.participants))
}
pub fn adoption_root_mappings(handoff: &AdoptionHandoff) -> Vec<RootMapping> {
    let mut result = vec![];
    for participant in &handoff.participants {
        for binding in &participant.roots {
            if !result.contains(binding) {
                result.push(binding.clone());
            }
        }
    }
    result
}
impl Coordinator {
    /// Does not replace OLD current-owner/held bytes before the original control archive is durable.
    pub fn for_adoption(gate: Gate, operation: Uuid) -> anyhow::Result<Self> {
        let owner = file(&gate.root.join("coordinator.lock"))?;
        ensure!(lock(&owner, true)?, "another coordinator owns control");
        let request: Request = read(&gate.root.join("request.json"))?;
        ensure!(
            request.schema_version == 1 && request.operation_id == operation && !operation.is_nil(),
            "adoption requires original closed operation"
        );
        Ok(Self {
            gate,
            _owner: owner,
            operation,
            nonce: Uuid::new_v4(),
        })
    }
    pub fn prepare_adoption(
        &self,
        fence: &Fence,
        handoff: &AdoptionHandoff,
        bytes: &[u8],
        old: &[Identity],
        retired_compose: Vec<RetiredCompose>,
    ) -> anyhow::Result<()> {
        ensure!(
            handoff.operation_id == self.operation,
            "foreign adoption operation"
        );
        self.gate.verify_no_effect_records(fence)?;
        ensure!(self.gate.all_exited(old)?, "old local kernel owners remain");
        let old_snapshot = snapshot(&self.gate.root, true)?;
        let old_sha = snapshot_sha(&old_snapshot)?;
        let state_path = self.gate.root.join("adoption.json");
        let existing = if state_path.try_exists()? {
            Some(read::<Adoption>(&state_path)?)
        } else if self.gate.root.join("adoption.next").try_exists()? {
            let pending: Adoption = read(&self.gate.root.join("adoption.next"))?;
            ensure!(
                pending.phase == "preparing"
                    && pending.operation_id == self.operation
                    && pending.generation_id == handoff.generation_id
                    && pending.handoff_sha256 == sha(bytes)
                    && pending.control_sha256 == old_sha,
                "unknown initial adoption CAS"
            );
            store(&self.gate, &pending)?;
            Some(pending)
        } else {
            None
        };
        let archive = generation_root(&self.gate, handoff.generation_id);
        if let Some(previous) = &existing {
            if previous.generation_id != handoff.generation_id {
                let old_bytes = bounded_bytes(&state_path)?;
                let prior_catalog = verify_prior(&self.gate, handoff, old, &old_bytes)?;
                let state = Adoption {
                    schema_version: 1,
                    operation_id: self.operation,
                    generation_id: handoff.generation_id,
                    handoff_sha256: sha(bytes),
                    prior_generation: handoff.prior_generation.clone(),
                    phase: "preparing".into(),
                    archive_sha256: String::new(),
                    basis_sha256: String::new(),
                    control_sha256: old_sha.clone(),
                    root_inode: inode(&self.gate.root)?,
                    coordinator_inode: inode(&self.gate.root.join("coordinator.lock"))?,
                    effects_inode: inode(&self.gate.root.join("effects.lock"))?,
                    retired_compose: retired_compose.clone(),
                    enrolled: vec![],
                };
                ensure!(
                    !archive.try_exists()? || self.gate.root.join("adoption.next").try_exists()?,
                    "generation ID already archived; branching forbidden"
                );
                let next_bytes = serde_json::to_vec(&state)?;
                ensure!(next_bytes.len() <= 32768, "adoption journal exceeds bound");
                save_exact(&self.gate.root.join("adoption.next"), &next_bytes)?;
                directory(&archive)?;
                save_exact(&archive.join("handoff.json"), bytes)?;
                save_exact(&archive.join("prior-state.json"), &old_bytes)?;
                save_exact(&archive.join("prior-catalog.json"), &prior_catalog)?;
                store(&self.gate, &state)?;
            }
        }
        let existing = if state_path.try_exists()? {
            Some(read::<Adoption>(&state_path)?)
        } else {
            None
        };
        if let Some(state) = &existing {
            ensure!(
                state.schema_version == 1
                    && state.operation_id == self.operation
                    && state.generation_id == handoff.generation_id
                    && state.handoff_sha256 == sha(bytes)
                    && state.control_sha256 == old_sha,
                "adoption preparation CAS changed"
            );
            ensure!(
                inode(&self.gate.root)? == state.root_inode
                    && inode(&self.gate.root.join("coordinator.lock"))? == state.coordinator_inode
                    && inode(&self.gate.root.join("effects.lock"))? == state.effects_inode,
                "adoption lock/root inode changed"
            );
            if state.phase != "preparing" {
                let _ = load(&self.gate)?.context("adoption archive missing")?;
                return Ok(());
            }
        } else {
            ensure!(
                handoff.prior_generation.is_none(),
                "predecessor state missing"
            );
            directory(&self.gate.root.join("generations"))?;
            ensure!(
                fs::read_dir(self.gate.root.join("generations"))?
                    .next()
                    .is_none(),
                "another generation archive exists"
            );
            let state = Adoption {
                schema_version: 1,
                operation_id: self.operation,
                generation_id: handoff.generation_id,
                handoff_sha256: sha(bytes),
                prior_generation: None,
                phase: "preparing".into(),
                archive_sha256: String::new(),
                basis_sha256: String::new(),
                control_sha256: old_sha,
                root_inode: inode(&self.gate.root)?,
                coordinator_inode: inode(&self.gate.root.join("coordinator.lock"))?,
                effects_inode: inode(&self.gate.root.join("effects.lock"))?,
                retired_compose,
                enrolled: vec![],
            };
            store(&self.gate, &state)?;
        }
        let mut state: Adoption = read(&state_path)?;
        directory(&archive)?;
        save_exact(&archive.join("handoff.json"), bytes)?;
        for entry in fs::read_dir(&archive)? {
            let name = entry?.file_name();
            ensure!(
                matches!(
                    name.to_str(),
                    Some(
                        "handoff.json"
                            | "original-control"
                            | "source-exited.json"
                            | "inventory.json"
                            | "basis.json"
                            | "prior-state.json"
                            | "prior-catalog.json"
                    )
                ),
                "unknown partial archive state; HOLD"
            );
        }
        let control = archive.join("original-control");
        directory(&control)?;
        for entry in &old_snapshot.directories {
            relative(entry)?;
        }
        for relative_path in &old_snapshot.directories {
            directory(&control.join(relative_path))?;
        }
        for entry in &old_snapshot.files {
            let bytes = bounded_bytes(&self.gate.root.join(&entry.path))?;
            ensure!(
                sha(&bytes) == entry.sha256,
                "old control changed during archive"
            );
            save_exact(&control.join(&entry.path), &bytes)?;
        }
        let exited = bounded_bytes(&handoff.source_exited_receipt.path)?;
        ensure!(
            sha(&exited) == handoff.source_exited_receipt.sha256,
            "source-exited evidence changed"
        );
        save_exact(&archive.join("source-exited.json"), &exited)?;
        ensure!(
            snapshot_sha(&snapshot(&control, false)?)? == state.control_sha256
                && snapshot_sha(&snapshot(&self.gate.root, true)?)? == state.control_sha256,
            "old control/archive CAS changed"
        );
        let inventory = serde_json::to_vec(&old_snapshot)?;
        save_exact(&archive.join("inventory.json"), &inventory)?;
        state.archive_sha256 = sha(&inventory);
        let immutable = basis(&state)?;
        save_exact(&archive.join("basis.json"), &immutable)?;
        state.basis_sha256 = sha(&immutable);
        state.phase = "prepared".into();
        store(&self.gate, &state)?;
        Ok(())
    }
    pub fn commit_adoption(
        &self,
        fence: &Fence,
        handoff: &AdoptionHandoff,
        bytes: &[u8],
        old: &[Identity],
    ) -> anyhow::Result<()> {
        self.gate.verify_no_effect_records(fence)?;
        ensure!(self.gate.all_exited(old)?, "old kernel owner appeared");
        let (mut state, _) = load(&self.gate)?.context("adoption was not prepared")?;
        ensure!(
            state.operation_id == self.operation
                && handoff.operation_id == self.operation
                && state.handoff_sha256 == sha(bytes)
                && state.generation_id == handoff.generation_id
                && matches!(state.phase.as_str(), "prepared" | "committed"),
            "adoption commit CAS changed"
        );
        ensure!(
            snapshot_sha(&snapshot(&self.gate.root, true)?)? == state.control_sha256,
            "old control changed after prepare"
        );
        state.phase = "committed".into();
        store(&self.gate, &state)?;
        Ok(())
    }
    pub fn resume_enrolled(self, fence: &Fence, expected: &[Identity]) -> anyhow::Result<()> {
        if let Some((mut state, handoff)) = load(&self.gate)? {
            ensure!(
                state.phase == "active"
                    || (state.phase == "committed" && state.operation_id == self.operation),
                "generation is not committed for release"
            );
            held_release(&self, expected)?;
            self.gate.verify_participants(expected)?;
            self.gate.verify_no_effect_records(fence)?;
            ensure!(
                expected.len() == handoff.participants.len(),
                "destination participant catalog is incomplete"
            );
            for identity in expected {
                validate_target(
                    &handoff,
                    &identity.participant,
                    &identity.kind,
                    &identity.custody_roots,
                )?;
            }
            state.enrolled = expected.to_vec();
            state.phase = "active".into();
            // Authorization is durable before the last admission marker is removed.
            store(&self.gate, &state)?;
        }
        self.remove_request()
    }
    pub fn verify_adoption_seals(&self) -> anyhow::Result<()> {
        if let Some((state, _)) = load(&self.gate)? {
            for entry in &state.retired_compose {
                ensure!(
                    custody_files(&entry.root.join(".compose-control").join(&entry.name))?
                        == entry.files,
                    "retired custody seal changed"
                );
            }
        }
        Ok(())
    }
}
pub(super) fn standalone_resume(gate: &Gate) -> anyhow::Result<()> {
    ensure!(
        load(gate)?.is_none(),
        "adopted generation release requires a live held enrollment; HOLD"
    );
    Ok(())
}
fn held_release(owner: &Coordinator, expected: &[Identity]) -> anyhow::Result<()> {
    let held: Held = read(&owner.gate.root.join("held.json"))?;
    ensure!(
        held.schema_version == 1
            && held.operation_id == owner.operation
            && held.coordinator_id == owner.nonce
            && !held.acknowledgement_id.is_nil()
            && held.participants == expected,
        "release requires this fresh held acknowledgement"
    );
    Ok(())
}
fn validate_target(
    handoff: &AdoptionHandoff,
    id: &str,
    kind: &str,
    roots: &[CustodyRoot],
) -> anyhow::Result<()> {
    let target = handoff
        .participants
        .iter()
        .find(|p| p.participant == id)
        .context("unapproved destination participant")?;
    let wanted: Vec<_> = target.roots.iter().map(|r| r.destination.clone()).collect();
    ensure!(
        target.kind == kind && wanted == roots,
        "destination participant roots/kind changed"
    );
    Ok(())
}
pub(super) fn validate_registration(
    gate: &Gate,
    id: &str,
    kind: &str,
    roots: &[CustodyRoot],
) -> anyhow::Result<()> {
    if let Some((state, handoff)) = load(gate)? {
        ensure!(
            !gate.root.join("adoption.next").try_exists()?,
            "adoption CAS pending; HOLD"
        );
        ensure!(
            matches!(state.phase.as_str(), "committed" | "active"),
            "generation preparation not committed"
        );
        ensure!(
            std::env::var("CICD_QUIESCENCE_GENERATION")
                .ok()
                .and_then(|v| Uuid::parse_str(&v).ok())
                == Some(state.generation_id),
            "destination generation env differs"
        );
        validate_target(&handoff, id, kind, roots)?;
    } else {
        ensure!(
            std::env::var_os("CICD_QUIESCENCE_GENERATION").is_none(),
            "generation enrollment lacks its preserved adoption state"
        );
    }
    Ok(())
}
pub(super) fn admission(gate: &Gate, identity: Option<&Identity>) -> anyhow::Result<()> {
    if let Some((state, _)) = load(gate)? {
        let identity = identity.context("adopted writer lacks exact enrolled identity")?;
        ensure!(
            state.phase == "active" && state.enrolled.contains(identity),
            "destination boot not released by owner"
        );
        validate_registration(
            gate,
            &identity.participant,
            &identity.kind,
            &identity.custody_roots,
        )?;
    }
    Ok(())
}
pub async fn wait_for_enrollment() -> anyhow::Result<()> {
    loop {
        let Some(gate) = Gate::from_env()? else {
            return Ok(());
        };
        if load(&gate)?.is_none() {
            ensure!(
                std::env::var_os("CICD_QUIESCENCE_GENERATION").is_none(),
                "generation enrollment state missing"
            );
            return Ok(());
        }
        if admission(&gate, IDENTITY.get()).is_ok() && !gate.requested()? {
            if let Ok(probe) = gate.enter(false) {
                drop(probe);
                return Ok(());
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}
pub fn retired_compose_all(gate: &Gate) -> anyhow::Result<Vec<RetiredCompose>> {
    Ok(load(gate)?
        .map(|(state, _)| state.retired_compose)
        .unwrap_or_default())
}
pub fn inherited_compose(
    gate: &Gate,
    handoff: &AdoptionHandoff,
) -> anyhow::Result<Vec<RetiredCompose>> {
    let Some(prior) = &handoff.prior_generation else {
        return Ok(vec![]);
    };
    let Some((state, _)) = load_inner(gate, false)? else {
        anyhow::bail!("predecessor custody absent")
    };
    let mut result = vec![];
    let entries = if state.generation_id == prior.generation_id {
        state.retired_compose
    } else {
        ensure!(
            state.generation_id == handoff.generation_id,
            "foreign adoption custody generation"
        );
        return Ok(state.retired_compose);
    };
    for entry in entries {
        for mapping in adoption_root_mappings(handoff)
            .iter()
            .filter(|m| m.source.kind == "workspace" && m.source.path == entry.root)
        {
            let mut next = entry.clone();
            next.root = mapping.destination.path.clone();
            ensure!(
                custody_files(&next.root.join(".compose-control").join(&next.name))? == next.files,
                "inherited retired custody changed"
            );
            result.push(next);
        }
    }
    Ok(result)
}
pub fn retired_compose_binding(root: &Path, name: &str) -> anyhow::Result<Option<RetiredCompose>> {
    let Some(gate) = Gate::from_env()? else {
        return Ok(None);
    };
    let Some((state, _)) = load(&gate)? else {
        return Ok(None);
    };
    ensure!(
        matches!(state.phase.as_str(), "committed" | "active"),
        "adoption not committed"
    );
    let selected = state
        .retired_compose
        .into_iter()
        .find(|entry| entry.root == root && entry.name == name);
    if let Some(entry) = &selected {
        let actual = custody_files(&root.join(".compose-control").join(name))?;
        ensure!(actual == entry.files, "retired Compose custody changed");
    }
    Ok(selected)
}
pub fn custody_files(root: &Path) -> anyhow::Result<Vec<CustodyFile>> {
    Ok(snapshot(root, false)?
        .files
        .into_iter()
        .map(|f| CustodyFile {
            path: f.path,
            sha256: f.sha256,
        })
        .collect())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    fn fixture() -> (PathBuf, Gate, AdoptionHandoff, Vec<u8>, Vec<Identity>) {
        let parent = std::env::temp_dir().join(format!("forge-adoption-{}", Uuid::new_v4()));
        fs::create_dir(&parent).unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let gate = Gate::open(&parent.join("control")).unwrap();
        let (participant, identity) = gate.register("old", "server", vec![]).unwrap();
        drop(participant);
        let operation = Uuid::new_v4();
        drop(Coordinator::request(gate.clone(), operation).unwrap());
        let exited = parent.join("exited.json");
        let bytes=serde_json::to_vec(&serde_json::json!({"schema":"forge/quiescence-exited/v1","operationId":operation,"participants":[identity],"allPinnedKernelOwnersExited":true,"exclusiveFenceStillHeld":true})).unwrap();
        write_new(&exited, &bytes).unwrap();
        let handoff:AdoptionHandoff=serde_json::from_value(serde_json::json!({
            "schemaVersion":1,"schema":"forge/quiescence-adoption/v1","operationId":operation,"generationId":Uuid::new_v4(),
            "sourceExitedReceipt":{"path":exited,"sha256":sha(&bytes)},
            "retiredProcesses":[{"participant":"old","bootId":identity.boot_id,"containerId":"1".repeat(64),"imageConfigDigest":format!("sha256:{}","2".repeat(64)),"processId":identity.process_id,"processStart":identity.process_start}],
            "destination":{"installationSha256":"3".repeat(64),"namespace":"fixture","namespaceUid":Uuid::new_v4(),"workspaceClaimUid":Uuid::new_v4()},
            "participants":[{"sourceParticipants":["old"],"participant":"new","kind":"server","roots":[]}],
            "sourceDockerDaemonId":null,"sourceWorkspaceVolume":null
        })).unwrap();
        let bytes = serde_json::to_vec(&handoff).unwrap();
        write_new(&parent.join("handoff.json"), &bytes).unwrap();
        (parent, gate, handoff, bytes, vec![identity])
    }
    #[test]
    fn adoption_requires_hash_bound_original_receipt_and_retired_catalog() {
        let (parent, _, _, bytes, _) = fixture();
        let path = parent.join("handoff.json");
        assert!(load_adoption_handoff(&path, &sha(&bytes)).is_ok());
        assert!(load_adoption_handoff(&path, &"0".repeat(64)).is_err());
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        value["retiredProcesses"][0]["bootId"] = serde_json::json!(Uuid::new_v4());
        let changed = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &changed).unwrap();
        assert!(load_adoption_handoff(&path, &sha(&changed)).is_err());
        fs::remove_dir_all(parent).unwrap();
    }
    #[test]
    fn prepare_commit_preserve_root_locks_and_archive_and_cannot_standalone_release() {
        let (parent, gate, handoff, bytes, old) = fixture();
        let owner = Coordinator::for_adoption(gate.clone(), handoff.operation_id).unwrap();
        let fence = owner.try_fence().unwrap().unwrap();
        let before = (
            inode(&gate.root).unwrap(),
            inode(&gate.root.join("coordinator.lock")).unwrap(),
            inode(&gate.root.join("effects.lock")).unwrap(),
        );
        owner
            .prepare_adoption(&fence, &handoff, &bytes, &old, vec![])
            .unwrap();
        owner
            .prepare_adoption(&fence, &handoff, &bytes, &old, vec![])
            .unwrap();
        owner
            .commit_adoption(&fence, &handoff, &bytes, &old)
            .unwrap();
        owner
            .commit_adoption(&fence, &handoff, &bytes, &old)
            .unwrap();
        assert_eq!(
            before,
            (
                inode(&gate.root).unwrap(),
                inode(&gate.root.join("coordinator.lock")).unwrap(),
                inode(&gate.root.join("effects.lock")).unwrap()
            )
        );
        assert!(gate.requested().unwrap());
        assert!(gate.enter(false).is_err());
        assert!(standalone_resume(&gate).is_err());
        let (state, _) = load(&gate).unwrap().unwrap();
        assert_eq!(state.phase, "committed");
        assert!(
            generation_root(&gate, handoff.generation_id)
                .join("original-control/participants/old.json")
                .is_file()
        );
        drop(fence);
        drop(owner);
        fs::remove_dir_all(parent).unwrap();
    }
    #[test]
    fn live_replaced_unknown_and_effect_custody_hold_before_preparation() {
        let (parent, gate, handoff, bytes, old) = fixture();
        let owner = Coordinator::for_adoption(gate.clone(), handoff.operation_id).unwrap();
        let fence = owner.try_fence().unwrap().unwrap();
        let (replacement, _) = gate.register("old", "server", vec![]).unwrap();
        assert!(
            owner
                .prepare_adoption(&fence, &handoff, &bytes, &old, vec![])
                .is_err()
        );
        drop(replacement);
        // Original identity must remain exact, not merely a free lock for its name.
        assert!(
            owner
                .prepare_adoption(&fence, &handoff, &bytes, &old, vec![])
                .is_err()
        );
        drop(fence);
        drop(owner);
        fs::remove_dir_all(parent).unwrap();
        let (parent, gate, handoff, bytes, old) = fixture();
        let owner = Coordinator::for_adoption(gate.clone(), handoff.operation_id).unwrap();
        let fence = owner.try_fence().unwrap().unwrap();
        write_new(&gate.root.join("unknown.json"), b"{}").unwrap();
        assert!(
            owner
                .prepare_adoption(&fence, &handoff, &bytes, &old, vec![])
                .is_err()
        );
        fs::remove_file(gate.root.join("unknown.json")).unwrap();
        directory(&gate.root.join("effects")).unwrap();
        write_new(&gate.root.join("effects/orphan.json"), b"{}").unwrap();
        assert!(
            owner
                .prepare_adoption(&fence, &handoff, &bytes, &old, vec![])
                .is_err()
        );
        drop(fence);
        drop(owner);
        fs::remove_dir_all(parent).unwrap();
    }
    #[test]
    fn interrupted_preparing_archive_and_durable_next_resume_without_discard() {
        let (parent, gate, handoff, bytes, old) = fixture();
        let owner = Coordinator::for_adoption(gate.clone(), handoff.operation_id).unwrap();
        let fence = owner.try_fence().unwrap().unwrap();
        owner
            .prepare_adoption(&fence, &handoff, &bytes, &old, vec![])
            .unwrap();
        let (prepared, _) = load(&gate).unwrap().unwrap();
        let pending_bytes = serde_json::to_vec(&prepared).unwrap();
        let mut interrupted: Adoption = serde_json::from_slice(&pending_bytes).unwrap();
        interrupted.phase = "preparing".into();
        interrupted.archive_sha256.clear();
        replace(
            &gate.root.join("adoption.json"),
            &serde_json::to_vec(&interrupted).unwrap(),
        )
        .unwrap();
        write_new(&gate.root.join("adoption.next"), &pending_bytes).unwrap();
        let archive = generation_root(&gate, handoff.generation_id);
        fs::remove_file(archive.join("inventory.json")).unwrap();
        fs::remove_file(archive.join("source-exited.json")).unwrap();
        owner
            .prepare_adoption(&fence, &handoff, &bytes, &old, vec![])
            .unwrap();
        assert_eq!(load(&gate).unwrap().unwrap().0.phase, "prepared");
        assert!(!gate.root.join("adoption.next").exists());
        let file = archive.join("original-control/participants/old.json");
        fs::write(&file, b"{}").unwrap();
        assert!(
            owner
                .commit_adoption(&fence, &handoff, &bytes, &old)
                .is_err()
        );
        drop(fence);
        drop(owner);
        fs::remove_dir_all(parent).unwrap();
    }
    #[test]
    fn replaced_lock_inode_prevents_commit_without_cleaning_evidence() {
        let (parent, gate, handoff, bytes, old) = fixture();
        let owner = Coordinator::for_adoption(gate.clone(), handoff.operation_id).unwrap();
        let fence = owner.try_fence().unwrap().unwrap();
        owner
            .prepare_adoption(&fence, &handoff, &bytes, &old, vec![])
            .unwrap();
        let replacement = gate.root.join("replacement");
        write_new(&replacement, b"").unwrap();
        fs::rename(replacement, gate.root.join("effects.lock")).unwrap();
        assert!(
            owner
                .commit_adoption(&fence, &handoff, &bytes, &old)
                .is_err()
        );
        assert!(gate.root.join("request.json").exists());
        assert!(
            generation_root(&gate, handoff.generation_id)
                .join("original-control")
                .exists()
        );
        drop(fence);
        drop(owner);
        fs::remove_dir_all(parent).unwrap();
    }
    #[test]
    fn incomplete_or_changed_destination_catalog_never_releases() {
        let (parent, gate, handoff, bytes, old) = fixture();
        let owner = Coordinator::for_adoption(gate.clone(), handoff.operation_id).unwrap();
        let fence = owner.try_fence().unwrap().unwrap();
        owner
            .prepare_adoption(&fence, &handoff, &bytes, &old, vec![])
            .unwrap();
        owner
            .commit_adoption(&fence, &handoff, &bytes, &old)
            .unwrap();
        assert!(validate_target(&handoff, "new", "external-runner", &[]).is_err());
        assert!(validate_target(&handoff, "foreign", "server", &[]).is_err());
        assert!(
            validate_target(
                &handoff,
                "new",
                "server",
                &[CustodyRoot::workspace(parent.clone())]
            )
            .is_err()
        );
        assert!(owner.resume_enrolled(&fence, &[]).is_err());
        assert!(gate.requested().unwrap());
        drop(fence);
        fs::remove_dir_all(parent).unwrap();
    }
    #[test]
    fn actual_child_enrolls_without_effect_then_owner_releases_exact_boot() {
        use std::io::{BufRead, Write};
        use std::process::{Child, Command, Stdio};
        struct Fixture(Child);
        impl Drop for Fixture {
            fn drop(&mut self) {
                if self.0.try_wait().ok().flatten().is_none() {
                    let _ = self.0.kill();
                    let _ = self.0.wait();
                }
            }
        }
        let (parent, gate, handoff, bytes, old) = fixture();
        let owner = Coordinator::for_adoption(gate.clone(), handoff.operation_id).unwrap();
        let fence = owner.try_fence().unwrap().unwrap();
        owner
            .prepare_adoption(&fence, &handoff, &bytes, &old, vec![])
            .unwrap();
        owner
            .commit_adoption(&fence, &handoff, &bytes, &old)
            .unwrap();
        drop(fence);
        drop(owner);
        let mut child = Fixture(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    &format!(
                        "{}::enrollment_fixture",
                        module_path!().split_once("::").unwrap().1
                    ),
                    "--nocapture",
                ])
                .env("FORGE_ADOPTION_CHILD", &parent)
                .env("FORGE_ADOPTION_FRESH_MAINTENANCE", "1")
                .stdin(Stdio::piped())
                .env("CICD_QUIESCENCE_DIR", &gate.root)
                .env("CICD_QUIESCENCE_PARTICIPANT", "new")
                .env(
                    "CICD_QUIESCENCE_GENERATION",
                    handoff.generation_id.to_string(),
                )
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let mut output = std::io::BufReader::new(child.0.stdout.take().unwrap());
        let identity = loop {
            let mut line = String::new();
            assert!(output.read_line(&mut line).unwrap() > 0);
            if let Some(offset) = line.find("ENROLLED ") {
                break serde_json::from_str::<Identity>(line[offset + 9..].trim()).unwrap();
            }
        };
        assert!(!parent.join("effect").exists());
        assert!(admission(&gate, Some(&identity)).is_err());
        let owner = Coordinator::request(gate.clone(), handoff.operation_id).unwrap();
        let fence = owner.try_fence().unwrap().unwrap();
        owner
            .publish_held(&fence, std::slice::from_ref(&identity))
            .unwrap();
        owner
            .resume_enrolled(&fence, std::slice::from_ref(&identity))
            .unwrap();
        drop(fence);
        for _ in 0..100 {
            if parent.join("effect").exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(parent.join("effect").exists());
        let maintenance = Coordinator::request(gate.clone(), Uuid::new_v4()).unwrap();
        let mut acquired = None;
        for _ in 0..100 {
            acquired = maintenance.try_fence().unwrap();
            if acquired.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let fence = acquired.expect("actual child effect did not release its kernel lease");
        assert!(held_release(&maintenance, std::slice::from_ref(&identity)).is_err());
        maintenance
            .publish_held(&fence, std::slice::from_ref(&identity))
            .unwrap();
        maintenance
            .resume_enrolled(&fence, std::slice::from_ref(&identity))
            .unwrap();
        drop(fence);
        child
            .0
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"finish\n")
            .unwrap();
        let mut stopped = false;
        for _ in 0..100 {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(status.success());
                stopped = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(stopped);
        assert_eq!(fs::read(parent.join("effect")).unwrap(), b"admitted");
        fs::remove_dir_all(parent).unwrap();
    }
    fn synthetic_active(gate: &Gate) {
        let (mut state, handoff) = load(gate).unwrap().unwrap();
        let target = &handoff.participants[0];
        let old: Identity = read(&gate.root.join("participants/old.json")).unwrap();
        let mut identity = old;
        identity.participant = target.participant.clone();
        identity.boot_id = Uuid::new_v4();
        identity.custody_roots = target.roots.iter().map(|r| r.destination.clone()).collect();
        let stem = gate.root.join("participants").join(&identity.participant);
        let _ = file(&stem.with_extension("lock")).unwrap();
        replace(
            &stem.with_extension("json"),
            &serde_json::to_vec(&identity).unwrap(),
        )
        .unwrap();
        state.phase = "active".into();
        state.enrolled = vec![identity];
        store(gate, &state).unwrap();
    }
    fn next_handoff(parent: &Path, gate: &Gate) -> (AdoptionHandoff, Vec<u8>, Vec<Identity>) {
        let (state, mut handoff) = load(gate).unwrap().unwrap();
        let old = state.enrolled.clone();
        let status = adoption_status(gate).unwrap();
        let prior:PriorGeneration=serde_json::from_value(serde_json::json!({
            "generationId":status["generationId"],"handoffSha256":status["handoffSha256"],
            "stateSha256":status["stateSha256"],"archiveCatalogSha256":status["archiveCatalogSha256"]})).unwrap();
        handoff.operation_id = Uuid::new_v4();
        handoff.generation_id = Uuid::new_v4();
        handoff.prior_generation = Some(prior);
        let evidence = parent.join(format!("exited-{}.json", handoff.generation_id));
        let bytes=serde_json::to_vec(&serde_json::json!({"schema":"forge/quiescence-exited/v1","operationId":handoff.operation_id,"participants":old,"allPinnedKernelOwnersExited":true,"exclusiveFenceStillHeld":true})).unwrap();
        write_new(&evidence, &bytes).unwrap();
        handoff.source_exited_receipt = EvidenceFile {
            path: evidence,
            sha256: sha(&bytes),
        };
        handoff.retired_processes = old
            .iter()
            .map(|i| RetiredProcess {
                participant: i.participant.clone(),
                boot_id: i.boot_id,
                container_id: "4".repeat(64),
                image_config_digest: format!("sha256:{}", "5".repeat(64)),
                process_id: i.process_id,
                process_start: i.process_start.clone(),
            })
            .collect();
        handoff.participants[0].source_participants =
            old.iter().map(|i| i.participant.clone()).collect();
        let bytes = serde_json::to_vec(&handoff).unwrap();
        (handoff, bytes, old)
    }
    fn copied_control(source: &Path, destination: &Path) {
        directory(destination).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let path = entry.unwrap().path();
            let target = destination.join(path.file_name().unwrap());
            if path.is_dir() {
                copied_control(&path, &target)
            } else {
                write_new(&target, &fs::read(&path).unwrap()).unwrap();
            }
        }
    }
    #[test]
    fn linear_g1_g2_g3_restore_keeps_history_and_rebinds_copied_inodes() {
        let (parent, gate, first, bytes, old) = fixture();
        let owner = Coordinator::for_adoption(gate.clone(), first.operation_id).unwrap();
        let fence = owner.try_fence().unwrap().unwrap();
        owner
            .prepare_adoption(&fence, &first, &bytes, &old, vec![])
            .unwrap();
        owner.commit_adoption(&fence, &first, &bytes, &old).unwrap();
        drop(fence);
        drop(owner);
        synthetic_active(&gate);
        let mut current = gate;
        for index in 2..=3 {
            let (handoff, bytes, old) = next_handoff(&parent, &current);
            let history = snapshot_sha(&catalog(&current, Uuid::nil()).unwrap()).unwrap();
            fs::remove_file(current.root.join("request.json")).unwrap();
            drop(Coordinator::request(current.clone(), handoff.operation_id).unwrap());
            let copied = parent.join(format!("destination-{index}"));
            copied_control(&current.root, &copied);
            let destination = Gate::open(&copied).unwrap();
            assert!(load(&destination).is_err());
            let owner =
                Coordinator::for_adoption(destination.clone(), handoff.operation_id).unwrap();
            let fence = owner.try_fence().unwrap().unwrap();
            owner
                .prepare_adoption(&fence, &handoff, &bytes, &old, vec![])
                .unwrap();
            if index == 2 {
                // Model a crash after durable next, before predecessor bytes/catalog were copied.
                let (prepared, _) = load(&destination).unwrap().unwrap();
                let archive = generation_root(&destination, handoff.generation_id);
                let prior_bytes = bounded_bytes(&archive.join("prior-state.json")).unwrap();
                let mut pending: Adoption =
                    serde_json::from_slice(&serde_json::to_vec(&prepared).unwrap()).unwrap();
                pending.phase = "preparing".into();
                pending.archive_sha256.clear();
                pending.basis_sha256.clear();
                replace(&destination.root.join("adoption.json"), &prior_bytes).unwrap();
                write_new(
                    &destination.root.join("adoption.next"),
                    &serde_json::to_vec(&pending).unwrap(),
                )
                .unwrap();
                fs::remove_dir_all(archive.join("original-control")).unwrap();
                for file in [
                    "prior-state.json",
                    "prior-catalog.json",
                    "source-exited.json",
                    "inventory.json",
                    "basis.json",
                ] {
                    fs::remove_file(archive.join(file)).unwrap();
                }
                assert!(validate_registration(&destination, "new", "server", &[]).is_err());
                owner
                    .prepare_adoption(&fence, &handoff, &bytes, &old, vec![])
                    .unwrap();
            }
            owner
                .commit_adoption(&fence, &handoff, &bytes, &old)
                .unwrap();
            assert_eq!(
                snapshot_sha(&catalog(&destination, handoff.generation_id).unwrap()).unwrap(),
                history
            );
            assert!(
                generation_root(&destination, handoff.generation_id)
                    .join("prior-state.json")
                    .is_file()
            );
            drop(fence);
            drop(owner);
            synthetic_active(&destination);
            current = destination;
        }
        assert_eq!(
            fs::read_dir(current.root.join("generations"))
                .unwrap()
                .count(),
            3
        );
        let (handoff, bytes, old) = next_handoff(&parent, &current);
        fs::remove_file(current.root.join("request.json")).unwrap();
        drop(Coordinator::request(current.clone(), handoff.operation_id).unwrap());
        let owner = Coordinator::for_adoption(current.clone(), handoff.operation_id).unwrap();
        let fence = owner.try_fence().unwrap().unwrap();
        let mut foreign = handoff.clone();
        foreign.prior_generation.as_mut().unwrap().state_sha256 = "0".repeat(64);
        assert!(
            owner
                .prepare_adoption(
                    &fence,
                    &foreign,
                    &serde_json::to_vec(&foreign).unwrap(),
                    &old,
                    vec![]
                )
                .is_err()
        );
        let first_archive = fs::read_dir(current.root.join("generations"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let mut reused = handoff.clone();
        reused.generation_id =
            Uuid::parse_str(first_archive.file_name().unwrap().to_str().unwrap()).unwrap();
        assert!(
            owner
                .prepare_adoption(
                    &fence,
                    &reused,
                    &serde_json::to_vec(&reused).unwrap(),
                    &old,
                    vec![]
                )
                .is_err()
        );
        write_new(&first_archive.join("unknown.json"), b"{}").unwrap();
        assert!(
            owner
                .prepare_adoption(&fence, &handoff, &bytes, &old, vec![])
                .is_err()
        );
        drop(fence);
        drop(owner);
        fs::remove_dir_all(parent).unwrap();
    }
    #[test]
    fn incomplete_predecessor_and_generation_reuse_are_not_reset_paths() {
        let (parent, gate, handoff, bytes, old) = fixture();
        let owner = Coordinator::for_adoption(gate.clone(), handoff.operation_id).unwrap();
        let fence = owner.try_fence().unwrap().unwrap();
        owner
            .prepare_adoption(&fence, &handoff, &bytes, &old, vec![])
            .unwrap();
        owner
            .commit_adoption(&fence, &handoff, &bytes, &old)
            .unwrap();
        drop(fence);
        drop(owner);
        synthetic_active(&gate);
        let (next, bytes, old) = next_handoff(&parent, &gate);
        fs::remove_file(gate.root.join("request.json")).unwrap();
        drop(Coordinator::request(gate.clone(), next.operation_id).unwrap());
        let (mut state, _) = load(&gate).unwrap().unwrap();
        state.phase = "committed".into();
        store(&gate, &state).unwrap();
        let owner = Coordinator::for_adoption(gate.clone(), next.operation_id).unwrap();
        let fence = owner.try_fence().unwrap().unwrap();
        assert!(
            owner
                .prepare_adoption(&fence, &next, &bytes, &old, vec![])
                .is_err()
        );
        drop(fence);
        drop(owner);
        fs::remove_dir_all(parent).unwrap();
    }
    #[test]
    fn enrollment_fixture() {
        use std::io::Write;
        let Some(parent) = std::env::var_os("FORGE_ADOPTION_CHILD") else {
            return;
        };
        initialize("server", vec![]).unwrap();
        println!(
            "ENROLLED {}",
            serde_json::to_string(IDENTITY.get().unwrap()).unwrap()
        );
        std::io::stdout().flush().unwrap();
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(wait_for_enrollment())
            .unwrap();
        {
            let _effect = enter(true).unwrap();
            fs::write(PathBuf::from(parent).join("effect"), b"admitted").unwrap();
        }
        if std::env::var_os("FORGE_ADOPTION_FRESH_MAINTENANCE").is_some() {
            let mut line = String::new();
            std::io::stdin().read_line(&mut line).unwrap();
            assert_eq!(line.trim(), "finish");
        }
    }
}
