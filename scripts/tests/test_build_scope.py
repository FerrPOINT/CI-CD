"""Scoped raw-container selection; all producer/verifier transports are inert."""
import contextlib
import io
import json
import os
from pathlib import Path
import runpy
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from scripts import select_build

ROOT = Path(__file__).resolve().parents[2]


class BuildScopeTests(unittest.TestCase):
    def invoke(self, argv, selected=(), pins=None, *, ui_override=False, rejected=None, fail_at=None):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            script = root / 'scripts/build.py'
            script.parent.mkdir()
            shutil.copyfile(ROOT / 'scripts/build.py', script)
            for name, value in (pins or {}).items():
                (root / name).write_text(value)
            previous = sys.path[:]
            try:
                with patch.object(sys, 'argv', [str(script), *argv]), \
                     patch.object(select_build, 'auto_services', return_value=list(selected)) as choose, \
                     patch('subprocess.run') as calls, contextlib.redirect_stdout(io.StringIO()), \
                     patch.dict(os.environ, {'BASE_UI_SOURCE_DIR': str(root / 'own-ui') if ui_override else ''}):
                    if fail_at:
                        calls.side_effect = [None] * (fail_at - 1) + [subprocess.CalledProcessError(1, 'verifier')]
                        with self.assertRaises(subprocess.CalledProcessError):
                            runpy.run_path(str(script), run_name='__main__')
                        self.assertEqual(calls.call_count, fail_at)
                        self.assertFalse(any(call.args[0][0] == 'docker' for call in calls.call_args_list))
                    elif rejected:
                        with self.assertRaisesRegex((SystemExit, ValueError), rejected):
                            runpy.run_path(str(script), run_name='__main__')
                        self.assertEqual(calls.call_count, 0)
                    else:
                        runpy.run_path(str(script), run_name='__main__')
                    return calls.call_args_list, choose.call_count, root
            finally:
                sys.path[:] = previous

    def test_automatic_no_change_stops_before_verifier_or_docker(self):
        calls, chosen, _ = self.invoke([])
        self.assertEqual(calls, [])
        self.assertEqual(chosen, 1)
        resolve = Path.resolve
        def deny_ui_resolution(path, *args, **kwargs):
            if path.name == 'own-ui':
                raise AssertionError('No selected producer must not resolve a UI source context')
            return resolve(path, *args, **kwargs)
        with patch.object(Path, 'resolve', deny_ui_resolution):
            calls, _, _ = self.invoke([], ui_override=True)
        self.assertEqual(calls, [])

    def test_selected_service_calls_only_its_verifier_and_producer(self):
        pins = {'.base-ui-revision': '1' * 40, '.namespace-base-revision': '2' * 40}
        for target, pin in [('frontend', '.base-ui-revision'), ('backend', '.namespace-base-revision')]:
            with self.subTest(target=target):
                calls, chosen, _ = self.invoke([], [target], pins)
                self.assertEqual(chosen, 1)
                self.assertEqual(len(calls), 2)
                self.assertTrue(calls[0].args[0][-1].endswith(pin))
                self.assertEqual(calls[1].args[0], ['docker', 'compose', 'build', target])

    def test_explicit_all_uses_both_services_and_legacy_pin(self):
        calls, chosen, _ = self.invoke(['--all'], pins={'.base-revision': '3' * 40})
        self.assertEqual(chosen, 0)
        self.assertEqual(len(calls), 2)
        self.assertEqual(calls[1].args[0], ['docker', 'compose', 'build', *select_build.SERVICES])
        self.assertTrue(calls[0].args[0][-1].endswith('.base-revision'))

    def test_invalid_duplicate_or_mixed_pin_selection_refuses_before_preparation(self):
        for argv in (['foreign'], ['frontend', 'frontend'], ['--all', 'frontend']):
            self.invoke(argv, rejected='Choose')
        pins = {'.base-ui-revision': '1' * 40, '.namespace-base-revision': '2' * 40}
        self.invoke(['--all'], pins=pins, rejected='Mixed SDK/UI pins')
        self.invoke([], ['backend', 'frontend'], pins, rejected='Mixed SDK/UI pins')

    def test_explicit_frontend_retains_its_own_source_path_when_supported(self):
        calls, chosen, root = self.invoke(['frontend'], pins={'.base-ui-revision': '1' * 40}, ui_override=True)
        self.assertEqual(chosen, 0)
        supports_override = 'BASE_UI_SOURCE_DIR' in (ROOT / 'scripts/build.py').read_text()
        base = root / 'own-ui' if supports_override else root.parent / 'services-base'
        self.assertEqual(calls[0].args[0][calls[0].args[0].index('--base') + 1], str(base))
        self.assertEqual(calls[0].args[0][-1], str(root / '.base-ui-revision'))

    def test_auto_services_loads_the_manifest_once_and_preserves_comparison_failure(self):
        manifest = {'container_inputs': {name: [name + '/Dockerfile*'] for name in select_build.SERVICES}}
        with patch.object(select_build.selector, 'load', return_value=manifest) as load, \
             patch.object(select_build.selector, 'plan', return_value={'gates': [], 'changed': []}) as plan:
            self.assertEqual(select_build.auto_services(ROOT), [])
            load.assert_called_once_with(ROOT)
            plan.assert_called_once_with(ROOT, profile='build', base='HEAD', manifest=manifest)
        with patch.object(select_build.selector, 'load', return_value=manifest), \
             patch.object(select_build.selector, 'plan', side_effect=ValueError('bad comparison')):
            with self.assertRaisesRegex(ValueError, 'bad comparison'):
                select_build.auto_services(ROOT)

    def test_malformed_map_or_unknown_artifact_requires_review(self):
        for inputs in (None, {}, {'backend': [], 'frontend': ['x']}, {'backend': 'x', 'frontend': ['y']}):
            with self.subTest(inputs=inputs), self.assertRaises(ValueError):
                select_build.build_services({'gates': [], 'changed': []}, {'container_inputs': inputs})
        with self.assertRaises(ValueError):
            select_build.build_services({'gates': [{'id': 'new:build', 'kind': 'artifact'}], 'changed': []},
                                        {'container_inputs': {name: [name + '/Dockerfile*'] for name in select_build.SERVICES}})

    def test_named_ui_context_verifies_both_pins_or_refuses_without_one(self):
        pins = {'.base-ui-revision': '1' * 40, '.namespace-base-revision': '2' * 40}
        if 'BASE_UI_SOURCE_DIR' not in (ROOT / 'scripts/build.py').read_text():
            self.invoke(['--all'], pins=pins, ui_override=True, rejected='Mixed SDK/UI pins')
            return
        calls, _, root = self.invoke(['--all'], pins=pins, ui_override=True)
        self.assertEqual(len(calls), 3)
        self.assertEqual(calls[0].args[0][-1], str(root / '.namespace-base-revision'))
        self.assertEqual(calls[1].args[0][-1], str(root / '.base-ui-revision'))
        self.assertIn(str(root / 'own-ui'), calls[1].args[0])
        self.assertTrue(calls[0].kwargs['check'] and calls[1].kwargs['check'])
        self.assertEqual(calls[2].args[0], ['docker', 'compose', 'build', *select_build.SERVICES])
        self.invoke(['--all'], pins={'.base-revision': '3' * 40}, ui_override=True,
                    rejected='requires an independent UI pin')

    def test_changed_helpers_have_a_source_owner_without_application_producers(self):
        manifest = select_build.selector.load(ROOT)
        owner = 'tooling:python-helpers' if any(g['id'] == 'tooling:python-helpers' for g in manifest['gates']) else 'tooling:quality-routes'
        gate = next(g for g in manifest['gates'] if g['id'] == owner)
        self.assertIn('scripts.tests.test_build_scope', gate['argv'])
        self.assertIn('scripts.tests.test_build_scope', (ROOT / '.github/workflows/ci.yml').read_text())
        with patch.object(select_build.selector, 'git', return_value='a' * 40):
            for path in ('scripts/build.py', 'scripts/select_build.py', 'scripts/tests/test_build_scope.py'):
                candidate = select_build.selector.plan(ROOT, profile='build', changed=[path], manifest=manifest)
                self.assertEqual([g['id'] for g in candidate['gates']], [owner])
                self.assertFalse(any(g.get('kind') == 'artifact' for g in candidate['gates']))
                self.assertTrue(select_build.selector.matches(path, gate['inputs']))

    def test_a_failed_selected_verifier_cannot_start_a_build(self):
        self.invoke(['frontend'], pins={'.base-ui-revision': '1' * 40}, fail_at=1)
        if 'BASE_UI_SOURCE_DIR' in (ROOT / 'scripts/build.py').read_text():
            self.invoke(['--all'], pins={'.base-ui-revision': '1' * 40, '.namespace-base-revision': '2' * 40},
                        ui_override=True, fail_at=2)

    def test_application_and_non_container_artifacts_use_the_product_service_graph(self):
        manifest = select_build.selector.load(ROOT)
        for gate in set(select_build.SERVICE_GATES.values()):
            expected = [name for name in select_build.SERVICES if select_build.SERVICE_GATES[name] == gate]
            self.assertEqual(select_build.build_services({'gates': [{'id': gate, 'kind': 'artifact'}], 'changed': []}, manifest), expected)
        for gate in select_build.NON_CONTAINER_GATES:
            self.assertEqual(select_build.build_services({'gates': [{'id': gate, 'kind': 'artifact'}], 'changed': []}, manifest), [])
