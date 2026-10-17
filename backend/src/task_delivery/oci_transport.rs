//! Exact snapshot transport for the existing task OCI owner and configured TLS daemon.
//! Parent intent/Compose remain the custody boundary; partial writes never authorize replay.
use super::*;
use serde_json::{Value, json};

const PURPOSE: &str = "disposable-oci-transport";
const OPERATION_LABEL: &str = "forge.oci-operation";
const TRANSPORT_COMMAND: &str = "umask 077; exec sleep 86400";

// Arguments are data, never shell source. Check mode performs no filesystem writes.
// Publication uses a same-directory hard link with -T, never replacement or a symlink walk.
const SNAPSHOT_SCRIPT: &str = r#"
set -eu
set -f
umask 077
root=$1 relative=$2 expected=$3 size=$4 action=$5 staged=$6
fail() { printf 'snapshot rejected\n' >&2; exit 1; }
directory_ok() {
    test ! -L "$1" && test -d "$1" || fail
    test "$(stat -c %u -- "$1")" = "$(id -u)" || fail
    mode=$(stat -c %a -- "$1")
    test "$((0$mode & 022))" = 0 || fail
}
file_ok() {
    test ! -L "$1" && test -f "$1" || fail
    identity=$(stat -c '%d:%i:%u:%a:%s' -- "$1")
    test "$(stat -c %u -- "$1")" = "$(id -u)" || fail
    test "$(stat -c %a -- "$1")" = 444 || fail
    test "$(stat -c %s -- "$1")" = "$size" || fail
    observed=$(sha256sum -- "$1"); observed=${observed%% *}
    test "$observed" = "$expected" || fail
    test ! -L "$1" && test "$(stat -c '%d:%i:%u:%a:%s' -- "$1")" = "$identity" || fail
    directory_ok "$root"
    directory_ok "$parent"
    test "$(stat -c '%d:%i' -- "$root")" = "$root_identity" || fail
    test "$(stat -c '%d:%i' -- "$parent")" = "$parent_identity" || fail
}
case "$relative" in ''|/*|*/|*//*|*[!a-zA-Z0-9_./-]*) fail;; esac
case /$relative/ in */../*|*/./*|*/-*) fail;; esac
case "$action" in check|publish) :;; *) fail;; esac
directory_ok "$root"
root_identity=$(stat -c '%d:%i' -- "$root")
remaining=$relative parent=$root
while :; do
    part=${remaining%%/*}
    case "$part" in ''|.|..|-*) fail;; esac
    if test "$part" = "$remaining"; then break; fi
    next=$parent/$part
    if test -L "$next"; then fail; fi
    if ! test -e "$next"; then
        if test "$action" = check; then printf 'missing\n'; exit 0; fi
        mkdir -- "$next" || fail
    fi
    directory_ok "$next"
    parent=$next remaining=${remaining#*/}
done
parent_identity=$(stat -c '%d:%i' -- "$parent")
destination=$parent/$part
if test -e "$destination" || test -L "$destination"; then
    file_ok "$destination"
    printf 'present\n'
    exit 0
fi
if test "$action" = check; then printf 'missing\n'; exit 0; fi
test ! -L "$staged" && test -f "$staged" || fail
test "$(stat -c %u -- "$staged")" = "$(id -u)" || fail
test "$(stat -c %s -- "$staged")" = "$size" || fail
observed=$(sha256sum -- "$staged"); observed=${observed%% *}
test "$observed" = "$expected" || fail
temporary=$(mktemp "$parent/.forge-snapshot-XXXXXXXXXX")
trap 'rm -f -- "$temporary"' 0
trap 'exit 1' 1 2 15
cp -- "$staged" "$temporary"
chmod 0444 -- "$temporary"
file_ok "$temporary"
sync -f "$temporary"
ln -T -- "$temporary" "$destination" || file_ok "$destination"
sync -f "$parent"
file_ok "$destination"
printf 'present\n'
"#;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TransportIdentity {
    container_id: String,
    image_id: String,
    compose_sha256: String,
    manifest_sha256: String,
    data_sha256: String,
}

fn relative_snapshot(root: &Path, path: &Path) -> anyhow::Result<String> {
    plain(root)?;
    plain(path)?;
    let relative = path
        .canonicalize()?
        .strip_prefix(root.canonicalize()?)?
        .to_str()
        .context("snapshot path is not UTF-8")?
        .to_owned();
    ensure!(
        crate::domain::image_build::valid_relative_path(&relative, false)
            && relative
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_./-".contains(&b)),
        "snapshot must be an exact portable path under the owner volume root"
    );
    Ok(relative)
}

fn transport_definition(image: &str, operation_hash: &str) -> Value {
    json!({
        "image":image,"pull_policy":"never","restart":"no","user":"0:0",
        "entrypoint":["/bin/sh","-ec"],"command":[TRANSPORT_COMMAND],
        "read_only":true,"cap_drop":["ALL"],
        "security_opt":["no-new-privileges:true"],"pids_limit":64,
        "mem_limit":"128m","cpus":0.5,
        "tmpfs":["/tmp:rw,noexec,nosuid,nodev,size=4m,mode=0700"],
        "environment":{"PATH":"/usr/local/bin:/usr/bin:/bin"},
        "labels":{"sdlc.task":"forge-task-delivery","sdlc.purpose":PURPOSE,
            "forge.oci-operation":operation_hash},
        "volumes":[{"type":"volume","source":"data","target":"/target",
            "read_only":false,"volume":{"nocopy":true}}],
        "networks":["owner"]
    })
}

impl Owner {
    pub(super) async fn remote_transport_image(&self) -> anyhow::Result<String> {
        ensure!(
            self.task_client.is_some(),
            "task TLS transport is not configured"
        );
        let image = self
            .policy
            .transport_image
            .as_deref()
            .context("owner transport image absent")?;
        ensure!(
            crate::domain::image_build::valid_image_id(image),
            "exact transport image ID required"
        );
        self.engine().await?;
        self.host_path(&self.policy.data_file).await?;
        let actual = self.docker(&["image", "inspect", image]).await?;
        ensure!(
            actual.as_array().is_some_and(|items| items.len() == 1)
                && actual[0]["Id"] == image
                && actual[0]["Os"] == "linux"
                && actual[0]["Config"]["Volumes"]
                    .as_object()
                    .is_none_or(|v| v.is_empty()),
            "available exact Linux transport image without implicit volumes required"
        );
        Ok(image.into())
    }

    pub(super) fn transport_service(&self, image: &str, operation_hash: &str) -> Value {
        transport_definition(image, operation_hash)
    }

    fn transport_scope(&self, receipt: &Receipt) -> anyhow::Result<(PathBuf, PathBuf)> {
        ensure!(
            self.task_client.is_some() && receipt.command.task_fence.is_some(),
            "task transport only"
        );
        receipt.command.validate().map_err(anyhow::Error::msg)?;
        ensure!(
            receipt.original_operation.project_id == self.project,
            "foreign transport project"
        );
        let hash = sha(receipt.command.operation_key.as_bytes());
        let dir = operation_dir(&self.root, &receipt.command.operation_key);
        plain(&dir)?;
        ensure!(
            dir.canonicalize()? == dir,
            "transport operation path changed"
        );
        let intent: Intent = read(&dir.join("intent.json"))?;
        self.validate_receipt(receipt, &intent)?;
        let manifest = intent
            .manifest
            .as_ref()
            .context("transport manifest absent")?;
        let (schema, data) = self.data(&manifest.descriptor)?;
        ensure!(
            manifest.candidate.target_policy_sha256 == self.policy_hash
                && manifest.data_schema_version == schema
                && manifest.data_sha256 == data
                && receipt.manifest_sha256.as_ref() == Some(&sha(&serde_json::to_vec(manifest)?)),
            "transport differs from durable parent intent"
        );
        let file = self.policy.compose_root.join(format!("{hash}.json"));
        plain(&file)?;
        ensure!(
            file.canonicalize()? == file,
            "transport Compose path changed"
        );
        let bytes = read_bytes(&file, 128 * 1024)?;
        ensure!(
            bytes == read_bytes(&dir.join("compose.json"), 128 * 1024)?,
            "transport Compose differs"
        );
        let compose: Value = serde_json::from_slice(&bytes)?;
        let image = self
            .policy
            .transport_image
            .as_deref()
            .context("owner transport image absent")?;
        ensure!(
            crate::domain::image_build::valid_image_id(image),
            "exact transport image ID required"
        );
        ensure!(
            compose["services"]["transport"] == self.transport_service(image, &hash)
                && compose["volumes"]["data"]
                    == json!({"external":true,"name":self.policy.volume_name})
                && compose["networks"]["owner"]
                    == json!({"external":true,"name":self.policy.network_name}),
            "saved transport service, volume or network differs from owner policy"
        );
        Ok((dir, file))
    }

    fn transport_identity(
        &self,
        receipt: &Receipt,
        file: &Path,
        id: String,
    ) -> anyhow::Result<TransportIdentity> {
        Ok(TransportIdentity {
            container_id: id,
            image_id: self
                .policy
                .transport_image
                .clone()
                .context("owner transport image absent")?,
            compose_sha256: sha(&read_bytes(file, 128 * 1024)?),
            manifest_sha256: receipt
                .manifest_sha256
                .clone()
                .context("transport manifest hash absent")?,
            data_sha256: self.policy.data_sha256.clone(),
        })
    }

    async fn inspect_transport(
        &self,
        receipt: &Receipt,
        file: &Path,
        expected: Option<&str>,
    ) -> anyhow::Result<String> {
        self.engine().await?;
        let name = format!("{}-transport-1", self.policy.project_name);
        let actual = self.docker(&["container", "inspect", &name]).await?;
        ensure!(
            actual.as_array().is_some_and(|items| items.len() == 1),
            "transport inventory differs"
        );
        let container = &actual[0];
        let id = container["Id"].as_str().context("transport ID absent")?;
        ensure!(
            digest(id) && expected.is_none_or(|value| value == id),
            "transport ID changed"
        );
        let labels = &container["Config"]["Labels"];
        ensure!(
            container["Name"] == format!("/{name}")
                && container["Image"].as_str() == self.policy.transport_image.as_deref()
                && container["State"]["Running"] == true
                && labels["com.docker.compose.project"] == self.policy.project_name
                && labels["com.docker.compose.service"] == "transport"
                && labels["com.docker.compose.project.config_files"].as_str() == file.to_str()
                && labels["sdlc.task"] == "forge-task-delivery"
                && labels["sdlc.purpose"] == PURPOSE
                && labels[OPERATION_LABEL] == sha(receipt.command.operation_key.as_bytes()),
            "transport owner, operation, image or running state differs"
        );
        let host = &container["HostConfig"];
        ensure!(
            container["Config"]["User"] == "0:0"
                && container["Config"]["Entrypoint"] == json!(["/bin/sh", "-ec"])
                && container["Config"]["Cmd"] == json!([TRANSPORT_COMMAND])
                && host["ReadonlyRootfs"] == true
                && host["Privileged"] == false
                && host["CapDrop"] == json!(["ALL"])
                && host["SecurityOpt"]
                    .as_array()
                    .is_some_and(|values| values.iter().any(|v| v == "no-new-privileges:true"))
                && host["Memory"] == 128 * 1024 * 1024
                && host["PidsLimit"] == 64
                && host["PortBindings"]
                    .as_object()
                    .is_none_or(|v| v.is_empty())
                && host["Devices"].as_array().is_none_or(|v| v.is_empty())
                && host["VolumesFrom"].as_array().is_none_or(|v| v.is_empty()),
            "transport execution isolation differs"
        );
        let tmpfs = host["Tmpfs"]
            .as_object()
            .context("transport private tmpfs absent")?;
        let options = tmpfs
            .get("/tmp")
            .and_then(Value::as_str)
            .context("transport tmpfs path differs")?;
        let options = options.split(',').collect::<Vec<_>>();
        ensure!(
            tmpfs.len() == 1
                && ["noexec", "nosuid", "nodev"]
                    .iter()
                    .all(|flag| options.contains(flag))
                && options
                    .iter()
                    .any(|v| matches!(*v, "size=4m" | "size=4194304"))
                && options
                    .iter()
                    .any(|v| matches!(*v, "mode=0700" | "mode=700")),
            "transport tmpfs isolation differs"
        );
        let mounts = container["Mounts"]
            .as_array()
            .context("transport mounts absent")?;
        let volumes = mounts
            .iter()
            .filter(|m| m["Type"] == "volume")
            .collect::<Vec<_>>();
        ensure!(
            volumes.len() == 1
                && volumes[0]["Name"] == self.policy.volume_name
                && volumes[0]["Destination"] == "/target"
                && volumes[0]["RW"] == true
                && mounts.iter().all(|m| m["Type"] == "volume"
                    || (m["Type"] == "tmpfs" && m["Destination"] == "/tmp")),
            "transport must mount only the exact owner data volume"
        );
        // This performs the existing remote volume ownership check, without assuming shared bytes.
        self.host_path(&self.policy.data_file).await?;
        let volume = self
            .docker(&["volume", "inspect", &self.policy.volume_name])
            .await?;
        ensure!(
            volumes[0]["Source"] == volume[0]["Mountpoint"],
            "transport volume subpath differs"
        );
        let networks = container["NetworkSettings"]["Networks"]
            .as_object()
            .context("transport networks absent")?;
        ensure!(
            networks.len() == 1 && networks.contains_key(&self.policy.network_name),
            "transport network differs"
        );
        ensure!(
            container["Config"]["Env"]
                .as_array()
                .is_some_and(|values| values.iter().all(|value| {
                    value
                        .as_str()
                        .is_some_and(|v| !v.starts_with("DOCKER_") && !v.starts_with("CICD_"))
                })),
            "transport image contains backend authority environment"
        );
        Ok(id.into())
    }

    async fn snapshot_check(
        &self,
        id: &str,
        path: &str,
        hash: &str,
        size: usize,
    ) -> anyhow::Result<bool> {
        let bytes = self
            .run(
                &self.policy.docker_bin,
                &[
                    "exec".into(),
                    id.into(),
                    "/bin/sh".into(),
                    "-ec".into(),
                    SNAPSHOT_SCRIPT.into(),
                    "snapshot".into(),
                    "/target".into(),
                    path.into(),
                    hash.into(),
                    size.to_string(),
                    "check".into(),
                    "".into(),
                ],
                None,
            )
            .await?;
        match bytes.as_slice() {
            b"present\n" => Ok(true),
            b"missing\n" => Ok(false),
            _ => anyhow::bail!("remote snapshot readback differs"),
        }
    }

    async fn transport_write(
        &self,
        args: &[String],
        step: &Path,
        receipt: &Receipt,
        task: &mut Option<TaskAuthority<'_>>,
        compose: bool,
    ) -> anyhow::Result<Vec<u8>> {
        let authority = task
            .as_mut()
            .context("task transport write authority absent")?;
        authority
            .refresh(self, &receipt.command, &receipt.original_operation)
            .await?;
        // A pre-existing step is uncertain custody, not permission to execute it again.
        std::fs::create_dir(step).context("transport step exists or is unavailable; held")?;
        plain(step)?;
        sync_dir(step.parent().context("transport step parent absent")?)?;
        authority.permit.check()?;
        self.run(
            if compose {
                &self.policy.compose_bin
            } else {
                &self.policy.docker_bin
            },
            args,
            Some(step),
        )
        .await
    }

    pub(super) async fn transfer_snapshots(
        &self,
        execution_file: &Path,
        operation_dir: &Path,
        manifest_file: &Path,
        receipt: &Receipt,
        task: &mut Option<TaskAuthority<'_>>,
    ) -> anyhow::Result<String> {
        let (dir, file) = self.transport_scope(receipt)?;
        ensure!(
            operation_dir == dir && execution_file == file,
            "foreign transport effect paths"
        );
        let hash = receipt
            .manifest_sha256
            .as_deref()
            .context("manifest hash absent")?;
        ensure!(
            manifest_file == self.root.join("manifests").join(format!("{hash}.json")),
            "foreign manifest file"
        );
        let manifest = read_bytes(manifest_file, 128 * 1024)?;
        let data = read_bytes(&self.policy.data_file, 1024 * 1024)?;
        ensure!(
            sha(&manifest) == hash && sha(&data) == self.policy.data_sha256,
            "local snapshot bytes differ"
        );
        let snapshots = [
            (
                "manifest",
                manifest_file,
                relative_snapshot(&self.policy.volume_root, manifest_file)?,
                hash,
                manifest.len(),
            ),
            (
                "data",
                self.policy.data_file.as_path(),
                relative_snapshot(&self.policy.volume_root, &self.policy.data_file)?,
                self.policy.data_sha256.as_str(),
                data.len(),
            ),
        ];
        if dir.join("transport-ready.json").exists() {
            return self.validate_transport(receipt).await;
        }
        ensure!(
            !dir.join("transport-up").exists() && !dir.join("transport-down").exists(),
            "partial transport custody; held without replay"
        );
        self.remote_transport_image().await?;
        let existing = self
            .run(
                &self.policy.docker_bin,
                &[
                    "container".into(),
                    "ls".into(),
                    "--all".into(),
                    "--quiet".into(),
                    "--no-trunc".into(),
                    "--filter".into(),
                    format!("name=^/{}-transport-1$", self.policy.project_name),
                ],
                None,
            )
            .await?;
        ensure!(
            existing.iter().all(u8::is_ascii_whitespace),
            "existing transport holds target; never recreate or adopt"
        );
        let replicas = self
            .run(
                &self.policy.docker_bin,
                &[
                    "container".into(),
                    "ls".into(),
                    "--all".into(),
                    "--quiet".into(),
                    "--no-trunc".into(),
                    "--filter".into(),
                    format!(
                        "label=com.docker.compose.project={}",
                        self.policy.project_name
                    ),
                    "--filter".into(),
                    "label=com.docker.compose.service=transport".into(),
                ],
                None,
            )
            .await?;
        ensure!(
            replicas.iter().all(u8::is_ascii_whitespace),
            "retained transport replica holds target"
        );
        let up = dir.join("transport-up");
        self.transport_write(
            &[
                "-p".into(),
                self.policy.project_name.clone(),
                "-f".into(),
                file.to_string_lossy().into_owned(),
                "up".into(),
                "--detach".into(),
                "--no-build".into(),
                "--pull".into(),
                "never".into(),
                "--no-deps".into(),
                "transport".into(),
            ],
            &up,
            receipt,
            task,
            true,
        )
        .await?;
        let id = self.inspect_transport(receipt, &file, None).await?;
        let identity = self.transport_identity(receipt, &file, id.clone())?;
        immutable(&up.join("identity.json"), &identity)?;
        // Fail before copying unless the trusted image really provides the fixed tools.
        self.run(&self.policy.docker_bin, &[
            "exec".into(),id.clone(),"/bin/sh".into(),"-ec".into(),
            "for tool in stat id sha256sum mktemp cp chmod sync ln rm mkdir curl; do command -v \"$tool\" >/dev/null || exit 1; done; test ! -e /tmp/manifest && test ! -L /tmp/manifest && test ! -e /tmp/data && test ! -L /tmp/data".into(),
        ], None).await?;
        for (name, source, relative, hash, size) in snapshots {
            self.inspect_transport(receipt, &file, Some(&id)).await?;
            if self.snapshot_check(&id, &relative, hash, size).await? {
                continue;
            }
            self.transport_write(
                &[
                    "cp".into(),
                    source.to_string_lossy().into_owned(),
                    format!("{id}:/tmp/{name}"),
                ],
                &up.join(format!("{name}-copy")),
                receipt,
                task,
                false,
            )
            .await?;
            self.inspect_transport(receipt, &file, Some(&id)).await?;
            let observed = self
                .transport_write(
                    &[
                        "exec".into(),
                        id.clone(),
                        "/bin/sh".into(),
                        "-ec".into(),
                        SNAPSHOT_SCRIPT.into(),
                        "snapshot".into(),
                        "/target".into(),
                        relative,
                        hash.into(),
                        size.to_string(),
                        "publish".into(),
                        format!("/tmp/{name}"),
                    ],
                    &up.join(format!("{name}-publish")),
                    receipt,
                    task,
                    false,
                )
                .await?;
            ensure!(
                observed == b"present\n",
                "remote publication readback differs"
            );
        }
        self.inspect_transport(receipt, &file, Some(&id)).await?;
        immutable(&dir.join("transport-ready.json"), &identity)?;
        Ok(id)
    }

    fn ready_transport(&self, receipt: &Receipt) -> anyhow::Result<(PathBuf, TransportIdentity)> {
        let (dir, file) = self.transport_scope(receipt)?;
        ensure!(
            !dir.join("transport-down").exists(),
            "transport cleanup started; held"
        );
        let saved: TransportIdentity = read(&dir.join("transport-ready.json"))?;
        ensure!(
            saved == read::<TransportIdentity>(&dir.join("transport-up/identity.json"))?,
            "transport ready custody differs"
        );
        ensure!(
            saved == self.transport_identity(receipt, &file, saved.container_id.clone())?,
            "transport original custody differs"
        );
        Ok((file, saved))
    }

    pub(super) async fn validate_transport(&self, receipt: &Receipt) -> anyhow::Result<String> {
        let (_, saved) = self.ready_transport(receipt)?;
        let manifest = self
            .root
            .join("manifests")
            .join(format!("{}.json", saved.manifest_sha256));
        self.verify_remote_snapshots(&saved.container_id, &manifest, receipt)
            .await?;
        Ok(saved.container_id)
    }

    pub(super) async fn verify_remote_snapshots(
        &self,
        helper: &str,
        manifest_file: &Path,
        receipt: &Receipt,
    ) -> anyhow::Result<()> {
        let (file, saved) = self.ready_transport(receipt)?;
        ensure!(
            helper == saved.container_id,
            "snapshot query helper differs from original custody"
        );
        let expected_manifest = self
            .root
            .join("manifests")
            .join(format!("{}.json", saved.manifest_sha256));
        ensure!(
            manifest_file == expected_manifest,
            "snapshot query manifest differs"
        );
        self.inspect_transport(receipt, &file, Some(helper)).await?;
        for (path, hash, limit) in [
            (manifest_file, saved.manifest_sha256.as_str(), 128 * 1024),
            (
                self.policy.data_file.as_path(),
                saved.data_sha256.as_str(),
                1024 * 1024,
            ),
        ] {
            let bytes = read_bytes(path, limit)?;
            ensure!(sha(&bytes) == hash, "local transport snapshot drift");
            let relative = relative_snapshot(&self.policy.volume_root, path)?;
            ensure!(
                self.snapshot_check(helper, &relative, hash, bytes.len())
                    .await?,
                "retained remote snapshot missing; held"
            );
        }
        Ok(())
    }

    pub(super) async fn stop_transport(
        &self,
        execution_file: &Path,
        operation_dir: &Path,
        receipt: &Receipt,
    ) -> anyhow::Result<()> {
        let (dir, file) = self.transport_scope(receipt)?;
        ensure!(
            execution_file == file && operation_dir == dir,
            "foreign transport cleanup paths"
        );
        ensure!(
            receipt.status != DeliveryStatus::Unknown,
            "unknown outcome retains transport"
        );
        let result = optional::<Receipt>(&dir.join("reconciled.json"))?
            .or(optional::<Receipt>(&dir.join("result.json"))?);
        let result = result.context("persist known application result before transport cleanup")?;
        ensure!(
            serde_json::to_vec(&result)? == serde_json::to_vec(receipt)?,
            "saved application result differs before cleanup"
        );
        let id = self.validate_transport(receipt).await?;
        let down = dir.join("transport-down");
        std::fs::create_dir(&down).context("transport cleanup exists or is unavailable; held")?;
        sync_dir(&dir)?;
        // Cleanup of the exact already-owned helper needs no new deployment grant.
        // Use immutable ID, not Compose service selection which could include another replica.
        let stop = down.join("stop");
        std::fs::create_dir(&stop)?;
        sync_dir(&down)?;
        self.run(
            &self.policy.docker_bin,
            &["stop".into(), "--time".into(), "2".into(), id.clone()],
            Some(&stop),
        )
        .await?;
        let actual = self.docker(&["container", "inspect", &id]).await?;
        ensure!(
            actual[0]["Id"] == id && actual[0]["State"]["Running"] == false,
            "transport stop remains unconfirmed"
        );
        let remove = down.join("remove");
        std::fs::create_dir(&remove)?;
        sync_dir(&down)?;
        self.run(
            &self.policy.docker_bin,
            &["rm".into(), id.clone()],
            Some(&remove),
        )
        .await?;
        self.engine().await?;
        let remaining = self
            .run(
                &self.policy.docker_bin,
                &[
                    "container".into(),
                    "ls".into(),
                    "--all".into(),
                    "--quiet".into(),
                    "--no-trunc".into(),
                    "--filter".into(),
                    format!(
                        "label=com.docker.compose.project={}",
                        self.policy.project_name
                    ),
                    "--filter".into(),
                    "label=com.docker.compose.service=transport".into(),
                ],
                None,
            )
            .await?;
        ensure!(
            remaining.iter().all(u8::is_ascii_whitespace),
            "transport remains after cleanup"
        );
        immutable(&down.join("removed.json"), &id)
    }
}

#[cfg(test)]
mod policy_tests {
    use super::*;

    #[test]
    fn optional_transport_image_preserves_legacy_bytes_and_binds_explicit_image_hash() {
        let legacy = r#"{"projectName":"sdlc-qa-forge-oci-test","networkName":"owner","daemonId":"test-daemon","dockerBin":"docker","composeBin":"compose","composeRoot":"/owner/compose","volumeName":"data","volumeRoot":"/owner","dataFile":"/owner/data.json","dataSha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","checks":{"origin":"http://127.0.0.1:8000","healthPath":"/health","healthBodySha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","acceptancePath":"/acceptance","acceptanceBodySha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"}}"#.as_bytes();
        let mut policy: Policy = serde_json::from_slice(legacy).unwrap();
        assert!(policy.transport_image.is_none());
        let none_bytes = serde_json::to_vec(&policy).unwrap();
        assert_eq!(none_bytes, legacy);
        assert_eq!(sha(&none_bytes), sha(legacy));
        let none_value = serde_json::to_value(&policy).unwrap();
        assert!(none_value.get("transportImage").is_none());
        let roundtrip: Policy = serde_json::from_slice(&none_bytes).unwrap();
        assert!(roundtrip.transport_image.is_none());
        assert_eq!(serde_json::to_vec(&roundtrip).unwrap(), legacy);

        let image = format!("sha256:{}", "d".repeat(64));
        policy.transport_image = Some(image.clone());
        let explicit_bytes = serde_json::to_vec(&policy).unwrap();
        assert_eq!(
            serde_json::to_value(&policy).unwrap()["transportImage"],
            image
        );
        assert_ne!(sha(&explicit_bytes), sha(legacy));
        let roundtrip: Policy = serde_json::from_slice(&explicit_bytes).unwrap();
        assert_eq!(roundtrip.transport_image.as_deref(), Some(image.as_str()));
        assert_eq!(serde_json::to_vec(&roundtrip).unwrap(), explicit_bytes);
        assert_eq!(
            sha(&serde_json::to_vec(&roundtrip).unwrap()),
            sha(&explicit_bytes)
        );
        policy.transport_image = Some(format!("sha256:{}", "e".repeat(64)));
        assert_ne!(
            sha(&serde_json::to_vec(&policy).unwrap()),
            sha(&explicit_bytes)
        );
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    struct Files {
        base: PathBuf,
        root: PathBuf,
        staged: PathBuf,
    }

    impl Files {
        fn new() -> Self {
            let base = std::env::temp_dir().join(format!("forge-oci-transport-{}", Uuid::new_v4()));
            std::fs::create_dir(&base).unwrap();
            std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700)).unwrap();
            let root = base.join("target");
            std::fs::create_dir(&root).unwrap();
            let staged = base.join("staged");
            std::fs::write(&staged, b"snapshot\n").unwrap();
            Self { base, root, staged }
        }

        fn run(&self, path: &str, action: &str, hash: &str, size: usize) -> std::process::Output {
            std::process::Command::new("/bin/sh")
                .args(["-ec", SNAPSHOT_SCRIPT, "snapshot"])
                .arg(&self.root)
                .arg(path)
                .arg(hash)
                .arg(size.to_string())
                .arg(action)
                .arg(&self.staged)
                .output()
                .unwrap()
        }

        fn matching(&self, path: &str, action: &str) -> std::process::Output {
            self.run(path, action, &sha(b"snapshot\n"), b"snapshot\n".len())
        }
    }

    impl Drop for Files {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.base).unwrap();
        }
    }

    #[test]
    fn transport_definition_has_only_data_mount_owner_network_and_no_authority() {
        let image = format!("sha256:{}", "a".repeat(64));
        let service = transport_definition(&image, &"b".repeat(64));
        assert_eq!(service["image"], image);
        assert_eq!(service["networks"], json!(["owner"]));
        assert_eq!(
            service["volumes"],
            json!([{"type":"volume","source":"data","target":"/target","read_only":false,"volume":{"nocopy":true}}])
        );
        assert_eq!(service["read_only"], true);
        assert_eq!(service["cap_drop"], json!(["ALL"]));
        assert_eq!(
            service["environment"],
            json!({"PATH":"/usr/local/bin:/usr/bin:/bin"})
        );
        for key in [
            "ports",
            "privileged",
            "secrets",
            "devices",
            "volumes_from",
            "build",
        ] {
            assert!(service.get(key).is_none(), "unexpected authority: {key}");
        }
    }

    #[test]
    fn snapshot_check_is_readonly_and_rejects_unsafe_paths_before_publication() {
        let f = Files::new();
        let missing = f.matching("new/path.json", "check");
        assert!(missing.status.success());
        assert_eq!(missing.stdout, b"missing\n");
        assert!(!f.root.join("new").exists());
        for path in [
            "../escape",
            "new/../escape",
            "./data",
            "/absolute",
            "new//data",
            "new/-option",
            "new/$HOME",
        ] {
            assert!(
                !f.matching(path, "publish").status.success(),
                "accepted {path}"
            );
        }
        assert_eq!(std::fs::read_dir(&f.root).unwrap().count(), 0);
    }

    #[test]
    fn snapshot_publication_is_readonly_atomic_and_matching_readback_is_idempotent() {
        let f = Files::new();
        let first = f.matching("manifests/pinned.json", "publish");
        assert!(
            first.status.success(),
            "{}",
            String::from_utf8_lossy(&first.stderr)
        );
        let path = f.root.join("manifests/pinned.json");
        assert_eq!(std::fs::read(&path).unwrap(), b"snapshot\n");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o444
        );
        std::fs::write(&f.staged, b"different staged bytes").unwrap();
        assert!(
            f.matching("manifests/pinned.json", "publish")
                .status
                .success()
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"snapshot\n");
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
        assert!(
            f.matching("manifests/pinned.json", "check")
                .status
                .success()
        );
    }

    #[test]
    fn snapshot_conflicts_links_and_writable_existing_files_are_never_repaired() {
        use std::os::unix::fs::symlink;
        let f = Files::new();
        let victim = f.base.join("victim");
        std::fs::write(&victim, b"keep\n").unwrap();
        symlink(&victim, f.root.join("linked.json")).unwrap();
        symlink(&f.base, f.root.join("linked-dir")).unwrap();
        std::fs::create_dir(f.root.join("directory.json")).unwrap();
        let conflict = f.root.join("conflict.json");
        std::fs::write(&conflict, b"keep\n").unwrap();
        std::fs::set_permissions(&conflict, std::fs::Permissions::from_mode(0o444)).unwrap();
        std::fs::write(f.root.join("writable.json"), b"snapshot\n").unwrap();
        for path in [
            "linked.json",
            "linked-dir/new.json",
            "directory.json",
            "conflict.json",
            "writable.json",
        ] {
            assert!(
                !f.matching(path, "publish").status.success(),
                "accepted {path}"
            );
        }
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep\n");
        assert_eq!(std::fs::read(&conflict).unwrap(), b"keep\n");
        assert!(!f.base.join("new.json").exists());
    }

    #[test]
    fn snapshot_rejects_corrupt_staging_and_local_scope_escape() {
        let f = Files::new();
        assert!(
            !f.run("bad-hash.json", "publish", &"a".repeat(64), 9)
                .status
                .success()
        );
        assert!(
            !f.run("bad-size.json", "publish", &sha(b"snapshot\n"), 8)
                .status
                .success()
        );
        assert!(!f.root.join("bad-hash.json").exists());
        assert!(!f.root.join("bad-size.json").exists());
        assert!(relative_snapshot(&f.root, &f.staged).is_err());
        let local = f.root.join("owned.json");
        std::fs::write(&local, b"snapshot\n").unwrap();
        assert_eq!(relative_snapshot(&f.root, &local).unwrap(), "owned.json");
        std::os::unix::fs::symlink(&local, f.root.join("alias.json")).unwrap();
        assert!(relative_snapshot(&f.root, &f.root.join("alias.json")).is_err());
    }

    #[test]
    fn readonly_snapshot_query_detects_late_remote_byte_change() {
        let f = Files::new();
        assert!(f.matching("data.json", "publish").status.success());
        assert!(f.matching("data.json", "check").status.success());
        let data = f.root.join("data.json");
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::write(&data, b"changed!\n").unwrap();
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o444)).unwrap();
        assert!(!f.matching("data.json", "check").status.success());
        assert_eq!(std::fs::read(&data).unwrap(), b"changed!\n");
    }
}
