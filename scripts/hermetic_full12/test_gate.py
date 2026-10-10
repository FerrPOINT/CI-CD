"""Pure regressions only; Docker, WSL, Cargo and hosted dispatch are never invoked."""
import ast
import copy
from contextlib import nullcontext
from contextvars import ContextVar
import errno
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

import host as h
import run as gate


class ControlsTests(unittest.TestCase):
    def test_all12_order_and_boundaries(self):
        self.assertEqual(gate.JOBS['A'], ('python', 'row-smoke', 'smoke', 'check', 'clippy'))
        self.assertEqual(gate.JOBS['B'], ('postgres',))
        self.assertEqual(gate.JOBS['C'], ('oci', 'workspace', 'integration', 'cli', 'openapi', 'release'))
        self.assertEqual(sum(gate.JOBS.values(), ()), gate.STAGES)

    def test_all_original_budgets(self):
        self.assertEqual(list(gate.BUDGETS.values()), [2400, 300, 300, 2400, 2400, 5400, 2400, 900, 900, 900, 900, 1800])
        self.assertEqual({job: gate.job_budget(job) for job in gate.JOBS}, {'A': 20232, 'B': 14350, 'C': 21570})
        for job in gate.JOBS:
            self.assertLess(gate.job_budget(job), 21600)
        coverage = h.read(h.HERE / 'coverage.json')
        self.assertEqual(coverage['jobs'], {job: list(stages) for job, stages in gate.JOBS.items()})

    def test_budget_includes_all_overhead_and_finally(self):
        self.assertEqual((gate.ENTRY_SECONDS, gate.EXIT_SECONDS, gate.FINAL_SECONDS), (300, 500, 1800))
        source = (h.HERE / 'run.py').read_text()
        self.assertIn('with h.wall_budget(ENTRY_SECONDS)', source)
        self.assertIn('with h.wall_budget(EXIT_SECONDS)', source)
        self.assertEqual(source.count('with h.wall_budget(h.BOOTSTRAP_DEADLINE - time.monotonic())'), 2)
        self.assertIn('with h.wall_budget(ASSERT_SECONDS)', source)
        self.assertIn('with h.wall_budget(1500)', source)
        self.assertIn('with h.wall_budget(220)', source)
        self.assertIn('with h.wall_budget(60)', source)

    def test_frozen_component_lock_matches_exact_control_files(self):
        self.assertTrue(h.verify_components())

    def test_declared_actions_budget_and_fresh_job_wiring(self):
        import re
        workflow = (h.HERE.parents[1] / '.github/workflows/forge-hermetic-full12.yml').read_text()
        blocks = re.split(r'^  ([ABC]):\n', workflow, flags=re.M)
        self.assertEqual(blocks[1::2], list(gate.JOBS))
        for job, block in zip(blocks[1::2], blocks[2::2]):
            self.assertIn('timeout-minutes: 360', block)
            declared = sum(int(value) * 60 for value in re.findall(r'^        timeout-minutes: (\d+)$', block, re.M))
            self.assertEqual(declared + 240, gate.ACTION_SECONDS[job])
            self.assertEqual(block.count('run.py run ' + job), 1)
        self.assertNotIn('workflow_dispatch', workflow)

    def test_overhead_alarm_restored_on_failure(self):
        with patch.object(h, 'require'), patch.object(h.signal, 'ITIMER_REAL', 0, create=True), patch.object(h.signal, 'SIGALRM', 14, create=True), patch.object(h.signal, 'getitimer', return_value=(0.0, 0.0), create=True), patch.object(h.signal, 'setitimer', create=True) as timer, patch.object(h.signal, 'signal', return_value='previous') as handler:
            with self.assertRaises(h.OverheadTimeout):
                with h.wall_budget(500):
                    handler.call_args_list[0].args[1]()
        self.assertEqual(timer.call_args_list[-1].args, (0, 0))
        self.assertEqual(handler.call_args_list[-1].args, (14, 'previous'))

    def test_original_public_components_bytes(self):
        origins = h.read(h.HERE / 'origins.json')['files']
        for name, digest in origins.items():
            self.assertFalse(name.startswith('maintenance/'))
            raw = (h.HERE / name).read_bytes()
            self.assertEqual(hashlib.sha256(raw).hexdigest(), digest, name)

    def test_source265_catalogues(self):
        catalogues = h.read(h.HERE / 'source-catalogue.json')['catalogues']
        self.assertEqual({key: value['count'] for key, value in catalogues.items()}, {'forge': 177, 'base': 88})
        for spec in catalogues.values():
            raw = json.dumps(spec['files'], sort_keys=True, separators=(',', ':')).encode()
            self.assertEqual(hashlib.sha256(raw).hexdigest(), spec['sha256'])
        self.assertNotIn('baseline_count', h.read(h.HERE / 'source-catalogue.json'))

    def test_original_smoke_assertions_and_deadlines(self):
        source = (h.HERE / 'sql_smoke.py').read_bytes()
        tree = ast.parse(source)
        self.assertEqual(sum(isinstance(node, ast.Assert) for node in ast.walk(tree)), 30)
        observer = h.load('test_original_observer', h.HERE / 'observer.py')
        self.assertEqual(len(observer.MARKERS), 13)
        self.assertEqual((observer.AGGREGATE_SECONDS, observer.CALL_SECONDS), (300, 30))
        waits = [node for node in ast.walk(tree) if isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute)
                 and node.func.attr == 'wait']
        self.assertTrue(any(any(key.arg == 'timeout' and isinstance(key.value, ast.Constant) and key.value.value == 10
                                for key in node.keywords) for node in waits))

    def test_row_assertions_unchanged(self):
        self.assertEqual(sum(isinstance(node, ast.Assert) for node in ast.walk(ast.parse((h.HERE / 'row_smoke.py').read_bytes()))), 16)

    def test_source_and_base_exact(self):
        self.assertEqual(h.SOURCE, '25be2e82d4d42673897c8a16070eb8a9519244f0')
        self.assertEqual(h.BASE, '19a7a381ae6dbea61a643bb96189e483fa64df5c')

    def test_no_old_cache_or_native_workspace_path_in_new_executor(self):
        for name in ('host.py', 'run.py', 'tools.Dockerfile'):
            raw = (h.HERE / name).read_text()
            self.assertNotIn('/home/sdlc1-runner', raw)
            self.assertNotIn('38138e49', raw)
            self.assertNotIn('70957', raw)
            self.assertNotIn('copytree', raw)

    def test_no_direct_docker_run_create_or_privilege_bypass(self):
        tree = ast.parse((h.HERE / 'run.py').read_bytes())
        for node in ast.walk(tree):
            if isinstance(node, ast.Call) and isinstance(node.func, ast.Name) and node.func.id == 'docker' and node.args and isinstance(node.args[0], ast.Constant):
                self.assertNotIn(node.args[0].value, ('run', 'create'))
        self.assertNotIn('privileged', (h.HERE / 'tools.Dockerfile').read_text())
        self.assertNotIn('buildx create', (h.HERE / 'run.py').read_text())

    def test_python_command_scope_exact(self):
        text = (h.HERE / 'gates.sh').read_text()
        self.assertIn('python3 -B -m unittest scripts.tests.test_postgres_delivery scripts.tests.test_oci_offline_fixture', text)
        self.assertIn('sdlc_pg_delivery -- --test-threads=1 --nocapture', text)
        self.assertIn('sdlc_oci_delivery -- --test-threads=1 --nocapture', text)

    def test_followups_not_downscoped(self):
        text = (h.HERE / 'followups.sh').read_text()
        self.assertIn('cargo test --locked --offline --workspace -- --test-threads=1', text)
        self.assertIn('cargo build --locked --offline --release --workspace', text)
        self.assertIn('diff --strip-trailing-cr', text)

    def test_capacity_reserve_not_waived(self):
        self.assertEqual((h.HOST_BYTES, h.DATA_BYTES, h.INODES), (108279229428, 71319483898, 300000))

    def test_capacity_below_original_reserve_rejected(self):
        with patch.dict(os.environ, RUNNER_TEMP='.'), patch.object(h.shutil, 'disk_usage', return_value=SimpleNamespace(free=h.HOST_BYTES - 1)), patch.object(h.os, 'statvfs', return_value=SimpleNamespace(f_favail=h.INODES), create=True):
            with self.assertRaises(ValueError):
                h.capacity(Path('.'))

    def test_capacity_exact_original_reserve(self):
        with patch.dict(os.environ, RUNNER_TEMP='.'), patch.object(h.shutil, 'disk_usage', return_value=SimpleNamespace(free=h.HOST_BYTES)), patch.object(h.os, 'statvfs', return_value=SimpleNamespace(f_favail=h.INODES), create=True):
            self.assertEqual(h.capacity(Path('.'))['host_min_bytes'], h.HOST_BYTES)

    def test_reclaim_high_capacity_does_not_remove_anything(self):
        with patch.dict(os.environ, RUNNER_TEMP='.'), patch.object(h, 'hosted_guard'), patch.object(h.shutil, 'disk_usage', return_value=SimpleNamespace(free=h.HOST_BYTES)), patch.object(h, 'capacity'), patch.object(h, 'command') as command:
            self.assertEqual(h.reclaim(Path('.')), [])
        command.assert_not_called()

    def test_reclaim_local_guard_before_side_effects(self):
        with patch.object(h, 'hosted_guard', side_effect=ValueError), patch.object(h, 'command') as command:
            with self.assertRaises(ValueError):
                h.reclaim(Path('.'))
        command.assert_not_called()


class AtomicAndFailureTests(unittest.TestCase):
    def test_atomic_replace(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'receipt.json'
            h.atomic(path, {'old': True})
            h.atomic(path, {'new': True})
            self.assertEqual(h.read(path), {'new': True})

    def test_enospc_preserves_receipt_and_cleans_temporary(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'receipt.json'
            h.atomic(path, {'old': True})
            before = path.read_bytes()
            with patch.object(h.os, 'fsync', side_effect=OSError(errno.ENOSPC, 'PRIVATE/path SQL')):
                with self.assertRaises(OSError):
                    h.atomic(path, {'new': True})
            self.assertEqual(path.read_bytes(), before)
            self.assertEqual(list(Path(temp).iterdir()), [path])

    def test_exclusive_receipt_no_overwrite(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'receipt.json'
            h.atomic(path, {'old': True}, exclusive=True)
            with self.assertRaises(FileExistsError):
                h.atomic(path, {'new': True}, exclusive=True)
            self.assertEqual(h.read(path), {'old': True})
            self.assertEqual(list(Path(temp).iterdir()), [path])

    def test_safe_errno_no_exception_payload(self):
        result = h.safe_error('smoke', OSError(28, 'PRIVATE SQL ROLE DB', '/secret/file'))
        self.assertEqual(result, {'stage': 'smoke', 'category': 'os_error', 'errno': 28})
        self.assertNotIn('PRIVATE', json.dumps(result))
        self.assertNotIn('secret', json.dumps(result))

    def test_safe_unknown_stage_and_errno(self):
        error = OSError(9000, 'secret')
        self.assertEqual(h.safe_error('private/path', error), {'stage': 'admission', 'category': 'os_error', 'errno': None})

    def test_terminal_stdout_if_preflight_raises(self):
        output = io.StringIO()
        with patch.object(gate.sys, 'argv', ['run.py', 'run', 'A']), patch.object(gate, 'run_job', side_effect=OSError(28, 'PRIVATE')), patch('sys.stdout', output):
            self.assertEqual(gate.main(), 1)
        self.assertIn('FORGE_TERMINAL_FAILURE', output.getvalue())
        self.assertNotIn('PRIVATE', output.getvalue())

    def test_reaped_leader_not_signalled_or_claimed_clean(self):
        process = Mock(returncode=0)
        with patch.object(h.os, 'killpg', create=True) as kill, patch.object(h.signal, 'SIGKILL', 9, create=True):
            with self.assertRaises(ValueError):
                h.terminate(process)
        kill.assert_not_called()
        process.wait.assert_not_called()

    def test_own_hung_child_killed_and_waited(self):
        process = Mock(pid=321, returncode=None)
        process.wait.return_value = 0
        with patch.object(h, 'leader_status'), patch.object(h.os, 'getpgid', return_value=321, create=True), patch.object(h.os, 'getsid', return_value=321, create=True), patch.object(h, 'live_group', return_value=False), patch.object(h.os, 'killpg', create=True) as kill, patch.object(h.signal, 'SIGKILL', 9, create=True):
            h.terminate(process)
        self.assertEqual(kill.call_count, 2)
        self.assertEqual(process.wait.call_count, 1)
        process.poll.assert_not_called()

    def test_exited_unreaped_leader_still_cleans_group_before_wait(self):
        process = Mock(pid=321, returncode=None)
        order = []
        process.wait.side_effect = lambda **_: order.append('wait')
        with patch.object(h, 'leader_status', return_value=SimpleNamespace(si_pid=321)), patch.object(h.os, 'getpgid', return_value=321, create=True), patch.object(h.os, 'getsid', return_value=321, create=True), patch.object(h, 'live_group', return_value=False), patch.object(h.os, 'killpg', side_effect=lambda *args: order.append(args), create=True), patch.object(h.signal, 'SIGKILL', 9, create=True):
            h.terminate(process)
        self.assertEqual(order, [(321, h.signal.SIGTERM), (321, 9), 'wait'])

    def test_unknown_reaped_identity_never_signalled(self):
        with patch.object(h, 'leader_status', side_effect=ChildProcessError), patch.object(h.os, 'killpg', create=True) as kill:
            with self.assertRaises(ChildProcessError):
                h.terminate(Mock(pid=321, returncode=None))
        kill.assert_not_called()

    def test_foreign_group_never_signalled(self):
        with patch.object(h, 'leader_status'), patch.object(h.os, 'getpgid', return_value=999, create=True), patch.object(h.os, 'killpg', create=True) as kill:
            with self.assertRaises(ValueError):
                h.terminate(Mock(pid=321, returncode=None))
        kill.assert_not_called()

    def test_group_still_live_never_claimed_clean(self):
        process = Mock(pid=321, returncode=None)
        with patch.object(h, 'leader_status'), patch.object(h.os, 'getpgid', return_value=321, create=True), patch.object(h.os, 'getsid', return_value=321, create=True), patch.object(h, 'live_group', return_value=True), patch.object(h.time, 'monotonic', side_effect=[0, 10]), patch.object(h.os, 'killpg', create=True), patch.object(h.signal, 'SIGKILL', 9, create=True):
            with self.assertRaises(subprocess.TimeoutExpired):
                h.terminate(process)
        process.wait.assert_not_called()

    def test_command_timeout_does_not_pass(self):
        child = Mock(returncode=None, stdout=None, stderr=None)
        selector = Mock()
        selector.__enter__ = Mock(return_value=selector)
        selector.__exit__ = Mock(return_value=False)
        with tempfile.TemporaryDirectory() as temp, patch.object(h, 'require'), patch.object(h.subprocess, 'Popen', return_value=child), patch.object(h, 'leader_status', return_value=None), patch.object(h.time, 'monotonic', side_effect=[0, 31]), patch.object(h.selectors, 'DefaultSelector', return_value=selector), patch.object(h, 'terminate') as close:
            with self.assertRaises(subprocess.TimeoutExpired):
                h.command(['not-executed'], timeout=30, log=Path(temp) / 'private.log')
        close.assert_called_once_with(child)
        child.poll.assert_not_called()
        child.communicate.assert_not_called()

    def test_unqualified_sdk_delivery_fails_before_writes(self):
        with tempfile.TemporaryDirectory() as temp:
            with self.assertRaises(h.m.QualificationFailure), patch.object(h, 'git') as git, patch.object(h.m, 'CANDIDATE_COMMIT', None):
                h.maintenance(Path(temp), Path(temp) / 'private-checkout', {})
            git.assert_not_called()
            self.assertEqual(list(Path(temp).iterdir()), [])

    def test_runtime_seal_mutation_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            h.atomic(root / 'admission-seal.json', {'current': True})
            h.atomic(root / 'execution-seal.json', {'current': True})
            expected = {name: h.sha(root / name) for name in ('admission-seal.json', 'execution-seal.json')}
            h.atomic(root / 'runtime-seal-hashes.json', expected)
            with patch.object(h.m, 'EXPECTED_FILES', {}):
                gate.check_runtime_seals(root, expected)
            h.atomic(root / 'execution-seal.json', {'mutated': True})
            with self.assertRaises(ValueError):
                gate.check_runtime_seals(root, expected)

    def test_receipt_enospc_still_reports_terminal_stdout_no_native(self):
        with tempfile.TemporaryDirectory() as temp:
            output = io.StringIO()
            original = h.atomic
            def atomic(path, value, exclusive=False):
                if Path(path).parent.name == 'public':
                    raise OSError(28, 'PRIVATE SQL PATH')
                return original(path, value, exclusive)
            env = {'GITHUB_WORKSPACE': temp, 'RUNNER_TEMP': temp, 'GITHUB_RUN_ID': '123', 'GITHUB_RUN_ATTEMPT': '1', 'GITHUB_SHA': 'a' * 40}
            with patch.dict(os.environ, env), patch.object(h.m, 'preflight'), patch.object(h.m, 'read_payloads', return_value=({}, {})), patch.object(h, 'wall_budget', side_effect=lambda *_: nullcontext()), patch.object(h, 'hosted_guard'), patch.object(h, 'verify_components', return_value={}), patch.object(h, 'checkout_proof', return_value={}), patch.object(h, 'reclaim', side_effect=h.CapacityFailure({'host_free_bytes': 0})), patch.object(h, 'atomic', side_effect=atomic), patch.object(gate, 'start_daemon') as start, patch('sys.stdout', output):
                self.assertEqual(gate.run_job('A'), 1)
            start.assert_not_called()
            self.assertIn('FORGE_RECEIPT_FAILURE', output.getvalue())
            self.assertIn('FORGE_TERMINAL', output.getvalue())
            self.assertNotIn('PRIVATE', output.getvalue())


class GateAssertionTests(unittest.TestCase):
    def test_python74_rejected(self):
        with self.assertRaises(ValueError):
            gate.assertions('python', b'Ran 74 tests\nFROZEN_LINUX_PYTHON_UNITS:PASS', Path('.'))

    def test_python75(self):
        self.assertEqual(gate.assertions('python', b'Ran 75 tests\nFROZEN_LINUX_PYTHON_UNITS:PASS', Path('.'))['tests_passed'], 75)

    def test_workspace219_rejected(self):
        raw = b'test result: ok. 219 passed; 0 failed;\nFROZEN_WORKSPACE_RUSTFMT:PASS\nFROZEN_WORKSPACE_UMASK=0022'
        with self.assertRaises(ValueError):
            gate.assertions('workspace', raw, Path('.'))

    def test_workspace221(self):
        raw = b'test result: ok. 221 passed; 0 failed;\nFROZEN_WORKSPACE_RUSTFMT:PASS\nFROZEN_WORKSPACE_UMASK=0022'
        self.assertEqual(gate.assertions('workspace', raw, Path('.'))['tests_passed'], 221)

    def test_oci_partial_marker_rejected(self):
        with self.assertRaises(ValueError):
            gate.assertions('oci', b'1 passed; 0 failed', Path('.'))

    def test_partial_smoke_marker_rejected(self):
        with self.assertRaises(ValueError):
            gate.assertions('smoke', b'PG17_HISTORY_FK_PASSIVE_ACTION_CONTRAST:PASS', Path('.'))

    def test_integration80(self):
        self.assertEqual(gate.assertions('integration', b'test result: ok. 80 passed; 0 failed;', Path('.'))['tests_passed'], 80)

    def test_cli_requires_clippy(self):
        with self.assertRaises(ValueError):
            gate.assertions('cli', b'test result: ok. 2 passed; 0 failed;', Path('.'))


class SmokeAndCleanupTests(unittest.TestCase):
    def setUp(self):
        self.observer = h.load('pure_smoke_observer', h.HERE / 'observer.py')

    def test_aggregate_deadline_is_not_restarted(self):
        clock = Mock(side_effect=[0, 300])
        observer = self.observer.Observer(io.StringIO(), clock=clock)
        with self.assertRaises(self.observer.ClosedFailure) as error:
            observer.check()
        self.assertEqual(error.exception.reason, 'aggregate_deadline')

    def test_percall_remaining_budget(self):
        observer = self.observer.Observer(io.StringIO(), clock=lambda: 0)
        self.assertEqual(observer.wait_budget(), (30, False))
        self.assertEqual(observer.wait_budget(10), (10, False))
        observer.clock = lambda: 295
        self.assertEqual(observer.wait_budget(), (5, True))

    def test_hung_call_is_not_pass(self):
        process = Mock(pid=44)
        process.communicate.side_effect = subprocess.TimeoutExpired('PRIVATE_SQL', 30)
        observer = self.observer.Observer(io.StringIO(), clock=lambda: 0, popen=Mock(return_value=process), signal_group=Mock())
        with self.assertRaises(self.observer.ClosedFailure) as error:
            observer.run(['not-executed'], capture_output=True)
        self.assertEqual(error.exception.reason, 'call_timeout')

    def test_partial_markers_never_finish(self):
        observer = self.observer.Observer(io.StringIO(), clock=lambda: 0)
        for marker in self.observer.MARKERS[:11]:
            observer.print(marker)
        with self.assertRaises(self.observer.ClosedFailure):
            observer.finish()

    def test_redaction_excludes_payload(self):
        sink = io.StringIO()
        observer = self.observer.Observer(sink, clock=lambda: 0)
        observer.print('PRIVATE_SQL_ROLE_DB')
        observer.phase(10)
        self.assertNotIn('PRIVATE', sink.getvalue())

    def test_translate_preserves_fresh_psql_argv(self):
        transport = h.load('pure_transport', h.HERE / 'transport.py')
        observer = self.observer.Observer(io.StringIO(), clock=lambda: 0)
        binding = transport.BoundExec(self.observer, observer, 'sdlc-qa-forge-delivery-' + 'a' * 12, '/owned/compose.json')
        binding.container_id = 'b' * 64
        tail = ['psql', '-X', '-qAt', '-v', 'ON_ERROR_STOP=1', '-h', '127.0.0.1', '-U', 'role', '-d', 'db', '-c', 'SQL']
        self.assertEqual(binding.translate(binding.prefix + tail), h.DOCKER + ['exec', '-i', 'b' * 64, *tail])

    def test_unowned_translate_rejected(self):
        transport = h.load('pure_transport_negative', h.HERE / 'transport.py')
        binding = transport.BoundExec(self.observer, Mock(), 'project', '/owned/compose.json')
        binding.container_id = 'b' * 64
        with self.assertRaises(self.observer.ClosedFailure):
            binding.translate(['docker', 'exec', 'foreign', 'psql'])

    def test_prelaunch_empty_cleanup_preserves_external(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for path in ('stages', 'children', 'registry', 'cache', 'sources', 'output', 'disposable/target', 'disposable/scratch'):
                (root / path).mkdir(parents=True, exist_ok=True)
            marker = root / 'disposable/ownership.json'
            h.atomic(marker, {'owned': True})
            q = SimpleNamespace(read=h.read)
            parent = SimpleNamespace(disposable_identity=lambda *_: {'owned': True})
            config = {'project': 'parent', 'pg_project': 'pg', 'oci_project': 'oci'}
            with patch.object(gate, 'identity'), patch.object(gate, 'inventory', return_value={'container': [], 'network': [], 'volume': []}):
                result = gate.cleanup_disposable(root, config, h.sha(marker), q, parent, {}, [])
            self.assertTrue(result['complete'])
            self.assertFalse(result['v2_execution_claimed'])
            self.assertTrue(all((root / name).is_dir() for name in ('cache', 'sources', 'output')))
            self.assertFalse((root / 'disposable/target').exists())

    def test_nonempty_prelaunch_path_preserved(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for path in ('stages', 'children', 'registry', 'disposable/target', 'disposable/scratch'):
                (root / path).mkdir(parents=True, exist_ok=True)
            marker = root / 'disposable/ownership.json'
            h.atomic(marker, {'owned': True})
            (root / 'disposable/target/unknown').write_text('writer')
            config = {'project': 'parent', 'pg_project': 'pg', 'oci_project': 'oci'}
            with patch.object(gate, 'identity'), patch.object(gate, 'inventory', return_value={'container': [], 'network': [], 'volume': []}):
                with self.assertRaises(ValueError):
                    gate.cleanup_disposable(root, config, h.sha(marker), SimpleNamespace(read=h.read),
                        SimpleNamespace(disposable_identity=lambda *_: {'owned': True}), {}, [])
            self.assertTrue((root / 'disposable/target/unknown').exists())

    def test_foreign_inventory_blocks_target_cleanup(self):
        with patch.object(gate, 'identity'), patch.object(gate, 'inventory', return_value={'container': ['unknown'], 'network': [], 'volume': []}):
            with self.assertRaises(ValueError):
                gate.cleanup_disposable(Path('.'), {'project': 'a', 'pg_project': 'b', 'oci_project': 'c'}, 'hash', Mock(), Mock(), {}, [])

    def test_launched_cleanup_uses_real_parent_adapter_with_plural_inventory(self):
        parent = h.load('real_retention_cleanup_pure', h.HERE / 'parent.py')
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve()
            for name in ('registry', 'cache', 'sources', 'output', 'children', 'stages/smoke'):
                (root / name).mkdir(parents=True)
            config = {'native_root': str(root), 'cache_root': str(root / 'cache'), 'forge_git_sha': h.SOURCE,
                'project': 'sdlc-qa-forge-delivery-' + 'a' * 20, 'pg_project': 'pg', 'oci_project': 'oci'}
            q = SimpleNamespace(plain=lambda value: Path(value).absolute(), read=h.read, atomic=h.atomic,
                digest=lambda raw: hashlib.sha256(raw).hexdigest(), DAEMON_ID='admitted', TASK=gate.TASK, SDK_SHA256='b' * 64)
            ownership = parent.create_disposable(q, config)
            (root / 'disposable/target/owned').write_text('disposable')
            manifest = root / 'stages/smoke/compose.json'
            h.atomic(manifest, {'owned': True})
            h.atomic(manifest.parent / 'journal.json', {'version': 2, 'phase': 'cleaned', 'task': gate.TASK,
                'daemon_id': q.DAEMON_ID, 'sdk_sha256': q.SDK_SHA256, 'project': config['project'],
                'owner_pid': os.getpid(), 'cleanup_id': 'a' * 32, 'manifest': str(manifest), 'manifest_sha256': h.sha(manifest)})
            with patch.object(parent, 'os', SimpleNamespace(name='posix', walk=os.walk)), patch.object(gate, 'identity'), patch.object(gate, 'inventory', return_value={'container': [], 'network': [], 'volume': []}):
                result = gate.cleanup_disposable(root, config, ownership, q, parent, {}, [])
            self.assertTrue(result['complete'])
            self.assertFalse((root / 'disposable/target').exists())
            self.assertFalse((root / 'disposable/scratch').exists())
            self.assertTrue(all((root / name).is_dir() for name in ('cache', 'sources', 'output')))

    def test_native_request_adapter_never_uses_reaping_checked(self):
        context = ContextVar('pure_request', default=None)
        request = SimpleNamespace(check=Mock(), checked=Mock(side_effect=AssertionError('frozen path used')))
        sdk = SimpleNamespace(checked=Mock())
        gate.install_process_adapter(sdk, SimpleNamespace(REQUEST=context))
        token = context.set(request)
        try:
            with patch.object(h, 'command', return_value=b'closed') as command:
                self.assertEqual(sdk.checked(['not-executed'], text=True), 'closed')
                self.assertIs(command.call_args.kwargs['check'], request.check)
            with patch.object(h, 'command', side_effect=h.CommandFailure(7)):
                with self.assertRaises(h.CommandFailure):
                    sdk.checked(['not-executed'])
        finally:
            context.reset(token)
        request.checked.assert_not_called()


class BootstrapDiagnosticTests(unittest.TestCase):
    STEPS = ('tools_download', 'dependencies', 'user_namespace', 'manager',
             'rootless_launch', 'context', 'socket_ready', 'identity_decode',
             'identity_version', 'identity_compose_rootless', 'identity_endpoint',
             'identity_cgroup_warnings', 'baseline', 'admission_seal')

    def test_closed_source_attested_step_inventory(self):
        # The old combined label remains readback-compatible, but is no longer emitted.
        steps = tuple(step for step in h.BOOTSTRAP_STEPS if step not in ('identity_cgroup_warnings', 'manager'))
        self.assertEqual(set(h.BOOTSTRAP_STEPS), set(self.STEPS) | {
            'identity_cgroup_version', 'identity_cgroup_driver', 'identity_cgroup_resources',
            'manager_dropin', 'manager_reload', 'manager_linger', 'manager_start',
            'manager_readback', 'manager_controllers'})
        tree = ast.parse((h.HERE / 'run.py').read_bytes())
        seen = {}
        for node in ast.walk(tree):
            if isinstance(node, ast.With):
                call = node.items[0].context_expr
                if isinstance(call, ast.Call) and ast.unparse(call.func) == 'h.bootstrap_step':
                    self.assertEqual(len(call.args), 1)
                    self.assertEqual(call.keywords, [])
                    step = ast.literal_eval(call.args[0])
                    self.assertNotIn(step, seen)
                    seen[step] = ast.unparse(ast.Module(body=node.body, type_ignores=[]))
        self.assertEqual(set(seen), set(steps))
        for step, operation in (
            ('tools_download', 'h.download_tools(root)'), ('dependencies', "'apt-get'"),
            ('user_namespace', "'usermod'"), ('manager_linger', "'loginctl'"),
            ('manager_dropin', 'install_delegation(root)'), ('manager_reload', "'daemon-reload'"),
            ('manager_start', "'start'"), ('manager_readback', 'verify_delegation(root)'),
            ('manager_controllers', "'cpu', 'memory', 'pids'"),
            ('rootless_launch', "'systemd-run'"), ('context', "'create', 'rootless'"),
            ('socket_ready', 'time.monotonic() + 90'), ('identity_decode', "'info'"),
            ('identity_version', "'29.8.2'"), ('identity_compose_rootless', "'name=rootless'"),
            ('identity_endpoint', "'daemon-data'"), ('identity_cgroup_version', "info.get('CgroupVersion') == '2'"),
            ('identity_cgroup_driver', "info.get('CgroupDriver') == 'systemd'"),
            ('identity_cgroup_resources', 'info.get(key) is True'),
            ('baseline', "not baseline['container'] and (not baseline['volume'])"),
            ('admission_seal', "'admission-seal.json'"),
        ):
            self.assertIn(operation, seen[step])
        self.assertIn('h.require(time.monotonic() < deadline)', seen['socket_ready'])
        self.assertIn('time.sleep(0.25)', seen['socket_ready'])

    def test_fixed_hints_preserve_original_safe_category_and_bounds(self):
        errors = (ValueError('PRIVATE_SENTINEL'), OSError(28, 'PRIVATE_SENTINEL'),
                  h.CommandFailure(7), subprocess.TimeoutExpired('PRIVATE_SENTINEL', 90),
                  AssertionError('PRIVATE_SENTINEL'))
        for step in h.BOOTSTRAP_STEPS:
            for error in errors:
                with self.subTest(step=step, error_type=type(error).__name__):
                    original = h.safe_error('bootstrap', error)
                    with patch.object(h, 'BOOTSTRAP_DEADLINE', 1):
                        with self.assertRaises(h.BootstrapFailure) as caught:
                            with h.bootstrap_step(step):
                                raise error
                    safe = h.safe_error('bootstrap', caught.exception)
                    self.assertEqual(safe, {**original, 'bootstrap_step': step})
                    h.validate_safe_error(safe)
                    self.assertNotIn('PRIVATE_SENTINEL', json.dumps(safe))
                    self.assertEqual(str(caught.exception), 'bootstrap_boundary_failed')

    def test_nested_boundary_preserves_innermost_fixed_step(self):
        with patch.object(h, 'BOOTSTRAP_DEADLINE', 1):
            with self.assertRaises(h.BootstrapFailure) as caught:
                with h.bootstrap_step('rootless_launch'):
                    with h.bootstrap_step('socket_ready'):
                        raise ValueError('PRIVATE_SENTINEL')
        self.assertEqual(h.safe_error('bootstrap', caught.exception)['bootstrap_step'], 'socket_ready')

    def test_postbootstrap_preserves_original_exception(self):
        error = h.CommandFailure(7)
        with patch.object(h, 'BOOTSTRAP_DEADLINE', None):
            with self.assertRaises(h.CommandFailure) as caught:
                with h.bootstrap_step('identity_decode'):
                    raise error
        self.assertIs(caught.exception, error)

    def test_capacity_failure_preserves_existing_structured_report(self):
        error = h.CapacityFailure({'host_free_bytes': 0})
        with patch.object(h, 'BOOTSTRAP_DEADLINE', 1):
            with self.assertRaises(h.CapacityFailure) as caught:
                with h.bootstrap_step('admission_seal'):
                    raise error
        self.assertIs(caught.exception, error)
        self.assertEqual(caught.exception.measurement, {'host_free_bytes': 0})
        self.assertNotIn('bootstrap_step', h.safe_error('bootstrap', caught.exception))

    def test_unknown_boundary_refused_before_body(self):
        for step in ('PRIVATE_SENTINEL', 'socket_ready_suffix', '', None, [], True):
            reached = []
            with self.subTest(step_type=type(step).__name__), self.assertRaises(ValueError):
                with h.bootstrap_step(step):
                    reached.append(True)
            self.assertEqual(reached, [])

    def test_previous_safe_error_schema_remains_valid(self):
        for error in (ValueError('private'), OSError(28, 'private'), h.CommandFailure(7),
                      h.m.QualificationFailure()):
            safe = h.safe_error('bootstrap', error)
            self.assertNotIn('bootstrap_step', safe)
            h.validate_safe_error(safe)

    def test_strict_error_schema_rejects_unsafe_unknown_and_out_of_bounds(self):
        base = {'stage': 'bootstrap', 'category': 'closed_failure', 'errno': None}
        changes = ({'bootstrap_step': 'PRIVATE_SENTINEL'}, {'bootstrap_step': 'socket_ready_suffix'},
                   {'bootstrap_step': ['socket_ready']}, {'error': 'PRIVATE_SENTINEL'},
                   {'stdout': 'PRIVATE_SENTINEL'}, {'stage': []}, {'category': 'PRIVATE_SENTINEL'},
                   {'errno': -1}, {'errno': 4096}, {'errno': True}, {'exit_code': 256},
                   {'exit_code': -256}, {'exit_code': True}, {'dependency': 'PRIVATE_SENTINEL'},
                   {'stage': 'cleanup', 'bootstrap_step': 'socket_ready'})
        for change in changes:
            with self.subTest(keys=tuple(change)), self.assertRaises(ValueError):
                h.validate_safe_error({**base, **change})
        for missing in base:
            with self.assertRaises(ValueError):
                h.validate_safe_error({key: value for key, value in base.items() if key != missing})

    def test_foreign_exception_attribute_and_tampered_wrapper_not_trusted(self):
        error = ValueError('PRIVATE_SENTINEL')
        error.bootstrap_step = 'socket_ready'
        self.assertNotIn('bootstrap_step', h.safe_error('bootstrap', error))
        wrapped = h.BootstrapFailure('socket_ready', error)
        wrapped.step = 'PRIVATE_SENTINEL'
        with self.assertRaises(ValueError):
            h.safe_error('bootstrap', wrapped)
        wrapped.step = 'socket_ready'
        wrapped.safe['stdout'] = 'PRIVATE_SENTINEL'
        with self.assertRaises(ValueError):
            h.safe_error('bootstrap', wrapped)

    @staticmethod
    def identity_info():
        root = Path('/synthetic-owned-f12')
        return {'ID': 'synthetic', 'DockerRootDir': str(root / 'daemon-data'),
                'ServerVersion': '29.8.2', 'SecurityOptions': ['name=rootless'],
                'Driver': 'overlay2', 'CgroupDriver': 'systemd', 'CgroupVersion': '2', 'Warnings': None,
                'MemoryLimit': True, 'CpuCfsQuota': True, 'CpuCfsPeriod': True, 'PidsLimit': True}

    @staticmethod
    def call_identity(info, compose='Docker Compose version v5.5.1', admission=None):
        root = Path('/synthetic-owned-f12')
        def docker(*args):
            if args == ('info', '--format', '{{json .}}'):
                return json.dumps(info).encode()
            if args == ('context', 'inspect', 'rootless', '--format', '{{.Endpoints.docker.Host}}'):
                return ('unix://' + str(root / 'docker.sock')).encode()
            if args == ('version', '--format', '{{.Client.Version}}'):
                return b'29.8.2'
            if args == ('compose', 'version'):
                return compose.encode()
            raise AssertionError('unexpected synthetic command')
        with patch.object(gate, 'docker', side_effect=docker), patch.object(h, 'BOOTSTRAP_DEADLINE', 1):
            return gate.identity(root, admission)

    def test_identity_guards_have_exact_safe_hints_without_private_warnings(self):
        original = self.identity_info()
        cases = (({}, 'Docker Compose version v5.5.1', None),
                 ({'ServerVersion': 'wrong'}, 'Docker Compose version v5.5.1', 'identity_version'),
                 ({}, 'wrong', 'identity_compose_rootless'),
                 ({'SecurityOptions': []}, 'Docker Compose version v5.5.1', 'identity_compose_rootless'),
                 ({'DockerRootDir': '/wrong'}, 'Docker Compose version v5.5.1', 'identity_endpoint'),
                 ({'CgroupVersion': '1'}, 'Docker Compose version v5.5.1', 'identity_cgroup_version'),
                 ({'CgroupDriver': 'none'}, 'Docker Compose version v5.5.1', 'identity_cgroup_driver'),
                 ({'Warnings': ['PRIVATE_SENTINEL']}, 'Docker Compose version v5.5.1', None))
        for change, compose, expected in cases:
            info = {**original, **change}
            with self.subTest(change=change, compose=compose):
                if expected is None:
                    result = self.call_identity(info, compose)
                    self.assertEqual(result['id'], 'synthetic')
                    self.assertNotIn('PRIVATE_SENTINEL', json.dumps(result))
                else:
                    with self.assertRaises(h.BootstrapFailure) as caught:
                        self.call_identity(info, compose)
                    safe = h.safe_error('bootstrap', caught.exception)
                    self.assertEqual(safe['bootstrap_step'], expected)
                    self.assertNotIn('PRIVATE_SENTINEL', json.dumps(safe))
                    h.validate_safe_error(safe)

    def test_resource_capabilities_require_exact_true_even_without_warnings(self):
        for key in ('MemoryLimit', 'CpuCfsQuota', 'CpuCfsPeriod', 'PidsLimit'):
            for value in (False, None, 0, 1, 'true', 'PRIVATE_SENTINEL', [], {}):
                with self.subTest(key=key, value=value):
                    info = self.identity_info()
                    info[key] = value
                    with self.assertRaises(h.BootstrapFailure) as caught:
                        self.call_identity(info)
                    safe = h.safe_error('bootstrap', caught.exception)
                    self.assertEqual(safe['bootstrap_step'], 'identity_cgroup_resources')
                    self.assertNotIn('PRIVATE_SENTINEL', json.dumps(safe))
                    h.validate_safe_error(safe)
            info = self.identity_info()
            del info[key]
            with self.assertRaises(h.BootstrapFailure) as caught:
                self.call_identity(info)
            self.assertEqual(caught.exception.step, 'identity_cgroup_resources')

    def test_missing_or_malformed_cgroup_identity_fail_closed(self):
        for key, expected in (('CgroupVersion', 'identity_cgroup_version'), ('CgroupDriver', 'identity_cgroup_driver')):
            for value in (None, False, 2, [], {}, 'PRIVATE_SENTINEL'):
                info = self.identity_info()
                info[key] = value
                with self.subTest(key=key, value=value), self.assertRaises(h.BootstrapFailure) as caught:
                    self.call_identity(info)
                self.assertEqual(caught.exception.step, expected)
                self.assertNotIn('PRIVATE_SENTINEL', json.dumps(h.safe_error('bootstrap', caught.exception)))
            info = self.identity_info()
            del info[key]
            with self.assertRaises(h.BootstrapFailure) as caught:
                self.call_identity(info)
            self.assertEqual(caught.exception.step, expected)

    def test_stage_recheck_rejects_lost_cpu_capability_with_same_daemon(self):
        info = self.identity_info()
        admission = {'daemon': self.call_identity(info)}
        info['CpuCfsQuota'] = False
        with self.assertRaises(h.BootstrapFailure) as caught:
            self.call_identity(info, admission=admission)
        self.assertEqual(caught.exception.step, 'identity_cgroup_resources')

    @staticmethod
    def daemon_fixture(controllers='cpu memory pids', failure=None):
        calls = []
        def command(argv, **kwargs):
            calls.append((argv, kwargs))
            if argv[:4] == ['sudo', '-n', 'python3', '-c'] and argv[-3] == 'install' and failure is not None:
                raise failure
            if argv[:2] == ['systemctl', 'show']:
                return ('Delegate=yes\nDelegateControllers=cpu memory pids\n'
                        'ControlGroup=/user.slice/user-1001.slice/user@1001.service\n'
                        'DropInPaths=/run/systemd/system/user@1001.service.d/90-forge-full12-' + 'a' * 32 + '.conf\n').encode()
            return b'runner' if argv == ['id', '-un'] else b''
        def read_text(path, *args, **kwargs):
            if str(path).replace('\\', '/') in ('/etc/subuid', '/etc/subgid'):
                return 'runner:100000:65536\n'
            if path.name == 'cgroup.controllers':
                return controllers
            raise AssertionError('unexpected synthetic filesystem read')
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'docker.sock').touch()
            with patch.object(h, 'download_tools', return_value=root / 'bin'), \
                    patch.object(h, 'command', side_effect=command), patch.object(gate, 'identity', return_value={'id': 'fixture'}), \
                    patch.object(gate.os, 'getuid', return_value=1001, create=True), patch.object(Path, 'read_text', read_text), \
                    patch.object(h, 'hosted_guard'), patch.object(gate.uuid, 'uuid4', return_value=SimpleNamespace(hex='a' * 32)), \
                    patch.dict(os.environ), patch.object(h, 'BOOTSTRAP_DEADLINE', 1):
                result = gate.start_daemon(root)
        return result, calls

    def test_hosted_delegation_runtime_only_before_rootless_launch(self):
        result, calls = self.daemon_fixture('io pids cpu memory cpuset')
        self.assertEqual(result, {'id': 'fixture'})
        argv = [args for args, _ in calls]
        delegation = ['sudo', '-n', 'python3', '-c', gate.DELEGATION_PROGRAM, 'install', '1001', 'a' * 32]
        self.assertIn(delegation, argv)
        self.assertLess(argv.index(delegation), next(index for index, args in enumerate(argv) if args[0] == 'systemd-run'))
        self.assertEqual(calls[argv.index(delegation)][1], {'timeout': 90})
        reload = ['sudo', '-n', 'systemctl', 'daemon-reload']
        start = ['sudo', '-n', 'systemctl', 'start', 'user@1001.service']
        self.assertLess(argv.index(delegation), argv.index(reload))
        self.assertLess(argv.index(reload), argv.index(start))
        self.assertFalse(any('set-property' in args for args in argv))
        self.assertFalse(any('restart' in args or 'revert' in args for args in argv))

    def test_hosted_delegation_missing_controllers_prevents_daemon_launch(self):
        for controllers in ('memory pids', 'cpu pids', 'cpu memory', '', 'PRIVATE_SENTINEL'):
            with self.subTest(controllers=controllers):
                with self.assertRaises(h.BootstrapFailure) as caught:
                    self.daemon_fixture(controllers)
                self.assertEqual(caught.exception.step, 'manager_controllers')
                self.assertNotIn('PRIVATE_SENTINEL', json.dumps(h.safe_error('bootstrap', caught.exception)))

    def test_hosted_delegation_command_failure_has_no_fallback(self):
        with self.assertRaises(h.BootstrapFailure) as caught:
            self.daemon_fixture(failure=h.CommandFailure(1))
        self.assertEqual(caught.exception.step, 'manager_dropin')
        self.assertEqual(h.safe_error('bootstrap', caught.exception)['exit_code'], 1)

    def test_retained_failure_hint_does_not_turn_failure_into_acceptance(self):
        report = {'status': 'FAIL', 'full12_pass': False, 'stages': []}
        error = h.BootstrapFailure('socket_ready', ValueError('PRIVATE_SENTINEL'))
        output = io.StringIO()
        with patch('sys.stdout', output):
            gate.retain_failure(report, 'bootstrap', error, Path('/unused'), 'A', [])
        self.assertEqual(report['status'], 'FAIL')
        self.assertFalse(report['full12_pass'])
        h.validate_safe_error(report['error'])
        self.assertEqual(report['error']['bootstrap_step'], 'socket_ready')
        self.assertNotIn('PRIVATE_SENTINEL', output.getvalue())


class DelegationDropinTests(unittest.TestCase):
    TOKEN = 'a' * 32

    def owner(self, root):
        owner = {'uid': '1001', 'token': self.TOKEN, 'owner_pid': os.getpid(), 'root': str(root)}
        h.atomic(root / 'delegation-owner.json', owner, exclusive=True)
        return owner

    def test_cleanup_before_daemon_marker_still_removes_owned_dropin(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.owner(root)
            with patch.object(h, 'hosted_guard'), patch.object(gate.os, 'getuid', return_value=1001, create=True), \
                    patch.object(h, 'command') as command:
                self.assertEqual(gate.stop_daemon(root), {'started': False, 'stopped': True})
                self.assertTrue(gate.cleanup_delegation(root))
            self.assertEqual(command.call_args_list[0].args[0][-3:], ['cleanup', '1001', self.TOKEN])
            self.assertEqual(command.call_args_list[1].args[0], ['sudo', '-n', 'systemctl', 'daemon-reload'])
            self.assertEqual([call.kwargs['timeout'] for call in command.call_args_list], [20, 20])

    def test_cleanup_no_intent_has_no_privileged_effect(self):
        with tempfile.TemporaryDirectory() as temp, patch.object(h, 'command') as command:
            self.assertTrue(gate.cleanup_delegation(Path(temp)))
        command.assert_not_called()

    def test_cleanup_foreign_owner_marker_has_no_privileged_effect(self):
        for field, value in [('uid', '1002'), ('owner_pid', -1), ('root', '/foreign'), ('token', '../foreign')]:
            with self.subTest(field=field), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                owner = self.owner(root)
                owner[field] = value
                h.atomic(root / 'delegation-owner.json', owner)
                with patch.object(h, 'hosted_guard'), patch.object(gate.os, 'getuid', return_value=1001, create=True), \
                        patch.object(h, 'command') as command:
                    with self.assertRaises(ValueError):
                        gate.cleanup_delegation(root)
                command.assert_not_called()

    def test_readback_requires_loaded_owned_dropin_and_runtime_controllers(self):
        good = {'Delegate': 'yes', 'DelegateControllers': 'cpu memory pids',
                'DropInPaths': '/run/systemd/system/user@1001.service.d/90-forge-full12-' + self.TOKEN + '.conf',
                'ControlGroup': '/user.slice/user-1001.slice/user@1001.service'}
        variants = [good]
        variants += [{**good, key: value} for key, value in (
            ('Delegate', 'no'), ('DelegateControllers', 'memory pids'),
            ('DropInPaths', '/foreign.conf'), ('ControlGroup', '/foreign'))]
        variants += [{key: value for key, value in good.items() if key != 'Delegate'}, {**good, 'foreign': 'private'}]
        for index, values in enumerate(variants):
            with self.subTest(index=index), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                self.owner(root)
                raw = ''.join(key + '=' + value + '\n' for key, value in values.items()).encode()
                with patch.object(gate.os, 'getuid', return_value=1001, create=True), patch.object(h, 'command', return_value=raw):
                    if index == 0:
                        path = gate.verify_delegation(root)
                        self.assertEqual(path.as_posix(), '/sys/fs/cgroup/user.slice/user-1001.slice/user@1001.service/cgroup.controllers')
                    else:
                        with self.assertRaises(ValueError):
                            gate.verify_delegation(root)

    def test_duplicate_or_malformed_manager_readback_rejected(self):
        for raw in (b'Delegate=yes\n' * 4, b'private\n', b''):
            with tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                self.owner(root)
                with patch.object(gate.os, 'getuid', return_value=1001, create=True), patch.object(h, 'command', return_value=raw):
                    with self.assertRaises(ValueError):
                        gate.verify_delegation(root)

    def test_outer_finally_requires_cleanup_including_bootstrap_failure(self):
        tree = ast.parse((h.HERE / 'run.py').read_bytes())
        run = next(node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == 'run_job')
        final = ast.unparse(run)
        self.assertIn("cleanup['delegation_removed'] = cleanup_delegation(root)", final)
        self.assertLess(final.index("cleanup['daemon_stopped'] = stop_daemon(root)"),
                        final.index("cleanup['delegation_removed'] = cleanup_delegation(root)"))
        self.assertIn("cleanup.get('delegation_removed') is True", final)
        aggregate = next(node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == 'aggregate')
        self.assertIn("report['cleanup'].get('delegation_removed') is True", ast.unparse(aggregate))

    @staticmethod
    def program(base):
        namespace = {'__name__': 'pure_test_not_main'}
        exec(compile(gate.DELEGATION_PROGRAM, '<delegation-program>', 'exec'), namespace)
        namespace['BASE'] = base
        return namespace

    @unittest.skipUnless(os.name == 'posix', 'real dir_fd/nofollow hard-link semantics require Linux')
    def test_real_files_atomic_no_overwrite_and_exact_inode_cleanup(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            program = self.program(base)
            fstat = os.fstat
            def root_stat(fd):
                values = list(fstat(fd))
                values[4] = 0
                return os.stat_result(values)
            with patch.object(os, 'geteuid', return_value=0), patch.object(os, 'fstat', side_effect=root_stat):
                program['apply']('install', '1001', self.TOKEN)
                parent = base / 'user@1001.service.d'
                target = parent / ('90-forge-full12-' + self.TOKEN + '.conf')
                owned = parent / ('.forge-full12-' + self.TOKEN) / 'delegate'
                self.assertEqual(target.stat().st_ino, owned.stat().st_ino)
                original = target.read_bytes()
                with self.assertRaises(FileExistsError):
                    program['apply']('install', '1001', self.TOKEN)
                self.assertEqual(target.read_bytes(), original)
                program['apply']('cleanup', '1001', self.TOKEN)
                self.assertEqual(list(parent.iterdir()), [])

    @unittest.skipUnless(os.name == 'posix', 'real dir_fd/nofollow hard-link semantics require Linux')
    def test_real_foreign_same_bytes_different_inode_never_removed(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            program = self.program(base)
            fstat = os.fstat
            def root_stat(fd):
                values = list(fstat(fd))
                values[4] = 0
                return os.stat_result(values)
            with patch.object(os, 'geteuid', return_value=0), patch.object(os, 'fstat', side_effect=root_stat):
                program['apply']('install', '1001', self.TOKEN)
                parent = base / 'user@1001.service.d'
                target = parent / ('90-forge-full12-' + self.TOKEN + '.conf')
                raw = target.read_bytes()
                target.unlink()
                target.write_bytes(raw)
                inode = target.stat().st_ino
                with self.assertRaises(ValueError):
                    program['apply']('cleanup', '1001', self.TOKEN)
                self.assertEqual(target.stat().st_ino, inode)
                self.assertEqual(target.read_bytes(), raw)
                self.assertTrue((parent / ('.forge-full12-' + self.TOKEN) / 'delegate').exists())

    def test_root_helper_closed_identity_and_no_transient_setter(self):
        program = self.program(Path('/not-used'))
        for action, uid, token in [('install', '../1001', self.TOKEN), ('install', '0', self.TOKEN),
                                   ('install', '1001', '../foreign'), ('private', '1001', self.TOKEN)]:
            with patch.object(os, 'geteuid', return_value=0, create=True), self.assertRaises(ValueError):
                program['apply'](action, uid, token)
        source = (h.HERE / 'run.py').read_text()
        self.assertNotIn("'set-property'", source)
        self.assertNotIn('/etc/systemd', source)
        self.assertNotIn("'restart'", source)

    @unittest.skipUnless(os.name == 'posix', 'real dir_fd/nofollow FIFO semantics require Linux')
    def test_real_fifo_corrupt_missing_or_symlink_source_is_held_without_unlink(self):
        for fault in ('fifo', 'corrupt', 'missing', 'symlink'):
            with self.subTest(fault=fault), tempfile.TemporaryDirectory() as temp:
                base = Path(temp)
                program = self.program(base)
                fstat = os.fstat
                def root_stat(fd):
                    values = list(fstat(fd))
                    values[4] = 0
                    return os.stat_result(values)
                with patch.object(os, 'geteuid', return_value=0), patch.object(os, 'fstat', side_effect=root_stat):
                    program['apply']('install', '1001', self.TOKEN)
                    parent = base / 'user@1001.service.d'
                    target = parent / ('90-forge-full12-' + self.TOKEN + '.conf')
                    owned = parent / ('.forge-full12-' + self.TOKEN) / 'delegate'
                    owned.unlink()
                    if fault == 'fifo':
                        os.mkfifo(owned)
                    elif fault == 'corrupt':
                        owned.write_bytes(b'foreign')
                    elif fault == 'symlink':
                        owned.symlink_to(target)
                    before = target.read_bytes()
                    with self.assertRaises((ValueError, OSError)):
                        program['apply']('cleanup', '1001', self.TOKEN)
                    self.assertEqual(target.read_bytes(), before)

    @unittest.skipUnless(os.name == 'posix', 'real nofollow/no-overwrite semantics require Linux')
    def test_real_foreign_publication_collision_and_parent_symlink_never_overwritten(self):
        for fault in ('collision', 'symlink'):
            with self.subTest(fault=fault), tempfile.TemporaryDirectory() as temp:
                base = Path(temp)
                program = self.program(base)
                parent = base / 'user@1001.service.d'
                if fault == 'collision':
                    parent.mkdir()
                else:
                    (base / 'foreign').mkdir()
                    parent.symlink_to(base / 'foreign', target_is_directory=True)
                target = parent / ('90-forge-full12-' + self.TOKEN + '.conf')
                target.write_bytes(b'foreign')
                fstat = os.fstat
                def root_stat(fd):
                    values = list(fstat(fd))
                    values[4] = 0
                    return os.stat_result(values)
                with patch.object(os, 'geteuid', return_value=0), patch.object(os, 'fstat', side_effect=root_stat):
                    with self.assertRaises(OSError):
                        program['apply']('install', '1001', self.TOKEN)
                self.assertEqual(target.read_bytes(), b'foreign')
                self.assertEqual(list(parent.iterdir()), [target])

    @unittest.skipUnless(os.name == 'posix', 'real dir_fd/fsync failure cleanup requires Linux')
    def test_real_install_fsync_failure_removes_only_new_unpublished_entries(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            program = self.program(base)
            fstat, fsync = os.fstat, os.fsync
            def root_stat(fd):
                values = list(fstat(fd))
                values[4] = 0
                return os.stat_result(values)
            calls = []
            def sync(fd):
                calls.append(fd)
                if len(calls) == 2:
                    raise OSError(errno.ENOSPC, 'private')
                fsync(fd)
            with patch.object(os, 'geteuid', return_value=0), patch.object(os, 'fstat', side_effect=root_stat), patch.object(os, 'fsync', side_effect=sync):
                with self.assertRaises(OSError):
                    program['apply']('install', '1001', self.TOKEN)
            self.assertEqual(list((base / 'user@1001.service.d').iterdir()), [])


class PhysicalResourceTests(unittest.TestCase):
    def run_readback(self, change=None, raw_change=None):
        images = {'tools': 'sha256:' + 'a' * 64, 'postgres': 'sha256:' + 'b' * 64}
        operation = SimpleNamespace(command=['owned-compose'], project='owned-project')
        calls = []
        inspections = {}
        def command(argv, **kwargs):
            calls.append(argv)
            self.assertEqual(kwargs, {'timeout': 10})
            if argv[:1] == operation.command:
                return (('c' if argv[-1] == 'qa' else 'd') * 64 + '\n').encode()
            container = argv[-1] if 'inspect' in argv else argv[argv.index('exec') + 1]
            service = 'qa' if container == 'c' * 64 else 'postgres'
            cpu, memory, pids = (2, 5 * 2**30, 512) if service == 'qa' else (1, 2**30, 0)
            if 'inspect' in argv:
                inspections[service] = inspections.get(service, 0) + 1
                value = {'Id': container, 'Image': images['tools' if service == 'qa' else 'postgres'],
                    'State': {'Running': True}, 'Config': {'Labels': {
                        'com.docker.compose.project': operation.project, 'com.docker.compose.service': service,
                        'sdlc.task': gate.TASK, 'sdlc.purpose': gate.PURPOSE}},
                    'HostConfig': {'NanoCpus': cpu * 10**9, 'Memory': memory, 'PidsLimit': pids, 'CgroupnsMode': 'private'}}
                if change:
                    change(service, inspections[service], value)
                return json.dumps([value]).encode()
            value = f'{cpu * 100000} 100000\n{memory}\n{pids if service == "qa" else "max"}\n'.encode()
            return raw_change(service, value) if raw_change else value
        with patch.object(h, 'command', side_effect=command):
            result = gate.resource_enforcement(operation, images)
        return result, calls

    def test_actual_hostconfig_and_cgroup_readback_both_existing_services(self):
        result, calls = self.run_readback()
        self.assertEqual(result, {'qa': {'cpu_count': 2, 'memory_bytes': 5 * 2**30, 'pids_limit': 512},
                                  'postgres': {'cpu_count': 1, 'memory_bytes': 2**30, 'pids_limit': None}})
        self.assertEqual(len(calls), 8)
        self.assertNotIn('sha256:', json.dumps(result))
        self.assertNotIn('owned-project', json.dumps(result))

    def test_missing_false_malformed_unlimited_or_wrong_hostconfig_refused(self):
        for field, value in [('NanoCpus', 0), ('NanoCpus', '2000000000'), ('Memory', True), ('Memory', 0),
                             ('PidsLimit', -1), ('PidsLimit', None), ('CgroupnsMode', 'host')]:
            def change(service, count, item):
                if service == 'qa':
                    item['HostConfig'][field] = value
            with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                self.run_readback(change=change)

    def test_unlimited_missing_malformed_and_wrong_runtime_limits_refused(self):
        for raw in (b'max 100000\n5368709120\n512\n', b'100000 100000\n5368709120\n512\n',
                    b'200000 100000\nmax\n512\n', b'200000 100000\n5368709120\nmax\n',
                    b'200000 100000\n5368709120\n', b'PRIVATE_SENTINEL\n', b'1' * 391):
            with self.subTest(raw=raw[:16]), self.assertRaises(ValueError):
                self.run_readback(raw_change=lambda service, original: raw if service == 'qa' else original)

    def test_foreign_owner_stopped_container_or_postreadback_mutation_refused(self):
        changes = [lambda item: item['Config']['Labels'].update({'sdlc.task': 'foreign'}),
                   lambda item: item['State'].update({'Running': False}),
                   lambda item: item.update(Image='sha256:' + 'f' * 64),
                   lambda item: item['Config']['Labels'].update({'com.docker.compose.service': 'foreign'})]
        for modify in changes:
            for at in (1, 2):
                with self.subTest(at=at), self.assertRaises(ValueError):
                    self.run_readback(change=lambda service, count, item: modify(item) if service == 'qa' and count == at else None)

    def test_entry_readback_precedes_work_and_aggregate_requires_proof(self):
        source = (h.HERE / 'run.py').read_text()
        stage = source.split('def run_stage(', 1)[1].split('def check_runtime_seals', 1)[0]
        self.assertLess(stage.index("['up', '-d'"), stage.index("proof['resource_enforcement']"))
        self.assertLess(stage.index("proof['resource_enforcement']"), stage.index("path = root / ('private/' + stage + '.log')"))
        with self.assertRaises(ValueError):
            AggregationTests().check(lambda a, b, c: a['stages'][0].pop('resource_enforcement'))
        with self.assertRaises(ValueError):
            AggregationTests().check(lambda a, b, c: c['cleanup'].pop('delegation_removed'))


class NativePolicyTests(unittest.TestCase):
    @staticmethod
    def fixture():
        calls = []
        class Helper:
            def __init__(self, *, resource_policy='local-v1', **kwargs):
                calls.append((resource_policy, kwargs))
        sdk = SimpleNamespace(ComposeHelper=Helper)
        def session_class(sdk):
            class Session(sdk.ComposeHelper):
                pass
            return Session
        q = SimpleNamespace(session_class=session_class, DAEMON_ID='owned-daemon')
        gate.install_native_ci_policy(q)
        kwargs = {'task': gate.TASK, 'docker': h.DOCKER[:], 'daemon_id': q.DAEMON_ID,
                  'kind': 'pg', 'project': 'owned-project', 'directory': 'owned-directory'}
        return q, sdk, calls, kwargs

    def test_hosted_native_policy_is_explicit_without_changing_sdk_default(self):
        q, sdk, calls, kwargs = self.fixture()
        with patch.object(h, 'hosted_guard') as guard:
            q.session_class(sdk)(**kwargs)
        guard.assert_called_once_with()
        self.assertEqual(calls, [('isolated-ci-v1', kwargs)])
        sdk.ComposeHelper()
        self.assertEqual(calls[-1], ('local-v1', {}))

    def test_nonhosted_policy_selection_refused_before_constructor(self):
        q, sdk, calls, kwargs = self.fixture()
        with patch.object(h, 'hosted_guard', side_effect=ValueError('closed_guard')):
            with self.assertRaises(ValueError):
                q.session_class(sdk)(**kwargs)
        self.assertEqual(calls, [])

    def test_foreign_owner_daemon_endpoint_or_policy_refused_before_constructor(self):
        for change in ({'task': 'foreign'}, {'daemon_id': 'foreign'},
                       {'docker': ['docker', '--context', 'foreign']},
                       {'resource_policy': 'local-v1'}, {'resource_policy': 'isolated-ci-v1'}):
            with self.subTest(change=change):
                q, sdk, calls, kwargs = self.fixture()
                kwargs.update(change)
                with patch.object(h, 'hosted_guard'), self.assertRaises(ValueError):
                    q.session_class(sdk)(**kwargs)
                self.assertEqual(calls, [])

    def test_parent_and_cache_explicit_ci_policy_call_sites(self):
        tree = ast.parse((h.HERE / 'run.py').read_bytes())
        for function in ('fill_cache', 'run_stage'):
            node = next(n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == function)
            policies = [keyword.value for call in ast.walk(node) if isinstance(call, ast.Call)
                        for keyword in call.keywords if keyword.arg == 'resource_policy']
            self.assertEqual(len(policies), 1)
            self.assertEqual(ast.literal_eval(policies[0]), 'isolated-ci-v1')

    def test_native_policy_install_after_qualification_and_hosted_admission(self):
        tree = ast.parse((h.HERE / 'run.py').read_bytes())
        job = ast.unparse(next(n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == 'run_job'))
        for preceding in ('h.hosted_guard()', 'h.m.read_payloads(', 'h.bind_maintenance(q, maintenance_proof)',
                          'q.load_sdk(maintenance)'):
            self.assertLess(job.index(preceding), job.index('install_native_ci_policy(q)'))


MAINTENANCE_TEST_FILES = {name: {'blob': 'd' * 40, 'sha256': 'c' * 64} for name in h.m.EXPECTED_FILES}


def report_fixture(job):
    catalogue = h.read(h.HERE / 'source-catalogue.json')['catalogues']
    counts = {'python': 75, 'postgres': 3, 'oci': 1, 'workspace': 221, 'integration': 80, 'cli': 2}
    stages = [{'stage': stage, 'status': 'PASS', 'timeout_seconds': gate.BUDGETS[stage],
               'source_parity': True, 'cleanup_complete': True, **({'tests_passed': counts[stage]} if stage in counts else {})}
              for stage in gate.JOBS[job]]
    for stage in stages:
        stage['resource_enforcement'] = {'qa': {'cpu_count': 2, 'memory_bytes': 5 * 2**30, 'pids_limit': 512},
                                         'postgres': {'cpu_count': 1, 'memory_bytes': 2**30, 'pids_limit': None}}
        if stage['stage'] == 'postgres':
            stage['negative_count'] = 24
        if stage['stage'] == 'release':
            stage['binaries'] = {name: 'e' * 64 for name in gate.BINARIES}
        if stage['stage'] == 'openapi':
            stage.update(export_sha256='f' * 64, contract_sha256='f' * 64)
    token = job.lower() * 20
    projects = {'project': 'sdlc-qa-forge-delivery-' + token, 'pg_project': 'sdlc-qa-forge-pg-' + token, 'oci_project': 'sdlc-qa-forge-oci-' + token}
    journal = {'version': 2, 'phase': 'cleaned', 'task': gate.TASK, 'daemon_id': 'independent-' + job,
        'sdk_sha256': 'c' * 64,
        'cleanup_id': 'd' * 32, 'journal_sha256': 'a' * 64, 'manifest_sha256': 'b' * 64}
    for stage in stages:
        stage['remaining'] = {project: gate.inventory_proof({'container': [], 'network': [], 'volume': []}) for project in projects.values()}
        stage['journal_proofs'] = [{**journal, 'project': projects['project'], 'purpose': gate.PURPOSE}]
        if stage['stage'] in ('postgres', 'oci'):
            key = 'pg_project' if stage['stage'] == 'postgres' else 'oci_project'
            stage['journal_proofs'].append({**journal, 'project': projects[key], 'purpose': 'native-pg-session' if key == 'pg_project' else 'native-oci-session'})
    baseline = gate.inventory_proof({'container': [], 'network': ['factory-network'], 'volume': []})
    return {'schema': 'forge/hermetic-full12-partition/v1', 'job': job, 'status': 'PARTITION_PASS',
        'inherited_gate_results': False, 'workflow_run': '123', 'workflow_attempt': '1',
        'checkout': {'controls': 'a' * 40, 'source': h.SOURCE, 'base': h.BASE}, 'components': {'fixture': 'a' * 64},
        'job_budget_seconds': gate.job_budget(job), 'source_count': 265, 'source_parity': True, 'checkout_parity': True,
        'source_hashes': {key: value['sha256'] for key, value in catalogue.items()},
        'maintenance_sha256': 'c' * 64,
        'maintenance_git': {'repository': h.m.REPOSITORY, 'commit': 'f' * 40, 'ref': h.m.CANDIDATE_REF, 'files': MAINTENANCE_TEST_FILES},
        'python_methods': {'test_postgres_delivery.py': 47, 'test_oci_offline_fixture.py': 28},
        'admission_sha256': 'b' * 64, 'execution_sha256': 'c' * 64,
        'cache': {'new_locked_cache': True, 'private_cache_imported': False,
            'journal': {**journal, 'project': 'sdlc-build-forge-cache-' + token, 'purpose': 'locked-dependency-fetch-only'}},
        'cache_preserved': True,
        **{key: {'host_min_bytes': h.HOST_BYTES, 'data_min_bytes': h.DATA_BYTES, 'minimum_inodes': h.INODES,
                 'host_free_bytes': h.HOST_BYTES, 'data_free_bytes': h.DATA_BYTES, 'free_inodes': h.INODES}
           for key in ('capacity_before', 'capacity_execution')},
        'daemon_id': 'independent-' + job,
        'daemon_versions': {'server': '29.8.2', 'client': '29.8.2', 'compose': 'Docker Compose version v5.5.1'},
        'projects': projects, 'baseline': baseline,
        'cleanup': {'complete': True, 'exact_inventory_matches_new_baseline': True, 'disposable_complete': True, 'daemon_stopped': True, 'delegation_removed': True,
            'inventory_after': baseline, 'baseline_images_preserved': True, 'own_images_preserved': True},
        'stages': stages}


class AggregationTests(unittest.TestCase):
    def check(self, modify=None):
        a, b, c = [report_fixture(job) for job in gate.JOBS]
        if modify:
            modify(a, b, c)
        with tempfile.TemporaryDirectory() as temp:
            paths = [Path(temp) / name for name in ('a.json', 'b.json', 'c.json')]
            h.atomic(paths[0], a)
            h.atomic(paths[1], b)
            h.atomic(paths[2], c)
            with patch.dict(os.environ, GITHUB_RUN_ID='123', GITHUB_RUN_ATTEMPT='1', GITHUB_SHA='a' * 40), patch.object(h.m, 'preflight', return_value={'commit': 'f' * 40, 'ref': h.m.CANDIDATE_REF, 'files': MAINTENANCE_TEST_FILES}), patch.object(h.m, 'EXPECTED_FILES', MAINTENANCE_TEST_FILES), patch.object(h, 'verify_components', return_value={'fixture': 'a' * 64}):
                return gate.aggregate(*paths, h.sha(paths[0]), h.sha(paths[1]))

    def test_other_maintenance_commit_rejected(self):
        with self.assertRaises(ValueError):
            self.check(lambda a, b, c: b['maintenance_git'].update(commit='e' * 40))

    def test_other_maintenance_ref_rejected(self):
        with self.assertRaises(ValueError):
            self.check(lambda a, b, c: b['maintenance_git'].update(ref='refs/heads/other'))

    def test_all12_combined_only(self):
        self.assertEqual(self.check()['stage_count'], 12)

    def test_missing_stage_rejected(self):
        with self.assertRaises(ValueError):
            self.check(lambda a, b, c: b['stages'].pop())

    def test_other_attempt_rejected(self):
        with self.assertRaises(ValueError):
            self.check(lambda a, b, c: a.update(workflow_attempt='2'))

    def test_unknown_cleanup_rejected(self):
        with self.assertRaises(ValueError):
            self.check(lambda a, b, c: b['cleanup'].update(complete=False))

    def test_same_daemon_rejected(self):
        with self.assertRaises(ValueError):
            self.check(lambda a, b, c: b.update(daemon_id=a['daemon_id']))

    def test_source_drift_rejected(self):
        with self.assertRaises(ValueError):
            self.check(lambda a, b, c: b['source_hashes'].update(forge='0' * 64))

    def test_inherited_result_rejected(self):
        with self.assertRaises(ValueError):
            self.check(lambda a, b, c: a.update(inherited_gate_results=True))

    def test_deadline_relaxation_rejected(self):
        with self.assertRaises(ValueError):
            self.check(lambda a, b, c: a['stages'][2].update(timeout_seconds=600))

    def test_wrong_workspace_count_rejected(self):
        with self.assertRaises(ValueError):
            self.check(lambda a, b, c: c['stages'][1].update(tests_passed=219))

    def test_skipped_gate_rejected(self):
        with self.assertRaises(ValueError):
            self.check(lambda a, b, c: b['stages'][0].update(status='SKIP'))

    def test_private_cache_import_rejected(self):
        with self.assertRaises(ValueError):
            self.check(lambda a, b, c: b['cache'].update(private_cache_imported=True))

    def test_decorative_nested_oci_journal_rejected(self):
        with self.assertRaises(ValueError):
            self.check(lambda a, b, c: c['stages'][0]['journal_proofs'].pop())

    def test_wrong_journal_daemon_rejected(self):
        with self.assertRaises(ValueError):
            self.check(lambda a, b, c: b['stages'][0]['journal_proofs'][0].update(daemon_id='wrong'))

    def test_cleanup_required_journal_rejected(self):
        with self.assertRaises(ValueError):
            self.check(lambda a, b, c: a['stages'][0]['journal_proofs'][0].update(phase='cleanup-required'))

    def test_lowered_capacity_receipt_rejected(self):
        with self.assertRaises(ValueError):
            self.check(lambda a, b, c: c['capacity_execution'].update(host_free_bytes=h.HOST_BYTES - 1))

    def test_job_budget_tampering_rejected(self):
        with self.assertRaises(ValueError):
            self.check(lambda a, b, c: a.update(job_budget_seconds=100))

    def test_third_partition_required(self):
        with self.assertRaises(ValueError):
            self.check(lambda a, b, c: c.update(status='NOT_RUN'))


if __name__ == '__main__':
    unittest.main()
