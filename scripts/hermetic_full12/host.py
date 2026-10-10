"""Hosted-only admission, immutable exports and bounded private subprocesses."""
from contextlib import contextmanager
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import re
import selectors
import shutil
import signal
import subprocess
import tarfile
import tempfile
import time
import urllib.request
import maintenance_git as m

HERE = Path(__file__).resolve().parent
SOURCE = '25be2e82d4d42673897c8a16070eb8a9519244f0'
BASE = '19a7a381ae6dbea61a643bb96189e483fa64df5c'
PUBLIC_CONTROLS = '1dbedf85242c3540b70005ce5f0c20badb682c41'
QUALIFICATION_CONTROLS = '632ea8347602fe4af22e7369790ed9c75e53f76a'
SAFETY_CONTROLS = '4d97c54f35b485224a8229495763658fcbbd40e9'
MAINTENANCE_CONTROLS = 'b9375dc4b4f070b0f6cff1733228f1a6865b1368'
CONTROLS_PARENT = '1310958a4b5cf68642a6ce8d4466ef5ff1e64f31'
BRANCH = 'build-only/forge-resource-full12-20261010'
CONSUMER_PATHS = {
    '.github/workflows/forge-hermetic-full12.yml',
    *('scripts/hermetic_full12/' + name for name in (
        'host.py', 'maintenance_git.py', 'maintenance-pin.json', 'test_maintenance_git.py',
        'test_gate.py', 'test_deadline_boundaries.py', 'run.py', 'private_bridge_proofs.py',
        'README.md', 'components.json')),
}
HOST_BYTES = 108279229428
DATA_BYTES = 71319483898
INODES = 300000
RECLAIM = ('/usr/share/dotnet', '/usr/local/lib/android', '/opt/ghc')
DOCKER = ['docker', '--context', 'rootless']
ERROR_STAGES = ('python', 'row-smoke', 'smoke', 'check', 'clippy', 'postgres', 'oci',
                'workspace', 'integration', 'cli', 'openapi', 'release')
BOOTSTRAP_STEPS = ('tools_download', 'dependencies', 'user_namespace', 'manager',
                   'rootless_launch', 'context', 'socket_ready', 'identity_decode',
                   'identity_version', 'identity_compose_rootless', 'identity_endpoint',
                   'identity_cgroup_warnings', 'identity_cgroup_version',
                   'identity_cgroup_driver', 'identity_cgroup_resources', 'baseline', 'admission_seal')
BOOTSTRAP_DEADLINE = None
PHASE_DEADLINE = None
TOOLS = {
    'docker-29.8.2.tgz': ('https://download.docker.com/linux/static/stable/x86_64/docker-29.8.2.tgz',
        '995d1ef289677f74fd58d8d2c35727b6a4ee389c69db8638a3e42d0487aa5b0f'),
    'docker-rootless-extras-29.8.2.tgz': ('https://download.docker.com/linux/static/stable/x86_64/docker-rootless-extras-29.8.2.tgz',
        '707ebf6a5afd88104086e7b6749997b2366e816aeaf2c3ef2305b08fde9ee007'),
    'docker-compose': ('https://github.com/docker/compose/releases/download/v5.5.1/docker-compose-linux-x86_64',
        'db1889184726840f75c4f9c001048430d4f25b3be3cb084d3ddd762bc0aed576'),
    'docker-buildx': ('https://github.com/docker/buildx/releases/download/v0.38.0/buildx-v0.38.0.linux-amd64',
        '4fe4cc38adf48169132749b6ca22a990928db0118e3407584ee553723115d287'),
}


def require(condition):
    if not condition:
        raise ValueError('closed_guard')


class CapacityFailure(ValueError):
    def __init__(self, measurement):
        self.measurement = measurement
        super().__init__('capacity_not_admitted')


class CommandFailure(RuntimeError):
    def __init__(self, code):
        self.code = code
        super().__init__('command_nonzero')


class BootstrapFailure(RuntimeError):
    def __init__(self, step, error):
        require(type(step) is str and step in BOOTSTRAP_STEPS)
        self.step = step
        self.safe = safe_error('bootstrap', error)
        super().__init__('bootstrap_boundary_failed')


@contextmanager
def bootstrap_step(step):
    require(type(step) is str and step in BOOTSTRAP_STEPS)
    try:
        yield
    except (BootstrapFailure, CapacityFailure):
        raise
    except Exception as error:
        if BOOTSTRAP_DEADLINE is None:
            raise
        raise BootstrapFailure(step, error) from None


class OverheadTimeout(subprocess.TimeoutExpired):
    pass


class PhaseDeadline:
    def __init__(self, seconds):
        self.seconds = seconds
        self.expires = time.monotonic() + seconds
        self.expired = False

    def check(self):
        if self.expired or time.monotonic() >= self.expires:
            raise OverheadTimeout('orchestration_overhead', self.seconds)


def check_deadline():
    if PHASE_DEADLINE is not None:
        PHASE_DEADLINE.check()


@contextmanager
def wall_budget(seconds):
    global PHASE_DEADLINE
    require(type(seconds) in (int, float) and 0 < seconds < float('inf'))
    require(os.name == 'posix' and hasattr(signal, 'setitimer'))
    require(PHASE_DEADLINE is None)
    require(signal.getitimer(signal.ITIMER_REAL) == (0.0, 0.0))
    deadline = PhaseDeadline(seconds)
    def expired(*_):
        deadline.expired = True
        raise OverheadTimeout('orchestration_overhead', seconds)
    previous = signal.signal(signal.SIGALRM, expired)
    PHASE_DEADLINE = deadline
    signal.setitimer(signal.ITIMER_REAL, seconds)
    try:
        yield
    finally:
        try:
            # A caught alarm is still an expired phase, never a successful context exit.
            deadline.check()
        finally:
            signal.setitimer(signal.ITIMER_REAL, 0)
            signal.signal(signal.SIGALRM, previous)
            PHASE_DEADLINE = None


def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def read(path):
    require(Path(path).stat().st_size <= 2**20)
    return json.loads(Path(path).read_bytes())


def atomic(path, value, exclusive=False):
    path = Path(path)
    raw = (json.dumps(value, sort_keys=True, indent=2, allow_nan=False) + '\n').encode()
    fd, temporary = tempfile.mkstemp(prefix='.' + path.name, dir=path.parent)
    try:
        with os.fdopen(fd, 'wb') as stream:
            stream.write(raw)
            stream.flush()
            os.fsync(stream.fileno())
        if exclusive:
            os.link(temporary, path)
        else:
            os.replace(temporary, path)
        if os.name == 'posix':
            directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
            try:
                os.fsync(directory)
            finally:
                os.close(directory)
    finally:
        Path(temporary).unlink(missing_ok=True)


def safe_error(stage, error):
    allowed = {'admission', 'bootstrap', 'cleanup', 'aggregate', 'parity', *ERROR_STAGES}
    if isinstance(error, BootstrapFailure):
        result = dict(error.safe)
        result['stage'] = stage if stage in allowed else 'admission'
        if stage == 'bootstrap':
            result['bootstrap_step'] = error.step
        validate_safe_error(result)
        return result
    category = ('os_error' if isinstance(error, OSError) else 'timeout' if isinstance(error, subprocess.TimeoutExpired)
                else 'assertion' if isinstance(error, AssertionError) else 'closed_failure')
    number = getattr(error, 'errno', None)
    result = {'stage': stage if stage in allowed else 'admission', 'category': category,
              'errno': number if type(number) is int and 0 <= number <= 4095 else None}
    if isinstance(error, CommandFailure):
        result['exit_code'] = error.code if type(error.code) is int and -255 <= error.code <= 255 else None
    if isinstance(error, m.QualificationFailure):
        result['dependency'] = 'maintenance_pin_unqualified'
    return result


def validate_safe_error(result):
    require(type(result) is dict and {'stage', 'category', 'errno'} <= set(result)
            and set(result) <= {'stage', 'category', 'errno', 'exit_code', 'dependency', 'bootstrap_step'})
    require(type(result['stage']) is str and type(result['category']) is str
            and result['stage'] in {'admission', 'bootstrap', 'cleanup', 'aggregate', 'parity', *ERROR_STAGES}
            and result['category'] in {'os_error', 'timeout', 'assertion', 'closed_failure'})
    require(result['errno'] is None or type(result['errno']) is int and 0 <= result['errno'] <= 4095)
    if 'exit_code' in result:
        require(result['exit_code'] is None or type(result['exit_code']) is int and -255 <= result['exit_code'] <= 255)
    if 'dependency' in result:
        require(result['dependency'] == 'maintenance_pin_unqualified')
    if 'bootstrap_step' in result:
        require(result['stage'] == 'bootstrap' and type(result['bootstrap_step']) is str
                and result['bootstrap_step'] in BOOTSTRAP_STEPS)


def leader_status(process):
    # WNOWAIT pins even an exited leader's PID/session until group cleanup finishes.
    require(process.returncode is None)
    return os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)


def live_group(process):
    for path in Path('/proc').glob('[0-9]*/stat'):
        try:
            fields = path.read_text().rsplit(') ', 1)[1].split()
        except FileNotFoundError:
            continue
        if int(fields[2]) == process.pid:
            require(int(fields[3]) == process.pid)
            if fields[0] not in ('Z', 'X'):
                return True
    return False


def terminate(process):
    leader_status(process)  # ECHILD/reaped leaders fail closed without signalling historical PIDs.
    require(os.getpgid(process.pid) == process.pid and os.getsid(process.pid) == process.pid)
    for sig in (signal.SIGTERM, signal.SIGKILL):
        os.killpg(process.pid, sig)
    deadline = time.monotonic() + 10
    while live_group(process):
        if time.monotonic() >= deadline:
            raise subprocess.TimeoutExpired('owned_group_cleanup', 10)
        time.sleep(0.01)
    process.wait(timeout=10)


def command(argv, timeout=90, log=None, env=None, check=None):
    check_deadline()
    if PHASE_DEADLINE is not None:
        timeout = min(timeout, PHASE_DEADLINE.expires - time.monotonic())
        check_deadline()
    if BOOTSTRAP_DEADLINE is not None:
        timeout = min(timeout, BOOTSTRAP_DEADLINE - time.monotonic())
        require(timeout > 0)
    stream = Path(log).open('xb') if log else None
    process = None
    cleaned = False
    try:
        require(os.name == 'posix' and hasattr(os, 'WNOWAIT'))
        process = subprocess.Popen(argv, stdout=stream or subprocess.PIPE,
            stderr=subprocess.STDOUT if stream else subprocess.PIPE, env=env, start_new_session=True)
        deadline = time.monotonic() + timeout
        stdout = bytearray()
        with selectors.DefaultSelector() as selector:
            if stream is None:
                for pipe in (process.stdout, process.stderr):
                    os.set_blocking(pipe.fileno(), False)
                    selector.register(pipe, selectors.EVENT_READ)
            while True:
                if check is not None:
                    check()
                if not cleaned and leader_status(process) is not None:
                    terminate(process)
                    cleaned = True
                if cleaned and not selector.get_map():
                    break
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise subprocess.TimeoutExpired('owned_command', timeout)
                for key, _ in selector.select(min(0.05, remaining)):
                    block = os.read(key.fileobj.fileno(), 65536)
                    if not block:
                        selector.unregister(key.fileobj)
                    elif key.fileobj is process.stdout:
                        stdout.extend(block)
        if process.returncode != 0:
            raise CommandFailure(process.returncode)
        if check is not None:
            check()
        if stream:
            stream.flush()
            os.fsync(stream.fileno())
        return bytes(stdout)
    finally:
        try:
            if process is not None and not cleaned:
                terminate(process)
        finally:
            if process is not None:
                for pipe in (process.stdout, process.stderr):
                    if pipe is not None:
                        pipe.close()
            if stream:
                stream.close()


def git(root, *args):
    return command(['git', '--no-replace-objects', '-C', str(root), '--no-optional-locks', *args])


def clean(root, revision):
    require(git(root, 'rev-parse', 'HEAD').decode().strip() == revision)
    require(not git(root, 'status', '--porcelain', '--untracked-files=all'))


def hosted_guard(env=os.environ):
    expected = {'GITHUB_ACTIONS': 'true', 'RUNNER_ENVIRONMENT': 'github-hosted', 'RUNNER_OS': 'Linux',
                'GITHUB_REPOSITORY': 'FerrPOINT/CI-CD', 'GITHUB_EVENT_NAME': 'push',
                'GITHUB_REF': 'refs/heads/' + BRANCH}
    require(os.name == 'posix' and os.geteuid() != 0)
    require(all(env.get(key) == value for key, value in expected.items()))
    require('microsoft' not in Path('/proc/sys/kernel/osrelease').read_text().lower())
    for key in ('GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT'):
        require(re.fullmatch('[1-9][0-9]*', env.get(key, '')))
    require(re.fullmatch('[a-f0-9]{40}', env.get('GITHUB_SHA', '')))
    require(not any(env.get(key) for key in ('DOCKER_HOST', 'DOCKER_CONTEXT', 'DOCKER_TLS_VERIFY',
                                           'DOCKER_CERT_PATH', 'SDLC_DOCKER_EXECUTABLE')))


def verify_components():
    pins = read(HERE / 'components.json')
    root = HERE.parent.parent
    expected = {path.relative_to(root).as_posix() for path in HERE.rglob('*')
                if path.is_file() and path.name != 'components.json'} | {'.github/workflows/forge-hermetic-full12.yml'}
    require(set(pins) == expected)
    for name, digest in pins.items():
        relative = PurePosixPath(name)
        require(str(relative) == name and not relative.is_absolute() and '..' not in relative.parts and '\\' not in name)
        require(re.fullmatch('[a-f0-9]{64}', digest))
        path = root / name
        require(not path.is_symlink() and sha(path) == digest)
    origins = read(HERE / 'origins.json')
    for name, digest in origins['files'].items():
        require(sha(HERE / name) == digest)
    return pins


def maintenance(root, checkout, expected):
    proof, payloads = m.read_payloads(checkout, git)
    require(proof == expected)
    destination = root / 'maintenance/scripts'
    destination.mkdir(parents=True, exist_ok=False)
    for name, payload in payloads.items():
        target = destination / Path(name).name
        with target.open('xb') as stream:
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
        target.chmod(0o444)
    return destination.parent


def bind_maintenance(module, proof):
    # Adapt only the expected identity, after exact private Git proof; never change source bytes.
    m.proof_matches(proof)
    module.SDK_SHA256 = proof['files']['scripts/compose_helpers.py']['sha256']


def controls_history(controls):
    require(git(controls, 'rev-parse', '--is-shallow-repository').strip() == b'false')
    for revision, parent in (('HEAD', CONTROLS_PARENT), (CONTROLS_PARENT, MAINTENANCE_CONTROLS),
                             (MAINTENANCE_CONTROLS, SAFETY_CONTROLS),
                             (SAFETY_CONTROLS, QUALIFICATION_CONTROLS),
                             (QUALIFICATION_CONTROLS, PUBLIC_CONTROLS),
                             (PUBLIC_CONTROLS, SOURCE)):
        row = git(controls, 'rev-list', '--parents', '-n', '1', revision).decode().split()
        require(len(row) == 2 and row[1] == parent)
    changes = git(controls, 'diff', '--name-status', CONTROLS_PARENT, 'HEAD').decode().splitlines()
    require(changes and all(line.startswith('M\t') and line[2:] in CONSUMER_PATHS for line in changes))
    changes = git(controls, 'diff', '--name-status', SOURCE, 'HEAD').decode().splitlines()
    require(changes and all(line.startswith('A\t') and
        (line[2:].startswith('scripts/hermetic_full12/') or line[2:] == '.github/workflows/forge-hermetic-full12.yml') for line in changes))


def checkout_proof(workspace):
    controls, source, base = [workspace / name for name in ('controls', 'source', 'services-base')]
    clean(controls, os.environ['GITHUB_SHA'])
    clean(source, SOURCE)
    clean(base, BASE)
    controls_history(controls)
    require(git(source, 'show', 'HEAD:.base-revision').decode().strip() == BASE)
    return {'source': SOURCE, 'base': BASE, 'controls': os.environ['GITHUB_SHA'],
            'source_tree': git(source, 'rev-parse', 'HEAD^{tree}').decode().strip()}


def export_sources(checkouts, destination):
    catalogues = read(HERE / 'source-catalogue.json')['catalogues']
    require(sum(x['count'] for x in catalogues.values()) == 265)
    for label, folder, revision in (('forge', 'CI-CD', SOURCE), ('base', 'services-base', BASE)):
        spec = catalogues[label]
        require(len(spec['files']) == spec['count'])
        encoded = json.dumps(spec['files'], sort_keys=True, separators=(',', ':')).encode()
        require(hashlib.sha256(encoded).hexdigest() == spec['sha256'])
        for item in spec['files']:
            name = item['path']
            relative = PurePosixPath(name)
            require(not relative.is_absolute() and str(relative) == name and '\\' not in name)
            require(not set(relative.parts) & {'..', '.local', 'target', '.git', 'node_modules', '.venv'})
            entry = git(checkouts[label], 'ls-tree', revision, '--', name).decode().split()
            require(len(entry) == 4 and entry[0] in ('100644', '100755') and entry[1] == 'blob' and entry[3] == name)
            raw = git(checkouts[label], 'cat-file', 'blob', entry[2])
            require(len(raw) == item['size'] and hashlib.sha256(raw).hexdigest() == item['sha256'])
            target = destination / folder / name
            target.parent.mkdir(parents=True, exist_ok=True)
            with target.open('xb') as stream:
                stream.write(raw)
            target.chmod(0o555 if entry[0] == '100755' else 0o444)
    return catalogues


def parity(root, catalogues):
    for label, folder in (('forge', 'CI-CD'), ('base', 'services-base')):
        expected = {item['path'] for item in catalogues[label]['files']}
        require({path.relative_to(root / folder).as_posix() for path in (root / folder).rglob('*') if path.is_file()} == expected)
        for item in catalogues[label]['files']:
            path = root / folder / item['path']
            require(not path.is_symlink() and sha(path) == item['sha256'])
    return True


def capacity(root):
    host_free = shutil.disk_usage(os.environ['RUNNER_TEMP']).free
    data_free = shutil.disk_usage(root).free
    inodes = os.statvfs(root).f_favail
    measurement = {'host_free_bytes': host_free, 'data_free_bytes': data_free, 'free_inodes': inodes,
                   'host_min_bytes': HOST_BYTES, 'data_min_bytes': DATA_BYTES, 'minimum_inodes': INODES}
    if host_free < HOST_BYTES or data_free < DATA_BYTES or inodes < INODES:
        raise CapacityFailure(measurement)
    return measurement


def reclaim(root):
    hosted_guard()
    result = []
    for literal in RECLAIM:
        if shutil.disk_usage(os.environ['RUNNER_TEMP']).free >= HOST_BYTES:
            break
        path = Path(literal)
        if not path.exists():
            continue
        require(path.resolve() == path and not any(p.is_symlink() for p in (path, *path.parents)))
        # Never recurse across a mount boundary, including bind mounts.
        mounts = [line.split()[4].replace('\\040', ' ') for line in Path('/proc/self/mountinfo').read_text().splitlines()]
        require(not any(Path(m).is_relative_to(path) for m in mounts))
        measured = int(command(['du', '-sb', '--', literal], timeout=120).split()[0])
        before = shutil.disk_usage(os.environ['RUNNER_TEMP']).free
        command(['sudo', '-n', 'rm', '-rf', '--one-file-system', '--', literal], timeout=300)
        require(not path.exists())
        after = shutil.disk_usage(os.environ['RUNNER_TEMP']).free
        result.append({'allowlisted_path': literal, 'measured_bytes': measured, 'free_before': before, 'free_after': after})
        atomic(root / 'reclaim.json', result)
    capacity(root)
    return result


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def download_tools(root):
    binary = root / 'bin'
    binary.mkdir(exist_ok=False)
    for name, (url, expected) in TOOLS.items():
        target = root / name
        with urllib.request.urlopen(url, timeout=60) as response, target.open('xb') as output:
            deadline = time.monotonic() + 300
            while block := response.read(2**20):
                require(time.monotonic() < deadline and output.tell() + len(block) <= 200 * 2**20)
                output.write(block)
        require(sha(target) == expected)
        if name.endswith('.tgz'):
            with tarfile.open(target) as archive:
                for member in archive:
                    if member.isdir():
                        continue
                    parts = PurePosixPath(member.name).parts
                    require(member.isfile() and len(parts) == 2 and parts[0] in ('docker', 'docker-rootless-extras'))
                    require(parts[1] not in ('.', '..') and not (binary / parts[1]).exists())
                    with archive.extractfile(member) as source, (binary / parts[1]).open('xb') as output:
                        shutil.copyfileobj(source, output)
                    (binary / parts[1]).chmod(0o755)
        else:
            shutil.copyfile(target, binary / name)
            (binary / name).chmod(0o755)
    plugins = root / 'docker-config/cli-plugins'
    plugins.mkdir(parents=True)
    for name in ('docker-compose', 'docker-buildx'):
        shutil.copyfile(binary / name, plugins / name)
        (plugins / name).chmod(0o755)
    os.environ['PATH'] = str(binary) + ':' + os.environ['PATH']
    os.environ['DOCKER_CONFIG'] = str(root / 'docker-config')
    return binary
