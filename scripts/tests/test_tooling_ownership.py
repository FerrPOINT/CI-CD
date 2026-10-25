"""Known helper test edits cannot select application producers."""
import shlex
import unittest
from unittest.mock import patch
from pathlib import Path
from scripts.vendor import base_quality_selection as selection

ROOT = Path(__file__).resolve().parents[2]
TEST_GATES = {'test_build_pin': 'tooling:python-helpers', 'test_forge_backup_umbrella': 'tooling:python-helpers', 'test_frontend_toolchain': 'tooling:python-helpers', 'test_observability_contract': 'tooling:python-helpers', 'test_oci_offline_fixture': 'tooling:python-helpers', 'test_postgres_delivery': 'tooling:python-helpers', 'test_postgres_rejection': 'tooling:python-helpers', 'test_tls_profile': 'tooling:python-helpers', 'test_umbrella_operations_docs': 'tooling:python-helpers', 'test_verify_docs': 'tooling:python-helpers', 'test_tooling_ownership': 'tooling:python-helpers'}


class PythonToolingOwnershipTests(unittest.TestCase):
    def test_css_scanner_fixture_is_a_unit_input_and_uses_the_ordinary_entrypoint(self):
        import json
        manifest = selection.load(ROOT)
        path = 'frontend/scripts/tailwind-source-inputs.test.mjs'
        build = next(g for g in manifest['gates'] if g['id'] == 'frontend:build')
        self.assertTrue(selection.matches(path, manifest['frontend_unit_inputs']))
        self.assertTrue(selection.matches(path, build['exclude_inputs']))
        package = json.loads((ROOT / 'frontend/package.json').read_text(encoding='utf-8'))
        self.assertTrue(package['scripts']['test:unit'].startswith(
            'node --test scripts/tailwind-source-inputs.test.mjs && vitest run '))
        self.assertEqual(package['scripts']['test:all'],
            'node --test scripts/tailwind-source-inputs.test.mjs && vitest run')

    def test_css_scanner_fixture_does_not_rebuild_the_application(self):
        path = 'frontend/scripts/tailwind-source-inputs.test.mjs'
        for profile in ('fast', 'unit', 'build'):
            with self.subTest(profile=profile):
                candidate = selection.plan(ROOT, profile=profile, changed=[path])
                self.assertIn('frontend:tests', [g['id'] for g in candidate['gates']])
                self.assertFalse(any(g.get('kind') == 'artifact' for g in candidate['gates']))
        mixed = selection.plan(ROOT, profile='build', changed=[path, 'frontend/src/index.css'])
        self.assertIn('frontend:build', [g['id'] for g in mixed['gates']])
        self.assertNotIn('backend', mixed['components'])

    def test_ci_css_fixture_source_preserves_actual_application_build_inputs(self):
        path = 'frontend/scripts/tailwind-source-inputs.test.mjs'
        for changed, build in ((path+'\n', 'false'), (path+'\nfrontend/src/index.css\n', 'true')):
            with self.subTest(build=build), patch.object(selection, 'git', return_value=changed):
                outputs = selection.ci_outputs('push', 'a'*40, 'b'*40, repo=ROOT)
            self.assertEqual(outputs, {'backend':'false', 'frontend':'true',
                                       'backend_build':'false', 'frontend_build':build})

    @classmethod
    def setUpClass(cls):
        head = selection.git(ROOT, 'rev-parse', 'HEAD')
        def current_head(repository, *arguments):
            if repository != ROOT or arguments != ('rev-parse', 'HEAD'):
                raise AssertionError('This pure path-selection fixture only supplies the current HEAD')
            return head
        cls.head_reader = patch.object(selection, 'git', side_effect=current_head)
        cls.addClassCleanup(cls.head_reader.stop)
        cls.head_reader.start()

    def test_known_tests_select_exact_lightweight_gate(self):
        manifest = selection.load(ROOT)
        for profile in ('fast', 'unit', 'build'):
            for name, expected in TEST_GATES.items():
                with self.subTest(profile=profile, name=name):
                    candidate = selection.plan(ROOT, profile=profile,
                        changed=['scripts/tests/' + name + '.py'], manifest=manifest)
                    self.assertEqual(candidate['components'], ['tooling'])
                    self.assertEqual([g['id'] for g in candidate['gates']], [expected])
                    gate = candidate['gates'][0]
                    self.assertEqual(gate['argv'][0], '{python}')
                    self.assertFalse(gate.get('kind') == 'artifact')
                    self.assertIn('scripts.tests.' + name, gate['argv'])

    def test_posix_backup_wrapper_selects_one_linux_unit(self):
        for profile in ('fast', 'unit', 'build'):
            with self.subTest(profile=profile):
                candidate = selection.plan(ROOT, profile=profile,
                    changed=['scripts/tests/test_base_stand_backup.py'])
                self.assertEqual(candidate['components'], ['maintenance'])
                self.assertEqual([g['id'] for g in candidate['gates']], ['maintenance:wrapper-unit'])
                gate = candidate['gates'][0]
                self.assertEqual(gate['platforms'], ['linux'])
                self.assertEqual(gate['test_tier'], 'unit')
                self.assertEqual(gate['execution_slot'], 'none')
                self.assertIn('scripts.tests.test_base_stand_backup', gate['argv'])

    def test_explicit_sdk_test_is_manual_integration_without_app_producers(self):
        for profile in ('fast', 'unit', 'build', 'integration', 'release'):
            with self.subTest(profile=profile):
                candidate = selection.plan(ROOT, profile=profile,
                    changed=['scripts/tests/test_native_qa_compose.py'])
                self.assertEqual(candidate['components'], ['maintenance-sdk'])
                gates = candidate['gates']
                if profile in ('fast', 'unit', 'build'):
                    self.assertEqual(gates, [])
                    continue
                self.assertEqual([g['id'] for g in gates], ['maintenance-sdk:integration'])
                gate = gates[0]
                # A direct script leaves the SDK-owned `scripts` namespace free;
                # `-m unittest scripts.tests...` would occupy it with this repo.
                self.assertEqual(gate['argv'], ['{python}', '-B', 'scripts/tests/test_native_qa_compose.py'])
                self.assertEqual(gate['requires_env'], ['SDLC_MAINTENANCE_BASE'])
                self.assertFalse(gate['cacheable'])
                self.assertTrue(gate['external_state'])
                # This direct test process does not own Base admission. Omission
                # keeps the normal parent-owned slot; "command" would bypass it.
                self.assertNotIn('execution_slot', gate)
                self.assertEqual(gate['test_tier'], 'integration')

    def test_platform_and_sdk_tests_do_not_schedule_ci_app_jobs(self):
        for name in ('test_base_stand_backup.py', 'test_native_qa_compose.py'):
            with self.subTest(path=name), patch.object(selection, 'git', return_value='scripts/tests/' + name + '\n'):
                values = selection.ci_outputs('push', 'a' * 40, 'b' * 40, repo=ROOT)
                self.assertEqual(set(values.values()), {'false'})

    def test_unknown_scripts_and_application_sources_keep_consumers(self):
        manifest = selection.load(ROOT)
        for path in ('scripts/new_generator.py', 'scripts/build.py'):
            with self.subTest(path=path):
                candidate = selection.plan(ROOT, profile='build', changed=[path], manifest=manifest)
                self.assertTrue(set(manifest['components']) <= set(candidate['components']))
                self.assertTrue(any(g.get('kind') == 'artifact' for g in candidate['gates']))
        candidate = selection.plan(ROOT, profile='build', changed=['backend/src/product.rs'], manifest=manifest)
        self.assertIn('backend', candidate['components'])
        self.assertTrue(any(g.get('kind') == 'artifact' for g in candidate['gates']))
        if 'frontend' in manifest['components']:
            candidate = selection.plan(ROOT, profile='build', changed=['frontend/src/product.tsx'], manifest=manifest)
            self.assertIn('frontend', candidate['components'])
            self.assertTrue(any(g.get('kind') == 'artifact' for g in candidate['gates']))

    def test_ci_runs_each_selected_module_once(self):
        ci = (ROOT / '.github/workflows/ci.yml').read_text(encoding='utf-8')
        commands = [shlex.split(line.strip().removeprefix('run: ')) for line in ci.splitlines()
                    if line.strip().removeprefix('run: ').startswith(('python3 -m unittest ', 'python3 -B -m unittest '))]
        for name in TEST_GATES:
            with self.subTest(module=name):
                self.assertEqual(sum(c.count('scripts.tests.' + name) for c in commands), 1)
        self.assertEqual(sum(c.count('scripts.tests.test_base_stand_backup') for c in commands), 1)
        self.assertNotIn('test_native_qa_compose', ci)


if __name__ == '__main__':
    unittest.main()
