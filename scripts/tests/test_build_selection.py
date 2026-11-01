"""Exercise the standalone command with real Git changes and inert producer endpoints."""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SOURCE = Path(__file__).resolve().parents[2]


class StandaloneBuildSelectionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='forge-build-selection-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name) / 'CI-CD'
        self.root.mkdir()
        self.calls = Path(self.temporary.name) / 'calls.jsonl'
        self.environment = {**os.environ, 'PYTHONDONTWRITEBYTECODE': '1',
                            'COMPOSE_PROFILES': '', 'FORGE_TEST_CALLS': str(self.calls)}
        for name in ('PYTHONHOME', 'UV_INTERNAL__PYTHONHOME'):
            self.environment.pop(name, None)
        for name in ('scripts/build.py', 'scripts/select_build.py', 'scripts/select_ci.py',
                     'scripts/vendor/base_quality_selection.py', 'scripts/vendor/base-quality-tooling.json',
                     '.quality-tooling-revision', '.quality-gate.json', '.github/workflows/ci.yml',
                     '.github/workflows/release.yml', 'frontend/scripts/base-generate-openapi.mjs'):
            destination = self.root / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(SOURCE / name, destination)
        for name in ('.base-revision', '.namespace-base-revision', '.base-ui-revision'):
            (self.root / name).write_text('a' * 40 + '\n')
        for name in ('backend/src/main.rs', 'frontend/src/main.ts', 'docs/guide.md',
                     'frontend/src/main.test.ts', 'docker-compose.yml'):
            destination = self.root / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_text('fixture\n')
        base = self.root.parent / 'services-base/scripts'
        base.mkdir(parents=True)
        endpoint = ('import json,os,sys\nfrom pathlib import Path\n'
                    'with Path(os.environ["FORGE_TEST_CALLS"]).open("a") as stream:\n'
                    '    stream.write(json.dumps([Path(__file__).name,*sys.argv[1:]])+"\\n")\n')
        for name in ('verify_base_revision.py', 'execution_slot.py'):
            (base / name).write_text(endpoint)
        self.git('init', '-q')
        self.git('add', '.')
        self.git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test',
                 '-c', 'core.hooksPath=.no-hooks', 'commit', '-qm', 'fixture')

    def git(self, *args):
        return subprocess.run(['git', '-C', str(self.root), *args], env=self.environment,
                              capture_output=True, check=True)

    def invoke(self, *arguments):
        done = subprocess.run([sys.executable, '-B', str(self.root / 'scripts/build.py'), *arguments],
                              cwd=self.root, env=self.environment, capture_output=True, timeout=20, check=False)
        calls = [json.loads(line) for line in self.calls.read_text().splitlines()] if self.calls.exists() else []
        return done, calls

    def change(self, name):
        (self.root / name).write_text('changed fixture\n')

    def assert_services(self, calls, services, pin='.namespace-base-revision'):
        self.assertEqual(len(calls), 2, calls)
        self.assertEqual(calls[0][0], 'verify_base_revision.py')
        self.assertEqual(Path(calls[0][-1]).name, pin)
        self.assertEqual(calls[1], ['execution_slot.py', '--', 'docker', 'compose', 'build', *services])

    def test_clean_has_no_preparation_or_producer(self):
        done, calls = self.invoke()
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertIn(b'NO_BUILD_SERVICES_SELECTED', done.stdout)
        self.assertEqual(calls, [])

    def test_docs_and_test_only_do_not_rebuild(self):
        self.change('docs/guide.md')
        self.change('frontend/src/main.test.ts')
        done, calls = self.invoke()
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(calls, [])

    def test_unstaged_backend_selects_one_service_and_one_slot(self):
        self.change('backend/src/main.rs')
        done, calls = self.invoke()
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assert_services(calls, ['backend'])

    def test_staged_frontend_uses_its_ui_pin(self):
        self.change('frontend/src/main.ts')
        self.git('add', 'frontend/src/main.ts')
        done, calls = self.invoke()
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assert_services(calls, ['frontend'], '.base-ui-revision')

    def test_deleted_backend_input_still_selects_its_producer(self):
        (self.root / 'backend/src/main.rs').unlink()
        done, calls = self.invoke()
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assert_services(calls, ['backend'])

    def test_untracked_frontend_source_selects_only_frontend(self):
        (self.root / 'frontend/src/new.ts').write_text('new fixture\n')
        done, calls = self.invoke()
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assert_services(calls, ['frontend'], '.base-ui-revision')

    def test_profile_selects_external_runner_alongside_backend(self):
        self.change('backend/src/main.rs')
        self.environment['COMPOSE_PROFILES'] = 'external-runner'
        done, calls = self.invoke()
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assert_services(calls, ['backend', 'runner'])

    def test_explicit_runner_remains_a_supported_cold_request(self):
        done, calls = self.invoke('runner')
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assert_services(calls, ['runner'])

    def test_invalid_scope_fails_before_preparation(self):
        done, calls = self.invoke('backend', 'backend')
        self.assertNotEqual(done.returncode, 0)
        self.assertEqual(calls, [])

    def test_mixed_pins_fail_before_any_preparation(self):
        (self.root / '.base-ui-revision').write_text('b' * 40 + '\n')
        done, calls = self.invoke('--all')
        self.assertNotEqual(done.returncode, 0)
        self.assertIn(b'Mixed SDK/UI pins', done.stderr)
        self.assertEqual(calls, [])


if __name__ == '__main__':
    unittest.main()
