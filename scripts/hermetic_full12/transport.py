"""Bound exec into an existing real Compose/v2-owned PostgreSQL service only."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import signal
import sys
from collections import Counter


sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
OBSERVER = 'smoke-diagnostic-20261009-v1/smoke_diagnostic.py'
OBSERVER_SHA256 = '1f8312b39bbd8db33972fd881a094fbb408fd8ecafd0f0039640b9fd1ff09405'
PG_IMAGE = 'sha256:3645570cccdfa447589da9f57dd740faa29b30938e861289a5574b6ca6b03826'
ENDPOINT = 'unix:///run/user/1000/docker.sock'
DAEMON_ID = '38138e49-da1a-4f64-ba35-15cc6ef9400e'
DAEMON_ROOT = '/home/sdlc1-runner/.local/share/docker'
DOCKER = ['docker', '--context', 'rootless']
TOOLS = frozenset(('psql', 'pg_dump', 'pg_restore'))


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def load_observer():
    path = HERE.parent / OBSERVER
    if sha(path) != OBSERVER_SHA256:
        raise ValueError('Reviewed observer changed')
    spec = importlib.util.spec_from_file_location('pinned_smoke_observer', path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def verify_review():
    seal = json.loads((HERE / 'seal.json').read_bytes())
    names = {'smoke_transport.py', 'test_smoke_transport.py', 'full_smoke_transport.py',
             'test_full_smoke_transport.py', 'REVIEW.md', 'timing-proof.json'}
    if (set(seal['files_sha256']) != names or seal['native_executed'] is not False
            or seal['materialized'] is not False or seal['observer_sha256'] != OBSERVER_SHA256
            or seal['deadlines_seconds'] != [300, 30, 10]):
        raise ValueError('Transport review scope changed')
    for name, expected in seal['files_sha256'].items():
        if sha(HERE / name) != expected:
            raise ValueError('Transport review bytes changed')


class BoundExec:
    def __init__(self, base, observer, project, manifest):
        self.base, self.observer = base, observer
        self.project, self.manifest = project, Path(manifest)
        self.journal = self.manifest.parent / 'journal.json'
        self.prefix = DOCKER + ['compose', '-p', project, '-f', str(manifest), 'exec', '-T', 'postgres']
        self.popen = observer.popen
        self.container_id = None
        self.counts = Counter()

    def reject(self):
        raise self.base.ClosedFailure('input_rejected')

    def control(self, argv):
        result = self.observer.run(argv, capture_output=True, text=True)
        if result.returncode:
            self.reject()
        return result.stdout.strip()

    def validate_container(self, items, journal, manifest):
        if not isinstance(items, list) or len(items) != 1:
            self.reject()
        item = items[0]
        labels = item.get('Config', {}).get('Labels') or {}
        expected = {
            'com.docker.compose.project': self.project,
            'com.docker.compose.service': 'postgres',
            'com.docker.compose.project.config_files': str(self.manifest),
            'com.docker.compose.container-number': '1',
            'sdlc.task': 'forge-task-delivery', 'sdlc.purpose': 'native-pg17-oci-gates',
            'sdlc.cleanup-id': journal.get('cleanup_id'), 'sdlc.lifecycle': 'disposable',
        }
        cleanup = journal.get('cleanup_id')
        service = manifest.get('services', {}).get('postgres', {})
        state = item.get('State', {})
        if (not isinstance(cleanup, str) or not re.fullmatch('[a-f0-9]{32}', cleanup)
                or any(labels.get(k) != v for k, v in expected.items())
                or labels.get('com.docker.compose.oneoff', '').lower() != 'false'
                or item.get('Id') != self.container_id or item.get('Image') != PG_IMAGE
                or service.get('image') != PG_IMAGE
                or state.get('Running') is not True or state.get('Paused') is not False
                or state.get('Restarting') is not False or state.get('Health', {}).get('Status') != 'healthy'
                or item.get('HostConfig', {}).get('RestartPolicy', {}).get('Name') != 'no'
                or 'user' in service or 'working_dir' in service):
            self.reject()

    def bind(self):
        # All discovery is bounded and charged to the SAME observer aggregate budget.
        self.observer.phase_id = 0
        journal_bytes, manifest_bytes = self.journal.read_bytes(), self.manifest.read_bytes()
        journal, manifest = json.loads(journal_bytes), json.loads(manifest_bytes)
        self.manifest_sha256 = hashlib.sha256(manifest_bytes).hexdigest()
        self.journal_sha256 = hashlib.sha256(journal_bytes).hexdigest()
        expected = {'version': 2, 'project': self.project, 'task': 'forge-task-delivery',
                    'purpose': 'native-pg17-oci-gates', 'daemon_id': DAEMON_ID,
                    'sdk_sha256': self.base.SDK_SHA256, 'manifest': str(self.manifest),
                    'manifest_sha256': self.manifest_sha256, 'docker': DOCKER}
        if (any(journal.get(k) != v for k, v in expected.items())
                or journal.get('phase') not in ('manifest-validated', 'execution-started')
                or type(journal.get('owner_pid')) is not int or journal['owner_pid'] <= 1):
            self.reject()
        self.owner_pid = journal['owner_pid']
        os.kill(self.owner_pid, 0)
        if os.environ.get('SDLC_DOCKER_EXECUTABLE') or os.environ.get('DOCKER_HOST') or os.environ.get('DOCKER_CONTEXT'):
            self.reject()
        endpoint = self.control(DOCKER + ['context', 'inspect', 'rootless', '--format', '{{.Endpoints.docker.Host}}'])
        info = json.loads(self.control(DOCKER + ['info', '--format', '{{json .}}']))
        if (endpoint != ENDPOINT or info.get('ID') != DAEMON_ID
                or info.get('DockerRootDir') != DAEMON_ROOT or info.get('ServerVersion') != '29.8.2'
                or 'name=rootless' not in info.get('SecurityOptions', [])):
            self.reject()
        if self.control(DOCKER + ['version', '--format', '{{.Client.Version}}']) != '29.8.2':
            self.reject()
        if self.control(DOCKER + ['compose', 'version']) != 'Docker Compose version v5.5.1':
            self.reject()
        ids = self.control(DOCKER + ['compose', '-p', self.project, '-f', str(self.manifest),
                                     'ps', '--all', '--quiet', 'postgres']).split()
        if len(ids) != 1 or not re.fullmatch('[a-f0-9]{64}', ids[0]):
            self.reject()
        self.container_id = ids[0]
        items = json.loads(self.control(DOCKER + ['container', 'inspect', self.container_id]))
        self.validate_container(items, journal, manifest)
        self.check_live_binding()
        self.observer.event('transport_bound', outcome='exact_compose_owned_id')

    def check_live_binding(self):
        if (self.container_id is None or sha(self.manifest) != self.manifest_sha256
                or sha(self.journal) != self.journal_sha256):
            self.reject()
        os.kill(self.owner_pid, 0)

    def translate(self, args):
        if (not isinstance(self.container_id, str) or not re.fullmatch('[a-f0-9]{64}', self.container_id)
                or not isinstance(args, (list, tuple)) or any(not isinstance(a, str) for a in args)
                or list(args[:len(self.prefix)]) != self.prefix):
            self.reject()
        tail = list(args[len(self.prefix):])
        if not tail or tail[0] not in TOOLS:
            self.reject()
        # -T disables TTY, NOT stdin. Compose exec's default interactive=true is -i.
        return DOCKER + ['exec', '-i', self.container_id, *tail]

    def spawn(self, args, **kwargs):
        self.check_live_binding()
        translated = self.translate(args)
        process = self.popen(translated, **kwargs)
        self.counts[args[len(self.prefix)]] += 1
        return process


def main():
    base = load_observer()
    observer = base.Observer(sys.stdout)
    handlers = {}
    transport = None
    try:
        if sys.flags.optimize or len(sys.argv) != 4:
            raise base.ClosedFailure('input_rejected')
        verify_review()
        project, manifest, executor = sys.argv[1:]
        raw = base.validate_inputs(project, manifest, executor, HERE.parent / 'sql_smoke.py')
        for sig in (signal.SIGTERM, signal.SIGINT):
            handlers[sig] = signal.signal(sig, observer.interrupt)
        transport = BoundExec(base, observer, project, manifest)
        transport.bind()
        observer.popen = transport.spawn
        return base.execute(raw, observer, [project, manifest, executor])
    except BaseException as error:
        reason = error.reason if isinstance(error, base.ClosedFailure) else 'input_rejected'
        try:
            observer.cleanup()
        except BaseException:
            reason = 'cleanup_unknown'
        observer.event('terminal', outcome=reason)
        return 1
    finally:
        if transport is not None:
            observer.emit({'event': 'transport_counts', 'counts': dict(transport.counts)})
        for sig, handler in handlers.items():
            signal.signal(sig, handler)


if __name__ == '__main__':
    sys.exit(main())
