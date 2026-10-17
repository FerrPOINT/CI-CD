use super::*;
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};

fn reset_oci_build_progress(temp: &std::path::Path) {
    let directory = temp.join("oci-build-progress");
    let pending = directory.join("next");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&pending)
        .unwrap();
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .unwrap();
    file.write_all(b"{\"schema\":\"forge/oci-build-progress/v1\",\"phase\":\"not_started\"}\n")
        .unwrap();
    drop(file);
    std::fs::rename(pending, directory.join("state.json")).unwrap();
}

fn oci_build_timeout_diagnostic(temp: &std::path::Path) {
    // Treat this only as the last published phase, never as acceptance evidence.
    let read = (|| -> std::io::Result<Vec<u8>> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(temp.join("oci-build-progress/state.json"))?;
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.len() > 128
            || metadata.permissions().mode() & 0o777 != 0o600
        {
            return Err(std::io::ErrorKind::InvalidData.into());
        }
        let mut bytes = Vec::new();
        file.take(129).read_to_end(&mut bytes)?;
        Ok(bytes)
    })();
    let phase = match read.as_deref() {
        Ok(b"{\"schema\":\"forge/oci-build-progress/v1\",\"phase\":\"not_started\"}\n") => {
            "not_started"
        }
        Ok(b"{\"schema\":\"forge/oci-build-progress/v1\",\"phase\":\"prebuild\"}\n") => "prebuild",
        Ok(b"{\"schema\":\"forge/oci-build-progress/v1\",\"phase\":\"build\"}\n") => "build",
        Ok(b"{\"schema\":\"forge/oci-build-progress/v1\",\"phase\":\"postbuild\"}\n") => {
            "postbuild"
        }
        Ok(b"{\"schema\":\"forge/oci-build-progress/v1\",\"phase\":\"completed\"}\n") => {
            "completed"
        }
        Ok(_) => "invalid",
        Err(_) => "unavailable",
    };
    eprintln!("OCI_RUNNER_COMPLETION_TIMEOUT:last_progress={phase}");
}

// Retain only closed rejection categories before the fixture's temporary state is dropped.
const OCI_CLI_FAILURE_DIAGNOSTIC: &str = r#"
import json
import os
from pathlib import Path
import re
import stat
import sys
import uuid

LIMIT = 16384
REJECTIONS = (
    ('blocked', 'unsupported_mutable_data_or_migration'),
    ('blocked', 'owner_data_snapshot_drift'),
    ('blocked', 'actual_data_schema_incompatible'),
    ('unknown_or_rejected', 'oci_command_rejected_or_unavailable'),
)
CHILD_ERROR_CLASSES = (
    'ValueError', 'RuntimeError', 'OSError', 'TimeoutError', 'FileNotFoundError',
    'PermissionError', 'AssertionError', 'JSONDecodeError', 'KeyError', 'TypeError',
    'TimeoutExpired', 'DiskSpaceGuard', 'DaemonIdentityGuard', 'ExistingResourcesGuard', 'unknown',
)

def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('duplicate key')
        result[key] = value
    return result

def checkpoint(root, command_hash, compose_root):
    # Metadata presence only: neither record validity nor a coherent execution snapshot.
    try:
        root, compose_root = Path(root), Path(compose_root)
        if (not re.fullmatch('[a-f0-9]{64}', command_hash)
                or any(not path.is_absolute() or '..' in path.parts
                       or len(str(path)) > 4096 for path in (root, compose_root))):
            return 'unavailable'
        operation = root / 'operations' / command_hash
        paths = (root / 'owner.json', operation / 'intent.json', operation / 'compose.json',
                 compose_root / (command_hash + '.json'), operation / 'child.json',
                 operation / 'result.json')
        def present(path):
            for parent in reversed(path.parents):
                try:
                    metadata = parent.lstat()
                except FileNotFoundError:
                    return False
                if (not stat.S_ISDIR(metadata.st_mode)
                        or getattr(metadata, 'st_file_attributes', 0) & 1024):
                    raise ValueError('unavailable')
            try:
                metadata = path.lstat()
            except FileNotFoundError:
                return False
            if (not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1
                    or getattr(metadata, 'st_file_attributes', 0) & 1024):
                raise ValueError('unavailable')
            return True
        observed = [present(path) for path in paths]
        count = sum(observed)
        if observed != [True] * count + [False] * (len(paths) - count):
            return 'unavailable'
        return ('none', 'owner', 'intent', 'compose', 'request', 'child', 'result')[count]
    except (OSError, ValueError, TypeError):
        return 'unavailable'

def project(payload, exit_code):
    result = {'schema': 'forge/oci-cli-failure-diagnostic/v1',
              'scope': 'fixture_observation_only', 'acceptanceVerified': False,
              'exitCode': exit_code if type(exit_code) is int and 0 <= exit_code <= 255 else None}
    if len(payload) > LIMIT:
        kind = 'oversized'
    elif not payload:
        kind = 'empty'
    else:
        try:
            value = json.loads(payload, object_pairs_hook=unique_object)
        except (ValueError, UnicodeError, RecursionError):
            kind = 'invalid_json'
        else:
            if not isinstance(value, dict) or value.get('schema') != 'forge/local-oci-rejection/v1':
                kind = 'other_document'
            elif (value.get('dispatchAllowed') is not False
                  or value.get('sdlcAcceptanceVerified') is not False
                  or (value.get('status'), value.get('reason')) not in REJECTIONS):
                kind = 'invalid_rejection'
            else:
                kind = 'rejection'
                result.update(status=value['status'], reason=value['reason'])
                # Optional QA observation; never promote it to reason, authority or success.
                if ((value['status'], value['reason']) == REJECTIONS[-1]
                        and type(exit_code) is int and 1 <= exit_code <= 255
                        and 'childErrorClass' in value):
                    child_class = value['childErrorClass']
                    result['childErrorClass'] = (child_class if type(child_class) is str
                        and child_class in CHILD_ERROR_CLASSES else 'unknown')
    result['payloadKind'] = kind
    return result

def retain(directory, diagnostic):
    try:
        os.mkdir(directory, 0o700)
    except FileExistsError:
        pass
    fd = os.open(directory, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        metadata = os.fstat(fd)
        if metadata.st_uid != os.getuid() or stat.S_IMODE(metadata.st_mode) != 0o700:
            raise ValueError('private directory required')
        output = os.open(uuid.uuid4().hex + '.json',
                         os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                         0o600, dir_fd=fd)
        with os.fdopen(output, 'wb') as stream:
            os.fchmod(stream.fileno(), 0o600)
            stream.write((json.dumps(diagnostic, separators=(',', ':')) + '\n').encode('ascii'))
            stream.flush()
            os.fsync(stream.fileno())
    finally:
        os.close(fd)

if __name__ == '__main__':
    try:
        code = None if sys.argv[1] == 'signal' else int(sys.argv[1])
        diagnostic = project(sys.stdin.buffer.read(LIMIT + 1), code)
        diagnostic['checkpoint'] = checkpoint(*sys.argv[2:]) if len(sys.argv) == 5 else 'unavailable'
        print('OCI_CLI_FAILURE_DIAGNOSTIC:' + json.dumps(diagnostic, separators=(',', ':')),
              file=sys.stderr)
        retain('/output/oci-cli-diagnostics', diagnostic)
    except Exception:
        print('OCI_CLI_FAILURE_DIAGNOSTIC_UNAVAILABLE', file=sys.stderr)
"#;

fn oci_cli_failure_diagnostic(
    output: &std::process::Output,
    root: &std::path::Path,
    command: &DeliveryCommand,
    compose_root: &std::path::Path,
) {
    let retained = (|| -> std::io::Result<()> {
        let mut child = std::process::Command::new("python3")
            .args(["-I", "-B", "-c", OCI_CLI_FAILURE_DIAGNOSTIC])
            .arg(
                output
                    .status
                    .code()
                    .map_or_else(|| "signal".to_owned(), |code| code.to_string()),
            )
            .arg(root)
            .arg(hash(command.operation_key.as_bytes()))
            .arg(compose_root)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()?;
        let written = child
            .stdin
            .take()
            .unwrap()
            .write_all(&output.stdout[..output.stdout.len().min(16385)]);
        let status = child.wait()?;
        written?;
        if !status.success() {
            return Err(std::io::ErrorKind::Other.into());
        }
        Ok(())
    })();
    if retained.is_err() {
        eprintln!("OCI_CLI_FAILURE_DIAGNOSTIC_UNAVAILABLE");
    }
}

// Named OCI contexts use local session blobs, not the registry image resolver.
// Keep the export outside Git and never create/remove Docker image aliases.
const OCI_OFFLINE_BASE_GUARD: &str = r#"
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile

PIN = 'sha256:e91fec3d1ac69f04e4eddcd29c327e630ce34658cf31075bfa7e8b0e052bafea'
SOURCE = 'python:3.12-bookworm@' + PIN
DIGEST_RE = re.compile(r'sha256:[0-9a-f]{64}')
PHASE = 'preflight'

class DockerFailure(Exception):
    pass

def phase(name):
    global PHASE
    require(name in ('preflight', 'prepare_identity', 'export', 'preserve',
                     'archive', 'validate', 'persist_identity', 'prebuild',
                     'build', 'postbuild', 'completed'))
    PHASE = name

def require(condition):
    if not condition:
        raise ValueError('offline base guard')

def progress(layout, name):
    require(name in ('prebuild', 'build', 'postbuild', 'completed'))
    data = (json.dumps({'schema': 'forge/oci-build-progress/v1', 'phase': name},
                       separators=(',', ':')) + '\n').encode('ascii')
    pending = layout.parent / 'oci-build-progress' / 'next'
    created = False
    try:
        fd = os.open(pending, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        created = True
        with os.fdopen(fd, 'wb') as output:
            os.chmod(pending, 0o600)
            output.write(data)
        os.replace(pending, pending.parent / 'state.json')
    except OSError:
        # Optional diagnostics must not replace a build/validation exception.
        print('OCI_BUILD_PROGRESS_WRITE_FAILED', file=sys.stderr)
    finally:
        if created:
            try:
                pending.unlink(missing_ok=True)
            except OSError:
                print('OCI_BUILD_PROGRESS_WRITE_FAILED', file=sys.stderr)

def docker_env(layout):
    env = os.environ.copy()
    for key in ('DOCKER_CONTEXT', 'DOCKER_TLS_VERIFY', 'DOCKER_CERT_PATH',
                'BUILDX_BUILDER', 'BUILDKIT_HOST', 'BUILDKIT_SYNTAX'):
        env.pop(key, None)
    env.update(DOCKER_HOST='unix:///var/run/docker.sock', DOCKER_BUILDKIT='1',
               DOCKER_CONFIG=str(layout.parent / 'offline-docker-config'))
    return env

def docker(layout, *args, stdout=subprocess.PIPE):
    result = subprocess.run(['/usr/local/bin/docker', *args], env=docker_env(layout),
                            stdout=stdout, stderr=subprocess.PIPE, timeout=25)
    if result.returncode != 0:
        raise DockerFailure()
    return result.stdout

def identity(layout, daemon):
    require(isinstance(daemon, str) and bool(daemon))
    require(json.loads(docker(layout, 'info', '--format', '{{json .ID}}')) == daemon)
    images = json.loads(docker(layout, 'image', 'inspect', SOURCE))
    require(len(images) == 1)
    image = images[0]
    require(DIGEST_RE.fullmatch(image['Id']) is not None)
    require((image['Os'], image['Architecture']) == ('linux', 'amd64'))
    digests = image.get('RepoDigests') or []
    require(any(ref in digests for ref in ('python@' + PIN, 'docker.io/library/python@' + PIN)))
    return {'id': image['Id'], 'tags': sorted(image.get('RepoTags') or []),
            'digests': sorted(digests)}

def preserve(original, current):
    require(original['id'] == current['id'])
    for key in ('tags', 'digests'):
        require(set(original[key]).issubset(current[key]))

def unpack(archive, layout):
    layout.mkdir(mode=0o700)
    seen = set()
    total = 0
    with tarfile.open(archive, 'r:') as source:
        for member in source:
            name = member.name
            require(name not in seen)
            seen.add(name)
            require(len(seen) <= 1024)
            if member.isdir():
                require(name.rstrip('/') in ('blobs', 'blobs/sha256'))
                continue
            require(member.isfile())
            require(name in ('oci-layout', 'index.json', 'manifest.json') or
                    re.fullmatch(r'blobs/sha256/[0-9a-f]{64}', name) is not None)
            total += member.size
            require(0 <= member.size <= 2**31 and total <= 2**31)
            if name == 'manifest.json':
                continue  # Docker compatibility metadata is not an OCI build input.
            target = layout / name
            target.parent.mkdir(parents=True, exist_ok=True)
            with source.extractfile(member) as reader, target.open('xb') as writer:
                shutil.copyfileobj(reader, writer)

def blob(layout, descriptor):
    digest = descriptor['digest']
    require(DIGEST_RE.fullmatch(digest) is not None)
    require(not descriptor.get('urls'))
    path = layout / 'blobs' / 'sha256' / digest[7:]
    require(not path.is_symlink() and path.is_file())
    require(path.stat().st_size == descriptor['size'])
    with path.open('rb') as reader:
        require('sha256:' + hashlib.file_digest(reader, 'sha256').hexdigest() == digest)
    return path

def validate(layout):
    require(not layout.is_symlink())
    for name in ('oci-layout', 'index.json', 'blobs', 'blobs/sha256'):
        require(not (layout / name).is_symlink())
    require(json.loads((layout / 'oci-layout').read_bytes()) == {'imageLayoutVersion': '1.0.0'})
    index = json.loads((layout / 'index.json').read_bytes())
    require(index['schemaVersion'] == 2 and len(index['manifests']) == 1)
    root = index['manifests'][0]
    require(root['digest'] == PIN)
    manifest = json.loads(blob(layout, root).read_bytes())
    if 'manifests' in manifest:
        matches = [d for d in manifest['manifests'] if
                   d.get('platform', {}).get('os') == 'linux' and
                   d.get('platform', {}).get('architecture') == 'amd64' and
                   d.get('platform', {}).get('variant', '') in ('', 'v1')]
        require(len(matches) == 1)
        manifest = json.loads(blob(layout, matches[0]).read_bytes())
    require(manifest['schemaVersion'] == 2)
    config = json.loads(blob(layout, manifest['config']).read_bytes())
    require((config['os'], config['architecture']) == ('linux', 'amd64'))
    require(len(manifest['layers']) > 0)
    for layer in manifest['layers']:
        blob(layout, layer)

def check(layout, daemon):
    original = json.loads((layout.parent / 'offline-base-identity.json').read_bytes())
    preserve(original, identity(layout, daemon))
    validate(layout)

def prepare(layout, daemon):
    (layout.parent / 'offline-docker-config').mkdir(mode=0o700)
    phase('prepare_identity')
    original = identity(layout, daemon)
    archive = layout.parent / 'offline-base.tar'
    try:
        with archive.open('xb') as output:
            phase('export')
            docker(layout, 'image', 'save', SOURCE, stdout=output)
        phase('preserve')
        preserve(original, identity(layout, daemon))
        phase('archive')
        unpack(archive, layout)
        phase('validate')
        validate(layout)
        phase('persist_identity')
        state = layout.parent / 'offline-base-identity.json'
        with state.open('x') as output:
            json.dump(original, output)
        check(layout, daemon)
    finally:
        archive.unlink(missing_ok=True)

def build(layout, daemon, label):
    phase('prebuild')
    progress(layout, 'prebuild')
    check(layout, daemon)
    try:
        phase('build')
        progress(layout, 'build')
        # default is the embedded docker driver on the explicitly selected daemon.
        result = subprocess.run([
            '/usr/local/bin/docker', 'buildx', 'build', '--builder', 'default',
            '--pull=false', '--network=none', '--platform=linux/amd64',
            '--build-context', 'offline-python-base=oci-layout://' + str(layout) + '@' + PIN,
            '--label', 'org.opencontainers.image.revision=' + label, '--iidfile', 'image-id',
            '.'], env=docker_env(layout))
        require(result.returncode == 0)
    finally:
        phase('postbuild')
        progress(layout, 'postbuild')
        check(layout, daemon)
    phase('completed')
    progress(layout, 'completed')

if __name__ == '__main__':
    try:
        action, directory, daemon = sys.argv[1:4]
        layout = Path(directory)
        if action == 'prepare':
            require(len(sys.argv) == 4)
            prepare(layout, daemon)
        else:
            require(action == 'build' and len(sys.argv) == 5)
            build(layout, daemon, sys.argv[4])
    except Exception as error:
        reason = ('command_timeout' if isinstance(error, subprocess.TimeoutExpired) else
                  'command_failed' if isinstance(error, DockerFailure) else
                  'validation_rejected' if isinstance(error, ValueError) else
                  'local_io' if isinstance(error, OSError) else 'invalid_document')
        sys.exit('OCI_OFFLINE_BASE_GUARD_FAILED:' + PHASE + ':' + reason)
"#;

fn oci_fixture_paths(id: Uuid, compose_parent: &std::path::Path) -> (PathBuf, PathBuf, PathBuf) {
    let suffix = id.simple();
    (
        PathBuf::from(format!("/delivery-qa/oci-root-{suffix}")),
        PathBuf::from(format!("/delivery-qa/oci-data-{suffix}.json")),
        compose_parent.join(format!("oci-{suffix}")),
    )
}

#[test]
fn oci_fixture_paths_isolate_retained_journals_data_and_compose_within_existing_mappings() {
    let volume = std::path::Path::new("/delivery-qa");
    let compose_parent = std::path::Path::new("/owned/oci-compose");
    let first = oci_fixture_paths(Uuid::from_u128(1), compose_parent);
    let second = oci_fixture_paths(Uuid::from_u128(2), compose_parent);
    assert_eq!(first, oci_fixture_paths(Uuid::from_u128(1), compose_parent));
    for paths in [&first, &second] {
        assert_eq!(paths.0.parent(), Some(volume));
        assert_eq!(paths.1.parent(), Some(volume));
        assert!(!paths.1.starts_with(&paths.0));
        assert_eq!(paths.2.parent(), Some(compose_parent));
    }
    for first_path in [&first.0, &first.1, &first.2] {
        for second_path in [&second.0, &second.1, &second.2] {
            assert_ne!(first_path, second_path);
            assert!(!first_path.starts_with(second_path));
            assert!(!second_path.starts_with(first_path));
        }
    }
}

struct Oci {
    t: ActualDelivery,
    root: PathBuf,
    data: PathBuf,
    policy: PathBuf,
    compose_root: PathBuf,
    project: String,
    base_guard: PathBuf,
    base_layout: PathBuf,
    daemon: String,
}
impl Drop for Oci {
    fn drop(&mut self) {
        if let Ok(entries) = std::fs::read_dir(&self.compose_root) {
            let mut files = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.exists())
                .collect::<Vec<_>>();
            files.sort_by_key(|p| std::fs::metadata(p).unwrap().modified().unwrap());
            if let Some(file) = files.last() {
                std::fs::copy(file, "/output/oci-compose.json").unwrap();
                let status = std::process::Command::new("/usr/local/bin/docker-compose")
                    .args(["-p", &self.project, "-f"])
                    .arg(file)
                    .args(["down", "--remove-orphans"])
                    .env("DOCKER_HOST", "unix:///var/run/docker.sock")
                    .status()
                    .unwrap();
                assert!(status.success(), "owned OCI cleanup failed");
            }
        }
    }
}
impl Oci {
    async fn new() -> Self {
        let mut t = ActualDelivery::new().await;
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(t.temp.join("oci-build-progress"))
            .unwrap();
        t.runner_timeout_diagnostic = Some(oci_build_timeout_diagnostic);
        let compose_parent = PathBuf::from(std::env::var("CICD_TEST_OCI_COMPOSE_ROOT").unwrap());
        let (root, data, compose_root) = oci_fixture_paths(t.f.project, &compose_parent);
        std::fs::DirBuilder::new()
            .mode(0o755)
            .create(&compose_root)
            .unwrap();
        let mut data_file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&data)
            .unwrap();
        data_file
            .write_all(br#"{"schemaVersion":1,"records":[{"id":"task-1","state":"queued"}]}"#)
            .unwrap();
        drop(data_file);
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o444)).unwrap();
        let project = std::env::var("CICD_TEST_OCI_PROJECT").unwrap();
        let id = std::process::Command::new("/usr/local/bin/docker")
            .args(["info", "--format", "{{json .ID}}"])
            .env_remove("DOCKER_CONTEXT")
            .env_remove("DOCKER_TLS_VERIFY")
            .env_remove("DOCKER_CERT_PATH")
            .env("DOCKER_HOST", "unix:///var/run/docker.sock")
            .output()
            .unwrap();
        assert!(id.status.success());
        let daemon: serde_json::Value = serde_json::from_slice(&id.stdout).unwrap();
        let base_guard = t.temp.join("oci-offline-base.py");
        let base_layout = t.temp.join("oci-offline-base");
        std::fs::write(&base_guard, OCI_OFFLINE_BASE_GUARD).unwrap();
        let prepared = std::process::Command::new("python3")
            .args(["-I", "-B"])
            .arg(&base_guard)
            .arg("prepare")
            .arg(&base_layout)
            .arg(daemon.as_str().unwrap())
            .status()
            .unwrap();
        assert!(prepared.success(), "offline OCI base preparation failed");
        let policy = t.temp.join("oci-policy.json");
        std::fs::write(&policy,serde_json::to_vec(&serde_json::json!({"projectName":project,"networkName":std::env::var("CICD_TEST_OCI_NETWORK").unwrap(),"daemonId":daemon,"dockerBin":"/usr/local/bin/docker","composeBin":"/usr/local/bin/docker-compose","composeRoot":compose_root,"volumeName":std::env::var("CICD_TEST_OCI_VOLUME").unwrap(),"volumeRoot":"/delivery-qa","dataFile":data,"dataSha256":hash(&std::fs::read(&data).unwrap()),"checks":{"origin":"http://127.0.0.1:8000","healthPath":"/health","healthBodySha256":hash(b"ok\n"),"acceptancePath":"/acceptance","acceptanceBodySha256":hash(b"accepted:task-1:queued\n")}})).unwrap()).unwrap();
        Self {
            t,
            root,
            data,
            policy,
            compose_root,
            project,
            base_guard,
            base_layout,
            daemon: daemon.as_str().unwrap().to_owned(),
        }
    }
    async fn build(
        &mut self,
        version: &str,
        schemas: &[u32],
        migrations: &[&str],
    ) -> DeliveryCommand {
        std::fs::write(
            self.t.source.join("app.py"),
            include_bytes!("../helpers/oci_application.py"),
        )
        .unwrap();
        std::fs::write(self.t.source.join("app-version"), version).unwrap();
        std::fs::write(self.t.source.join("Dockerfile"),"FROM offline-python-base\nCOPY app.py /app.py\nCOPY app-version /app-version\nEXPOSE 8000\nENTRYPOINT [\"python\",\"-B\",\"/app.py\"]\n").unwrap();
        let schemas = serde_json::to_string(schemas).unwrap();
        let migrations = serde_json::to_string(migrations).unwrap();
        let label = if version == "wrong-image-commit" {
            "invalid"
        } else {
            "$commit"
        };
        let script = format!(
            "commit=$(git rev-parse HEAD); python3 -I -B '{}' build '{}' '{}' \"{label}\"; image=$(cat image-id); printf '{{\"schema\":\"forge/oci-candidate/v1\",\"imageId\":\"%s\",\"sourceCommit\":\"%s\",\"dataProtocol\":\"readonly_snapshot_v1\",\"readableSchemaVersions\":{schemas},\"migrations\":{migrations}}}' \"$image\" \"$commit\" > product.txt",
            self.base_guard.display(),
            self.base_layout.display(),
            self.daemon,
        );
        reset_oci_build_progress(&self.t.temp);
        self.t.build_script(version, &script).await.0
    }
    fn process(&self, command: &DeliveryCommand) -> tokio::process::Command {
        let mut child = self.t.process(command);
        child
            .arg("--oci")
            .env("CICD_LOCAL_OCI_ROOT", &self.root)
            .env("CICD_LOCAL_OCI_POLICY", &self.policy);
        child
    }
    async fn execute(
        &self,
        command: &DeliveryCommand,
        mode: Option<&str>,
    ) -> (i32, Option<serde_json::Value>) {
        let mut child = self.process(command);
        if let Some(mode) = mode {
            child.arg(mode);
        }
        let output = tokio::time::timeout(Duration::from_secs(50), child.output())
            .await
            .unwrap()
            .unwrap();
        if !output.status.success() {
            eprintln!("OCI_CLI_EXIT={:?}", output.status.code());
            oci_cli_failure_diagnostic(&output, &self.root, command, &self.compose_root);
        }
        (
            output.status.code().unwrap(),
            serde_json::from_slice(&output.stdout).ok(),
        )
    }
}

#[tokio::test]
async fn task_bound_oci_http_requires_current_authority_and_preserves_actual_rollback_and_replay() {
    use axum::{Json, Router, routing::get};
    use serde_json::{Value, json};
    let mut t = Oci::new().await;
    t.t.f.request.role = cicd::domain::sdlc_workspace::WorkspaceRole::Developer;
    t.t.f.request.binding.root_task_id = t.t.f.request.binding.task_id;
    let environment = Uuid::new_v4();
    sqlx::query("INSERT INTO environments(id,project_id,name,protected,required_approvals) VALUES($1,$2,'task-oci-nonproduction',false,0)")
        .bind(environment).bind(t.t.f.project).execute(&t.t.f.pool).await.unwrap();
    let mut policy: Value = serde_json::from_slice(&std::fs::read(&t.policy).unwrap()).unwrap();
    policy["checks"]["taskExecution"] = json!({"environment":"nonproduction","environmentId":environment,
        "repositoryId":t.t.f.request.repository_id,"trackerInstanceId":t.t.f.request.binding.tracker_instance_id,
        "trackerProjectId":t.t.f.request.binding.tracker_project_id,"holderSubject":"fleet-delivery"});
    std::fs::write(&t.policy, serde_json::to_vec(&policy).unwrap()).unwrap();
    let data_before = std::fs::read(&t.data).unwrap();
    let a = t.build("A", &[1], &[]).await;
    // Privileged existing local path enrolls the owner target, never the HTTP caller.
    let (code, initial) = t.execute(&a, None).await;
    assert_eq!(code, 0);
    let initial = initial.unwrap();
    assert_eq!(latest(&initial)["scope"], "owner_local_verification");
    assert_eq!(latest(&initial)["dispatchAllowed"], false);
    let manifest_a = initial["confirmedManifestSha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut b = t.build("B", &[1], &[]).await;
    b.expected_manifest_sha256 = Some(manifest_a.clone());
    let binding = t.t.f.request.binding.clone();
    let agent = Uuid::new_v4();
    let mut fence = TaskDeliveryFence {
        development_result_id: Uuid::new_v4(),
        assignment_id: Uuid::new_v4(),
        execution_id: Uuid::new_v4(),
        agent_id: agent,
        lease_id: Uuid::new_v4(),
        fencing_token: 1,
        expected_lease_version: 1,
        assignment_hash: String::new(),
    };
    let route = json!({"agent_id":agent});
    let routing = json!({"snapshot_id":binding.routing_snapshot_id,"tracker_instance_id":binding.tracker_instance_id,
        "project_id":binding.tracker_project_id,"task_id":binding.task_id,"root_task_id":binding.task_id,
        "requirement_revision":binding.requirement_revision,"policy":{"tracker_instance_id":binding.tracker_instance_id,
        "project_id":binding.tracker_project_id,"routes":{"devops":route}}});
    let mut assignment = json!({"assignment_id":fence.assignment_id,"execution_id":fence.execution_id,
        "task_id":binding.task_id,"owner_subject":"owner","role":"devops","stage":"Deployment","cycle":0,
        "attempt":1,"agent_id":agent,"routing":routing,"previous_result_id":Uuid::new_v4(),
        "fencing_token":fence.fencing_token,"lease_id":fence.lease_id});
    fn sorted(value: Value) -> Value {
        match value {
            Value::Object(object) => Value::Object(
                object
                    .into_iter()
                    .map(|(key, value)| (key, sorted(value)))
                    .collect::<std::collections::BTreeMap<_, _>>()
                    .into_iter()
                    .collect(),
            ),
            Value::Array(array) => Value::Array(array.into_iter().map(sorted).collect()),
            value => value,
        }
    }
    fence.assignment_hash = hash(&serde_json::to_vec(&sorted(assignment.clone())).unwrap());
    assignment["assignment_hash"] = json!(fence.assignment_hash);
    b.task_fence = Some(fence);
    let reply = std::sync::Arc::new(std::sync::Mutex::new(
        json!({"runtime_state":"running","receipt":{
        "task":{"task_id":binding.task_id,"owner_subject":"owner","version":1,"stage":"Deployment",
            "status":"InProgress","runtime":"running","cycle":0,"waiting_reason":null,"routing":routing},
        "assignment":assignment,"lease":{"lease_id":b.task_fence.as_ref().unwrap().lease_id,"version":1,"holder_subject":"fleet-delivery"},
        "binding":{"chat_id":Uuid::new_v4(),"run_id":Uuid::new_v4(),"machine_subject":"devops-agent"}}}),
    ));
    let observed = reply.clone();
    let expired = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let expired_response = expired.clone();
    let expire_at = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(usize::MAX));
    let late_expiry = expire_at.clone();
    let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls = hits.clone();
    let source_result = json!({"source":"delivery","receipt":{"result_id":b.task_fence.as_ref().unwrap().development_result_id,
        "assignment_id":binding.assignment_id,"task_id":binding.task_id,"cycle":0,
        "output":{"role":"developer","commit_sha":t.t.f.request.source_commit},"next_stage":"Review"}});
    let tracker = Router::new()
        .route(
            &format!(
                "/api/v1/issues/{}/sdlc/lifecycle/results/{}",
                binding.task_id,
                b.task_fence.as_ref().unwrap().development_result_id
            ),
            get(move |headers: axum::http::HeaderMap| {
                let value = source_result.clone();
                async move {
                    assert_eq!(headers["authorization"], "Bearer task-reader-fixture");
                    Json(value)
                }
            }),
        )
        .route(
            &format!("/api/v1/issues/{}/sdlc/lifecycle", binding.task_id),
            get(move |headers: axum::http::HeaderMap| {
                let value = observed.clone();
                let expired = expired_response.clone();
                let late_expiry = late_expiry.clone();
                let calls = calls.clone();
                async move {
                    assert_eq!(headers["authorization"], "Bearer task-reader-fixture");
                    let call = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                    let mut value = value.lock().unwrap().clone();
                    let now = Utc::now();
                    value["observed_at"] = json!(now);
                    value["receipt"]["lease"]["claimed_at"] =
                        json!(now - chrono::Duration::seconds(60));
                    value["receipt"]["lease"]["heartbeat_at"] =
                        json!(now - chrono::Duration::seconds(1));
                    value["receipt"]["lease"]["expires_at"] = json!(
                        now + chrono::Duration::seconds(
                            if expired.load(std::sync::atomic::Ordering::SeqCst)
                                || call >= late_expiry.load(std::sync::atomic::Ordering::SeqCst)
                            {
                                -1
                            } else {
                                29
                            }
                        )
                    );
                    Json(value)
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let tracker_url = format!("http://{}", listener.local_addr().unwrap());
    let tracker_server = tokio::spawn(async move { axum::serve(listener, tracker).await.unwrap() });
    let mut config = cicd::config::RuntimeConfig::test_default()
        .with_auth_secret(Some("workspace-test-secret".into()));
    config.sdlc_workspace.operation_subject = Some(t.t.f.subject);
    config.sdlc_workspace.local_delivery_kind = cicd::config::LocalDeliveryKind::Oci;
    config.sdlc_workspace.local_delivery_root = Some(t.root.clone());
    config.sdlc_workspace.local_delivery_policy = Some(t.policy.clone());
    config.sdlc_workspace.tracker_read_url = Some(tracker_url);
    config.sdlc_workspace.tracker_read_token = Some("task-reader-fixture".into());
    config.artifacts.root = t.t.artifact_root.clone();
    t.t.f.app = cicd::api::app_with_git_and_config(
        Some(t.t.f.pool.clone()),
        cicd::git_host::GitConfig {
            root: t.t.git_root.clone(),
            token: None,
            internal_token: None,
        },
        None,
        config,
    )
    .unwrap();
    let path = format!(
        "/api/v1/projects/{}/sdlc/workspace-operations/{}/delivery-operations",
        t.t.f.project, b.workspace_operation_key
    );
    sqlx::query("UPDATE environments SET protected=true,required_approvals=1 WHERE id=$1")
        .bind(environment)
        .execute(&t.t.f.pool)
        .await
        .unwrap();
    assert_eq!(
        call(
            &t.t.f.app,
            "POST",
            &path,
            &t.t.f.token,
            serde_json::to_string(&b).unwrap()
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0);
    sqlx::query("UPDATE environments SET protected=false,required_approvals=0 WHERE id=$1")
        .bind(environment)
        .execute(&t.t.f.pool)
        .await
        .unwrap();
    reply.lock().unwrap()["receipt"]["lease"]["holder_subject"] = json!("foreign");
    assert_eq!(
        call(
            &t.t.f.app,
            "POST",
            &path,
            &t.t.f.token,
            serde_json::to_string(&b).unwrap()
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    reply.lock().unwrap()["receipt"]["lease"]["holder_subject"] = json!("fleet-delivery");
    expired.store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        call(
            &t.t.f.app,
            "POST",
            &path,
            &t.t.f.token,
            serde_json::to_string(&b).unwrap()
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert!(
        !t.root
            .join("operations")
            .join(hash(b.operation_key.as_bytes()))
            .exists()
    );
    expired.store(false, std::sync::atomic::Ordering::SeqCst);
    let (status, failed) = call(
        &t.t.f.app,
        "POST",
        &path,
        &t.t.f.token,
        serde_json::to_string(&b).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{failed}");
    assert_eq!(latest(&failed)["schema"], "forge/local-oci-operation/v1");
    assert_eq!(latest(&failed)["scope"], "task_nonproduction_delivery");
    assert_eq!(latest(&failed)["dispatchAllowed"], true);
    assert_eq!(latest(&failed)["sdlcAcceptanceVerified"], false);
    assert_eq!(latest(&failed)["status"], "failed");
    assert_eq!(latest(&failed)["health"]["httpStatus"], 503);
    assert_eq!(failed["confirmedManifestSha256"], manifest_a);
    assert!(hits.load(std::sync::atomic::Ordering::SeqCst) >= 4);
    let read_path = format!(
        "{}/{}?requestHash={}&taskId={}&rootTaskId={}&assignmentId={}&executionId={}&fencingToken={}",
        path,
        b.operation_key,
        b.original.request_hash,
        b.original.task_id,
        b.original.root_task_id,
        b.original.assignment_id,
        b.original.execution_id,
        b.original.fencing_token,
    );
    let hits_before = hits.load(std::sync::atomic::Ordering::SeqCst);
    expired.store(true, std::sync::atomic::Ordering::SeqCst);
    let (status, saved) = call(&t.t.f.app, "GET", &read_path, &t.t.f.token, String::new()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved, failed);
    assert_eq!(
        call(
            &t.t.f.app,
            "POST",
            &path,
            &t.t.f.token,
            serde_json::to_string(&b).unwrap()
        )
        .await
        .1,
        failed
    );
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), hits_before);
    let mut changed = b.clone();
    changed.expected_manifest_sha256 = None;
    assert_eq!(
        call(
            &t.t.f.app,
            "POST",
            &path,
            &t.t.f.token,
            serde_json::to_string(&changed).unwrap()
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(t.execute(&b, None).await.0, 1); // Local CLI cannot acquire task authority.
    expired.store(false, std::sync::atomic::Ordering::SeqCst);
    let mut rollback = b.clone();
    rollback.operation_key = "http:oci:rollback".into();
    rollback.action = DeliveryAction::Rollback;
    rollback.artifact_id = None;
    rollback.expected_manifest_sha256 = failed["currentManifestSha256"].as_str().map(str::to_owned);
    let (status, restored) = call(
        &t.t.f.app,
        "POST",
        &path,
        &t.t.f.token,
        serde_json::to_string(&rollback).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(latest(&restored)["status"], "verified");
    assert_eq!(latest(&restored)["imageId"], latest(&initial)["imageId"]);
    assert_ne!(
        latest(&restored)["containerId"],
        latest(&failed)["containerId"]
    );
    assert_eq!(restored["currentManifestSha256"], manifest_a);
    assert_eq!(std::fs::read(&t.data).unwrap(), data_before);
    assert_eq!(latest(&initial)["dispatchAllowed"], false);
    // Expiry after async image/mount preflight must hold the original intent before publication.
    let pointer = t.root.join("current.json");
    let pointer_before = std::fs::read(&pointer).unwrap();
    let mut late = b.clone();
    late.operation_key = "http:oci:late-preflight".into();
    late.expected_manifest_sha256 = Some(manifest_a);
    expire_at.store(
        hits.load(std::sync::atomic::Ordering::SeqCst) + 2,
        std::sync::atomic::Ordering::SeqCst,
    );
    assert_eq!(
        call(
            &t.t.f.app,
            "POST",
            &path,
            &t.t.f.token,
            serde_json::to_string(&late).unwrap()
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let held = t
        .root
        .join("operations")
        .join(hash(late.operation_key.as_bytes()));
    assert!(held.join("intent.json").exists());
    assert!(!held.join("child.json").exists());
    assert!(!held.join("result.json").exists());
    assert_eq!(std::fs::read(&pointer).unwrap(), pointer_before);
    let read_path = read_path.replacen(
        &format!("/{}?", b.operation_key),
        &format!("/{}?", late.operation_key),
        1,
    );
    let (status, pending) = call(&t.t.f.app, "GET", &read_path, &t.t.f.token, String::new()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(latest(&pending)["status"], "unknown");
    assert_eq!(pending["reconciliationNeeded"], true);
    assert_eq!(std::fs::read(&pointer).unwrap(), pointer_before);
    tracker_server.abort();
}

#[tokio::test]
async fn sdlc_oci_delivery_actual_image_data_acceptance_rollback_and_unknown() {
    let mut t = Oci::new().await;
    let data = std::fs::read(&t.data).unwrap();
    let a = t.build("A", &[1], &[]).await;
    let (code, first) = t.execute(&a, None).await;
    assert_eq!(code, 0);
    let first = first.unwrap();
    assert_eq!(latest(&first)["compatibility"]["status"], "verified");
    assert_eq!(latest(&first)["dataSha256"], hash(&data));
    let manifest = first["confirmedManifestSha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let container = latest(&first)["containerId"].clone();
    assert_eq!(t.execute(&a, None).await.1.unwrap(), first);
    let mut b = t.build("B", &[1], &[]).await;
    b.expected_manifest_sha256 = Some(manifest.clone());
    let failed = t.execute(&b, None).await.1.unwrap();
    assert_eq!(latest(&failed)["status"], "failed");
    assert_eq!(latest(&failed)["health"]["httpStatus"], 503);
    assert_eq!(failed["confirmedManifestSha256"], manifest);
    let mut rollback = b.clone();
    rollback.operation_key = "oci:rollback:B".into();
    rollback.action = DeliveryAction::Rollback;
    rollback.artifact_id = None;
    rollback.expected_manifest_sha256 = failed["currentManifestSha256"].as_str().map(str::to_owned);
    let restored = t.execute(&rollback, None).await.1.unwrap();
    assert_eq!(latest(&restored)["status"], "verified");
    assert_eq!(restored["currentManifestSha256"], manifest);
    assert_eq!(latest(&restored)["imageId"], latest(&first)["imageId"]);
    assert_ne!(latest(&restored)["containerId"], container);
    assert_eq!(std::fs::read(&t.data).unwrap(), data);
    let mut c = t.build("C", &[1], &[]).await;
    c.expected_manifest_sha256 = Some(manifest.clone());
    let failed = t.execute(&c, None).await.1.unwrap();
    assert_eq!(latest(&failed)["acceptance"]["httpStatus"], 422);
    assert_eq!(failed["confirmedManifestSha256"], manifest);
    rollback.operation_key = "oci:rollback:C".into();
    rollback.workspace_operation_key = c.workspace_operation_key.clone();
    rollback.original = c.original.clone();
    rollback.expected_manifest_sha256 = failed["currentManifestSha256"].as_str().map(str::to_owned);
    assert_eq!(t.execute(&rollback, None).await.0, 0);
    let mut wrong = t.build("schema2", &[2], &[]).await;
    wrong.expected_manifest_sha256 = Some(manifest.clone());
    let blocked = t.execute(&wrong, None).await;
    assert_eq!(blocked.0, 1);
    assert_eq!(
        blocked.1.unwrap()["reason"],
        "actual_data_schema_incompatible"
    );
    assert_eq!(
        t.execute(&a, Some("--readback")).await.1.unwrap()["currentManifestSha256"],
        manifest
    );
    let mut migration = t.build("migration", &[1], &["DROP TABLE tasks"]).await;
    migration.expected_manifest_sha256 = Some(manifest.clone());
    let blocked = t.execute(&migration, None).await;
    assert_eq!(blocked.0, 1);
    assert_eq!(
        blocked.1.unwrap()["reason"],
        "unsupported_mutable_data_or_migration"
    );
    let mut wrong_image = t.build("wrong-image-commit", &[1], &[]).await;
    wrong_image.expected_manifest_sha256 = Some(manifest.clone());
    assert_eq!(t.execute(&wrong_image, None).await.0, 1);
    let mut d = t.build("D", &[1], &[]).await;
    d.expected_manifest_sha256 = Some(manifest.clone());
    let mut child = t
        .process(&d)
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let dir = t
        .root
        .join("operations")
        .join(hash(d.operation_key.as_bytes()));
    let deadline = Instant::now() + Duration::from_secs(25);
    while !dir.join("child-stopped.json").exists() {
        assert!(Instant::now() < deadline, "Compose effect deadline");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let name = format!("{}-application-1", t.project);
    loop {
        let logs = std::process::Command::new("docker")
            .args(["logs", "--tail", "20", &name])
            .output()
            .unwrap();
        if String::from_utf8_lossy(&logs.stdout).contains("HEALTH_DELAY_ENTERED") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "actual application health entry deadline"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let actual_id = std::process::Command::new("docker")
        .args(["container", "inspect", "--format", "{{.Id}}", &name])
        .output()
        .unwrap();
    child.start_kill().unwrap();
    assert!(!child.wait().await.unwrap().success());
    let unknown = t.execute(&d, Some("--readback")).await.1.unwrap();
    assert_eq!(latest(&unknown)["status"], "unknown");
    let pointer_time = std::fs::metadata(t.root.join("current.json"))
        .unwrap()
        .modified()
        .unwrap();
    let mut other = d.clone();
    other.operation_key = "oci:held".into();
    other.expected_manifest_sha256 = unknown["currentManifestSha256"].as_str().map(str::to_owned);
    assert_eq!(t.execute(&other, None).await.0, 1);
    let recovered = t.execute(&d, Some("--reconcile")).await.1.unwrap();
    assert_eq!(latest(&recovered)["status"], "verified");
    assert_eq!(
        latest(&recovered)["containerId"],
        String::from_utf8(actual_id.stdout).unwrap().trim()
    );
    assert_eq!(
        std::fs::metadata(t.root.join("current.json"))
            .unwrap()
            .modified()
            .unwrap(),
        pointer_time
    );
    assert_eq!(recovered["receipt"], unknown["receipt"]);
    assert_eq!(std::fs::read(&t.data).unwrap(), data);
    let mut e = t.build("E", &[1], &[]).await;
    e.expected_manifest_sha256 = recovered["currentManifestSha256"]
        .as_str()
        .map(str::to_owned);
    let proof_dir = t
        .root
        .join("operations")
        .join(hash(e.operation_key.as_bytes()));
    let proof_path = proof_dir.join("verified-checks.json");
    let mut child = t
        .process(&e)
        .env("CICD_TEST_OCI_PAUSE_AFTER_VERIFIED_CHECKS", "1")
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(35);
    while !proof_dir.join("qa-verified-checks-ready.json").exists() {
        assert!(
            child.try_wait().unwrap().is_none(),
            "child stopped before verified checks"
        );
        assert!(
            Instant::now() < deadline,
            "verified checks checkpoint deadline"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let proof_bytes = std::fs::read(&proof_path).unwrap();
    let proof_time = std::fs::metadata(&proof_path).unwrap().modified().unwrap();
    let proof: serde_json::Value = serde_json::from_slice(&proof_bytes).unwrap();
    assert_eq!(proof["status"], "verified");
    assert!(!proof_dir.join("result.json").exists());
    let actual_id = std::process::Command::new("docker")
        .args(["container", "inspect", "--format", "{{.Id}}", &name])
        .output()
        .unwrap();
    assert!(actual_id.status.success());
    child.start_kill().unwrap();
    assert!(!child.wait().await.unwrap().success());
    let unknown = t.execute(&e, Some("--readback")).await.1.unwrap();
    assert_eq!(latest(&unknown)["status"], "unknown");
    let recovered = t.execute(&e, Some("--reconcile")).await.1.unwrap();
    assert_eq!(latest(&recovered)["status"], "verified");
    assert_eq!(recovered["receipt"], unknown["receipt"]);
    assert_eq!(
        latest(&recovered)["containerId"],
        String::from_utf8(actual_id.stdout).unwrap().trim()
    );
    assert_eq!(std::fs::read(&proof_path).unwrap(), proof_bytes);
    assert_eq!(
        std::fs::metadata(&proof_path).unwrap().modified().unwrap(),
        proof_time
    );
    assert_eq!(std::fs::read(&t.data).unwrap(), data);
    println!("ACTUAL_OCI_SIGKILL_AFTER_VERIFIED_CHECKS_HISTORICAL_PROOF_PRESERVED:PASS");
    // Owner-side drift is not silently repaired or accepted for rollback.
    std::fs::set_permissions(&t.data, std::fs::Permissions::from_mode(0o644)).unwrap();
    std::fs::write(&t.data, br#"{"schemaVersion":2,"records":[]}"#).unwrap();
    rollback.operation_key = "oci:rollback:incompatible".into();
    rollback.expected_manifest_sha256 = recovered["currentManifestSha256"]
        .as_str()
        .map(str::to_owned);
    let blocked = t.execute(&rollback, None).await;
    assert_eq!(blocked.0, 1);
    assert_eq!(blocked.1.unwrap()["reason"], "owner_data_snapshot_drift");
    std::fs::write(&t.data, &data).unwrap();
    std::fs::set_permissions(&t.data, std::fs::Permissions::from_mode(0o444)).unwrap();
    println!(
        "ACTUAL_OCI_SCENARIOS=real-runner-build,image-commit,data-readback,health503,acceptance422,rollback-exact-image,readonly-data-preserved,unknown-SIGKILL,reconcile-no-recreate,verified-checks-SIGKILL-proof-preserved,incompatible-schema-blocked,migration-blocked,data-drift-blocked"
    );
}
