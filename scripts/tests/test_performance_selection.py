"""Exercise the public pinned selector against the actual Forge delivery graph."""
import re
import unittest
from pathlib import Path

import tomllib

from scripts import select_build
from scripts.vendor import base_quality_selection as selector

ROOT = Path(__file__).resolve().parents[2]


class PerformanceSelectionTests(unittest.TestCase):
    def plan(self, *paths):
        return selector.plan(ROOT, profile='build', base='HEAD', changed=list(paths))

    def artifacts(self, plan):
        return {gate['id'] for gate in plan['gates'] if gate.get('kind') == 'artifact'}

    def test_new_markdown_does_not_select_application_producers(self):
        self.assertEqual(self.artifacts(self.plan('docs/new-guide.md')), set())

    def test_frontend_test_selects_units_without_either_application_build(self):
        plan = self.plan('frontend/src/api/client.test.ts')
        self.assertIn('frontend:tests', {gate['id'] for gate in plan['gates']})
        self.assertEqual(self.artifacts(plan), set())
        self.assertNotIn('backend', plan['components'])

    def test_backend_test_preserves_the_production_artifacts(self):
        self.assertEqual(self.artifacts(self.plan('backend/tests/integration_db.rs')), set())

    def test_explicit_cli_source_keeps_the_neighboring_cli_delivery(self):
        plan = self.plan('backend/cli/src/main.rs')
        self.assertEqual(self.artifacts(plan), {'cli:build'})
        self.assertEqual(select_build.build_services(plan, selector.load(ROOT), profiles=()), [])

    def test_unknown_input_expands_to_product_including_both_application_producers(self):
        artifacts = self.artifacts(self.plan('unclassified-build-input'))
        self.assertTrue({'frontend:build', 'backend:build', 'cli:build'} <= artifacts)

    def test_optional_external_runner_requires_its_profile_or_explicit_service(self):
        plan = self.plan('backend/src/lib.rs')
        manifest = selector.load(ROOT)
        self.assertEqual(select_build.build_services(plan, manifest, profiles=()), ['backend'])
        self.assertEqual(select_build.build_services(plan, manifest, profiles=('external-runner',)),
                         ['backend', 'runner'])
        self.assertIn('runner', select_build.SERVICES)

    def test_declared_backend_outputs_equal_existing_runtime_consumers_and_real_bins(self):
        manifest = selector.load(ROOT)
        artifact = next(gate for gate in manifest['gates'] if gate['id'] == 'backend:build')
        cargo = tomllib.loads((ROOT / 'backend/Cargo.toml').read_text())
        targets = {target['name']: target for target in cargo['bin']}
        runtime = (ROOT / 'backend/Dockerfile.umbrella').read_text().split('FROM debian:bookworm-slim', 1)[1]
        delivered = set(re.findall(r'COPY --from=build /app/target/release/([\w-]+) ', runtime))
        self.assertEqual(set(artifact['artifact_outputs']), delivered)
        for name in artifact['artifact_outputs']:
            self.assertTrue((ROOT / 'backend' / targets[name]['path']).is_file(), name)

    def test_full_manual_gate_retains_original_database_and_workspace_obligations(self):
        release = (ROOT / '.github/workflows/release.yml').read_text()
        triggers = release.split('\njobs:', 1)[0]
        self.assertIn('workflow_dispatch:', triggers)
        self.assertNotIn('\n  push:', triggers)
        self.assertNotIn('\n  pull_request:', triggers)
        self.assertIn('uses: ./.github/workflows/qualification.yml', release)
        self.assertIn('git_receive_only: false', release)
        manual = (ROOT / '.github/workflows/qualification.yml').read_text()
        self.assertIn('workflow_call:', manual)
        self.assertIn('workflow_dispatch:', manual)
        for command in ('cargo clippy --locked --workspace --all-targets -- -D warnings',
                        'cargo test --locked --workspace', 'cargo build --locked --release --workspace',
                        '--features integration --test integration_db', '--test cli_real_api',
                        'pnpm run test:all'):
            self.assertIn(command, manual)
        automatic = (ROOT / '.github/workflows/ci.yml').read_text()
        backend = automatic.split('\n  backend:', 1)[1].split('\n  frontend:', 1)[0]
        self.assertNotIn('\n    services:', backend)
        self.assertNotIn('--workspace --all-targets', backend)
        self.assertNotIn('cargo build --locked --release --workspace', backend)
        self.assertIn('cargo test --locked -p cicd-domain --lib', backend)

    def test_declared_python_unittest_entrypoints_exist(self):
        for gate in selector.load(ROOT)['gates']:
            argv = gate['argv']
            if '-m' not in argv or argv[argv.index('-m') + 1] != 'unittest':
                continue
            for name in argv[argv.index('-m') + 2:]:
                if name.startswith('scripts.'):
                    parts = name.split('.')
                    while len(parts) > 1 and not (ROOT / Path(*parts).with_suffix('.py')).is_file():
                        parts.pop()
                    self.assertGreater(len(parts), 1, (gate['id'], name))


if __name__ == '__main__':
    unittest.main()
