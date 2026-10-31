"""Current Forge selection contracts; no compiler, install, Docker or runtime."""

import json
import re
import unittest
from pathlib import Path
from unittest.mock import patch

from scripts import select_build, select_ci
from scripts.vendor import base_quality_selection as selection

ROOT = Path(__file__).resolve().parents[2]
RUST_COMPONENTS = ['backend', 'cli', 'delivery', 'pg-migrate', 'migration']


class CurrentPerformanceRoutesTests(unittest.TestCase):
    def plan(self, components, profile):
        with patch.object(selection, 'git', return_value='a' * 40):
            return selection.plan(ROOT, profile=profile, changed=[], components=components)

    def test_cli_formatter_needs_no_sdk_or_cargo_target(self):
        manifest = selection.load(ROOT)
        gate = next(gate for gate in manifest['gates'] if gate['id'] == 'cli:format')
        self.assertEqual(gate['argv'][:4], ['rustfmt', '--edition', '2024', '--check'])
        self.assertTrue(all(name.startswith('cli/src/') and name.endswith('.rs') for name in gate['argv'][4:]))
        self.assertNotIn('sdk_revision', gate)
        self.assertEqual(gate['test_tier'], 'static')
        self.assertEqual(gate['cwd'], 'backend')
        self.assertEqual(gate['platforms'], ['linux'])
        self.assertEqual(gate['tools'][0]['pattern'], r'^rustc 1\.88\.0\b')
        self.assertIn('sdk_revision', next(gate for gate in manifest['gates'] if gate['id'] == 'cli:build'))

    def test_current_node_is_shared_by_all_frontend_preparation_routes(self):
        manifest = selection.load(ROOT)
        prepared = [gate for gate in manifest['gates'] if gate.get('prepare_frontend')]
        self.assertEqual(len(prepared), 5)
        self.assertEqual({gate['prepare_frontend'] for gate in prepared}, {'22.22.2'})
        for name in ('frontend/Dockerfile', 'frontend/Dockerfile.umbrella'):
            self.assertIn('FROM node:22.22.2-bookworm-slim', (ROOT / name).read_text(encoding='utf-8'))
        for name in ('ci.yml', 'qualification.yml'):
            source = (ROOT / '.github/workflows' / name).read_text(encoding='utf-8')
            self.assertRegex(source, r'node-version: [\"\x27]?22\.22\.2')
            self.assertNotIn('22.20.0', source)

    def test_each_rust_tool_has_a_fresh_complete_manual_suite(self):
        manifest = selection.load(ROOT)
        original = next(gate for gate in manifest['gates'] if gate['id'] == 'backend:quality')
        for component in RUST_COMPONENTS:
            with self.subTest(component=component):
                gates = self.plan([component], 'release')['gates']
                shared = [gate for gate in gates if gate.get('shared_check')]
                self.assertEqual(len(shared), 1)
                self.assertEqual(shared[0]['component'], component)
                self.assertEqual(shared[0]['argv'], original['argv'])
                self.assertEqual(shared[0]['sdk_revision'], original['sdk_revision'])
                self.assertFalse(shared[0]['cacheable'])
                self.assertNotIn('artifact_outputs', shared[0])

    def test_full_selection_runs_one_rust_suite_and_no_second_unit_subset(self):
        gates = self.plan([*RUST_COMPONENTS, 'frontend'], 'release')['gates']
        rust = [gate for gate in gates if gate.get('shared_check')]
        self.assertEqual(len(rust), 1)
        self.assertEqual({member['component'] for member in rust[0]['shared_check_members']}, set(RUST_COMPONENTS))
        self.assertIn('cargo test --locked --workspace', rust[0]['argv'][-1])
        ids = {gate['id'] for gate in gates}
        self.assertNotIn('backend:unit', ids)
        self.assertNotIn('frontend:tests', ids)
        full = next(gate for gate in gates if gate['id'] == 'frontend:full-tests')
        self.assertEqual(full['argv'], ['pnpm', 'test:all'])
        self.assertFalse(full['cacheable'])

    def test_automatic_profiles_keep_units_without_manual_work(self):
        for profile in ('fast', 'unit', 'build'):
            with self.subTest(profile=profile):
                gates = self.plan(['backend', 'frontend', 'tooling'], profile)['gates']
                ids = {gate['id'] for gate in gates}
                self.assertTrue({'backend:unit', 'frontend:tests'} <= ids)
                self.assertNotIn('tooling:delivery-preflight', ids)
                self.assertNotIn('frontend:full-tests', ids)
                self.assertFalse(any(gate.get('shared_check') for gate in gates))

    def test_selected_delivery_preflight_requires_powershell_and_stays_manual(self):
        gates = self.plan(['tooling'], 'integration')['gates']
        gate = next(gate for gate in gates if gate['id'] == 'tooling:delivery-preflight')
        self.assertEqual(gate['test_tier'], 'integration')
        self.assertFalse(gate['cacheable'])
        self.assertIn('scripts.tests.test_delivery_pin_preflight_integration', gate['argv'])
        self.assertIn({'argv': ['pwsh', '--version'], 'pattern': r'^PowerShell 7\.'}, gate['tools'])
        automatic = (ROOT / '.github/workflows/ci.yml').read_text(encoding='utf-8')
        manual = (ROOT / '.github/workflows/qualification.yml').read_text(encoding='utf-8')
        self.assertNotIn('scripts.tests.test_delivery_pin_preflight_integration', automatic)
        self.assertIn('scripts.tests.test_delivery_pin_preflight_integration', manual)

    def test_just_names_unit_and_full_frontend_suites_explicitly(self):
        source = (ROOT / 'justfile').read_text(encoding='utf-8')
        self.assertIn('test-backend:\n    cd backend && cargo test --locked -p cicd-domain --lib', source)
        self.assertIn('test-backend-all:\n    cd backend && cargo test --locked --workspace', source)
        self.assertIn('test-frontend:\n    cd frontend && pnpm test:unit', source)
        self.assertIn('test-frontend-all:\n    cd frontend && pnpm test:all', source)
        self.assertNotRegex(source, r'(?m)^build:.*build-backend.*build-frontend')

    def test_non_container_tools_do_not_select_runtime_images(self):
        manifest = selection.load(ROOT)
        for gate in ('cli:build', 'delivery:build', 'pg-migrate:build', 'migration:build'):
            with self.subTest(gate=gate):
                plan = {'gates': [{'id': gate, 'kind': 'artifact'}], 'changed': []}
                self.assertEqual(select_build.build_services(plan, manifest), [])
        plan = {'gates': [{'id': 'backend:build', 'kind': 'artifact'}], 'changed': []}
        self.assertEqual(select_build.build_services(plan, manifest), ['backend', 'runner'])

    def test_automatic_ci_checks_the_same_exact_candidate_as_selection(self):
        source = (ROOT / '.github/workflows/ci.yml').read_text(encoding='utf-8')
        checkouts = re.findall(r'^          path: CI-CD\n(.*?)(?=^      -|^  [\w-]+:|\Z)',
                               source, re.MULTILINE | re.DOTALL)
        self.assertEqual(len(checkouts), 4)
        for checkout in checkouts:
            self.assertIn('ref: ${{ github.event.pull_request.head.sha || github.sha }}', checkout)
        manifest = json.loads((ROOT / '.quality-gate.json').read_bytes())
        owner = next(gate for gate in manifest['gates'] if gate['id'] == 'tooling:quality-routes')
        self.assertIn('scripts.tests.test_current_performance_routes', owner['argv'])

    def test_ci_and_root_source_owners_execute_the_same_test_modules(self):
        manifest = selection.load(ROOT)
        source_gates = [gate for gate in manifest['gates']
                        if gate['id'] in ('tooling:quality-routes', 'tooling:bin-test-inputs', 'pinned-imports:unit')]
        self.assertEqual(len(source_gates), 3)
        root_modules = {arg for gate in source_gates for arg in gate['argv'] if arg.startswith('scripts.tests.')}
        automatic = (ROOT / '.github/workflows/ci.yml').read_text(encoding='utf-8')
        ci_modules = set(re.findall(r'scripts\.tests\.\w+', automatic))
        self.assertEqual(root_modules, ci_modules)
        self.assertIn('scripts.tests.test_postgres_rejection', root_modules)
        self.assertIn('scripts.tests.test_base_stand_backup', root_modules)
        self.assertIn('scripts.tests.test_bin_test_inputs', ci_modules)

    def test_sdk_import_test_change_does_not_prepare_rust_or_produce_an_app(self):
        with patch.object(selection, 'git', return_value='a' * 40):
            plan = selection.plan(ROOT, profile='build', changed=['scripts/tests/test_postgres_import_boundary.py'])
        ids = {gate['id'] for gate in plan['gates']}
        self.assertIn('pinned-imports:unit', ids)
        self.assertNotIn('backend:unit', ids)
        self.assertFalse(any(gate.get('kind') == 'artifact' for gate in plan['gates']))
        flags = select_ci.describe(plan)
        self.assertTrue(flags['pinned_imports'])
        self.assertFalse(flags['backend'])
        self.assertEqual(select_build.build_services(plan, selection.load(ROOT)), [])
        workflow = (ROOT / '.github/workflows/ci.yml').read_text(encoding='utf-8')
        self.assertIn("if: needs.changes.outputs.backend == 'true' || needs.changes.outputs.pinned_imports == 'true'", workflow)
        for step in ('      - uses: dtolnay/rust-toolchain@stable\n',
                     '      - uses: Swatinem/rust-cache@v2\n', '      - name: fmt\n',
                     '      - name: Native QA Compose ownership regressions\n'):
            self.assertIn(step + "        if: needs.changes.outputs.backend == 'true'\n", workflow)

    def test_current_runtime_receives_every_declared_binary(self):
        gate = next(gate for gate in selection.load(ROOT)['gates'] if gate['id'] == 'backend:build')
        expected = {'cicd-server', 'forge-runner', 'forge-quiesce'}
        self.assertEqual(set(gate['artifact_outputs']), expected)
        for name in expected:
            self.assertIn('--bin ' + name, gate['argv'][-1])
        for filename in ('Dockerfile', 'Dockerfile.umbrella'):
            recipe = (ROOT / 'backend' / filename).read_text(encoding='utf-8')
            shipped = set(re.findall(r'COPY --from=build /app/target/release/([^ ]+) ', recipe))
            self.assertEqual(shipped, expected)
            final = recipe.split('RUN find src ', 1)[1].split('FROM debian:', 1)[0]
            self.assertIn('--bin forge-quiesce', final)

    def test_lineage_packaging_avoids_an_unnecessary_rust_producer(self):
        with patch.object(selection, 'git', return_value='a' * 40):
            plan = selection.plan(ROOT, profile='build', changed=['scripts/migration_lineages.py'])
        self.assertFalse(any(gate.get('kind') == 'artifact' for gate in plan['gates']))
        self.assertIn('tooling:migration-lineages', {gate['id'] for gate in plan['gates']})
        self.assertEqual(select_build.build_services(plan, selection.load(ROOT)), ['backend', 'runner'])
        for name in ('Dockerfile.dockerignore', 'Dockerfile.umbrella.dockerignore'):
            ignore = (ROOT / 'backend' / name).read_text(encoding='utf-8')
            self.assertIn('!CI-CD/backend/migration-lineages/**', ignore)
            self.assertIn('!CI-CD/scripts/migration_lineages.py', ignore)
        for name in ('ci.yml', 'qualification.yml'):
            workflow = (ROOT / '.github/workflows' / name).read_text(encoding='utf-8')
            self.assertIn('discover -s scripts -p test_migration_lineages.py', workflow)

    def test_optional_kubernetes_recipe_does_not_rebuild_ordinary_images(self):
        with patch.object(selection, 'git', return_value='a' * 40):
            plan = selection.plan(ROOT, profile='build', changed=['backend/Dockerfile.kubernetes-runner'])
        self.assertFalse(any(gate.get('kind') == 'artifact' for gate in plan['gates']))
        self.assertEqual(select_build.build_services(plan, selection.load(ROOT)), [])
        recipe = (ROOT / 'backend/Dockerfile.kubernetes-runner').read_text(encoding='utf-8')
        self.assertIn('FROM ${FORGE_IMAGE}', recipe)
        self.assertNotIn('cargo ', recipe)
        self.assertNotIn('FROM rust:', recipe)
        allowed = (ROOT / 'backend/Dockerfile.kubernetes-runner.dockerignore').read_text(encoding='utf-8')
        self.assertEqual([line for line in allowed.splitlines() if line.startswith('!')],
                         ['!scripts/', '!scripts/host_image_build.py', '!scripts/helm_deploy.py'])


if __name__ == '__main__':
    unittest.main()
