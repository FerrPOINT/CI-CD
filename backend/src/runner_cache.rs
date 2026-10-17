//! Public, operator-registered read-only downloads; never a shared writable target.
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, io, path::Path};

const SCHEMA: &str = "base-forge-cache-registry/v1";
const SEED_SCHEMA: &str = "base-forge-cargo-seed/v1";
const TARGET: &str = "/opt/sdlc/cargo-seed";
const BOOTSTRAP: &str = include_str!("../../scripts/vendor/base_forge_job_cache.py");
const POLICY: &str = include_str!("../../scripts/vendor/base-build-cache-policy.json");
const PACKET: &str = include_str!("../../scripts/vendor/base-forge-cache-tooling.json");

fn invalid(message: &str) -> io::Error {
    io::Error::other(message)
}
fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn hexadecimal(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Identity {
    pub workspace: String,
    pub project: String,
    pub daemon: String,
    pub image: String,
    pub base_revision: String,
    pub lock_sha256: String,
    pub platform: String,
}
impl Identity {
    fn key(&self) -> io::Result<String> {
        if !matches!(self.workspace.as_str(), "pdlc1" | "pdlc2" | "pdlc3")
            || uuid::Uuid::parse_str(&self.project)
                .map(|id| id.to_string())
                .ok()
                .as_deref()
                != Some(self.project.as_str())
            || self.daemon.is_empty()
            || self.daemon.len() > 256
            || !self
                .daemon
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
            || !self
                .image
                .strip_prefix("sha256:")
                .is_some_and(|id| hexadecimal(id, 64))
            || !hexadecimal(&self.base_revision, 40)
            || !hexadecimal(&self.lock_sha256, 64)
            || !matches!(self.platform.as_str(), "linux/amd64" | "linux/arm64")
        {
            return Err(invalid(
                "cache identity is not an exact supported input set",
            ));
        }
        Ok(sha(format!(
            "{SCHEMA}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
            self.workspace,
            self.project,
            self.daemon,
            self.image,
            self.base_revision,
            self.lock_sha256,
            self.platform
        )
        .as_bytes()))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    volume: String,
    bytes: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    workspace: String,
    project: String,
    daemon: String,
    image: String,
    base_revision: String,
    lock_sha256: String,
    platform: String,
    key: String,
    seed: String,
}
impl Binding {
    fn identity(&self) -> Identity {
        Identity {
            workspace: self.workspace.clone(),
            project: self.project.clone(),
            daemon: self.daemon.clone(),
            image: self.image.clone(),
            base_revision: self.base_revision.clone(),
            lock_sha256: self.lock_sha256.clone(),
            platform: self.platform.clone(),
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    schema: String,
    seeds: BTreeMap<String, Payload>,
    bindings: Vec<Binding>,
}

pub(super) struct Seed {
    pub key: String,
    pub manifest: String,
    pub volume: String,
    pub bytes: u64,
    pub identity: Identity,
}
pub(super) struct Lookup {
    pub seed: Option<Seed>,
    pub reason: &'static str,
}

pub(super) fn configured() -> bool {
    [
        "CICD_RUNNER_CACHE_REGISTRY",
        "CICD_RUNNER_CACHE_REGISTRY_SHA256",
    ]
    .iter()
    .any(|name| std::env::var(name).is_ok_and(|value| !value.is_empty()))
}

fn tooling_budget() -> io::Result<u64> {
    let packet: Value = serde_json::from_str(PACKET)?;
    if packet["schema"] != "base-forge-cache-tooling/v1"
        || !packet["revision"]
            .as_str()
            .is_some_and(|value| hexadecimal(value, 40))
        || packet["files"].as_object().map(|files| files.len()) != Some(2)
        || packet["files"]["base_forge_job_cache.py"]["sha256"] != sha(BOOTSTRAP.as_bytes())
        || packet["files"]["base-build-cache-policy.json"]["sha256"] != sha(POLICY.as_bytes())
    {
        return Err(invalid("packaged cache protocol identity/hash differs"));
    }
    let policy: Value = serde_json::from_str(POLICY)?;
    let limit = policy["forge_seed_pool_bytes"].as_u64().unwrap_or_default();
    if policy["schema"] != "base-build-cache-policy/v1"
        || limit == 0
        || limit > policy["download_pool_bytes"].as_u64().unwrap_or_default()
    {
        return Err(invalid(
            "cache seed budget is outside the shared download pool",
        ));
    }
    Ok(limit)
}

fn regular(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    use std::io::Read;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > limit {
        return Err(invalid(
            "cache input must be a bounded regular file without links",
        ));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid("cache input changed beyond its byte limit"));
    }
    Ok(bytes)
}

fn lock_digest(workspace: &Path) -> io::Result<Option<String>> {
    if fs::symlink_metadata(workspace)?.file_type().is_symlink() {
        return Err(invalid("cache workspace links are forbidden"));
    }
    let backend = workspace.join("backend");
    if backend.exists() && fs::symlink_metadata(&backend)?.file_type().is_symlink() {
        return Err(invalid("cache backend directory links are forbidden"));
    }
    let candidates = [workspace.join("Cargo.lock"), backend.join("Cargo.lock")];
    let present: Vec<_> = candidates
        .iter()
        .filter(|path| path.symlink_metadata().is_ok())
        .collect();
    if present.len() != 1 {
        return Ok(None);
    }
    Ok(Some(sha(&regular(present[0], 16 * 1024 * 1024)?)))
}

fn select(
    payload: &[u8],
    expected: &str,
    identity: &Identity,
    maximum: u64,
) -> io::Result<Option<Seed>> {
    if maximum == 0 {
        return Err(invalid("cache seed budget must be positive"));
    }
    if !hexadecimal(expected, 64) || sha(payload) != expected {
        return Err(invalid(
            "cache registry changed after operator registration",
        ));
    }
    let registry: Registry =
        serde_json::from_slice(payload).map_err(|_| invalid("invalid cache registry document"))?;
    if registry.schema != SCHEMA || registry.seeds.len() > 64 || registry.bindings.len() > 256 {
        return Err(invalid("unknown or oversized cache registry"));
    }
    let mut total = 0u64;
    for (manifest, seed) in &registry.seeds {
        if !hexadecimal(manifest, 64)
            || seed.volume != format!("pdlc-cargo-seed-{manifest}")
            || seed.bytes == 0
        {
            return Err(invalid("cache seed name/content registration differs"));
        }
        total = total
            .checked_add(seed.bytes)
            .ok_or_else(|| invalid("cache seed budget overflow"))?;
    }
    if total > maximum {
        return Err(invalid(
            "registered seed pool exceeds its shared byte budget",
        ));
    }
    let mut keys = std::collections::BTreeSet::new();
    for binding in &registry.bindings {
        if binding.identity().key()? != binding.key
            || !keys.insert(binding.key.clone())
            || !registry.seeds.contains_key(&binding.seed)
        {
            return Err(invalid("cache binding/seed identity differs"));
        }
    }
    let key = identity.key()?;
    Ok(registry
        .bindings
        .iter()
        .find(|binding| binding.key == key)
        .map(|binding| {
            let seed = &registry.seeds[&binding.seed];
            Seed {
                key,
                manifest: binding.seed.clone(),
                volume: seed.volume.clone(),
                bytes: seed.bytes,
                identity: binding.identity(),
            }
        }))
}

pub(super) fn from_env(workspace: &Path, mut identity: Identity) -> io::Result<Lookup> {
    let path = std::env::var("CICD_RUNNER_CACHE_REGISTRY").unwrap_or_default();
    let expected = std::env::var("CICD_RUNNER_CACHE_REGISTRY_SHA256").unwrap_or_default();
    if path.is_empty() || expected.is_empty() {
        return Err(invalid(
            "cache registry path and SHA-256 must be configured together",
        ));
    }
    let maximum = tooling_budget()?;
    // Unsupported repository layouts do not acquire cache authority. Preserve
    // the ordinary isolated job instead of turning a performance miss into failure.
    let Ok(Some(lock)) = lock_digest(workspace) else {
        return Ok(Lookup {
            seed: None,
            reason: "no unique regular supported Cargo.lock source; cache not attached",
        });
    };
    identity.lock_sha256 = lock;
    let payload = regular(Path::new(&path), 1024 * 1024)?;
    let seed = select(&payload, &expected, &identity, maximum)?;
    Ok(Lookup {
        reason: if seed.is_some() {
            "registered immutable public seed"
        } else {
            "no binding for actual owner/toolchain/dependency inputs"
        },
        seed,
    })
}

pub(super) fn volume_matches(seed: &Seed, volume: &Value) -> bool {
    let labels = &volume[0]["Labels"];
    volume[0]["Name"] == seed.volume
        && volume[0]["Driver"] == "local"
        && (volume[0]["Options"].is_null()
            || volume[0]["Options"]
                .as_object()
                .is_some_and(|value| value.is_empty()))
        && labels["pdlc.cache.owner"] == "operator"
        && labels["pdlc.cache.schema"] == SEED_SCHEMA
        && labels["pdlc.cache.manifest-sha256"] == seed.manifest
        && labels["pdlc.cache.bytes"] == seed.bytes.to_string()
        && labels["pdlc.gateway.workspace"].is_null()
        && labels["com.docker.compose.project"].is_null()
}

pub(super) fn attach(service: &mut Value, seed: &Seed) -> io::Result<()> {
    let original = service["entrypoint"]
        .as_array()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid("seed bootstrap requires an explicit executable"))?
        .clone();
    if original.iter().any(|value| !value.is_string()) {
        return Err(invalid("seed executable arguments must be strings"));
    }
    let mut entrypoint = vec![
        json!("python3"),
        json!("-I"),
        json!("-c"),
        json!(BOOTSTRAP),
        json!("--source"),
        json!(TARGET),
        json!("--manifest-sha256"),
        json!(seed.manifest),
        json!("--bytes"),
        json!(seed.bytes.to_string()),
        json!("--"),
    ];
    entrypoint.extend(original);
    service["entrypoint"] = json!(entrypoint);
    service["volumes"].as_array_mut().ok_or_else(|| invalid("job volume declaration missing"))?.push(json!({
        "type":"volume", "source":"cache-seed", "target":TARGET, "read_only":true, "volume":{"nocopy":true}}));
    service["labels"]["pdlc.cache.key"] = json!(seed.key);
    service["labels"]["pdlc.cache.project"] = json!(seed.identity.project);
    service["labels"]["pdlc.cache.lock-sha256"] = json!(seed.identity.lock_sha256);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> Identity {
        Identity {
            workspace: "pdlc1".into(),
            project: "11111111-2222-3333-4444-555555555555".into(),
            daemon: "expected-daemon".into(),
            image: format!("sha256:{}", "a".repeat(64)),
            base_revision: "b".repeat(40),
            lock_sha256: "c".repeat(64),
            platform: "linux/amd64".into(),
        }
    }
    fn document() -> Value {
        let value = identity();
        let manifest = "d".repeat(64);
        json!({"schema":SCHEMA,"seeds":{manifest.clone():{"volume":format!("pdlc-cargo-seed-{manifest}"),"bytes":100}},
            "bindings":[{"workspace":value.workspace,"project":value.project,"daemon":value.daemon,
                "image":value.image,"base_revision":value.base_revision,"lock_sha256":value.lock_sha256,
                "platform":value.platform,"key":identity().key().unwrap(),"seed":manifest}]})
    }
    fn lookup(value: &Value, requested: &Identity, maximum: u64) -> io::Result<Option<Seed>> {
        let payload = serde_json::to_vec(value).unwrap();
        select(&payload, &sha(&payload), requested, maximum)
    }

    #[test]
    fn key_matches_the_base_python_golden_packet_and_changes_on_actual_inputs() {
        assert_eq!(
            identity().key().unwrap(),
            "ec9ccf857ce51d4ebc3c4c3a521d22ae9661c940a2e10ee9da1689ba5350f5a7"
        );
        let original = identity().key().unwrap();
        let mut changed = identity();
        changed.project = "22222222-2222-3333-4444-555555555555".into();
        assert_ne!(original, changed.key().unwrap());
        assert!(lookup(&document(), &changed, 100).unwrap().is_none());
        changed = identity();
        changed.image = "rust:latest".into();
        assert!(changed.key().is_err());
        changed = identity();
        changed.workspace = "pdlc-common".into();
        assert!(changed.key().is_err());
        changed = identity();
        changed.daemon = "invalid\ndaemon".into();
        assert!(changed.key().is_err());
        for field in [
            "workspace",
            "daemon",
            "image",
            "base_revision",
            "lock_sha256",
            "platform",
        ] {
            let mut value = document();
            let replacement = match field {
                "workspace" => "pdlc2".to_string(),
                "daemon" => "another-daemon".to_string(),
                "image" => format!("sha256:{}", "e".repeat(64)),
                "base_revision" => "e".repeat(40),
                "lock_sha256" => "e".repeat(64),
                _ => "linux/arm64".to_string(),
            };
            value["bindings"][0][field] = json!(replacement);
            let other = Binding::deserialize(value["bindings"][0].clone())
                .unwrap()
                .identity();
            assert!(
                lookup(&document(), &other, 100).unwrap().is_none(),
                "{field}"
            );
        }
    }

    #[test]
    fn registration_rejects_changed_hash_unknown_fields_duplicates_and_budget_growth() {
        let document = document();
        assert!(lookup(&document, &identity(), 100).unwrap().is_some());
        assert!(lookup(&document, &identity(), 99).is_err());
        let payload = serde_json::to_vec(&document).unwrap();
        assert!(select(&payload, &"0".repeat(64), &identity(), 100).is_err());
        let mut invalid = document.clone();
        invalid["unknown"] = json!(true);
        assert!(lookup(&invalid, &identity(), 100).is_err());
        invalid = document.clone();
        invalid["bindings"]
            .as_array_mut()
            .unwrap()
            .push(document["bindings"][0].clone());
        assert!(lookup(&invalid, &identity(), 100).is_err());
        invalid = document.clone();
        invalid["seeds"]["d".repeat(64)]["volume"] = json!("old-unowned-volume");
        assert!(lookup(&invalid, &identity(), 100).is_err());
        // Two bindings may reference a single immutable public payload without doubling bytes.
        let mut shared = document.clone();
        let mut neighbor: Binding =
            serde_json::from_value(document["bindings"][0].clone()).unwrap();
        neighbor.workspace = "pdlc2".into();
        let mut binding = document["bindings"][0].clone();
        binding["workspace"] = json!("pdlc2");
        binding["key"] = json!(neighbor.identity().key().unwrap());
        shared["bindings"].as_array_mut().unwrap().push(binding);
        assert!(lookup(&shared, &identity(), 100).unwrap().is_some());
    }

    #[test]
    fn attachment_preserves_private_registry_and_executes_original_program_after_bootstrap() {
        assert!(tooling_budget().unwrap() > 0);
        let seed = lookup(&document(), &identity(), 100).unwrap().unwrap();
        let mut service = json!({"entrypoint":["sh","-ec","cargo test --locked -p domain --lib"],
            "volumes":[{"source":"cargo","target":"/usr/local/cargo/registry"}], "labels":{}});
        attach(&mut service, &seed).unwrap();
        assert_eq!(service["volumes"][0]["source"], "cargo");
        assert_eq!(service["volumes"][1]["read_only"], true);
        assert_eq!(service["volumes"][1]["volume"]["nocopy"], true);
        let arguments = service["entrypoint"].as_array().unwrap();
        assert_eq!(arguments[0], "python3");
        assert_eq!(arguments[1], "-I");
        assert_eq!(
            arguments[arguments.len() - 3..],
            [
                json!("sh"),
                json!("-ec"),
                json!("cargo test --locked -p domain --lib")
            ]
        );
        assert_eq!(
            service["labels"]["pdlc.cache.project"],
            seed.identity.project
        );
    }

    #[test]
    fn cache_volume_never_adopts_other_owner_driver_or_payload() {
        let seed = lookup(&document(), &identity(), 100).unwrap().unwrap();
        let resource = json!([{"Name":seed.volume,"Driver":"local","Options":null,"Labels":{
            "pdlc.cache.owner":"operator","pdlc.cache.schema":SEED_SCHEMA,
            "pdlc.cache.manifest-sha256":seed.manifest,"pdlc.cache.bytes":"100"}}]);
        assert!(volume_matches(&seed, &resource));
        for (field, value) in [
            ("Options", json!({"device":"/host"})),
            ("Driver", json!("nfs")),
            ("Name", json!("foreign")),
        ] {
            let mut invalid = resource.clone();
            invalid[0][field] = value;
            assert!(!volume_matches(&seed, &invalid));
        }
        let mut owned = resource.clone();
        owned[0]["Labels"]["pdlc.gateway.workspace"] = json!("pdlc1");
        assert!(!volume_matches(&seed, &owned));
    }

    #[test]
    fn lock_path_is_unique_regular_and_derived_from_workspace_bytes() {
        let directory =
            std::env::temp_dir().join(format!("forge-cache-lock-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        assert!(lock_digest(&directory).unwrap().is_none());
        fs::write(directory.join("Cargo.lock"), b"one").unwrap();
        assert_eq!(lock_digest(&directory).unwrap().unwrap(), sha(b"one"));
        fs::create_dir(directory.join("backend")).unwrap();
        fs::write(directory.join("backend/Cargo.lock"), b"two").unwrap();
        assert!(
            lock_digest(&directory).unwrap().is_none(),
            "ambiguous layouts must not guess a lock"
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
