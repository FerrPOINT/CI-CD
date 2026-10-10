"""Pure deadline boundary regressions; all native operations are test doubles."""
import ast
from contextlib import contextmanager, ExitStack
import copy
import os
from pathlib import Path
import tempfile
import threading
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

import host as h
import run as gate


def git_response(argv):
    if argv[:3] != ['git', '--no-replace-objects', '-C'] or argv[4] != '--no-optional-locks':
        raise AssertionError('unexpected Git prefix')
    folder, args = Path(argv[3]).name, argv[5:]
    if args == ['rev-parse', 'HEAD']:
        return ({'controls': os.environ['GITHUB_SHA'], 'source': h.SOURCE, 'services-base': h.BASE}[folder] + '\n').encode()
    if args[0] == 'status':
        return b''
    if args == ['rev-parse', '--is-shallow-repository']:
        return b'false\n'
    if args[0] == 'rev-list':
        chain = {'HEAD': (os.environ['GITHUB_SHA'], h.CONTROLS_PARENT),
                 h.CONTROLS_PARENT: (h.CONTROLS_PARENT, h.DELEGATION_CONTROLS),
                 h.DELEGATION_CONTROLS: (h.DELEGATION_CONTROLS, h.RESOURCE_CONTROLS),
                 h.RESOURCE_CONTROLS: (h.RESOURCE_CONTROLS, h.DIAGNOSTIC_CONTROLS),
                 h.DIAGNOSTIC_CONTROLS: (h.DIAGNOSTIC_CONTROLS, h.MAINTENANCE_CONTROLS),
                 h.MAINTENANCE_CONTROLS: (h.MAINTENANCE_CONTROLS, h.SAFETY_CONTROLS),
                 h.SAFETY_CONTROLS: (h.SAFETY_CONTROLS, h.QUALIFICATION_CONTROLS),
                 h.QUALIFICATION_CONTROLS: (h.QUALIFICATION_CONTROLS, h.PUBLIC_CONTROLS),
                 h.PUBLIC_CONTROLS: (h.PUBLIC_CONTROLS, h.SOURCE)}
        return (' '.join(chain[args[-1]]) + '\n').encode()
    if args[0] == 'diff':
        return (b'M' if args[2] == h.CONTROLS_PARENT else b'A') + b'\tscripts/hermetic_full12/run.py\n'
    if args[0] == 'show':
        return (h.BASE + '\n').encode()
    if args == ['rev-parse', 'HEAD^{tree}']:
        return b'fc51650527dcd109eea2495c98417d45f5cc7f96\n'
    raise AssertionError('unexpected Git shape')


@contextmanager
def stage_fixture(assertion, close, native_bridge=None, stage='release'):
    """Exercise actual run_stage control flow without any Docker subprocess."""
    with tempfile.TemporaryDirectory() as directory, ExitStack() as stack:
        root = Path(directory)
        (root / 'private').mkdir()
        operation = SimpleNamespace(project='owned', manifest_hash='hash', command=['not-executed'],
            journal=root / 'journal.json', write_reviewed=Mock(), close=Mock(side_effect=close), record=Mock())
        bridge = native_bridge if native_bridge is not None else SimpleNamespace(lock=threading.Lock(), operations={}, closed=[])
        parent = SimpleNamespace(parent_class=lambda *_: lambda **kwargs: operation, parent_manifest=lambda *_: {})
        q = SimpleNamespace(DAEMON_ID='owned-daemon', NativeComposeBridge=lambda **kwargs: bridge,
                            serve=lambda *_: (None, None))
        receipts = []
        def command(argv, **kwargs):
            if kwargs.get('log'):
                Path(kwargs['log']).write_bytes(b'fixture output')
            return b'owned-daemon'
        replacements = [(gate, 'check_runtime_seals', Mock()), (h, 'verify_components', Mock()),
            (h, 'capacity', Mock()), (gate, 'identity', Mock()), (h, 'parity', Mock(return_value=True)),
            (gate, 'inventory', Mock(return_value={'container': [], 'network': [], 'volume': []})),
            (gate, 'manifest', Mock(return_value={})), (h, 'command', command),
            (gate, 'resource_enforcement', Mock(return_value={})),
            (gate, 'assertions', assertion), (gate, 'journal_proof', Mock(return_value={})),
            (h, 'atomic', lambda path, value, **kwargs: receipts.append(copy.deepcopy(value)))]
        for module, name, value in replacements:
            stack.enter_context(patch.object(module, name, value))
        def execute():
            return gate.run_stage(stage, root, {'project': 'owned', 'pg_project': 'pg', 'oci_project': 'oci'},
                {}, {'daemon': {'id': 'owned-daemon'}}, q, None, parent, {}, {})
        yield execute, operation, receipts


class DeadlineBoundaryTests(unittest.TestCase):
    def test_timeout_redaction_has_no_unbudgeted_filesystem_read(self):
        with patch.object(h, 'read', side_effect=AssertionError('error handler performed I/O')):
            result = h.safe_error('release', h.OverheadTimeout('private-payload', 10))
        self.assertEqual(result, {'stage': 'release', 'category': 'timeout', 'errno': None})
        self.assertEqual(h.ERROR_STAGES, gate.STAGES)

    def test_all_initial_source_proof_and_setup_enclosed_from_entry(self):
        tree = ast.parse((h.HERE / 'run.py').read_bytes())
        function = next(x for x in tree.body if isinstance(x, ast.FunctionDef) and x.name == 'run_job')
        self.assertIsInstance(function.body[0], ast.Assign)
        self.assertEqual(ast.unparse(function.body[0].targets[0]), 'h.BOOTSTRAP_DEADLINE')
        assignments = [x for x in ast.walk(function) if isinstance(x, ast.Assign)
                       and ast.unparse(x.targets[0]) == 'h.BOOTSTRAP_DEADLINE'
                       and not (isinstance(x.value, ast.Constant) and x.value.value is None)]
        self.assertEqual(len(assignments), 1, 'source preparation must not reset bootstrap deadline')
        first = next(x for x in ast.walk(function) if isinstance(x, ast.With))
        for name in ('hosted_guard', 'verify_components', 'checkout_proof'):
            call = next(x for x in ast.walk(function) if isinstance(x, ast.Call)
                        and isinstance(x.func, ast.Attribute) and x.func.attr == name)
            self.assertLess(first.lineno, call.lineno)
            self.assertLessEqual(call.lineno, first.end_lineno)

    def test_real_checkout_proof_consumes_shared_bootstrap_not_setup_slack(self):
        clock, envelopes, git_calls = [0], [], []
        @contextmanager
        def wall(seconds):
            envelopes.append(seconds)
            yield
        def command(argv, timeout=90, **kwargs):
            self.assertEqual(timeout, 90)
            clock[0] += 60
            git_calls.append(argv)
            return git_response(argv)
        with tempfile.TemporaryDirectory() as directory:
            env = {'GITHUB_WORKSPACE': directory, 'RUNNER_TEMP': directory, 'GITHUB_RUN_ID': '123',
                   'GITHUB_RUN_ATTEMPT': '1', 'GITHUB_SHA': 'a' * 40}
            with patch.dict(os.environ, env), patch.object(h.m, 'preflight'), patch.object(h.m, 'read_payloads', return_value=({}, {})), patch.object(h, 'hosted_guard'), patch.object(h, 'verify_components', return_value={}), patch.object(h, 'wall_budget', side_effect=wall), patch.object(h.time, 'monotonic', side_effect=lambda: clock[0]), patch.object(h, 'command', side_effect=command), patch.object(h, 'reclaim', side_effect=h.CapacityFailure({'host_free_bytes': 0})), patch.object(gate, 'start_daemon') as start, patch.object(gate, 'stop_daemon', return_value={'stopped': True}):
                self.assertEqual(gate.run_job('C'), 1)
            self.assertEqual(envelopes[:2], [5400, 4200])  # Twenty successful Git calls consume1200.
            self.assertEqual(len(git_calls), 40)  # Initial + final parity proof, private qualification separately tested.
            start.assert_not_called()
            self.assertIsNone(h.BOOTSTRAP_DEADLINE)

    def test_expired_assertions_get_fresh_teardown_alarm_and_never_pass(self):
        envelopes, active = [], [None]
        @contextmanager
        def wall(seconds):
            self.assertIsNone(active[0])
            active[0] = seconds
            envelopes.append(seconds)
            try:
                yield
            finally:
                active[0] = None
        def assertion(*_):
            self.assertEqual(active[0], gate.ASSERT_SECONDS)
            active[0] = None  # Model a consumed one-shot assertion alarm.
            raise h.OverheadTimeout('assertions', gate.ASSERT_SECONDS)
        def close():
            self.assertEqual(active[0], gate.EXIT_SECONDS)
        with patch.object(h, 'wall_budget', side_effect=wall), stage_fixture(assertion, close) as (execute, operation, receipts):
            with self.assertRaises(h.OverheadTimeout):
                execute()
        self.assertEqual(envelopes, [gate.ENTRY_SECONDS, gate.ASSERT_SECONDS, gate.EXIT_SECONDS])
        operation.close.assert_called_once_with()
        self.assertEqual(receipts[-1]['status'], 'FAIL')
        self.assertEqual(receipts[-1]['error']['category'], 'timeout')

    def test_cache_failure_does_not_close_inside_consumed_bootstrap_alarm(self):
        operation = SimpleNamespace(write=Mock(), close=Mock(), command=['not-executed'])
        parent = SimpleNamespace(parent_class=lambda *_: lambda **kwargs: operation)
        pending = []
        with patch.object(h, 'command', side_effect=h.OverheadTimeout('bootstrap', 5400)):
            with self.assertRaises(h.OverheadTimeout):
                gate.fill_cache(Path('/fixture'), None, SimpleNamespace(DAEMON_ID='admitted'), parent,
                    {'tools': 'sha256:' + 'a' * 64}, 'a' * 20, pending)
        operation.close.assert_not_called()
        self.assertEqual(pending, [operation])

    def test_assertion_reserve_added_without_reducing_teardown_or_native(self):
        self.assertEqual((gate.ASSERT_SECONDS, gate.EXIT_SECONDS), (10, 500))
        old = {'A': 20182, 'B': 14340, 'C': 21510}
        for job in gate.JOBS:
            self.assertEqual(gate.job_budget(job), old[job] + len(gate.JOBS[job]) * 10)
            self.assertLess(gate.job_budget(job), 21600)
        coverage = h.read(h.HERE / 'coverage.json')['orchestration_budget']
        self.assertEqual(coverage['total_job_seconds'], {job: gate.job_budget(job) for job in gate.JOBS})
        self.assertEqual(coverage['assertions_seconds'], gate.ASSERT_SECONDS)


if __name__ == '__main__':
    unittest.main()
