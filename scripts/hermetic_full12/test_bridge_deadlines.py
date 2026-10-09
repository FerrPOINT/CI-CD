"""Public pure/bounded timer checks; no private SDK source embedded."""
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


class AbsoluteDeadlineTests(unittest.TestCase):
    def test_expired_clock_rejects_next_bridge_session_without_alarm(self):
        clock = [0]
        deadline = h.PhaseDeadline(1)
        deadline.expires = 1
        first = SimpleNamespace(journal='first', close=Mock(side_effect=lambda: clock.__setitem__(0, 2)), record=Mock())
        later = SimpleNamespace(journal='later', close=Mock(), record=Mock())
        import threading
        bridge = SimpleNamespace(lock=threading.Lock(), operations={'first': first, 'later': later}, closed=[])
        with patch.object(h, 'PHASE_DEADLINE', deadline), patch.object(h.time, 'monotonic', side_effect=lambda: clock[0]):
            with self.assertRaises(h.OverheadTimeout):
                gate.close_native_bridge(bridge)
        later.close.assert_not_called()
        self.assertEqual(bridge.closed, [])
        self.assertEqual(set(bridge.operations), {'first', 'later'})

    def test_expired_phase_never_spawns_another_sdk_command(self):
        deadline = h.PhaseDeadline(1)
        deadline.expired = True
        with patch.object(h, 'PHASE_DEADLINE', deadline), patch.object(h.subprocess, 'Popen') as spawn:
            with self.assertRaises(h.OverheadTimeout):
                h.command(['not-started'], timeout=90)
        spawn.assert_not_called()

    def test_sdk_failure_record_after_expiry_is_not_started_and_method_restored(self):
        clock = [0]
        deadline = h.PhaseDeadline(1)
        deadline.expires = 1
        record = Mock()
        operation = SimpleNamespace(record=record)
        def close():
            clock[0] = 2
            operation.record('cleanup-required')
        operation.close = close
        with patch.object(h, 'PHASE_DEADLINE', deadline), patch.object(h.time, 'monotonic', side_effect=lambda: clock[0]):
            with self.assertRaises(h.OverheadTimeout):
                gate.close_operation(operation)
        record.assert_not_called()
        self.assertIs(operation.record, record)

    def test_command_clamped_to_absolute_phase_not_only_bootstrap(self):
        import ast
        tree = ast.parse((h.HERE / 'host.py').read_bytes())
        command = next(node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == 'command')
        self.assertLess(ast.unparse(command).index('PHASE_DEADLINE.expires'), ast.unparse(command).index('subprocess.Popen'))
        self.assertIsNone(h.BOOTSTRAP_DEADLINE)



@unittest.skipUnless(sys.platform == 'linux', 'actual Linux timers required')
class ActualTimerTests(unittest.TestCase):
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



if __name__ == '__main__':
    unittest.main()
