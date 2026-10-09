"""Bounded Linux subprocess regressions; no Docker, WSL, Cargo or hosted calls."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

import host
import run as gate
from test_deadline_boundaries import git_response, stage_fixture


@unittest.skipUnless(sys.platform == 'linux', 'actual Linux process groups required')
class ProcessGroupTests(unittest.TestCase):
    def exercise(self, code, expected=None, log=True, timeout=3, native=False):
        created = []
        original = subprocess.Popen
        original_kill = os.killpg
        pinned_at_signal = []

        def capture(*args, **kwargs):
            process = original(*args, **kwargs)
            created.append(process)
            return process

        def checked_kill(pid, sig):
            process = created[0]
            self.assertIsNone(process.returncode)
            info = os.waitid(os.P_PID, pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
            self.assertEqual(os.getpgid(pid), pid)
            self.assertEqual(os.getsid(pid), pid)
            pinned_at_signal.append(info is not None)
            original_kill(pid, sig)

        with tempfile.TemporaryDirectory(prefix='forge-owned-process-') as directory:
            root = Path(directory)
            child = 'import signal,time; signal.signal(signal.SIGTERM,signal.SIG_IGN); time.sleep(30)'
            prefix = ('import json,os,pathlib,subprocess,sys,time; '
                      'p=subprocess.Popen([sys.executable,"-c",sys.argv[2]]); '
                      'pathlib.Path(sys.argv[1]).write_text(json.dumps({"child":p.pid,"group":os.getpgrp()})); ')
            try:
                with patch.object(host.subprocess, 'Popen', capture), patch.object(host.os, 'killpg', checked_kill):
                    argv = [sys.executable, '-c', prefix + code, str(root / 'owned.json'), child]
                    def execute():
                        if not native:
                            return host.command(argv, timeout=timeout, log=root / 'private.log' if log else None)
                        q = host.load('native_request_group_regression', host.HERE.parent / 'native_qa_compose.py')
                        sdk = type('SDK', (), {})()
                        gate.install_process_adapter(sdk, q)
                        request = q.RequestLease(None)
                        token = q.REQUEST.set(request)
                        try:
                            with patch.object(request, 'checked', side_effect=AssertionError('reaping path used')):
                                return sdk.checked(argv, timeout=timeout)
                        finally:
                            q.REQUEST.reset(token)
                    if expected is None:
                        execute()
                    else:
                        with self.assertRaises(expected):
                            execute()
                self.assertEqual(len(created), 1)
                process = created[0]
                self.assertIsNotNone(process.returncode)
                record = json.loads((root / 'owned.json').read_text())
                self.assertEqual(record['group'], process.pid)
                self.assertFalse(host.live_group(process), 'returned with a live command-group descendant')
                self.assertTrue(pinned_at_signal)
                return pinned_at_signal
            finally:
                # Only an unreaped owned leader may authorize a test-emergency group signal.
                for process in created:
                    if process.returncode is None:
                        os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
                        if os.getpgid(process.pid) == process.pid and os.getsid(process.pid) == process.pid:
                            original_kill(process.pid, signal.SIGKILL)
                        process.wait(timeout=5)

    def test_successful_exited_leader_with_log_and_live_child(self):
        self.assertTrue(self.exercise('sys.exit(0)')[0])

    def test_failed_exited_leader_with_live_child(self):
        self.assertTrue(self.exercise('sys.exit(7)', host.CommandFailure)[0])

    def test_exited_leader_with_child_held_pipes(self):
        self.assertTrue(self.exercise('sys.exit(0)', log=False)[0])

    def test_timeout_with_live_leader_and_child_held_pipes(self):
        self.exercise('time.sleep(30)', subprocess.TimeoutExpired, log=False, timeout=0.5)

    def test_native_request_nonzero_leader_with_held_pipes(self):
        self.assertTrue(self.exercise('sys.exit(7)', host.CommandFailure, log=False, native=True)[0])

    def test_native_request_timeout_cleans_owned_group(self):
        self.exercise('time.sleep(30)', subprocess.TimeoutExpired, log=False, timeout=0.5, native=True)

    def test_overhead_alarm_kills_group_without_shortening_native_timeout(self):
        with self.assertRaises(host.OverheadTimeout):
            with host.wall_budget(0.5):
                self.exercise('time.sleep(30)', host.OverheadTimeout, log=False, timeout=30)

    def test_initial_real_checkout_proof_is_inside_actual_entry_alarm(self):
        calls = []
        def command(argv, **kwargs):
            calls.append(argv)
            self.assertGreater(signal.getitimer(signal.ITIMER_REAL)[0], 0)
            time.sleep(0.04)
            return git_response(argv)
        with tempfile.TemporaryDirectory() as directory:
            env = {'GITHUB_WORKSPACE': directory, 'RUNNER_TEMP': directory, 'GITHUB_RUN_ID': '123',
                'GITHUB_RUN_ATTEMPT': '1', 'GITHUB_SHA': 'a' * 40}
            with patch.dict(os.environ, env), patch.object(host.m, 'preflight'), patch.object(gate, 'BOOTSTRAP_SECONDS', 0.12), patch.object(gate, 'job_budget', return_value=1), patch.object(host, 'hosted_guard'), patch.object(host, 'verify_components', return_value={}), patch.object(host, 'command', side_effect=command), patch.object(host, 'reclaim') as reclaim, patch.object(gate, 'start_daemon') as start:
                with self.assertRaises(host.OverheadTimeout):
                    gate.run_job('C')
            self.assertGreater(len(calls), 0)
            self.assertLess(len(calls), 11)
            self.assertIsNone(host.BOOTSTRAP_DEADLINE)
            reclaim.assert_not_called()
            start.assert_not_called()
            self.assertFalse(list(Path(directory).iterdir()))

    def test_consumed_assertion_alarm_rearms_real_teardown_and_preserves_failure(self):
        timers = []
        def assertion(*_):
            time.sleep(1)
        def close():
            timers.append(signal.getitimer(signal.ITIMER_REAL)[0])
        with patch.object(gate, 'ASSERT_SECONDS', 0.05), patch.object(gate, 'EXIT_SECONDS', 0.2), stage_fixture(assertion, close) as (execute, operation, receipts):
            with self.assertRaises(host.OverheadTimeout):
                execute()
        self.assertEqual(len(timers), 1)
        self.assertGreater(timers[0], 0)
        self.assertEqual(receipts[-1]['status'], 'FAIL')
        self.assertTrue(receipts[-1]['cleanup_complete'])
        self.assertEqual(signal.getitimer(signal.ITIMER_REAL), (0, 0))

    def test_teardown_after_expired_assertions_is_itself_bounded(self):
        timers = []
        def assertion(*_):
            time.sleep(1)
        def close():
            timers.append(signal.getitimer(signal.ITIMER_REAL)[0])
            time.sleep(1)
        begin = time.monotonic()
        with patch.object(gate, 'ASSERT_SECONDS', 0.05), patch.object(gate, 'EXIT_SECONDS', 0.05), stage_fixture(assertion, close) as (execute, operation, receipts):
            with self.assertRaises(host.OverheadTimeout) as error:
                execute()
        self.assertEqual(error.exception.timeout, 0.05)
        self.assertGreater(timers[0], 0)
        self.assertLess(time.monotonic() - begin, 0.8)
        self.assertFalse(any(value.get('status') == 'PASS' for value in receipts))
        self.assertEqual(signal.getitimer(signal.ITIMER_REAL), (0, 0))


if __name__ == '__main__':
    unittest.main()
