"""QA-only Compose session adapter over the pinned maintenance SDK.

Native delivery deliberately replaces applications within an owned project.
Each transition keeps an immutable generation, while the v2 cleanup journal
pins the current invocation manifest. No production receipt is synthesized.
"""
from __future__ import annotations

import copy
from contextvars import ContextVar
from datetime import datetime, timedelta, timezone
import hashlib
import importlib
import json
import os
from pathlib import Path
import re
import select
import signal
import socket
import socketserver
import subprocess
import sys
import threading
import time
import uuid

SDK_SHA256 = '2569882c3bb6a8b86ebb42367254720c5f802d5423bc98ef02c6fe3283e0874f'
DAEMON_ID = '38138e49-da1a-4f64-ba35-15cc6ef9400e'
TASK = 'forge-task-delivery'
PURPOSES = {
    'pg': {'database': 'disposable-postgres-target', 'application': 'disposable-postgres-application'},
    'oci': {'application': 'disposable-oci-application'},
}
MAX_DOCUMENT = 1024 * 1024
REQUEST = ContextVar('native_qa_request', default=None)


class RequestLease:
    """A killed native CLI cannot leave a detached host Compose command running."""

    def __init__(self, connection):
        self.connection = connection
        self.cancelled = threading.Event()
        self.finished = threading.Event()
        self.thread = threading.Thread(target=self.watch, daemon=True)

    def watch(self):
        while not self.finished.is_set():
            ready, _, _ = select.select([self.connection], [], [], 0.05)
            if ready:
                # The request has already been read; any extra bytes are forbidden too.
                self.cancelled.set()
                return

    def check(self):
        if self.cancelled.is_set():
            raise RuntimeError('Native Compose client disconnected; outcome not accepted')

    def checked(self, command, *, timeout=90, **kwargs):
        self.check()
        if command[0] == 'docker' and os.environ.get('SDLC_DOCKER_EXECUTABLE'):
            command = [os.environ['SDLC_DOCKER_EXECUTABLE'], *command[1:]]
        started = time.monotonic()
        process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   start_new_session=os.name == 'posix', **kwargs)
        complete = False
        try:
            while True:
                self.check()
                remaining = timeout - (time.monotonic() - started)
                if remaining <= 0:
                    raise subprocess.TimeoutExpired(command, timeout)
                try:
                    stdout, _ = process.communicate(timeout=min(0.05, remaining))
                    break
                except subprocess.TimeoutExpired:
                    continue
            self.check()
            if process.returncode:
                raise RuntimeError('Compose maintenance command failed; inspect the private journal')
            complete = True
            return stdout
        finally:
            if not complete and os.name == 'posix':
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            elif process.poll() is None:
                process.kill()
            process.wait()
            for stream in (process.stdout, process.stderr):
                if stream is not None:
                    stream.close()


def bind_request_lifetime(sdk):
    if getattr(sdk.checked, '_forge_request_lifetime', False):
        return
    original = sdk.checked

    def checked(command, **kwargs):
        request = REQUEST.get()
        return original(command, **kwargs) if request is None else request.checked(command, **kwargs)

    checked._forge_request_lifetime = True
    sdk.checked = checked


def digest(data):
    return hashlib.sha256(data).hexdigest()


def plain(path):
    path = Path(path).absolute()
    for part in (path, *path.parents):
        if part.is_symlink() or (part.exists() and getattr(part.lstat(), 'st_file_attributes', 0) & 1024):
            raise ValueError('Linked QA path')
    return path


def read(path):
    path = plain(path)
    if path.stat().st_size > MAX_DOCUMENT:
        raise ValueError('Oversized QA metadata')
    return json.loads(path.read_text(encoding='utf-8'))


def atomic(path, value, *, exclusive=False):
    path = plain(path)
    data = (json.dumps(value, sort_keys=True, indent=2, allow_nan=False) + '\n').encode()
    temporary = path.with_name('.' + path.name + '.' + uuid.uuid4().hex + '.pending')
    created = False
    try:
        descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        created = True
        with os.fdopen(descriptor, 'wb') as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        if exclusive:
            os.link(temporary, path)
            temporary.unlink()
        else:
            os.replace(temporary, path)
        if os.name != 'nt':
            descriptor = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
            try:
                os.fsync(descriptor)
            finally:
                os.close(descriptor)
    finally:
        if created:
            temporary.unlink(missing_ok=True)


def load_sdk(directory):
    root = plain(directory).resolve()
    source = root / 'scripts/compose_helpers.py'
    if digest(plain(source).read_bytes()) != SDK_SHA256:
        raise ValueError('Maintenance SDK hash mismatch')
    for name in ('scripts', 'scripts.compose_helpers'):
        module = sys.modules.get(name)
        if module is not None:
            origins = list(getattr(module, '__path__', ()))
            if getattr(module, '__file__', None):
                origins.append(module.__file__)
            if not origins or any(not Path(p).resolve().is_relative_to(root) for p in origins):
                raise ValueError('Foreign scripts package already loaded')
    sys.path.insert(0, str(root))
    module = importlib.import_module('scripts.compose_helpers')
    if Path(module.__file__).resolve() != source or digest(source.read_bytes()) != SDK_SHA256:
        raise ValueError('Maintenance SDK import mismatch')
    bind_request_lifetime(module)
    return module


def session_class(sdk):
    class NativeSession(sdk.ComposeHelper):
        """An SDK-owned native session, not adoption of arbitrary QA resources."""

        def __init__(self, *, kind, source_root, **kwargs):
            if kind not in PURPOSES:
                raise ValueError('Unknown native QA kind')
            self.kind = kind
            self.source_root = plain(source_root).resolve()
            self.services = {}
            self.generations = []
            self.source_records = []
            super().__init__(purpose='native-' + kind + '-session', **kwargs)

        def record(self, phase):
            data = dict(version=2, project=self.project, task=self.task, purpose=self.purpose,
                        daemon_id=self.identity, manifest=str(self.path), manifest_sha256=self.manifest_hash,
                        docker=self.docker, cleanup_id=self.cleanup_id, created_at=self.created_at,
                        updated_at=sdk.utc_now(), owner_pid=self.owner_pid, owner_host=self.owner_host,
                        phase=phase, session_kind=self.kind, source_root=str(self.source_root),
                        generations=self.generations, native_sources=self.source_records,
                        sdk_sha256=SDK_SHA256,
                        adapter_sha256=digest(Path(__file__).read_bytes()))
            atomic(self.journal, data)
            self.registry.mkdir(parents=True, exist_ok=True)
            sdk.restrict(self.registry)
            pointer = self.registry / (digest(str(self.journal).encode()) + '.json')
            if phase in ('cleaned', 'disposable-cleaned'):
                pointer.unlink(missing_ok=True)
            else:
                atomic(pointer, {'journal': str(self.journal)})

        def normalize(self, spec):
            spec = copy.deepcopy(spec)
            if set(spec) - {'name', 'services', 'networks', 'volumes'}:
                raise ValueError('Unknown native Compose section')
            if spec.get('name', self.project) != self.project:
                raise ValueError('Native project mismatch')
            services = spec.get('services', {})
            if not services or set(services) - set(PURPOSES[self.kind]):
                raise ValueError('Unknown native service')
            volumes = spec.setdefault('volumes', {})
            # Preserve the existing physical PG volume, using the SDK's scoped logical name.
            if 'data' in volumes:
                if self.kind != 'pg' or set(volumes) != {'data'}:
                    raise ValueError('Unexpected native volume alias')
                definition = volumes.pop('data')
                if definition.get('name') != self.project + '_postgres-data':
                    raise ValueError('Native PG volume mismatch')
                volumes['postgres-data'] = definition
                for service in services.values():
                    for mount in service.get('volumes', []):
                        if mount.get('type') == 'volume' and mount.get('source') == 'data':
                            mount['source'] = 'postgres-data'
            if set(volumes) - ({'postgres-data'} if self.kind == 'pg' else set()):
                raise ValueError('Unknown native disposable volume')
            for name, service in services.items():
                labels = service.get('labels', {})
                if labels.get('sdlc.task') != TASK or labels.get('sdlc.purpose') != PURPOSES[self.kind][name]:
                    raise ValueError('Native service purpose mismatch')
                if name == 'application' and (service.get('user') != '65532:65532'
                        or service.get('read_only') is not True or service.get('cap_drop') != ['ALL']
                        or service.get('security_opt') != ['no-new-privileges:true']
                        or service.get('cap_add') or service.get('devices')
                        or any(mount.get('type') != 'bind' or mount.get('read_only') is not True
                               for mount in service.get('volumes', []))):
                    raise ValueError('Native application sandbox changed')
            for definition in volumes.values():
                labels = definition.get('labels', {})
                if labels.get('sdlc.task') != TASK or labels.get('sdlc.purpose') != 'disposable-postgres-data':
                    raise ValueError('Native volume purpose mismatch')
            return spec

        def purpose_labels(self, config):
            labels = {'sdlc.task': TASK, 'sdlc.cleanup-id': self.cleanup_id, 'sdlc.lifecycle': 'disposable'}
            for name, service in config['services'].items():
                service.setdefault('labels', {}).update(labels, **{'sdlc.purpose': PURPOSES[self.kind][name]})
            for definition in config.get('volumes', {}).values():
                if not definition.get('external'):
                    definition.setdefault('labels', {}).update(labels, **{'sdlc.purpose': 'disposable-postgres-data'})
            return config

        def source_proof(self, path):
            path = plain(path).resolve()
            if not path.is_relative_to(self.source_root):
                raise ValueError('Unowned native source manifest')
            if path == self.path:
                raise ValueError('Invocation manifest is not a new native generation')
            source = read(path)
            value = digest(path.read_bytes())
            if any(item['path'] == str(path) or item['sha256'] == value for item in self.source_records):
                raise ValueError('Native Compose up replay refused')
            if len(self.source_records) >= 256:
                raise ValueError('Native generation budget exhausted')
            return source, {'path': str(path), 'sha256': value}

        def publish(self, config, source):
            config = self.purpose_labels(config)
            generation = self.directory / ('generation-' + str(len(self.generations)) + '.json')
            atomic(generation, config, exclusive=True)
            generation_hash = digest(generation.read_bytes())
            atomic(self.path, config)
            self.manifest_hash = digest(self.path.read_bytes())
            self.services = config['services']
            self.definitions = {kind: config.get(kind + 's', {}) for kind in ('volume', 'network')}
            self.generations.append({'path': str(generation), 'sha256': generation_hash})
            self.source_records.append(source)
            sdk.checked(self.command + ['config', '--quiet'])
            self.record('manifest-validated')

        def start_manifest(self, path):
            source, proof = self.source_proof(path)
            spec = self.normalize(source)
            # The installed SDK performs its original no-adoption, image, mount and network checks.
            super().write(spec['services'], volumes=spec['volumes'], networks=spec.get('networks', {}))
            self.publish(read(self.path), proof)

        def validate_transition(self, spec):
            if set(self.services) - set(spec['services']):
                raise ValueError('Native transition cannot discard owned service definitions')
            if self.kind == 'pg':
                prior = self.normalize(read(self.source_records[-1]['path']))
                if spec['services'].get('database') != prior['services'].get('database'):
                    raise ValueError('Native transition cannot alter the existing PostgreSQL service')
            for key, kind in (('networks', 'network'), ('volumes', 'volume')):
                requested = spec.get(key, {})
                current = self.definitions[kind]
                if set(requested) != set(current):
                    raise ValueError('Native transition cannot change resource inventory')
                for name, value in requested.items():
                    old = current[name]
                    if bool(value.get('external')) != bool(old.get('external')):
                        raise ValueError('Native resource lifecycle changed')
                    if value.get('name', self.project + '_' + name) != old.get('name'):
                        raise ValueError('Native resource identity changed')
                    if set(value) - {'external', 'name', 'labels'}:
                        raise ValueError('Native resource options changed')
            config = {'name': self.project, 'services': spec['services'],
                      'networks': copy.deepcopy(self.definitions['network']),
                      'volumes': copy.deepcopy(self.definitions['volume'])}
            for service in config['services'].values():
                if any(key in service for key in ('build', 'container_name', 'privileged', 'volumes_from')):
                    raise ValueError('Unsafe native maintenance service')
                networks = service.get('networks', [])
                if not networks or any(name not in config['networks'] for name in networks) or 'network_mode' in service:
                    raise ValueError('Native service requires explicit owned/external networks')
                image = sdk.checked(self.docker + ['image', 'inspect', service['image'], '--format', '{{.Id}}'], text=True).strip()
                if not re.fullmatch('sha256:[a-f0-9]{64}', image):
                    raise ValueError('Native image is not immutable')
                service.update(image=image, pull_policy='never', restart='no')
                service.setdefault('logging', {'driver': 'local', 'options': {'max-size': '20m', 'max-file': '3'}})
                for mount in service.get('volumes', []):
                    if not isinstance(mount, dict):
                        raise ValueError('Native mounts require long syntax')
                    if mount.get('type') == 'bind':
                        path = plain(mount['source'])
                        if not path.exists():
                            raise ValueError('Native bind source missing')
                        mount.setdefault('bind', {})['create_host_path'] = False
                    elif mount.get('type') == 'volume' and mount.get('source') in config['volumes']:
                        mount.setdefault('volume', {})['nocopy'] = True
                    else:
                        raise ValueError('Unknown native mount')
            return config

        def transition(self, path):
            self.check_endpoint()
            self.check_ownership()
            source, proof = self.source_proof(path)
            config = self.validate_transition(self.normalize(source))
            self.publish(sdk.literals(config), proof)
            self.check_ownership()

        def check_ownership(self):
            if not self.path.is_file() or digest(plain(self.path).read_bytes()) != self.manifest_hash:
                raise RuntimeError('Native invocation manifest changed')
            for item in self.generations:
                if digest(plain(item['path']).read_bytes()) != item['sha256']:
                    raise RuntimeError('Native immutable generation changed')
            for item in self.source_records:
                if digest(plain(item['path']).read_bytes()) != item['sha256']:
                    raise RuntimeError('Native source manifest changed')
            config = read(self.path)
            for kind in ('container', 'network', 'volume'):
                ids = self.resources(kind)
                items = json.loads(sdk.checked(self.docker + [kind, 'inspect', *ids], text=True)) if ids else []
                for item in items:
                    labels = (item.get('Config', {}).get('Labels') if kind == 'container' else item.get('Labels')) or {}
                    logical = labels.get('com.docker.compose.service' if kind == 'container' else 'com.docker.compose.' + kind)
                    definition = (config['services'] if kind == 'container' else self.definitions[kind]).get(logical)
                    if not definition or (kind != 'container' and definition.get('external')):
                        raise RuntimeError('Unknown native resource')
                    expected = definition['labels']
                    if any(labels.get(key) != expected.get(key) for key in
                           ('sdlc.task', 'sdlc.purpose', 'sdlc.cleanup-id', 'sdlc.lifecycle')):
                        raise RuntimeError('Foreign native resource labels')
                    if labels.get('com.docker.compose.project') != self.project:
                        raise RuntimeError('Foreign native project')
                    if kind == 'container':
                        if labels.get('com.docker.compose.project.config_files') != str(self.path):
                            raise RuntimeError('Foreign native invocation manifest')
                    elif item['Name'] != definition['name']:
                        raise RuntimeError('Foreign native resource name')

    return NativeSession


def command_parts(argv):
    if argv == ['--version']:
        return None, None, argv
    if len(argv) < 5 or argv[0] != '-p' or argv[2] != '-f':
        raise ValueError('Only exact native Compose invocations are accepted')
    project, path, tail = argv[1], argv[3], argv[4:]
    if not tail or tail[0] not in ('up', 'stop', 'down', 'logs'):
        raise ValueError('Unsupported native Compose action')
    allowed = {
        ('up', '--detach', '--pull', 'never', '--wait', 'database'),
        ('up', '--detach', '--no-build', '--pull', 'never', '--force-recreate', 'application'),
        ('down', '--remove-orphans'),
        ('stop', '--timeout', '5', 'application'),
        ('logs', '--tail', '50', 'database'),
    }
    if tuple(tail) not in allowed:
        raise ValueError('Native Compose arguments differ from reviewed commands')
    return project, path, tail


class NativeComposeBridge:
    def __init__(self, *, sdk, children, pg_project, oci_project, registry):
        self.sdk = sdk
        self.children = plain(children).resolve()
        self.projects = {pg_project: 'pg', oci_project: 'oci'}
        self.registry = plain(registry).resolve()
        if os.environ.get('SDLC_RESOURCE_REGISTRY') != str(self.registry):
            raise ValueError('Explicit private v2 registry required')
        self.operations = {}
        self.closed = []
        self.lock = threading.Lock()

    def execute(self, argv):
        project, path, tail = command_parts(argv)
        with self.lock:
            if project is None:
                return self.sdk.checked(['docker', '--context', 'rootless', 'compose', 'version'])
            if project not in self.projects:
                raise ValueError('Unowned native project')
            kind = self.projects[project]
            source = plain(path).resolve()
            source_root = self.children / kind
            if not source.is_relative_to(source_root):
                raise ValueError('Unowned native manifest path')
            operation = self.operations.get(project)
            if tail[0] == 'up':
                if operation is None:
                    directory = (source.parent if kind == 'pg' else self.children) / ('sdk-' + kind + '-' + uuid.uuid4().hex)
                    operation = session_class(self.sdk)(kind=kind, source_root=source_root,
                        project=project, task=TASK, docker=['docker', '--context', 'rootless'],
                        directory=directory, daemon_id=DAEMON_ID)
                    self.operations[project] = operation
                    operation.start_manifest(source)
                else:
                    operation.transition(source)
                operation.check_endpoint()
                operation.check_ownership()
                operation.record('execution-started')
                return self.sdk.checked(operation.command + tail)
            if operation is None:
                if any(self.sdk.checked(['docker', '--context', 'rootless', kind_name, 'ls',
                    '-aq' if kind_name == 'container' else '-q', '--filter',
                    'label=com.docker.compose.project=' + project], text=True).strip()
                       for kind_name in ('container', 'network', 'volume')):
                    raise RuntimeError('Unknown native resources without a v2 owner')
                return b''
            operation.check_endpoint()
            operation.check_ownership()
            if source != operation.path and str(source) not in {item['path'] for item in operation.source_records}:
                raise ValueError('Unknown native invocation manifest')
            if tail[0] == 'down':
                operation.close()
                self.closed.append(str(operation.journal))
                del self.operations[project]
                return b''
            if tail[0] == 'logs':
                # Do not export database logs or SQL through the bridge diagnostic channel.
                return b'NATIVE_QA_LOG_DIAGNOSTIC_CLOSED\n'
            return self.sdk.checked(operation.command + tail)

    def close(self):
        errors = []
        with self.lock:
            for project, operation in list(self.operations.items()):
                try:
                    operation.close()
                    self.closed.append(str(operation.journal))
                    del self.operations[project]
                except BaseException as error:
                    errors.append(type(error).__name__)
        if errors:
            raise RuntimeError('Native v2 cleanup incomplete: ' + ','.join(errors))


class BridgeHandler(socketserver.StreamRequestHandler):
    rbufsize = 0

    def handle(self):
        lease, token = None, None
        try:
            self.request.settimeout(90)
            line = self.rfile.readline(MAX_DOCUMENT + 1)
            if len(line) > MAX_DOCUMENT or not line.endswith(b'\n'):
                raise ValueError('Invalid native QA request framing')
            request = json.loads(line)
            if set(request) != {'argv'} or not isinstance(request['argv'], list) or not all(isinstance(v, str) for v in request['argv']):
                raise ValueError('Invalid native QA request')
            lease = RequestLease(self.request)
            lease.thread.start()
            token = REQUEST.set(lease)
            output = self.server.bridge.execute(request['argv'])
            if len(output) > MAX_DOCUMENT:
                raise ValueError('Oversized native QA response')
            response = {'exit': 0, 'stdout': output.decode('utf-8'), 'error_class': None}
        except BaseException as error:
            response = {'exit': 1, 'stdout': '', 'error_class': type(error).__name__}
        finally:
            if token is not None:
                REQUEST.reset(token)
            if lease is not None:
                lease.finished.set()
                lease.thread.join()
        try:
            self.wfile.write((json.dumps(response) + '\n').encode())
        except (BrokenPipeError, ConnectionResetError):
            pass


def serve(bridge, path):
    path = plain(path)
    if path.exists():
        raise ValueError('Never reuse a native QA bridge socket')
    server = socketserver.UnixStreamServer(str(path), BridgeHandler)
    server.bridge = bridge
    path.chmod(0o600)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server, thread


def owner_dead(pid):
    if type(pid) is not int or pid <= 0 or os.name != 'posix':
        raise ValueError('A known Linux owner PID is required')
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return True
    except PermissionError:
        return False
    return False


def recover_session(sdk, journal, owned_root, *, apply=False, now=None):
    """Reconcile only this adapter's dead-owner, aged v2 journals; never adopt."""
    owned_root, journal = plain(owned_root).resolve(), plain(journal).resolve()
    if not journal.is_relative_to(owned_root / 'children') or journal.name != 'journal.json':
        raise ValueError('Unowned native recovery journal')
    data = read(journal)
    if (data.get('version') != 2 or data.get('sdk_sha256') != SDK_SHA256
            or data.get('adapter_sha256') != digest(Path(__file__).read_bytes())
            or data.get('daemon_id') != DAEMON_ID or data.get('docker') != ['docker', '--context', 'rootless']
            or data.get('task') != TASK or data.get('session_kind') not in PURPOSES
            or data.get('purpose') != 'native-' + data['session_kind'] + '-session'
            or not re.fullmatch('sdlc-qa-forge-' + data['session_kind'] + '-[0-9a-f]{12,32}', data.get('project', ''))
            or not re.fullmatch('[0-9a-f]{32}', data.get('cleanup_id', ''))):
        raise ValueError('Native recovery identity mismatch')
    if data.get('owner_host') != socket.gethostname():
        raise ValueError('Native recovery owner is on another host')
    pid = data.get('owner_pid')
    if not owner_dead(pid):
        raise ValueError('Native recovery owner is alive or inaccessible')
    updated = datetime.fromisoformat(data['updated_at'])
    if updated.tzinfo is None or (now or datetime.now(timezone.utc)) - updated < timedelta(hours=24):
        raise ValueError('Native recovery journal is not old enough')
    path = journal.parent / 'compose.json'
    source_root = owned_root / 'children' / data['session_kind']
    if data.get('manifest') != str(path) or data.get('source_root') != str(source_root):
        raise ValueError('Native recovery path mismatch')
    if digest(plain(path).read_bytes()) != data['manifest_sha256']:
        raise ValueError('Native recovery manifest changed')
    generations, sources = data['generations'], data['native_sources']
    if not 1 <= len(generations) == len(sources) <= 256:
        raise ValueError('Native recovery generation chain mismatch')
    for index, generation in enumerate(generations):
        if Path(generation['path']) != journal.parent / ('generation-' + str(index) + '.json'):
            raise ValueError('Native recovery generation path mismatch')
    for source in sources:
        if not plain(source['path']).resolve().is_relative_to(source_root):
            raise ValueError('Native recovery source path mismatch')
    config = read(path)
    cls = session_class(sdk)
    operation = cls.__new__(cls)
    operation.project, operation.task, operation.purpose = data['project'], TASK, data['purpose']
    operation.kind, operation.source_root = data['session_kind'], source_root
    operation.directory, operation.path, operation.journal = journal.parent, path, journal
    operation.identity, operation.docker = DAEMON_ID, data['docker']
    operation.command = operation.docker + ['compose', '-p', operation.project, '-f', str(path)]
    operation.manifest_hash, operation.cleanup_id = data['manifest_sha256'], data['cleanup_id']
    operation.created_at, operation.owner_pid, operation.owner_host = data['created_at'], pid, data['owner_host']
    operation.registry = owned_root / 'registry'
    operation.generations, operation.source_records = generations, sources
    operation.services = config['services']
    operation.definitions = {kind: config.get(kind + 's', {}) for kind in ('volume', 'network')}
    operation.check_endpoint()
    operation.check_ownership()
    ids = operation.resources('container')
    items = json.loads(sdk.checked(operation.docker + ['container', 'inspect', *ids], text=True)) if ids else []
    if any(item['State']['Running'] for item in items):
        raise ValueError('Native recovery containers still running')
    if apply:
        if owner_dead(pid):
            operation.close()
        else:
            raise ValueError('Native recovery owner became live')
    return {'status': 'cleaned' if apply else 'eligible', 'project': operation.project, 'journal_version': 2}
