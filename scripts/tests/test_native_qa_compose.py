"""Pure integration with the actual pinned SDK; Docker is a stateful test double."""
import copy
from datetime import datetime, timedelta, timezone
import errno
import importlib.util
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import threading
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('forge_native_qa', Path(__file__).resolve().parents[1] / 'native_qa_compose.py')
q = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(q)
CLIENT_SPEC = importlib.util.spec_from_file_location('forge_native_qa_client', Path(__file__).resolve().parents[1] / 'native_qa_compose_client.py')
client = importlib.util.module_from_spec(CLIENT_SPEC)
CLIENT_SPEC.loader.exec_module(client)
IMAGE = 'sha256:' + 'a' * 64
IMAGE2 = 'sha256:' + 'b' * 64
PG = 'sdlc-qa-forge-pg-' + '1' * 12
OCI = 'sdlc-qa-forge-oci-' + '1' * 12


class DockerModel:
    def __init__(self):
        self.items = {kind: {} for kind in ('container', 'network', 'volume')}
        self.commands = []
        self.items['network']['parent_qa'] = {'Name': 'parent_qa', 'Labels': {'com.docker.compose.project': 'parent'}}

    def checked(self, command, *, text=False, **_):
        self.commands.append(command)
        assert command[:3] == ['docker', '--context', 'rootless']
        args = command[3:]
        value = ''
        if args[0] == 'info':
            value = q.DAEMON_ID
        elif args[:2] == ['image', 'inspect']:
            value = args[2]
        elif args[0] in self.items:
            kind, action = args[:2]
            if action == 'ls':
                items = self.items[kind]
                if '--filter' in args:
                    expression = args[args.index('--filter') + 1]
                    if expression.startswith('label=com.docker.compose.project='):
                        project = expression.split('=', 2)[2]
                        items = {key: item for key, item in items.items() if self.labels(kind, item).get('com.docker.compose.project') == project}
                    elif expression.startswith('volume='):
                        volume = expression.split('=', 1)[1]
                        items = {key: item for key, item in items.items() if volume in item.get('Volumes', [])}
                    else:
                        raise AssertionError(expression)
                value = '\n'.join(items)
            elif action == 'inspect':
                value = json.dumps([self.items[kind][name] for name in args[2:]])
            elif action == 'rm' and kind == 'volume':
                del self.items[kind][args[2]]
            else:
                raise AssertionError(args)
        elif args[0] == 'compose':
            if args[1:] == ['version']:
                value = 'Docker Compose version test-model\n'
            else:
                project = args[args.index('-p') + 1]
                path = args[args.index('-f') + 1]
                tail = args[args.index('-f') + 2:]
                config = q.read(path)
                if tail[:2] == ['config', '--quiet']:
                    pass
                elif tail[0] == 'up':
                    services = [tail[-1]] if tail[-1] in config['services'] else list(config['services'])
                    for logical, definition in config.get('volumes', {}).items():
                        if not definition.get('external'):
                            labels = dict(definition['labels'], **{'com.docker.compose.project': project, 'com.docker.compose.volume': logical})
                            self.items['volume'].setdefault(definition['name'], {'Name': definition['name'], 'Labels': labels})
                    for name in services:
                        service = config['services'][name]
                        labels = dict(service['labels'], **{'com.docker.compose.project': project,
                            'com.docker.compose.service': name, 'com.docker.compose.project.config_files': path})
                        volumes = [config['volumes'][mount['source']]['name'] for mount in service.get('volumes', []) if mount['type'] == 'volume']
                        self.items['container'][project + '-' + name + '-1'] = {
                            'Config': {'Labels': labels}, 'State': {'Running': True}, 'Volumes': volumes}
                elif tail[0] == 'stop':
                    self.items['container'][project + '-' + tail[-1] + '-1']['State']['Running'] = False
                elif tail[0] == 'down':
                    for kind in ('container', 'network'):
                        self.items[kind] = {key: item for key, item in self.items[kind].items()
                            if self.labels(kind, item).get('com.docker.compose.project') != project}
                else:
                    raise AssertionError(tail)
        else:
            raise AssertionError(args)
        return value if text else value.encode()

    @staticmethod
    def labels(kind, item):
        return item['Config']['Labels'] if kind == 'container' else item['Labels']


class NativeSdkTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        path = os.environ.get('SDLC_MAINTENANCE_BASE')
        if not path:
            raise RuntimeError('Pure SDK integration requires explicit SDLC_MAINTENANCE_BASE; no old Base fallback')
        cls.sdk = q.load_sdk(path)

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.children = self.root / 'children'
        for kind in ('pg', 'oci'):
            (self.children / kind).mkdir(parents=True)
        self.model = DockerModel()
        self.addCleanup(patch.stopall)
        patch.object(self.sdk, 'checked', side_effect=self.model.checked).start()
        patch.object(self.sdk, 'restrict', lambda _: None).start()
        patch.object(self.sdk.shutil, 'disk_usage', return_value=type('Usage', (), {'free': 200 * 1024 ** 3})()).start()
        patch.dict(os.environ, {'SDLC_RESOURCE_REGISTRY': str(self.root / 'registry'), 'SDLC_MIN_FREE_GIB': '30'}).start()
        self.bridge = q.NativeComposeBridge(sdk=self.sdk, children=self.children,
            pg_project=PG, oci_project=OCI, registry=self.root / 'registry')
        self.data = self.root / 'data.json'
        self.data.write_bytes(b'{}')

    def app(self, kind, image=IMAGE):
        return {'image': image, 'user': '65532:65532', 'read_only': True,
                'cap_drop': ['ALL'], 'security_opt': ['no-new-privileges:true'],
                'labels': {'sdlc.task': q.TASK, 'sdlc.purpose': q.PURPOSES[kind]['application']},
                'networks': ['owner'], 'volumes': [{'type': 'bind', 'source': str(self.data),
                    'target': '/forge/data.json', 'read_only': True}]}

    def pg(self):
        return {'services': {'database': {'image': IMAGE, 'networks': ['owner'],
            'labels': {'sdlc.task': q.TASK, 'sdlc.purpose': q.PURPOSES['pg']['database']},
            'volumes': [{'type': 'volume', 'source': 'data', 'target': '/var/lib/postgresql/data'}]}},
            'networks': {'owner': {'external': True, 'name': 'parent_qa'}},
            'volumes': {'data': {'name': PG + '_postgres-data',
                'labels': {'sdlc.task': q.TASK, 'sdlc.purpose': 'disposable-postgres-data'}}}}

    def source(self, kind, spec, name='initial.json'):
        path = self.children / kind / name
        path.write_text(json.dumps(spec))
        return path

    def up(self, kind, path, service):
        project = PG if kind == 'pg' else OCI
        tail = ['up', '--detach', '--pull', 'never', '--wait', 'database'] if service == 'database' else [
            'up', '--detach', '--no-build', '--pull', 'never', '--force-recreate', 'application']
        return self.bridge.execute(['-p', project, '-f', str(path), *tail])

    def start_pg(self):
        source = self.source('pg', self.pg())
        self.up('pg', source, 'database')
        return self.bridge.operations[PG], source

    def start_oci(self):
        source = self.source('oci', {'services': {'application': self.app('oci')},
            'networks': {'owner': {'external': True, 'name': 'parent_qa'}}})
        self.up('oci', source, 'application')
        return self.bridge.operations[OCI], source

    def test_actual_sdk_parent_class_and_v2_journal(self):
        operation, _ = self.start_pg()
        self.assertIsInstance(operation, self.sdk.ComposeHelper)
        journal = q.read(operation.journal)
        self.assertEqual(journal['version'], 2)
        self.assertEqual(journal['daemon_id'], q.DAEMON_ID)
        self.assertEqual(journal['owner_pid'], os.getpid())
        self.assertEqual(journal['manifest_sha256'], q.digest(operation.path.read_bytes()))
        self.assertEqual(len(journal['generations']), 1)
        self.assertEqual(len(list(operation.registry.glob('*.json'))), 1)

    def test_pg_actual_sdk_owns_and_deletes_disposable_volume(self):
        operation, source = self.start_pg()
        volume = self.model.items['volume'][PG + '_postgres-data']
        self.assertEqual(volume['Labels']['sdlc.cleanup-id'], operation.cleanup_id)
        self.bridge.execute(['-p', PG, '-f', str(source), 'down', '--remove-orphans'])
        self.assertFalse(self.model.items['volume'])
        self.assertFalse(self.model.items['container'])
        self.assertIn('parent_qa', self.model.items['network'])
        self.assertEqual(q.read(operation.journal)['phase'], 'cleaned')
        self.assertFalse(list(operation.registry.glob('*.json')))

    def test_pg_application_transition_preserves_db_and_purpose_guards(self):
        operation, _ = self.start_pg()
        database = self.model.items['container'][PG + '-database-1']
        spec = self.pg()
        spec['services']['application'] = self.app('pg')
        source = self.source('pg', spec, 'application.json')
        self.up('pg', source, 'application')
        self.assertIs(database, self.model.items['container'][PG + '-database-1'])
        labels = self.model.items['container'][PG + '-application-1']['Config']['Labels']
        self.assertEqual(labels['sdlc.purpose'], 'disposable-postgres-application')
        self.assertEqual(labels['com.docker.compose.project.config_files'], str(operation.path))
        self.assertEqual(database['Config']['Labels']['sdlc.purpose'], 'disposable-postgres-target')
        self.assertEqual(len(operation.generations), 2)
        operation.check_ownership()
        self.bridge.close()
        self.assertFalse(self.model.items['volume'])

    def test_oci_transition_retains_native_source_and_old_generation(self):
        operation, first = self.start_oci()
        before = first.read_bytes()
        generation = Path(operation.generations[0]['path']).read_bytes()
        second = self.source('oci', {'services': {'application': self.app('oci', IMAGE2)},
            'networks': {'owner': {'external': True, 'name': 'parent_qa'}}}, 'next.json')
        self.up('oci', second, 'application')
        self.assertEqual(first.read_bytes(), before)
        self.assertEqual(Path(operation.generations[0]['path']).read_bytes(), generation)
        self.assertEqual(q.read(operation.path)['services']['application']['image'], IMAGE2)
        self.bridge.close()
        self.assertFalse(self.model.items['container'])

    def test_initial_resource_adoption_refused_by_actual_sdk(self):
        self.model.items['container']['foreign'] = {'Config': {'Labels': {'com.docker.compose.project': PG}}}
        with self.assertRaises(RuntimeError):
            self.start_pg()
        self.assertIn('foreign', self.model.items['container'])

    def test_foreign_cleanup_id_blocks_mutation(self):
        operation, _ = self.start_pg()
        self.model.items['container'][PG + '-database-1']['Config']['Labels']['sdlc.cleanup-id'] = 'foreign'
        count = len(self.model.commands)
        with self.assertRaises(RuntimeError):
            self.bridge.close()
        self.assertFalse(any(command[-2:] == ['down', '--remove-orphans'] for command in self.model.commands[count:]))
        self.assertEqual(q.read(operation.journal)['phase'], 'cleanup-required')

    def test_foreign_manifest_label_blocks_cleanup(self):
        self.start_pg()
        self.model.items['container'][PG + '-database-1']['Config']['Labels']['com.docker.compose.project.config_files'] = '/foreign'
        with self.assertRaises(RuntimeError):
            self.bridge.close()
        self.assertTrue(self.model.items['volume'])

    def test_native_source_changed_blocks_cleanup(self):
        _, source = self.start_pg()
        source.write_text('{}')
        with self.assertRaises(RuntimeError):
            self.bridge.close()
        self.assertTrue(self.model.items['container'])

    def test_manifest_changed_blocks_cleanup(self):
        operation, _ = self.start_pg()
        operation.path.write_text('{}')
        with self.assertRaises(RuntimeError):
            self.bridge.close()

    def test_immutable_generation_changed_blocks_cleanup(self):
        operation, _ = self.start_pg()
        Path(operation.generations[0]['path']).write_text('{}')
        with self.assertRaises(RuntimeError):
            self.bridge.close()

    def test_native_up_replay_refused(self):
        _, source = self.start_pg()
        with self.assertRaises(ValueError):
            self.up('pg', source, 'database')

    def test_new_resource_inventory_refused(self):
        self.start_pg()
        spec = self.pg()
        spec['networks']['other'] = {'external': True, 'name': 'other'}
        source = self.source('pg', spec, 'wrong.json')
        with self.assertRaises(ValueError):
            self.up('pg', source, 'database')

    def test_new_application_sandbox_relaxation_refused(self):
        self.start_oci()
        app = self.app('oci', IMAGE2)
        app['read_only'] = False
        source = self.source('oci', {'services': {'application': app},
            'networks': {'owner': {'external': True, 'name': 'parent_qa'}}}, 'unsafe.json')
        with self.assertRaises(ValueError):
            self.up('oci', source, 'application')

    def test_application_cannot_add_capabilities_or_writable_mount(self):
        validator = q.session_class(self.sdk).__new__(q.session_class(self.sdk))
        validator.kind, validator.project = 'oci', OCI
        for key, value in [('cap_add', ['ALL']), ('devices', ['/dev/null'])]:
            app = self.app('oci')
            app[key] = value
            source = self.source('oci', {'services': {'application': app},
                'networks': {'owner': {'external': True, 'name': 'parent_qa'}}}, key + '.json')
            with self.subTest(key=key), self.assertRaises(ValueError):
                validator.normalize(q.read(source))
        app = self.app('oci')
        app['volumes'][0]['read_only'] = False
        source = self.source('oci', {'services': {'application': app},
            'networks': {'owner': {'external': True, 'name': 'parent_qa'}}}, 'writable.json')
        with self.assertRaises(ValueError):
            validator.normalize(q.read(source))

    def test_pg_service_cannot_change_during_application_transition(self):
        self.start_pg()
        spec = self.pg()
        spec['services']['database']['image'] = IMAGE2
        spec['services']['application'] = self.app('pg')
        source = self.source('pg', spec, 'db-changed.json')
        with self.assertRaises(ValueError):
            self.up('pg', source, 'application')
        self.assertEqual(self.model.items['container'][PG + '-database-1']['State']['Running'], True)

    def test_down_cannot_select_unknown_manifest_in_owned_tree(self):
        self.start_pg()
        source = self.source('pg', self.pg(), 'unknown.json')
        with self.assertRaises(ValueError):
            self.bridge.execute(['-p', PG, '-f', str(source), 'down', '--remove-orphans'])
        self.assertTrue(self.model.items['volume'])

    def test_foreign_volume_reference_is_preserved(self):
        self.start_pg()
        self.model.items['container']['foreign-reference'] = {'Config': {'Labels': {'com.docker.compose.project': 'other'}},
            'Volumes': [PG + '_postgres-data']}
        with self.assertRaises(RuntimeError):
            self.bridge.close()
        self.assertIn(PG + '_postgres-data', self.model.items['volume'])

    def test_unknown_project_refused(self):
        with self.assertRaises(ValueError):
            self.bridge.execute(['-p', 'sdlc1', '-f', '/tmp/file', 'down', '--remove-orphans'])

    def test_unowned_manifest_refused(self):
        with self.assertRaises(ValueError):
            self.up('pg', self.root / 'foreign.json', 'database')

    def test_actual_drain_timeout_five_preserved(self):
        operation, _ = self.start_oci()
        self.bridge.execute(['-p', OCI, '-f', str(operation.source_records[0]['path']),
            'stop', '--timeout', '5', 'application'])
        self.assertEqual(self.model.commands[-1][-4:], ['stop', '--timeout', '5', 'application'])

    def test_unknown_arguments_refused(self):
        for tail in (['rm', '--force'], ['up', '--build'], ['down', '--volumes'], ['stop', '--timeout', '30', 'application']):
            with self.subTest(tail=tail), self.assertRaises(ValueError):
                q.command_parts(['-p', PG, '-f', '/tmp/unused', *tail])

    def test_atomic_enospc_preserves_complete_previous_receipt(self):
        path = self.root / 'receipt.json'
        q.atomic(path, {'old': True})
        original = path.read_bytes()
        with patch.object(q.os, 'fsync', side_effect=OSError(errno.ENOSPC, 'injected')):
            with self.assertRaises(OSError):
                q.atomic(path, {'new': True})
        self.assertEqual(path.read_bytes(), original)
        self.assertFalse(list(self.root.glob('*.pending')))

    def test_exclusive_generation_never_overwritten(self):
        path = self.root / 'generation.json'
        q.atomic(path, {'old': True}, exclusive=True)
        with self.assertRaises(FileExistsError):
            q.atomic(path, {'new': True}, exclusive=True)
        self.assertEqual(q.read(path), {'old': True})

    def expired_journal(self):
        operation, _ = self.start_pg()
        data = q.read(operation.journal)
        data['updated_at'] = (datetime.now(timezone.utc) - timedelta(hours=25)).isoformat()
        q.atomic(operation.journal, data)
        for item in self.model.items['container'].values():
            item['State']['Running'] = False
        return operation

    def test_v2_recovery_requires_dead_owner(self):
        operation = self.expired_journal()
        with patch.object(q, 'owner_dead', return_value=False), self.assertRaises(ValueError):
            q.recover_session(self.sdk, operation.journal, self.root)

    def test_v2_recovery_requires_24_hours(self):
        operation, _ = self.start_pg()
        with patch.object(q, 'owner_dead', return_value=True), self.assertRaises(ValueError):
            q.recover_session(self.sdk, operation.journal, self.root)

    def test_v2_recovery_requires_stopped_containers(self):
        operation = self.expired_journal()
        for item in self.model.items['container'].values():
            item['State']['Running'] = True
        with patch.object(q, 'owner_dead', return_value=True), self.assertRaises(ValueError):
            q.recover_session(self.sdk, operation.journal, self.root)

    def test_v2_recovery_applies_sdk_exact_cleanup_only_after_guards(self):
        operation = self.expired_journal()
        with patch.object(q, 'owner_dead', return_value=True):
            report = q.recover_session(self.sdk, operation.journal, self.root, apply=True)
        self.assertEqual(report['status'], 'cleaned')
        self.assertFalse(self.model.items['volume'])
        self.assertIn('parent_qa', self.model.items['network'])

    def test_v2_recovery_refuses_changed_manifest(self):
        operation = self.expired_journal()
        operation.path.write_text('{}')
        with patch.object(q, 'owner_dead', return_value=True), self.assertRaises(ValueError):
            q.recover_session(self.sdk, operation.journal, self.root, apply=True)

    def test_recovery_never_uses_windows_kill(self):
        with patch.object(q.os, 'name', 'nt'), patch.object(q.os, 'kill') as kill, self.assertRaises(ValueError):
            q.owner_dead(42)
        kill.assert_not_called()

    def test_recovery_permission_error_is_not_dead_owner(self):
        with patch.object(q.os, 'name', 'posix'), patch.object(q.os, 'kill', side_effect=PermissionError):
            self.assertFalse(q.owner_dead(42))


class TransportTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.config = Path(self.temp.name) / 'transport.json'
        self.socket_path = str(Path(self.temp.name) / 'maintenance.sock')
        q.atomic(self.config, {'version': 1, 'socket': self.socket_path})
        self.addCleanup(patch.stopall)
        patch.object(client, 'CONFIG', self.config).start()
        # Linux-only transport is modelled on Windows too, never used as a TCP fallback.
        patch.object(client.socket, 'AF_UNIX', 1, create=True).start()

    def test_env_cleared_native_client_uses_only_sealed_config(self):
        connection = unittest.mock.MagicMock()
        connection.__enter__.return_value = connection
        stream = connection.makefile.return_value.__enter__.return_value
        stream.readline.return_value = b'{"exit":0,"stdout":"ok","error_class":null}\n'
        with patch.dict(os.environ, {}, clear=True), patch.object(client.socket, 'socket', return_value=connection), \
                patch.object(sys, 'argv', ['docker-compose', '--version']), patch.object(sys.stdout, 'write') as output:
            self.assertEqual(client.main(), 0)
        connection.connect.assert_called_once_with(self.socket_path)
        output.assert_called_once_with('ok')

    def test_client_observation_timeout_never_reconnects(self):
        connection = unittest.mock.MagicMock()
        connection.__enter__.return_value = connection
        connection.makefile.return_value.__enter__.return_value.readline.side_effect = TimeoutError
        with patch.object(client.socket, 'socket', return_value=connection), self.assertRaises(TimeoutError):
            client.main()
        connection.connect.assert_called_once()
        connection.sendall.assert_called_once()

    def test_client_rejects_unknown_or_relative_transport(self):
        for data in ({'version': 1, 'socket': 'relative'}, {'version': 1, 'socket': '/tmp/s', 'fallback': True}):
            q.atomic(self.config, data)
            with self.subTest(data=data), patch.object(client.socket, 'socket') as create, self.assertRaises(ValueError):
                client.main()
            create.assert_not_called()

    def test_client_rejects_malformed_response_without_retry(self):
        connection = unittest.mock.MagicMock()
        connection.__enter__.return_value = connection
        connection.makefile.return_value.__enter__.return_value.readline.return_value = b'{"exit":true}\n'
        with patch.object(client.socket, 'socket', return_value=connection), self.assertRaises(ValueError):
            client.main()
        connection.connect.assert_called_once()

    def lease(self):
        left, right = socket.socketpair()
        self.addCleanup(left.close)
        self.addCleanup(right.close)
        lease = q.RequestLease(left)
        lease.thread.start()
        self.addCleanup(lambda: (lease.finished.set(), lease.thread.join(timeout=2)))
        return lease, right

    def test_disconnect_cancels_and_reaps_host_process_no_retry(self):
        lease, peer = self.lease()
        timer = threading.Timer(0.15, peer.close)
        timer.start()
        self.addCleanup(timer.join)
        original = subprocess.Popen
        processes = []
        def create(*args, **kwargs):
            process = original(*args, **kwargs)
            processes.append(process)
            return process
        started = time.monotonic()
        with patch.object(q.subprocess, 'Popen', side_effect=create) as spawn, self.assertRaises(RuntimeError):
            lease.checked([sys.executable, '-B', '-c', 'import time; time.sleep(10)'], timeout=5)
        self.assertEqual(spawn.call_count, 1)
        self.assertTrue(lease.cancelled.is_set())
        self.assertIsNotNone(processes[0].poll())
        self.assertLess(time.monotonic() - started, 3)

    def test_command_deadline_not_extended_or_retried(self):
        lease, _ = self.lease()
        started = time.monotonic()
        with self.assertRaises(subprocess.TimeoutExpired):
            lease.checked([sys.executable, '-B', '-c', 'import time; time.sleep(10)'], timeout=0.15)
        self.assertLess(time.monotonic() - started, 3)

    def test_closed_request_cannot_start_new_host_command(self):
        lease, peer = self.lease()
        peer.close()
        self.assertTrue(lease.cancelled.wait(timeout=2))
        with patch.object(q.subprocess, 'Popen') as spawn, self.assertRaises(RuntimeError):
            lease.checked(['docker', '--context', 'rootless', 'compose', 'up'])
        spawn.assert_not_called()

    def test_sdk_binding_preserves_nonrequest_original_api(self):
        original = unittest.mock.Mock(return_value=b'original')
        sdk = SimpleNamespace(checked=original)
        q.bind_request_lifetime(sdk)
        bound = sdk.checked
        q.bind_request_lifetime(sdk)
        self.assertIs(bound, sdk.checked)
        self.assertEqual(sdk.checked(['docker'], timeout=20), b'original')
        original.assert_called_once_with(['docker'], timeout=20)

    def handler_response(self, argv, execute):
        local, peer = socket.socketpair()
        self.addCleanup(local.close)
        self.addCleanup(peer.close)
        server = SimpleNamespace(bridge=SimpleNamespace(execute=execute))
        thread = threading.Thread(target=q.BridgeHandler, args=(local, None, server))
        thread.start()
        peer.settimeout(3)
        try:
            peer.sendall((json.dumps({'argv': argv}) + '\n').encode())
            with peer.makefile('rb') as stream:
                response = stream.readline(q.MAX_DOCUMENT + 1)
            return response
        finally:
            peer.close()
            thread.join(timeout=3)
            self.assertFalse(thread.is_alive())

    def test_actual_rpc_handler_binds_request_lifetime_and_frames_once(self):
        requests = []
        def execute(argv):
            requests.append(argv)
            self.assertIsInstance(q.REQUEST.get(), q.RequestLease)
            return b'closed diagnostic\n'
        response = self.handler_response(['--version'], execute)
        self.assertEqual(json.loads(response), {'exit': 0, 'stdout': 'closed diagnostic\n', 'error_class': None})
        self.assertEqual(requests, [['--version']])
        self.assertIsNone(q.REQUEST.get())

    def test_rpc_failure_never_exports_raw_command_error(self):
        def execute(_):
            raise RuntimeError('PRIVATE_SQL_ROLE_DB_CREDENTIAL_DO_NOT_EXPORT')
        response = self.handler_response(['--version'], execute)
        self.assertEqual(json.loads(response), {'exit': 1, 'stdout': '', 'error_class': 'RuntimeError'})
        self.assertNotIn(b'PRIVATE_SQL', response)


if __name__ == '__main__':
    unittest.main()
