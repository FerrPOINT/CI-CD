"""Fresh hosted partitions; no native execution occurs on import."""
import ast
import copy
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import threading
import time
import uuid

import host as h

HERE = h.HERE
STAGES = tuple(h.read(HERE / 'coverage.json')['stage_order'])
JOBS = {'A': STAGES[:5], 'B': STAGES[5:6], 'C': STAGES[6:]}
BUDGETS = dict(zip(STAGES, (2400, 300, 300, 2400, 2400, 5400, 2400, 900, 900, 900, 900, 1800)))
BOOTSTRAP_SECONDS, ENTRY_SECONDS, EXIT_SECONDS, FINAL_SECONDS = 5400, 300, 500, 1800
ASSERT_SECONDS = 10
COMMAND_CLEANUP_SECONDS, HOST_SECONDS = 20, 21600
ACTION_SECONDS = {'A': 840, 'B': 840, 'C': 1260}
TASK, PURPOSE = 'forge-task-delivery', 'native-pg17-oci-gates'
POSTGRES = 'postgres:17-bookworm@sha256:3645570cccdfa447589da9f57dd740faa29b30938e861289a5574b6ca6b03826'
PYTHON = 'python:3.12-bookworm@sha256:e91fec3d1ac69f04e4eddcd29c327e630ce34658cf31075bfa7e8b0e052bafea'
RUST = 'rust:1.88.0@sha256:af306cfa71d987911a781c37b59d7d67d934f49684058f96cf72079c3626bfe0'
BINARIES = ('cicd-server', 'forge-runner', 'forge-delivery', 'forge-pg-migrate', 'openapi-dump', 'cicd-cli', 'cicd-migrate')

# Root-only filesystem operation; no shell, environment, private input or unit restart.
DELEGATION_PROGRAM = r'''
import os
from pathlib import Path
import re
import stat
import sys

BASE = Path('/run/systemd/system')


def directory(path):
    fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    info = os.fstat(fd)
    if info.st_uid != 0 or info.st_mode & 0o022:
        os.close(fd)
        raise ValueError('root_directory_required')
    return fd


def apply(action, uid, token):
    if (os.geteuid() != 0 or action not in ('install', 'cleanup')
            or not re.fullmatch('[1-9][0-9]{0,9}', uid)
            or not re.fullmatch('[a-f0-9]{32}', token)):
        raise ValueError('closed_delegation_identity')
    unit = 'user@' + uid + '.service'
    name = '90-forge-full12-' + token + '.conf'
    owned = '.forge-full12-' + token
    payload = ('# Forge full12 ' + token + '\n[Service]\nDelegate=cpu memory pids\n').encode()
    base = directory(BASE)
    parent = owner = None
    created = file_created = published = False
    try:
        try:
            os.mkdir(unit + '.d', 0o755, dir_fd=base)
        except FileExistsError:
            pass
        parent = directory(BASE / (unit + '.d'))
        if action == 'install':
            # The hidden owner directory is not a systemd drop-in. Publication is
            # one no-overwrite hard link to a completed, fsynced regular file.
            os.mkdir(owned, 0o700, dir_fd=parent)
            created = True
            os.fsync(parent)
            owner = directory(BASE / (unit + '.d') / owned)
            fd = os.open('delegate', os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=owner)
            file_created = True
            with os.fdopen(fd, 'wb') as stream:
                stream.write(payload)
                stream.flush()
                os.fsync(stream.fileno())
            os.fsync(owner)
            os.link('delegate', name, src_dir_fd=owner, dst_dir_fd=parent, follow_symlinks=False)
            published = True
            os.fsync(parent)
        else:
            try:
                owner = directory(BASE / (unit + '.d') / owned)
            except FileNotFoundError:
                # No installation happened. An unrelated final path is never removed.
                if os.path.lexists(BASE / (unit + '.d') / name):
                    raise ValueError('unproven_delegation_file')
                return
            if set(os.listdir(owner)) != {'delegate'}:
                raise ValueError('unproven_owner_directory')
            fd = os.open('delegate', os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=owner)
            with os.fdopen(fd, 'rb') as stream:
                info = os.fstat(stream.fileno())
                if not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022:
                    raise ValueError('owned_regular_required')
                if info.st_size != len(payload) or stream.read(len(payload) + 1) != payload:
                    raise ValueError('unproven_delegation_bytes')
            try:
                published = os.stat(name, dir_fd=parent, follow_symlinks=False)
            except FileNotFoundError:
                published = None
            if published is not None:
                if (published.st_dev, published.st_ino) != (info.st_dev, info.st_ino):
                    raise ValueError('foreign_delegation_inode')
                os.unlink(name, dir_fd=parent)
                os.fsync(parent)
            os.unlink('delegate', dir_fd=owner)
            os.fsync(owner)
            os.close(owner)
            owner = None
            os.rmdir(owned, dir_fd=parent)
            os.fsync(parent)
    except BaseException:
        # On a reported installation failure remove only entries created by this
        # invocation. A killed/unknown helper is checked again by outer finally.
        if action == 'install' and created:
            if published:
                os.unlink(name, dir_fd=parent)
            if file_created:
                os.unlink('delegate', dir_fd=owner)
            if owner is not None:
                os.close(owner)
                owner = None
            os.rmdir(owned, dir_fd=parent)
            os.fsync(parent)
        raise
    finally:
        for fd in (owner, parent, base):
            if fd is not None:
                os.close(fd)


if __name__ == '__main__':
    apply(*sys.argv[1:])
'''


def delegation_owner(root):
    owner = h.read(root / 'delegation-owner.json')
    h.require(set(owner) == {'uid', 'token', 'owner_pid', 'root'}
              and owner['uid'] == str(os.getuid()) and owner['owner_pid'] == os.getpid()
              and owner['root'] == str(root) and re.fullmatch('[a-f0-9]{32}', owner['token']))
    return owner


def install_delegation(root):
    h.hosted_guard()
    owner = {'uid': str(os.getuid()), 'token': uuid.uuid4().hex, 'owner_pid': os.getpid(), 'root': str(root)}
    h.atomic(root / 'delegation-owner.json', owner, exclusive=True)
    h.command(['sudo', '-n', 'python3', '-c', DELEGATION_PROGRAM, 'install', owner['uid'], owner['token']], timeout=90)


def cleanup_delegation(root):
    if not (root / 'delegation-owner.json').exists():
        return True
    h.hosted_guard()
    owner = delegation_owner(root)
    h.command(['sudo', '-n', 'python3', '-c', DELEGATION_PROGRAM, 'cleanup', owner['uid'], owner['token']], timeout=20)
    h.command(['sudo', '-n', 'systemctl', 'daemon-reload'], timeout=20)
    return True


def verify_delegation(root):
    owner = delegation_owner(root)
    unit = 'user@' + owner['uid'] + '.service'
    raw = h.command(['systemctl', 'show', unit, '--property=Delegate', '--property=DelegateControllers',
                     '--property=DropInPaths', '--property=ControlGroup']).decode()
    rows = [line.split('=', 1) for line in raw.splitlines()]
    h.require(all(len(row) == 2 for row in rows) and len(rows) == 4)
    values = dict(rows)
    h.require(set(values) == {'Delegate', 'DelegateControllers', 'DropInPaths', 'ControlGroup'})
    h.require(values['Delegate'] == 'yes' and {'cpu', 'memory', 'pids'} <= set(values['DelegateControllers'].split()))
    path = '/run/systemd/system/' + unit + '.d/90-forge-full12-' + owner['token'] + '.conf'
    h.require(path in values['DropInPaths'].split())
    group = '/user.slice/user-' + owner['uid'] + '.slice/' + unit
    h.require(values['ControlGroup'] == group)
    return Path('/sys/fs/cgroup' + group) / 'cgroup.controllers'


def job_budget(job):
    # Three possible command-group cleanups per stage: entry, native call, exit.
    stages = sum(BUDGETS[name] + ENTRY_SECONDS + ASSERT_SECONDS + EXIT_SECONDS + 3 * COMMAND_CLEANUP_SECONDS
                 + (30 + COMMAND_CLEANUP_SECONDS if name == 'cli' else 2 if name == 'smoke' else 0) for name in JOBS[job])
    return BOOTSTRAP_SECONDS + COMMAND_CLEANUP_SECONDS + stages + FINAL_SECONDS + COMMAND_CLEANUP_SECONDS + ACTION_SECONDS[job]


def install_process_adapter(sdk, q):
    """Keep request cancellation but never call the frozen reaping RequestLease.checked path."""
    def checked(argv, *, timeout=90, text=False):
        h.require(type(text) is bool and not os.environ.get('SDLC_DOCKER_EXECUTABLE'))
        request = q.REQUEST.get()
        raw = h.command(argv, timeout=timeout, check=request.check if request is not None else None)
        return raw.decode('utf-8') if text else raw
    checked._forge_request_lifetime = True
    sdk.checked = checked


def install_native_ci_policy(q):
    """Select CI policy only at this hosted bridge, never change Base's local default."""
    session_class = q.session_class

    def hosted_session_class(sdk):
        class HostedNativeSession(session_class(sdk)):
            def __init__(self, **kwargs):
                h.hosted_guard()
                h.require('resource_policy' not in kwargs and kwargs.get('task') == TASK
                          and kwargs.get('docker') == h.DOCKER and kwargs.get('daemon_id') == q.DAEMON_ID)
                super().__init__(resource_policy='isolated-ci-v1', **kwargs)
        return HostedNativeSession

    q.session_class = hosted_session_class


def docker(*args):
    return h.command(h.DOCKER + list(args))


def inventory(project=None):
    filters = ['--filter', 'label=com.docker.compose.project=' + project] if project else []
    return {kind: sorted(docker(kind, 'ls', '-aq' if kind == 'container' else '-q', *filters).decode().split())
            for kind in ('container', 'network', 'volume')}


def inventory_proof(value):
    return {kind: {'count': len(ids), 'sha256': hashlib.sha256(json.dumps(ids, separators=(',', ':')).encode()).hexdigest()}
            for kind, ids in value.items()}


def journal_proof(path):
    data = h.read(path)
    keys = ('version', 'project', 'task', 'purpose', 'daemon_id', 'manifest_sha256', 'phase', 'cleanup_id', 'sdk_sha256')
    return {**{key: data.get(key) for key in keys}, 'journal_sha256': h.sha(path)}


def identity(root, admission=None):
    with h.bootstrap_step('identity_decode'):
        info = json.loads(docker('info', '--format', '{{json .}}'))
        endpoint = docker('context', 'inspect', 'rootless', '--format', '{{.Endpoints.docker.Host}}').decode().strip()
        result = {'id': info['ID'], 'endpoint': endpoint, 'root': info['DockerRootDir'],
                  'server': info['ServerVersion'], 'client': docker('version', '--format', '{{.Client.Version}}').decode().strip(),
                  'compose': docker('compose', 'version').decode().strip(), 'security': sorted(info['SecurityOptions']),
                  'storage_driver': info['Driver'], 'cgroup_driver': info.get('CgroupDriver')}
    with h.bootstrap_step('identity_version'):
        h.require(result['id'] and result['server'] == result['client'] == '29.8.2')
    with h.bootstrap_step('identity_compose_rootless'):
        h.require(result['compose'] == 'Docker Compose version v5.5.1' and 'name=rootless' in result['security'])
    with h.bootstrap_step('identity_endpoint'):
        h.require(result['endpoint'] == 'unix://' + str(root / 'docker.sock') and Path(result['root']) == root / 'daemon-data')
    with h.bootstrap_step('identity_cgroup_version'):
        h.require(info.get('CgroupVersion') == '2')
    with h.bootstrap_step('identity_cgroup_driver'):
        h.require(info.get('CgroupDriver') == 'systemd')
    with h.bootstrap_step('identity_cgroup_resources'):
        h.require(all(info.get(key) is True for key in ('MemoryLimit', 'CpuCfsQuota', 'CpuCfsPeriod', 'PidsLimit')))
    if admission is not None:
        h.require(result == admission['daemon'])
    return result


def start_daemon(root):
    with h.bootstrap_step('tools_download'):
        binary = h.download_tools(root)
    uid = str(os.getuid())
    # These changes are confined to the disposable hosted VM, never a local daemon.
    with h.bootstrap_step('dependencies'):
        h.command(['sudo', '-n', 'apt-get', 'update'], timeout=300, log=root / 'private/apt-update.log')
        h.command(['sudo', '-n', 'apt-get', 'install', '-y', '--no-install-recommends',
                   'uidmap', 'slirp4netns', 'dbus-user-session'], timeout=300, log=root / 'private/apt-install.log')
    with h.bootstrap_step('user_namespace'):
        if Path('/proc/sys/kernel/apparmor_restrict_unprivileged_userns').exists():
            h.command(['sudo', '-n', 'sysctl', '-w', 'kernel.apparmor_restrict_unprivileged_userns=0'])
        user = h.command(['id', '-un']).decode().strip()
        for file in ('subuid', 'subgid'):
            ranges = [line.split(':') for line in Path('/etc/' + file).read_text().splitlines()]
            if not any(parts[0] == user and int(parts[2]) >= 65536 for parts in ranges):
                h.require(not any(int(parts[1]) < 165536 and int(parts[1]) + int(parts[2]) > 100000 for parts in ranges))
                h.command(['sudo', '-n', 'usermod', '--add-' + file + 's', '100000-165535', user])
    with h.bootstrap_step('manager_dropin'):
        install_delegation(root)
    with h.bootstrap_step('manager_reload'):
        h.command(['sudo', '-n', 'systemctl', 'daemon-reload'], timeout=90)
    with h.bootstrap_step('manager_linger'):
        h.command(['sudo', '-n', 'loginctl', 'enable-linger', user])
    with h.bootstrap_step('manager_start'):
        h.command(['sudo', '-n', 'systemctl', 'start', 'user@' + uid + '.service'])
    with h.bootstrap_step('manager_readback'):
        controllers = verify_delegation(root)
    with h.bootstrap_step('manager_controllers'):
        h.require({'cpu', 'memory', 'pids'} <= set(controllers.read_text().split()))
        os.environ.update(XDG_RUNTIME_DIR='/run/user/' + uid, DBUS_SESSION_BUS_ADDRESS='unix:path=/run/user/' + uid + '/bus')
    with h.bootstrap_step('rootless_launch'):
        unit = 'forge-full12-' + root.name
        h.atomic(root / 'daemon-owner.json', {'unit': unit, 'root': str(root), 'owner_pid': os.getpid()}, exclusive=True)
        h.command(['systemd-run', '--user', '--unit=' + unit, '--property=Delegate=yes',
            '--property=TimeoutStopSec=90',
            '--setenv=PATH=' + os.environ['PATH'], '--setenv=XDG_RUNTIME_DIR=' + os.environ['XDG_RUNTIME_DIR'],
            '--setenv=DBUS_SESSION_BUS_ADDRESS=' + os.environ['DBUS_SESSION_BUS_ADDRESS'],
            '--setenv=DOCKERD_ROOTLESS_ROOTLESSKIT_NET=slirp4netns',
            str(binary / 'dockerd-rootless.sh'), '--host=unix://' + str(root / 'docker.sock'),
            '--data-root=' + str(root / 'daemon-data'), '--exec-root=' + str(root / 'daemon-exec'),
            '--pidfile=' + str(root / 'dockerd.pid')], timeout=90)
    with h.bootstrap_step('context'):
        h.command(['docker', 'context', 'create', 'rootless', '--docker', 'host=unix://' + str(root / 'docker.sock')])
    with h.bootstrap_step('socket_ready'):
        deadline = time.monotonic() + 90
        while not (root / 'docker.sock').exists():
            h.require(time.monotonic() < deadline)
            time.sleep(0.25)
    # A failed info is an actual admission failure, not a fallback to the host daemon.
    return identity(root)


def stop_daemon(root):
    marker = root / 'daemon-owner.json'
    if not marker.exists():
        return {'started': False, 'stopped': True}
    owner = h.read(marker)
    h.require(owner == {'unit': 'forge-full12-' + root.name, 'root': str(root), 'owner_pid': os.getpid()})
    h.command(['systemctl', '--user', 'stop', owner['unit']], timeout=90)
    state = h.command(['systemctl', '--user', 'list-units', '--all', '--plain', '--no-legend', owner['unit'] + '.service']).decode().strip()
    h.require(not state or ' inactive ' in state or ' failed ' in state)
    return {'started': True, 'stopped': True}


def build_tools(root):
    result = {}
    for label, reference in (('postgres', POSTGRES), ('python', PYTHON), ('rust', RUST)):
        with h.bootstrap_step('image_pull'):
            h.command(h.DOCKER + ['pull', '--platform', 'linux/amd64', reference], timeout=1200, log=root / ('private/pull-' + label + '.log'))
        with h.bootstrap_step('image_readback'):
            item = json.loads(docker('image', 'inspect', reference))[0]
            h.require(item['Os'] == 'linux' and item['Architecture'] == 'amd64')
            result[label] = item['Id']
    with h.bootstrap_step('tools_build'):
        build = root / 'build'
        build.mkdir()
        for name in ('docker', 'docker-compose', 'docker-buildx'):
            shutil.copyfile(root / 'bin' / name, build / name)
            (build / name).chmod(0o555)
        shutil.copyfile(HERE / 'tools.Dockerfile', build / 'Dockerfile')
        tag = 'sdlc-build-forge-tools:' + root.name
        h.command(h.DOCKER + ['build', '--pull=false', '-t', tag, str(build)], timeout=1800, log=root / 'private/build-tools.log')
    with h.bootstrap_step('tools_readback'):
        result['tools'] = json.loads(docker('image', 'inspect', tag))[0]['Id']
        h.require(all(re.fullmatch('sha256:[a-f0-9]{64}', value) for value in result.values()))
    return result


def bind(source, target, readonly=False):
    return {'type': 'bind', 'source': str(source), 'target': target, 'read_only': readonly,
            'bind': {'create_host_path': False}}


def manifest(root, config, images):
    labels = {'sdlc.task': TASK, 'sdlc.purpose': PURPOSE}
    env = {'DOCKER_HOST': 'unix:///var/run/docker.sock', 'RUSTUP_TOOLCHAIN': '1.88.0', 'RUSTUP_HOME': '/usr/local/rustup',
        'CARGO_HOME': '/cache/cargo', 'CARGO_TARGET_DIR': '/cache/target/rust-gates', 'CARGO_BUILD_JOBS': '2',
        'CARGO_INCREMENTAL': '0', 'CARGO_PROFILE_DEV_DEBUG': '0', 'CARGO_PROFILE_TEST_DEBUG': '0',
        'CARGO_NET_RETRY': '2', 'CARGO_HTTP_TIMEOUT': '60', 'PYTHONDONTWRITEBYTECODE': '1',
        'CICD_TEST_DATABASE_URL': 'postgres://postgres@postgres:5432/forge_test_delivery',
        'CICD_TEST_PG_IMAGE': images['postgres']}
    for kind in ('pg', 'oci'):
        for suffix, value in (('PROJECT', config[kind + '_project']), ('NETWORK', config['project'] + '_qa'),
                ('VOLUME', config['project'] + '_delivery-qa'), ('COMPOSE_ROOT', str(root / 'children' / kind))):
            env['CICD_TEST_' + kind.upper() + '_' + suffix] = value
    qa = {'image': images['tools'], 'pull_policy': 'never', 'privileged': True, 'init': True,
        'cpus': 2, 'mem_limit': '5g', 'pids_limit': 512, 'labels': labels, 'networks': ['qa'],
        'working_dir': '/work/CI-CD/backend', 'entrypoint': ['sleep', 'infinity'], 'environment': env,
        'depends_on': {'postgres': {'condition': 'service_healthy'}}, 'volumes': [
            bind(root / 'sources/CI-CD', '/work/CI-CD', True), bind(root / 'sources/services-base', '/work/services-base', True),
            bind(root / 'output', '/output'), bind(root / 'cache', '/cache'), bind(root / 'children', str(root / 'children')),
            bind(root / 'docker.sock', '/var/run/docker.sock'), {'type': 'volume', 'source': 'delivery-qa', 'target': '/delivery-qa'}]}
    postgres = {'image': images['postgres'], 'pull_policy': 'never', 'init': True, 'labels': labels,
        'environment': {'POSTGRES_USER': 'postgres', 'POSTGRES_DB': 'forge_test_delivery', 'POSTGRES_HOST_AUTH_METHOD': 'trust'},
        'tmpfs': ['/var/lib/postgresql/data:rw,size=512m'], 'cpus': 1, 'mem_limit': '1g', 'networks': ['qa'],
        'healthcheck': {'test': ['CMD-SHELL', "pg_isready -h 127.0.0.1 -U postgres -d forge_test_delivery && psql -X -qAt -U postgres -d forge_test_delivery -c 'SELECT 1'"],
                        'interval': '1s', 'timeout': '2s', 'retries': 60, 'start_period': '15s'}}
    return {'services': {'postgres': postgres, 'qa': qa}, 'networks': {'qa': {'internal': True, 'labels': labels}},
            'volumes': {'delivery-qa': {'labels': labels}}}


def resource_enforcement(operation, images):
    """Read the two existing containers; no helpers or workload start before proof."""
    proof = {}
    for service, cpu, memory, pids, image in (
            ('qa', 2, 5 * 2**30, 512, images['tools']), ('postgres', 1, 2**30, None, images['postgres'])):
        raw = h.command(operation.command + ['ps', '--all', '-q', service], timeout=10).decode().split()
        h.require(len(raw) == 1 and re.fullmatch('[a-f0-9]{64}', raw[0]))
        container = raw[0]
        def inspect():
            raw = h.command(h.DOCKER + ['inspect', '--type', 'container', container], timeout=10)
            h.require(len(raw) <= 65536)
            items = json.loads(raw)
            h.require(type(items) is list and len(items) == 1)
            item = items[0]
            h.require(item['Id'] == container and item['Image'] == image and item['State']['Running'] is True)
            labels = item['Config']['Labels']
            h.require(all(labels.get(key) == value for key, value in {
                'com.docker.compose.project': operation.project, 'com.docker.compose.service': service,
                'sdlc.task': TASK, 'sdlc.purpose': PURPOSE}.items()))
            limits = item['HostConfig']
            h.require(type(limits.get('NanoCpus')) is int and limits['NanoCpus'] == cpu * 10**9
                      and type(limits.get('Memory')) is int and limits['Memory'] == memory
                      and limits.get('CgroupnsMode') == 'private')
            if pids is not None:
                h.require(type(limits.get('PidsLimit')) is int and limits['PidsLimit'] == pids)
            return {key: limits.get(key) for key in ('NanoCpus', 'Memory', 'PidsLimit', 'CgroupnsMode')}
        before = inspect()
        # Fixed read-only script, bounded per-file reads, bound to inspected ID not service alias.
        script = 'set -eu; for f in cpu.max memory.max pids.max; do v="$(head -c 129 /sys/fs/cgroup/$f)"; printf "%s\\n" "$v"; done'
        raw = h.command(h.DOCKER + ['exec', container, 'sh', '-c', script], timeout=10)
        h.require(len(raw) <= 390)
        lines = raw.decode('ascii').splitlines()
        h.require(len(lines) == 3 and re.fullmatch('[1-9][0-9]{0,9} [1-9][0-9]{0,9}', lines[0]))
        quota, period = map(int, lines[0].split())
        h.require(quota == cpu * period and 1000 <= period <= 1000000 and lines[1] == str(memory))
        if pids is not None:
            h.require(lines[2] == str(pids))
        else:
            h.require(lines[2] == 'max' or re.fullmatch('[1-9][0-9]{0,9}', lines[2]))
        h.require(inspect() == before)
        proof[service] = {'cpu_count': cpu, 'memory_bytes': memory, 'pids_limit': pids}
    return proof


def fill_cache(root, sdk, q, parent, images, token, cache_operations):
    with h.bootstrap_step('cache_prepare'):
        with h.bootstrap_step('cache_allocate'):
            operation = parent.parent_class(sdk, q)(project='sdlc-build-forge-cache-' + token, task=TASK,
                purpose='locked-dependency-fetch-only', docker=h.DOCKER, directory=root / 'cache-fetch', daemon_id=q.DAEMON_ID,
                resource_policy='isolated-ci-v1')
            cache_operations.append(operation)
        with h.bootstrap_step('cache_manifest'):
            service = {'image': images['tools'], 'init': True, 'user': '0:0', 'cap_drop': ['ALL'], 'networks': ['fetch'],
                'entrypoint': ['cargo'], 'command': ['fetch', '--locked', '--target', 'x86_64-unknown-linux-gnu'],
                'working_dir': '/work/CI-CD/backend', 'environment': {'CARGO_HOME': '/cache/cargo', 'RUSTUP_TOOLCHAIN': '1.88.0',
                    'CARGO_TARGET_DIR': '/tmp/fetch-target', 'CARGO_NET_RETRY': '2', 'CARGO_HTTP_TIMEOUT': '60'},
                'volumes': [bind(root / 'sources/CI-CD', '/work/CI-CD', True),
                    bind(root / 'sources/services-base', '/work/services-base', True), bind(root / 'cache', '/cache')]}
            operation.write({'fetch': service}, networks={'fetch': {}})
    with h.bootstrap_step('cache_fetch'):
        h.command(operation.command + ['up', '--pull', 'never', '--abort-on-container-exit', '--exit-code-from', 'fetch'],
            timeout=2400, log=root / 'private/cache-fetch.log')
    with h.bootstrap_step('cache_cleanup'):
        close_operation(operation)
        h.require(not any(inventory(operation.project).values()))
    with h.bootstrap_step('cache_seal'):
        h.require((root / 'cache/cargo/registry').is_dir() and not (root / 'cache/target').exists())
        files = []
        for path in sorted((root / 'cache/cargo').rglob('*')):
            h.require(not path.is_symlink())
            if path.is_file():
                files.append({'path': path.relative_to(root / 'cache/cargo').as_posix(), 'sha256': h.sha(path), 'size': path.stat().st_size})
        h.atomic(root / 'cache-source-tree.json', files, exclusive=True)
        return {'new_locked_cache': True, 'private_cache_imported': False, 'journal_sha256': h.sha(operation.journal),
                'journal': journal_proof(operation.journal),
                'initial_tree_sha256': h.sha(root / 'cache-source-tree.json'), 'initial_file_count': len(files),
                'initial_bytes': sum(item['size'] for item in files)}


def smoke(root, operation, q, admission, images):
    base = h.load('hosted_smoke_observer', HERE / 'observer.py')
    transport = h.load('hosted_smoke_transport', HERE / 'transport.py')
    # Admission adaptation only: assertions and transport/observer bodies stay byte-identical.
    h.bind_maintenance(base, admission['maintenance_git'])
    h.require(base.SDK_SHA256 == q.SDK_SHA256)
    base.DAEMON_ID = transport.DAEMON_ID = admission['daemon']['id']
    transport.ENDPOINT, transport.DAEMON_ROOT = admission['daemon']['endpoint'], admission['daemon']['root']
    transport.PG_IMAGE = images['postgres']
    observer = base.Observer(sys.stdout)
    project, path = operation.project, str(operation.path)
    executor = str(root / 'sources/CI-CD/scripts/postgres-delivery.py')
    binding = transport.BoundExec(base, observer, project, path)
    handlers = {sig: signal.signal(sig, observer.interrupt) for sig in (signal.SIGTERM, signal.SIGINT)}
    try:
        raw = base.validate_inputs(project, path, executor, HERE / 'sql_smoke.py')
        binding.bind()
        observer.popen = binding.spawn
        code = base.execute(raw, observer, [project, path, executor])
        h.require(code == 0)
    finally:
        observer.cleanup()
        for sig, previous in handlers.items():
            signal.signal(sig, previous)


def assertions(stage, raw, root):
    content = raw.decode('utf-8', errors='replace')
    proof = {}
    markers = {
        'python': ('FROZEN_LINUX_PYTHON_UNITS:PASS', 'Ran 75 tests'),
        'row-smoke': ('ACTUAL_PG17_RECORD_TEXT_SMOKE:PASS',),
        'smoke': ('ACTUAL_PG17_SQL_SMOKE:PASS',),
        'check': ('LOCKED_INTEGRATION_FEATURES_CHECK:PASS',),
        'clippy': ('LOCKED_INTEGRATION_FEATURES_CLIPPY:PASS',),
        'postgres': ('3 passed; 0 failed', 'ACTUAL_PG_READER_LO_DENIED_AND_EXCLUSIVE_DATABASE_ACCESS:PASS',
            'ACTUAL_PG_WRITER_HISTORY_VIEW_DML_DENIED:PASS', 'ACTUAL_PG_FOREIGN_PROOF_REJECTED_CURRENT_WRITER_POOL_RECOVERY:PASS'),
        'oci': ('1 passed; 0 failed', 'ACTUAL_OCI_SIGKILL_AFTER_VERIFIED_CHECKS_HISTORICAL_PROOF_PRESERVED:PASS'),
        'workspace': ('FROZEN_WORKSPACE_RUSTFMT:PASS', 'FROZEN_WORKSPACE_UMASK=0022'),
        'integration': (), 'cli': ('CLI_INTEGRATION_CLIPPY:PASS',),
        'openapi': ('OPENAPI_EXPORTER_EQUALITY_STRIP_TRAILING_CR:PASS',), 'release': ('RELEASE_WORKSPACE_BUILD:PASS',)}
    h.require(all(marker in content for marker in markers[stage]))
    if stage in ('workspace', 'integration', 'cli'):
        counts = re.findall(r'test result: ok\. (\d+) passed; 0 failed;', content)
        total = sum(map(int, counts))
        h.require(counts and total == {'workspace': 221, 'integration': 80, 'cli': 2}[stage])
        proof['tests_passed'] = total
    if stage == 'postgres':
        matches = re.findall(r'^ACTUAL_PG_RESTORE_NEGATIVES=([^:\n]+):', content, re.M)
        h.require(len(matches) == 1)
        cases = matches[0].split(',')
        h.require(len(cases) == len(set(cases)) == 24)
        h.require({'writer-history-child', 'writer-history-grandchild', 'writer-history-parent',
                   'writer-history-fk-delete', 'writer-history-fk-update'} <= set(cases))
        h.require(all(value in content for value in ('writer-history-column', 'writer-history-view',
                                                    'writer-history-view-column', 'writer-history-rule')))
        proof['tests_passed'], proof['negative_count'] = 3, 24
    if stage == 'oci':
        proof['tests_passed'] = 1
    if stage == 'python':
        proof['tests_passed'] = 75
    if stage == 'release':
        proof['binaries'] = {name: h.sha(root / 'disposable/target/rust-gates/release' / name) for name in BINARIES}
    if stage == 'openapi':
        proof['export_sha256'] = h.sha(root / 'output/openapi-export.yaml')
        proof['contract_sha256'] = h.sha(root / 'sources/CI-CD/openapi/openapi.yaml')
    return proof


def close_operation(operation):
    # SDK close records cleanup-required in its BaseException handler; guard that late I/O too.
    record = operation.record
    def bounded_record(*args, **kwargs):
        h.check_deadline()
        result = record(*args, **kwargs)
        h.check_deadline()
        return result
    operation.record = bounded_record
    try:
        h.check_deadline()
        operation.close()
        h.check_deadline()
    finally:
        operation.record = record


def close_native_bridge(bridge):
    """Preserve SDK cleanup/bookkeeping, but never absorb a phase deadline as RuntimeError."""
    errors = []
    h.check_deadline()
    with bridge.lock:
        h.check_deadline()
        for project, operation in list(bridge.operations.items()):
            h.check_deadline()
            try:
                close_operation(operation)
                h.check_deadline()
            except BaseException as error:
                h.check_deadline()
                if isinstance(error, h.OverheadTimeout):
                    raise
                errors.append(type(error).__name__)
            else:
                bridge.closed.append(str(operation.journal))
                del bridge.operations[project]
    if errors:
        raise RuntimeError('Native v2 cleanup incomplete: ' + ','.join(errors))


def run_stage(stage, root, config, images, admission, q, sdk, parent, catalogues, expected_seals):
    operation = bridge = None
    server = thread = None
    native_complete = False
    failure = None
    proof = {'stage': stage, 'status': 'FAIL', 'timeout_seconds': BUDGETS[stage]}
    try:
        with h.wall_budget(ENTRY_SECONDS):
            check_runtime_seals(root, expected_seals)
            h.verify_components()
            h.capacity(root)
            identity(root, admission)
            h.parity(root / 'sources', catalogues)
            h.require(not any(any(inventory(config[key]).values()) for key in ('project', 'pg_project', 'oci_project')))
            operation = parent.parent_class(sdk, q)(project=config['project'], task=TASK, purpose=PURPOSE,
                docker=h.DOCKER, directory=root / 'stages' / stage, daemon_id=q.DAEMON_ID,
                resource_policy='isolated-ci-v1')
            bridge = q.NativeComposeBridge(sdk=sdk, children=root / 'children', pg_project=config['pg_project'],
                                          oci_project=config['oci_project'], registry=root / 'registry')
            operation.write_reviewed(parent.parent_manifest(q, manifest(root, config, images), config))
            server, thread = q.serve(bridge, root / 'children/maintenance.sock')
            h.command(operation.command + ['up', '-d', '--pull', 'never', '--wait', '--wait-timeout', '90'],
                timeout=180, log=root / ('private/' + stage + '-up.log'))
            proof['resource_enforcement'] = resource_enforcement(operation, images)
            inner_id = h.command(operation.command + ['exec', '-T', 'qa', 'docker', 'info', '--format', '{{.ID}}']).decode().strip()
            h.require(inner_id == admission['daemon']['id'])
        path = root / ('private/' + stage + '.log')
        if stage == 'smoke':
            # The observer owns all smoke children and its unchanged 300/30/10 budget.
            import contextlib
            with path.open('x') as sink, contextlib.redirect_stdout(sink):
                smoke(root, operation, q, admission, images)
        elif stage == 'row-smoke':
            h.command(['python3', '-B', str(HERE / 'row_smoke.py'), operation.project, str(operation.path),
                str(root / 'sources/CI-CD/scripts/postgres-delivery.py')], timeout=300, log=path)
        else:
            env = []
            if stage == 'cli':
                h.command(operation.command + ['exec', '-T', 'postgres', 'createdb', '-U', 'postgres', 'forge_test_cli'], timeout=30)
                env = ['-e', 'CICD_TEST_DATABASE_URL=postgres://postgres@postgres:5432/forge_test_cli']
            script = 'gates.sh' if stage in STAGES[:7] else 'followups.sh'
            h.command(operation.command + ['exec', '-T', *env, 'qa', 'bash', '/output/' + script, stage],
                timeout=BUDGETS[stage], log=path)
        native_complete = True
    except BaseException as error:
        failure = error
        raise
    finally:
        try:
            if native_complete:
                with h.wall_budget(ASSERT_SECONDS):
                    proof.update(assertions(stage, path.read_bytes(), root), status='PASS')
        except BaseException as error:
            failure = error
            raise
        finally:
            # A consumed assertions alarm cannot disable the independent teardown reserve.
            with h.wall_budget(EXIT_SECONDS):
                if failure is not None:
                    proof['error'] = h.safe_error(stage, failure)
                errors = []
                try:
                    if operation is not None and operation.manifest_hash:
                        h.command(operation.command + ['stop', '-t', '10', 'qa'], timeout=60)
                except BaseException as error:
                    h.check_deadline()
                    if isinstance(error, h.OverheadTimeout):
                        raise
                    errors.append(h.safe_error('cleanup', error))
                if server is not None:
                    try:
                        shutdown = threading.Thread(target=server.shutdown, daemon=True)
                        shutdown.start()
                        shutdown.join(timeout=100)
                        h.require(not shutdown.is_alive())
                        thread.join(timeout=100)
                        h.require(not thread.is_alive())
                        server.server_close()
                        h.check_deadline()
                        (root / 'children/maintenance.sock').unlink(missing_ok=True)
                    except BaseException as error:
                        h.check_deadline()
                        if isinstance(error, h.OverheadTimeout):
                            raise
                        errors.append(h.safe_error('cleanup', error))
                closes = ([lambda: close_native_bridge(bridge)] if bridge is not None else [])
                closes += [lambda: close_operation(operation)] if operation is not None else []
                for close in closes:
                    h.check_deadline()
                    try:
                        close()
                    except BaseException as error:
                        h.check_deadline()
                        if isinstance(error, h.OverheadTimeout):
                            raise
                        errors.append(h.safe_error('cleanup', error))
                    h.check_deadline()
                h.check_deadline()
                remaining = {config[key]: inventory(config[key]) for key in ('project', 'pg_project', 'oci_project')}
                proof['cleanup_complete'] = not errors and not any(any(x.values()) for x in remaining.values())
                h.check_deadline()
                proof['source_parity'] = h.parity(root / 'sources', catalogues)
                h.check_deadline()
                proof['journals'] = ([str(operation.journal)] if operation is not None else []) + (bridge.closed if bridge is not None else [])
                proof['journal_proofs'] = [journal_proof(Path(path)) for path in proof['journals']]
                proof['remaining'] = {project: inventory_proof(value) for project, value in remaining.items()}
                proof['cleanup_errors'] = errors
                h.check_deadline()
                if not proof['cleanup_complete'] or not proof['source_parity']:
                    proof['status'] = 'FAIL'
                h.atomic(root / ('stage-' + stage + '.json'), proof, exclusive=True)
                h.require(proof['cleanup_complete'])
    return proof


def check_runtime_seals(root, expected):
    h.require(h.read(root / 'runtime-seal-hashes.json') == expected)
    h.require(set(expected) == {'admission-seal.json', 'execution-seal.json'})
    h.require(all(h.sha(root / name) == digest for name, digest in expected.items()))
    for name, item in h.m.EXPECTED_FILES.items():
        h.require(h.sha(root / 'maintenance' / name) == item['sha256'])


def python_counts(root):
    counts = {}
    for name, expected in (('test_postgres_delivery.py', 47), ('test_oci_offline_fixture.py', 28)):
        tree = ast.parse((root / 'sources/CI-CD/scripts/tests' / name).read_bytes())
        methods = [node for cls in tree.body if isinstance(cls, ast.ClassDef) for node in cls.body
                   if isinstance(node, ast.FunctionDef) and node.name.startswith('test_')]
        h.require(len(methods) == expected)
        counts[name] = len(methods)
    return counts


def cleanup_disposable(root, config, ownership, q, parent, admission, proofs):
    if ownership is None:
        return {'created': False, 'complete': True}
    identity(root, admission)
    remaining = {config[key]: inventory(config[key]) for key in ('project', 'pg_project', 'oci_project')}
    h.require(not any(any(value.values()) for value in remaining.values()))
    # Read actual journals independently: a disk error may have lost the stage receipt.
    journals = [*sorted((root / 'stages').rglob('journal.json')), *sorted((root / 'children').rglob('journal.json'))]
    valid = []
    projects = {config[key] for key in ('project', 'pg_project', 'oci_project')}
    for journal in journals:
        data = q.read(q.plain(journal))
        h.require(data.get('version') == 2 and data.get('phase') == 'cleaned' and data.get('task') == TASK
            and data.get('daemon_id') == q.DAEMON_ID and data.get('sdk_sha256') == q.SDK_SHA256
            and data.get('project') in projects and data.get('owner_pid') == os.getpid()
            and re.fullmatch('[a-f0-9]{32}', data.get('cleanup_id', '')))
        path = journal.parent / 'compose.json'
        if data.get('manifest_sha256') is None:
            h.require(not path.exists())
        else:
            h.require(data.get('manifest') == str(path) and data['manifest_sha256'] == h.sha(q.plain(path)))
            valid.append(journal)
    h.require(not list((root / 'registry').iterdir()))
    if valid:
        parents = [path for path in valid if h.read(path)['project'] == config['project']]
        nested = [path for path in valid if path not in parents]
        h.require(parents)
        normalized = {project: {kind + 's': ids for kind, ids in state.items()} for project, state in remaining.items()}
        result = parent.cleanup_disposable(q, config, ownership, normalized, parents[-1], nested)
        return {'created': True, 'complete': result['status'] == 'cleaned', 'journal_count': len(journals)}
    # Prelaunch failure: exact owned empty binds only. This is NOT a v2 execution journal.
    marker = root / 'disposable/ownership.json'
    h.require(h.sha(marker) == ownership and h.read(marker) == parent.disposable_identity(q, config))
    h.require(set(path.name for path in marker.parent.iterdir()) == {'ownership.json', 'target', 'scratch'})
    targets = [marker.parent / name for name in ('target', 'scratch')]
    h.require(all(not path.is_symlink() and path.resolve().parent == marker.parent and not list(path.iterdir()) for path in targets))
    for path in targets:
        path.rmdir()
    return {'created': True, 'complete': True, 'prelaunch_empty_only': True, 'v2_execution_claimed': False}


def public_stage(proof):
    keys = ('stage', 'status', 'timeout_seconds', 'tests_passed', 'negative_count', 'binaries',
            'export_sha256', 'contract_sha256', 'error', 'cleanup_complete', 'source_parity', 'journal_proofs', 'remaining', 'resource_enforcement')
    return {key: proof[key] for key in keys if key in proof}


def retain_failure(report, phase, error, root, job, proofs):
    report['error'] = h.safe_error(phase, error)
    if isinstance(error, h.CapacityFailure):
        report['capacity_failure'] = error.measurement
        if (root / 'reclaim.json').exists():
            report['reclaim'] = h.read(root / 'reclaim.json')
    if phase in JOBS[job]:
        path = root / ('stage-' + phase + '.json')
        proof = h.read(path) if path.exists() else {'stage': phase, 'status': 'FAIL', 'timeout_seconds': BUDGETS[phase]}
        if 'journals' in proof:
            proofs.append(proof)
        report['stages'][list(JOBS[job]).index(phase)] = public_stage(proof)
    print('FORGE_FAILURE ' + json.dumps(report['error'], sort_keys=True), flush=True)


def run_job(job):
    h.BOOTSTRAP_DEADLINE = time.monotonic() + BOOTSTRAP_SECONDS
    try:
        with h.wall_budget(h.BOOTSTRAP_DEADLINE - time.monotonic()):
            h.require(job in JOBS and not sys.flags.optimize)
            h.require(job_budget(job) < HOST_SECONDS)
            h.m.preflight()
            h.hosted_guard()
            components = h.verify_components()
            workspace = Path(os.environ['GITHUB_WORKSPACE']).resolve()
            checkout = h.checkout_proof(workspace)
            maintenance_proof, _ = h.m.read_payloads(workspace / 'services-base', h.git)
            # A deterministic attempt/job path rejects a second execution, even after observation timeout.
            root = Path(os.environ['RUNNER_TEMP']).resolve() / ('f12-' + os.environ['GITHUB_RUN_ID'] + '-' + os.environ['GITHUB_RUN_ATTEMPT'] + '-' + job)
            h.require(len(str(root / 'children/maintenance.sock').encode()) < 104)
            root.mkdir(mode=0o700, exist_ok=False)
            for name in ('private', 'public', 'output', 'cache', 'stages', 'registry', 'children/pg', 'children/oci'):
                (root / name).mkdir(parents=True)
            h.atomic(root / 'launch.json', {'job': job, 'run_id': os.environ['GITHUB_RUN_ID'],
                'attempt': os.environ['GITHUB_RUN_ATTEMPT'], 'owner_pid': os.getpid()}, exclusive=True)
            report = {'schema': 'forge/hermetic-full12-partition/v1', 'job': job, 'workflow_run': os.environ['GITHUB_RUN_ID'],
                'workflow_attempt': os.environ['GITHUB_RUN_ATTEMPT'], 'checkout': checkout, 'components': components,
                'source_count': 265, 'inherited_gate_results': False, 'full12_pass': False, 'status': 'FAIL',
                'maintenance_sha256': h.m.EXPECTED_FILES['scripts/compose_helpers.py']['sha256'],
                'maintenance_git': maintenance_proof,
                'job_budget_seconds': job_budget(job),
                'stages': [{'stage': name, 'status': 'NOT_RUN', 'timeout_seconds': BUDGETS[name]} for name in JOBS[job]]}
    except BaseException:
        h.BOOTSTRAP_DEADLINE = None
        raise
    admission = q = sdk = parent = catalogues = ownership = config = None
    phase = 'admission'
    proofs = []
    cache_operations = []
    failure = None
    images = {}
    try:
        with h.wall_budget(h.BOOTSTRAP_DEADLINE - time.monotonic()):
            report['reclaim'] = h.reclaim(root)
            report['capacity_before'] = h.capacity(root)
            catalogues = h.export_sources({'forge': workspace / 'source', 'base': workspace / 'services-base'}, root / 'sources')
            h.parity(root / 'sources', catalogues)
            report['source_hashes'] = {label: spec['sha256'] for label, spec in catalogues.items()}
            report['python_methods'] = python_counts(root)
            phase = 'bootstrap'
            daemon = start_daemon(root)
            with h.bootstrap_step('baseline'):
                baseline = inventory()
                h.require(not baseline['container'] and not baseline['volume'])
                baseline_images = sorted(docker('image', 'ls', '-q', '--no-trunc').decode().split())
            with h.bootstrap_step('admission_seal'):
                admission = {'daemon': daemon, 'baseline': baseline, 'baseline_images': baseline_images,
                    'checkout': checkout, 'components': components, 'source_hashes': report['source_hashes'],
                    'maintenance_git': maintenance_proof,
                    'capacity': h.capacity(root), 'new_independent_environment': True}
                h.atomic(root / 'admission-seal.json', admission, exclusive=True)
                report['admission_sha256'] = h.sha(root / 'admission-seal.json')
            report['daemon_id'] = daemon['id']
            report['baseline'] = inventory_proof(baseline)
            report['daemon_versions'] = {key: daemon[key] for key in ('server', 'client', 'compose')}
            q = h.load('exact_product_native_qa', root / 'sources/CI-CD/scripts/native_qa_compose.py')
            q.DAEMON_ID = daemon['id']
            h.bind_maintenance(q, maintenance_proof)
            maintenance = h.maintenance(root, workspace / 'services-base', maintenance_proof)
            os.environ['SDLC_MAINTENANCE_BASE'] = str(maintenance)
            os.environ['SDLC_RESOURCE_REGISTRY'] = str(root / 'registry')
            os.environ['SDLC_MIN_FREE_GIB'] = str(h.HOST_BYTES / 2**30)
            sdk = q.load_sdk(maintenance)
            install_process_adapter(sdk, q)
            install_native_ci_policy(q)
            parent = h.load('reviewed_hosted_parent', HERE / 'parent.py')
            token = uuid.uuid4().hex[:20]
            config = {'native_root': str(root), 'cache_root': str(root / 'cache'), 'forge_git_sha': h.SOURCE,
                'base_git_sha': h.BASE, 'project': 'sdlc-qa-forge-delivery-' + token,
                'pg_project': 'sdlc-qa-forge-pg-' + token, 'oci_project': 'sdlc-qa-forge-oci-' + token}
            report['projects'] = {key: config[key] for key in ('project', 'pg_project', 'oci_project')}
            with h.bootstrap_step('project_ownership'):
                h.atomic(root / 'ownership.json', config, exclusive=True)
            with h.bootstrap_step('disposable_prepare'):
                ownership = parent.create_disposable(q, config)
            images = build_tools(root)
            report['cache'] = fill_cache(root, sdk, q, parent, images, token, cache_operations)
            for name in ('gates.sh', 'followups.sh'):
                shutil.copyfile(HERE / name, root / 'output' / name)
                (root / 'output' / name).chmod(0o555)
            h.atomic(root / 'transport.json', {'version': 1, 'socket': str(root / 'children/maintenance.sock')}, exclusive=True)
            execution = {'admission_sha256': report['admission_sha256'], 'config': config, 'images': images,
                'cache': report['cache'], 'ownership_sha256': ownership, 'capacity': h.capacity(root),
                'stages': list(JOBS[job]), 'budgets': {name: BUDGETS[name] for name in JOBS[job]},
                'components': components, 'source_hashes': report['source_hashes'],
                'maintenance_git': maintenance_proof, 'inherited_gate_results': False}
            h.atomic(root / 'execution-seal.json', execution, exclusive=True)
            report['execution_sha256'] = h.sha(root / 'execution-seal.json')
            report['capacity_execution'] = execution['capacity']
            expected_seals = {'admission-seal.json': report['admission_sha256'], 'execution-seal.json': report['execution_sha256']}
            h.atomic(root / 'runtime-seal-hashes.json', expected_seals, exclusive=True)
            h.BOOTSTRAP_DEADLINE = None
        for stage in JOBS[job]:
            phase = stage
            print('FORGE_STAGE_START ' + job + ' ' + stage, flush=True)
            proof = run_stage(stage, root, config, images, admission, q, sdk, parent, catalogues, expected_seals)
            proofs.append(proof)
            report['stages'][list(JOBS[job]).index(stage)] = public_stage(proof)
            print('FORGE_STAGE_CLOSED ' + job + ' ' + stage + ' PASS', flush=True)
        report['status'] = 'PARTITION_PASS'
    except BaseException as error:
        failure = error
    finally:
        h.BOOTSTRAP_DEADLINE = None
        cleanup = {'complete': False, 'disposable_complete': ownership is None, 'daemon_stopped': False}
        try:
            with h.wall_budget(1500):
                # Failure cleanup is outside the possibly consumed bootstrap alarm.
                for operation in cache_operations:
                    h.check_deadline()
                    if h.read(operation.journal).get('phase') != 'cleaned':
                        close_operation(operation)
                    h.check_deadline()
                if failure is not None:
                    retain_failure(report, phase, failure, root, job, proofs)
                if admission is not None:
                    h.require(h.sha(root / 'admission-seal.json') == report['admission_sha256'])
                    if 'execution_sha256' in report:
                        check_runtime_seals(root, {'admission-seal.json': report['admission_sha256'], 'execution-seal.json': report['execution_sha256']})
                    identity(root, admission)
                    current = inventory()
                    h.require(current == admission['baseline'])
                    actual_images = set(docker('image', 'ls', '-q', '--no-trunc').decode().split())
                    h.require(set(admission['baseline_images']) <= actual_images and set(images.values()) <= actual_images)
                    cleanup.update(exact_inventory_matches_new_baseline=True, baseline_images_preserved=True,
                                   own_images_preserved=True, inventory_after=inventory_proof(current))
                    if config is not None:
                        result = cleanup_disposable(root, config, ownership, q, parent, admission, proofs)
                        cleanup['disposable_complete'] = result['complete']
                        cleanup['disposable'] = result
                elif ownership is not None:
                    raise ValueError('unproven_cleanup_context')
                if catalogues is not None:
                    report['source_parity'] = h.parity(root / 'sources', catalogues)
                h.require(h.verify_components() == components)
                h.checkout_proof(workspace)
                report['checkout_parity'] = True
                report['cache_preserved'] = (root / 'cache').is_dir()
                cleanup['complete'] = cleanup['disposable_complete']
        except BaseException as error:
            cleanup['error'] = h.safe_error('cleanup', error)
            report['status'] = 'FAIL'
        finally:
            try:
                with h.wall_budget(220):
                    cleanup['daemon_stopped'] = stop_daemon(root)['stopped']
                    cleanup['delegation_removed'] = cleanup_delegation(root)
            except BaseException as error:
                cleanup['daemon_error'] = h.safe_error('cleanup', error)
            cleanup['complete'] = cleanup['complete'] and cleanup['daemon_stopped'] and cleanup.get('delegation_removed') is True
            report['cleanup'] = cleanup
            if not cleanup['complete']:
                report['status'] = 'FAIL'
        if q is not None:
            report['maintenance_sha256'] = q.SDK_SHA256
        # No argv, SQL, role/database names, credentials, filesystem journals or raw logs leave the VM.
        try:
            with h.wall_budget(60):
                h.atomic(root / 'public/report.json', report, exclusive=True)
                provenance = {key: report.get(key) for key in ('checkout', 'components', 'source_count', 'source_hashes',
                    'workflow_run', 'workflow_attempt', 'job', 'admission_sha256', 'execution_sha256', 'maintenance_sha256', 'maintenance_git')}
                h.atomic(root / 'public/provenance.json', provenance, exclusive=True)
                sums = {name: h.sha(root / 'public' / name) for name in ('report.json', 'provenance.json')}
                h.atomic(root / 'public/SHA256SUMS.json', sums, exclusive=True)
                if report['status'] == 'PARTITION_PASS':
                    with Path(os.environ['GITHUB_OUTPUT']).open('a') as output:
                        output.write('report_sha256=' + sums['report.json'] + '\n')
        except BaseException as error:
            report['status'] = 'FAIL'
            print('FORGE_RECEIPT_FAILURE ' + json.dumps(h.safe_error('cleanup', error), sort_keys=True), flush=True)
        print('FORGE_TERMINAL ' + json.dumps({'job': job, 'status': report['status'],
            'cleanup_complete': cleanup['complete'], 'full12_pass': False}), flush=True)
    return 0 if report['status'] == 'PARTITION_PASS' else 1


def aggregate(a, b, c, expected_a, expected_b):
    h.require(h.sha(a) == expected_a and h.sha(b) == expected_b)
    reports = [h.read(path) for path in (a, b, c)]
    components = h.verify_components()
    for report, job in zip(reports, JOBS):
        h.require(report['schema'] == 'forge/hermetic-full12-partition/v1' and report['job'] == job)
        h.require(report['status'] == 'PARTITION_PASS' and report['inherited_gate_results'] is False)
        h.require(report['job_budget_seconds'] == job_budget(job) < HOST_SECONDS)
        h.require(report['workflow_run'] == os.environ['GITHUB_RUN_ID'] and report['workflow_attempt'] == os.environ['GITHUB_RUN_ATTEMPT'])
        h.require(report['checkout']['controls'] == os.environ['GITHUB_SHA'] and report['checkout']['source'] == h.SOURCE and report['checkout']['base'] == h.BASE)
        h.require(report['components'] == components and report['source_count'] == 265)
        h.require(report['maintenance_sha256'] == h.m.EXPECTED_FILES['scripts/compose_helpers.py']['sha256'])
        h.m.proof_matches(report['maintenance_git'])
        h.require(report['python_methods'] == {'test_postgres_delivery.py': 47, 'test_oci_offline_fixture.py': 28})
        h.require(report['cache']['new_locked_cache'] is True and report['cache']['private_cache_imported'] is False)
        h.require(report['cache_preserved'] is True)
        for key in ('capacity_before', 'capacity_execution'):
            measurement = report[key]
            h.require(measurement['host_min_bytes'] == h.HOST_BYTES and measurement['data_min_bytes'] == h.DATA_BYTES
                      and measurement['minimum_inodes'] == h.INODES)
            h.require(measurement['host_free_bytes'] >= h.HOST_BYTES and measurement['data_free_bytes'] >= h.DATA_BYTES
                      and measurement['free_inodes'] >= h.INODES)
        h.require(all(re.fullmatch('[a-f0-9]{64}', report[key]) for key in ('admission_sha256', 'execution_sha256')))
        h.require(report['source_parity'] and report['checkout_parity'] and report['cleanup']['complete'])
        h.require(report['cleanup']['exact_inventory_matches_new_baseline'] and report['cleanup']['disposable_complete'] and report['cleanup']['daemon_stopped'])
        h.require(report['cleanup'].get('delegation_removed') is True)
        h.require(report['cleanup']['inventory_after'] == report['baseline'])
        h.require(set(report['baseline']) == {'container', 'network', 'volume'})
        h.require(report['baseline']['container']['count'] == report['baseline']['volume']['count'] == 0)
        h.require(all(type(item['count']) is int and item['count'] >= 0 and re.fullmatch('[a-f0-9]{64}', item['sha256'])
                      for item in report['baseline'].values()))
        h.require(report['cleanup']['baseline_images_preserved'] and report['cleanup']['own_images_preserved'])
        projects = report['projects']
        h.require(set(projects) == {'project', 'pg_project', 'oci_project'})
        token = projects['project'].removeprefix('sdlc-qa-forge-delivery-')
        h.require(re.fullmatch('[a-f0-9]{20}', token) and projects == {'project': 'sdlc-qa-forge-delivery-' + token,
            'pg_project': 'sdlc-qa-forge-pg-' + token, 'oci_project': 'sdlc-qa-forge-oci-' + token})
        def closed_journal(proof):
            h.require(proof['version'] == 2 and proof['phase'] == 'cleaned' and proof['task'] == TASK)
            h.require(proof['daemon_id'] == report['daemon_id'] and proof['sdk_sha256'] == report['maintenance_sha256'])
            h.require(re.fullmatch('[a-f0-9]{32}', proof['cleanup_id']))
            h.require(all(re.fullmatch('[a-f0-9]{64}', proof[key]) for key in ('journal_sha256', 'manifest_sha256')))
        closed_journal(report['cache']['journal'])
        h.require(report['cache']['journal']['project'] == 'sdlc-build-forge-cache-' + token)
        h.require(report['cache']['journal']['purpose'] == 'locked-dependency-fetch-only')
        h.require([proof['stage'] for proof in report['stages']] == list(JOBS[job]))
        for proof in report['stages']:
            h.require(proof['status'] == 'PASS' and proof['cleanup_complete'] and proof['source_parity'])
            h.require(proof.get('resource_enforcement') == {
                'qa': {'cpu_count': 2, 'memory_bytes': 5 * 2**30, 'pids_limit': 512},
                'postgres': {'cpu_count': 1, 'memory_bytes': 2**30, 'pids_limit': None}})
            h.require(proof['timeout_seconds'] == BUDGETS[proof['stage']])
            h.require(set(proof['remaining']) == set(projects.values()))
            for remaining in proof['remaining'].values():
                h.require(set(remaining) == {'container', 'network', 'volume'})
                h.require(all(item['count'] == 0 and item['sha256'] == hashlib.sha256(b'[]').hexdigest() for item in remaining.values()))
            h.require(proof['journal_proofs'] and any(item['project'] == projects['project'] for item in proof['journal_proofs']))
            for journal in proof['journal_proofs']:
                closed_journal(journal)
                h.require(journal['project'] in projects.values())
                purpose = {projects['project']: PURPOSE, projects['pg_project']: 'native-pg-session', projects['oci_project']: 'native-oci-session'}
                h.require(journal['purpose'] == purpose[journal['project']])
            if proof['stage'] in ('postgres', 'oci'):
                key = 'pg_project' if proof['stage'] == 'postgres' else 'oci_project'
                h.require(any(item['project'] == projects[key] for item in proof['journal_proofs']))
            expected = {'python': 75, 'postgres': 3, 'oci': 1, 'workspace': 221, 'integration': 80, 'cli': 2}.get(proof['stage'])
            if expected is not None:
                h.require(proof.get('tests_passed') == expected)
            if proof['stage'] == 'postgres':
                h.require(proof.get('negative_count') == 24)
            if proof['stage'] == 'release':
                h.require(set(proof.get('binaries', {})) == set(BINARIES))
                h.require(all(re.fullmatch('[a-f0-9]{64}', value) for value in proof['binaries'].values()))
            if proof['stage'] == 'openapi':
                h.require(all(re.fullmatch('[a-f0-9]{64}', proof.get(key, '')) for key in ('export_sha256', 'contract_sha256')))
        h.require(report['daemon_versions'] == {'server': '29.8.2', 'client': '29.8.2', 'compose': 'Docker Compose version v5.5.1'})
    h.require(len({report['daemon_id'] for report in reports}) == len(JOBS))
    expected_sources = {
        key: value['sha256'] for key, value in h.read(HERE / 'source-catalogue.json')['catalogues'].items()}
    h.require(all(report['source_hashes'] == expected_sources for report in reports))
    h.require([p['stage'] for report in reports for p in report['stages']] == list(STAGES))
    return {'schema': 'forge/hermetic-full12-acceptance/v1', 'status': 'PASS', 'source': h.SOURCE,
        'controls': os.environ['GITHUB_SHA'], 'workflow_run': os.environ['GITHUB_RUN_ID'],
        'workflow_attempt': os.environ['GITHUB_RUN_ATTEMPT'], 'stage_count': 12,
        'source_count': 265, 'source_hashes': reports[0]['source_hashes'], 'components': components,
        'partitions': {'A': expected_a, 'B': expected_b, 'C': h.sha(c)}, 'independent_daemons': True,
        'cleanup_all_complete': True, 'product_runtime_promoted': False}


def main():
    handlers = {}
    def interrupted(*_):
        raise KeyboardInterrupt()
    try:
        for sig in (signal.SIGTERM, signal.SIGINT):
            handlers[sig] = signal.signal(sig, interrupted)
        if len(sys.argv) == 3 and sys.argv[1] == 'run':
            return run_job(sys.argv[2])
        if len(sys.argv) == 7 and sys.argv[1] == 'aggregate':
            result = aggregate(Path(sys.argv[2]), Path(sys.argv[3]), Path(sys.argv[4]), sys.argv[5], sys.argv[6])
            h.atomic(Path(sys.argv[4]).parent / 'full12.json', result, exclusive=True)
            print('FORGE_FULL12_TERMINAL PASS', flush=True)
            return 0
        raise ValueError('closed_action')
    except BaseException as error:
        print('FORGE_TERMINAL_FAILURE ' + json.dumps(h.safe_error('aggregate' if 'aggregate' in sys.argv else 'admission', error)), flush=True)
        return 1
    finally:
        for sig, previous in handlers.items():
            signal.signal(sig, previous)


if __name__ == '__main__':
    sys.exit(main())
