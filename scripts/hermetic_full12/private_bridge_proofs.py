"""Explicit private-dependent Linux proofs; missing pin is a failure, never a skipped acceptance."""
from contextlib import contextmanager
import hashlib
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
from types import ModuleType, SimpleNamespace
import unittest
from unittest.mock import Mock, patch

import host as h
import run as gate
from test_deadline_boundaries import stage_fixture


def qualified_modules():
    # Private-dependent proof; no installed packet or unqualified local byte fallback.
    proof, payloads = h.m.read_payloads(h.HERE.parents[2] / 'services-base', h.git)
    source = h.HERE.parent / 'native_qa_compose.py'
    raw = source.read_bytes().replace(b'\r\n', b'\n')
    entry = next(item for item in h.read(h.HERE / 'source-catalogue.json')['catalogues']['forge']['files']
                 if item['path'] == 'scripts/native_qa_compose.py')
    h.require(hashlib.sha256(raw).hexdigest() == entry['sha256'])
    q = ModuleType('deadline_product_native')
    q.__file__ = str(source)
    exec(compile(raw, str(source), 'exec'), q.__dict__)
    h.bind_maintenance(q, proof)
    sdk_raw = payloads['scripts/compose_helpers.py']
    h.require(hashlib.sha256(sdk_raw).hexdigest() == q.SDK_SHA256)
    sdk = ModuleType('deadline_actual_maintenance')
    exec(compile(sdk_raw, 'exact_maintenance_sdk', 'exec'), sdk.__dict__)
    return q, sdk


@contextmanager
def native_fixture(count, checked):
    q, sdk = qualified_modules()
    sdk.checked = checked
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        with patch.dict(os.environ, {'SDLC_RESOURCE_REGISTRY': str(root / 'registry')}):
            bridge = q.NativeComposeBridge(sdk=sdk, children=root / 'children', pg_project='pg',
                                          oci_project='oci', registry=root / 'registry')
        sessions = []
        for index in range(count):
            cls = q.session_class(sdk)
            session = cls.__new__(cls)
            session.kind = 'oci'
            session.path = root / ('compose-' + str(index) + '.json')
            session.path.touch()
            session.journal = root / ('journal-' + str(index) + '.json')
            session.command = ['owned-compose', str(index)]
            session.definitions = {'volume': {}, 'network': {}}
            # External daemon/journal I/O only; close's SDK control flow is unchanged.
            session.check_endpoint = Mock()
            session.check_ownership = Mock()
            session.resources = Mock(return_value=[])
            session.record = Mock()
            bridge.operations['oci-' + str(index)] = session
            sessions.append(session)
        yield bridge, sessions


class ActualCleanupFailureTests(unittest.TestCase):
    def test_regular_cleanup_failure_preserves_other_owned_sessions_and_fails(self):
        def checked(argv):
            if argv[1] == '0':
                raise RuntimeError('private-payload')
        with native_fixture(2, checked) as (bridge, sessions):
            with self.assertRaises(RuntimeError):
                gate.close_native_bridge(bridge)
            self.assertEqual(set(bridge.operations), {'oci-0'})
            self.assertEqual(bridge.closed, [str(sessions[1].journal)])
            sessions[0].record.assert_called_once_with('cleanup-required')
            sessions[1].record.assert_called_once_with('cleaned')


class ActualResourcePolicyTests(unittest.TestCase):
    def test_actual_sdk_local_default_refuses_five_before_effects(self):
        _, sdk = qualified_modules()
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, SDLC_MIN_FREE_GIB='5'), patch.object(sdk, 'checked') as checked:
            root = Path(directory) / 'session'
            with self.assertRaises(ValueError):
                sdk.ComposeHelper(project='sdlc-qa-forge-policy-local', task=gate.TASK, purpose='pure-policy',
                                  docker=h.DOCKER, directory=root)
            self.assertFalse(root.exists())
            checked.assert_not_called()

    def test_actual_sdk_local_default_refuses_twenty_nine(self):
        _, sdk = qualified_modules()
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, SDLC_MIN_FREE_GIB='29'), patch.object(sdk, 'checked') as checked:
            with self.assertRaises(ValueError):
                sdk.ComposeHelper(project='sdlc-qa-forge-policy-local', task=gate.TASK, purpose='pure-policy',
                                  docker=h.DOCKER, directory=Path(directory) / 'session')
            checked.assert_not_called()

    def test_actual_native_constructor_ci_policy_and_v2_journal_with_fake_engine(self):
        q, sdk = qualified_modules()
        q.DAEMON_ID = 'policy-fixture-daemon'
        gate.install_native_ci_policy(q)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            env = {'SDLC_MIN_FREE_GIB': '5', 'SDLC_RESOURCE_REGISTRY': str(root / 'registry')}
            def checked(argv, **kwargs):
                if argv == h.DOCKER + ['info', '--format', '{{.ID}}']:
                    return q.DAEMON_ID
                if argv[:4] == h.DOCKER + ['container'] or argv[:4] == h.DOCKER + ['network'] or argv[:4] == h.DOCKER + ['volume']:
                    self.assertEqual(argv[4:6], ['ls', '-aq'] if argv[3] == 'container' else ['ls', '-q'])
                    self.assertEqual(argv[6:], ['--filter', 'label=com.docker.compose.project=sdlc-qa-forge-policy-ci'])
                    return ''
                raise AssertionError('unexpected fake engine command')
            with patch.dict(os.environ, env), patch.object(h, 'hosted_guard') as guard, patch.object(sdk, 'checked', side_effect=checked), patch.object(sdk.shutil, 'disk_usage', return_value=SimpleNamespace(free=5 * 2**30)):
                operation = q.session_class(sdk)(kind='oci', source_root=root / 'sources',
                    project='sdlc-qa-forge-policy-ci', task=gate.TASK, docker=h.DOCKER,
                    directory=root / 'session', daemon_id=q.DAEMON_ID)
            guard.assert_called_once_with()
            journal = h.read(operation.journal)
            self.assertEqual(journal['version'], 2)
            self.assertEqual(journal['phase'], 'prepared')
            self.assertEqual(journal['sdk_sha256'], h.m.EXPECTED_FILES['scripts/compose_helpers.py']['sha256'])
            self.assertEqual(journal['daemon_id'], q.DAEMON_ID)
            self.assertEqual(journal['task'], gate.TASK)
            self.assertTrue((root / 'registry').is_dir())


@unittest.skipUnless(sys.platform == 'linux', 'actual Linux timers required')
class ActualBridgeDeadlineTests(unittest.TestCase):
    def exercise(self, count, cut, swallowed=False):
        calls = []
        parent_close = Mock(side_effect=lambda: time.sleep(0.25))
        def checked(argv):
            calls.append(int(argv[1]))
            if int(argv[1]) == cut:
                try:
                    time.sleep(1)
                except h.OverheadTimeout:
                    if swallowed:
                        raise RuntimeError('converted-by-boundary') from None
                    raise
        with native_fixture(count, checked) as (bridge, sessions):
            sessions[cut].record.side_effect = lambda *_: time.sleep(0.25)
            with patch.object(gate, 'EXIT_SECONDS', 0.12), stage_fixture(
                    lambda *_: {}, parent_close, native_bridge=bridge, stage='oci') as (execute, operation, receipts):
                begin = time.monotonic()
                with self.assertRaises(h.OverheadTimeout):
                    execute()
                elapsed = time.monotonic() - begin
            self.assertLess(elapsed, 0.30, 'continued cleanup after the120ms phase deadline')
            self.assertEqual(calls, list(range(cut + 1)))
            parent_close.assert_not_called()
            operation.close.assert_not_called()
            self.assertEqual(bridge.closed, [str(item.journal) for item in sessions[:cut]])
            self.assertEqual(set(bridge.operations), {'oci-' + str(index) for index in range(cut, count)})
            sessions[cut].record.assert_not_called()  # Preserve the prior journal, never start I/O after expiry.
            self.assertFalse(any(proof.get('cleanup_complete') or proof.get('status') == 'PASS' for proof in receipts))
            self.assertEqual(signal.getitimer(signal.ITIMER_REAL), (0, 0))
            self.assertIsNone(h.PHASE_DEADLINE)
            print('BRIDGE_DEADLINE_CUT count={} cut={} converted={} elapsed_ms={:.2f} parent_calls=0 remaining={}'.format(
                count, cut, int(swallowed), elapsed * 1000, len(bridge.operations)), flush=True)

    def test_one_actual_oci_cleanup_alarm_stops_before_parent(self):
        self.exercise(1, 0)

    def test_multiple_actual_oci_first_cut_leaves_later_sessions_unstarted(self):
        self.exercise(3, 0)

    def test_multiple_actual_oci_second_cut_preserves_completed_first_only(self):
        self.exercise(3, 1)

    def test_converted_alarm_still_stops_before_next_oci_or_parent(self):
        self.exercise(3, 0, swallowed=True)

    def test_swallowed_one_shot_cannot_exit_phase_successfully(self):
        begin = time.monotonic()
        with self.assertRaises(h.OverheadTimeout):
            with h.wall_budget(0.05):
                try:
                    time.sleep(1)
                except h.OverheadTimeout:
                    pass
        self.assertLess(time.monotonic() - begin, 0.20)
        self.assertIsNone(h.PHASE_DEADLINE)

    def test_no_signal_delivery_still_checks_absolute_context_exit(self):
        with self.assertRaises(h.OverheadTimeout):
            with h.wall_budget(0.02):
                signal.setitimer(signal.ITIMER_REAL, 0)
                time.sleep(0.04)
        self.assertIsNone(h.PHASE_DEADLINE)

    def test_actual_sdk_close_command_is_killed_and_next_session_not_started(self):
        created, calls = [], []
        popen = subprocess.Popen
        def capture(*args, **kwargs):
            process = popen(*args, **kwargs)
            created.append(process)
            return process
        def checked(argv):
            calls.append(int(argv[1]))
            return h.command([sys.executable, '-c', 'import time; time.sleep(3)'], timeout=3)
        with native_fixture(2, checked) as (bridge, _):
            with patch.object(h.subprocess, 'Popen', side_effect=capture):
                with self.assertRaises(h.OverheadTimeout):
                    with h.wall_budget(0.12):
                        gate.close_native_bridge(bridge)
            self.assertEqual(calls, [0])
            self.assertEqual(len(created), 1)
            self.assertIsNotNone(created[0].returncode)
            self.assertFalse(h.live_group(created[0]))
            self.assertEqual(len(bridge.operations), 2)
            self.assertEqual(bridge.closed, [])


if __name__ == '__main__':
    h.m.preflight()
    unittest.main()
