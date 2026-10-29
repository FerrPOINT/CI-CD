"""Forge public selection/caller contracts; native graphs are qualified separately."""
import contextlib
import hashlib
import io
import json
from pathlib import Path
import runpy
import shutil
import sys
import tempfile
import unittest
from unittest.mock import patch

from scripts.vendor import base_quality_selection as selector
from scripts.select_ci import build_services, describe, outputs

ROOT = Path(__file__).resolve().parents[2]


class QualityRouteTests(unittest.TestCase):
    def setUp(self):
        def git(_root, *args):
            if args == ('rev-parse', 'HEAD'): return 'b' * 40
            raise AssertionError(args)
        self.git = patch.object(selector, 'git', side_effect=git)
        self.git.start()
        self.addCleanup(self.git.stop)

    def plan(self, path=None):
        return selector.plan(ROOT, profile='build', changed=[] if path is None else [path])

    def artifacts(self, path):
        return [g['id'] for g in self.plan(path)['gates'] if g.get('kind') == 'artifact']

    def test_no_change_docs_and_own_tooling_do_not_produce_an_application(self):
        self.assertEqual(self.plan()['gates'], [])
        for path in ['README.md', 'docs/DEVELOPMENT_GUIDE.md', 'scripts/tests/test_quality_routes.py']:
            self.assertEqual(self.artifacts(path), [])

    def test_frontend_ui_pin_is_independent_of_the_sdk(self):
        for path in ['frontend/src/main.tsx', '.base-ui-revision', 'frontend/pnpm-lock.yaml']:
            self.assertEqual(self.plan(path)['components'], ['frontend'])
            self.assertEqual(self.artifacts(path), ['frontend:build'])

    def test_frontend_test_setup_and_config_keep_checks_without_vite(self):
        for path in ['frontend/src/page.spec.tsx', 'frontend/src/test-setup.ts',
                     'frontend/vitest.config.ts',
                     'frontend/scripts/tailwind-source-inputs.test.mjs']:
            self.assertEqual(self.artifacts(path), [])
            self.assertIn('frontend:tests', [g['id'] for g in self.plan(path)['gates']])

    def test_runtime_and_standalone_tools_select_their_real_targets(self):
        expected = {'backend/src/main.rs':['backend:build'],
            'backend/src/bin/forge-runner.rs':['backend:build'],
            'backend/cli/src/main.rs':['cli:build'],
            'backend/src/bin/forge-delivery.rs':['delivery:build'],
            'backend/src/bin/forge-pg-migrate.rs':['pg-migrate:build'],
            'backend/migration/src/main.rs':['migration:build']}
        for path, ids in expected.items():
            with self.subTest(path=path): self.assertEqual(self.artifacts(path), ids)

    def test_shared_root_library_selects_only_actual_consumers(self):
        self.assertEqual(self.artifacts('backend/src/lib.rs'),
                         ['backend:build', 'delivery:build', 'pg-migrate:build'])
        self.assertEqual(self.artifacts('backend/domain/src/lib.rs'),
                         ['backend:build', 'delivery:build', 'pg-migrate:build'])

    def test_lock_toolchain_sdk_select_rust_consumers_without_frontend(self):
        for path in ['backend/Cargo.lock', '.namespace-base-revision', 'rust-toolchain.toml']:
            self.assertEqual(self.artifacts(path),
                ['backend:build', 'cli:build', 'delivery:build', 'pg-migrate:build', 'migration:build'])

    def test_rust_external_tests_do_not_invalidate_production_artifacts(self):
        for path in ['backend/tests/integration_db.rs', 'backend/cli/tests/cli_real_api.rs',
                     'backend/migration/tests/upgrade.rs']:
            self.assertEqual(self.artifacts(path), [])

    def test_embedded_product_witnesses_are_build_inputs_of_their_consumers(self):
        for path in ['scripts/postgres-delivery.py', 'scripts/workspace_deploy_admission.py']:
            self.assertEqual(self.artifacts(path), ['backend:build', 'delivery:build', 'pg-migrate:build'])

    def test_unknown_sources_keep_the_conservative_full_fallback(self):
        self.assertEqual(len(self.artifacts('backend/new-crate/src/lib.rs')), 6)
        self.assertEqual(set(self.plan('scripts/new-generator.py')['components']),
                         set(selector.load(ROOT)['components']))

    def test_recipe_only_change_selects_packaging_without_application_compile(self):
        for path, service in [('backend/Dockerfile.umbrella','backend'),
                              ('backend/Dockerfile.umbrella.dockerignore','backend'),
                              ('frontend/Dockerfile','frontend'), ('frontend/nginx.conf','frontend')]:
            self.assertEqual(self.artifacts(path), [])
            self.assertEqual(build_services(self.plan(path)), [service])

    def test_cli_and_delivery_do_not_invent_a_container(self):
        for path in ['backend/cli/src/main.rs', 'backend/src/bin/forge-delivery.rs',
                     'backend/src/bin/forge-pg-migrate.rs', 'backend/migration/src/main.rs']:
            self.assertEqual(build_services(self.plan(path)), [])
            self.assertTrue(describe(self.plan(path))['backend'])

    def test_units_and_static_checks_precede_all_producers(self):
        gates = self.plan('scripts/new-generator.py')['gates']
        first = next(i for i,g in enumerate(gates) if g.get('kind') == 'artifact')
        self.assertTrue(all(i < first for i,g in enumerate(gates) if g.get('test_tier') in ('unit','static')))

    def test_runtime_recipe_builds_only_its_two_shipped_binaries(self):
        for name in ['Dockerfile','Dockerfile.umbrella']:
            source = (ROOT/'backend'/name).read_text(encoding='utf-8')
            commands = [line for line in source.splitlines() if '/cargo build ' in line]
            self.assertEqual(len(commands), 2)
            for command in commands:
                self.assertIn('-p cicd-server --bin cicd-server --bin forge-runner', command)
                self.assertNotIn('-p cicd-cli', command)
                self.assertNotIn('--workspace', command)
            self.assertIn('cargo test --locked --release -p cicd-domain --lib', source)
            self.assertIn('cargo clean -p cicd-server', source)
            self.assertIn("find src domain app infra api cli migration -name '*.rs' -exec touch", source)

    def test_vite_excludes_test_config_while_typechecking_it_explicitly(self):
        source = (ROOT/'frontend/vite.config.ts').read_text(encoding='utf-8')
        self.assertNotIn('  test:', source)
        package = json.loads((ROOT/'frontend/package.json').read_text(encoding='utf-8'))
        self.assertEqual(package['scripts']['typecheck'], 'tsc --noEmit')
        config = json.loads((ROOT/'frontend/tsconfig.json').read_text(encoding='utf-8'))
        self.assertIn('vitest.config.ts', config['include'])
        self.assertEqual(package['scripts']['build:compile'], 'vite build')
        for name in ['Dockerfile.dockerignore','Dockerfile.umbrella.dockerignore']:
            self.assertIn('CI-CD/frontend/src/api/schema.d.ts', (ROOT/'frontend'/name).read_text(encoding='utf-8'))

    def test_packaged_selector_keeps_exact_provenance(self):
        receipt = json.loads((ROOT/'scripts/vendor/base-quality-tooling.json').read_text(encoding='utf-8'))
        self.assertEqual(receipt['revision'], (ROOT/'.quality-tooling-revision').read_text().strip())
        self.assertEqual(receipt['sha256'], hashlib.sha256((ROOT/'scripts/vendor/base_quality_selection.py').read_bytes()).hexdigest())

    def test_initial_push_is_explicit_all_and_invalid_base_never_prepares(self):
        self.assertTrue(outputs(ROOT,'push','0'*40,'b'*40)['frontend_build'])
        with patch.object(selector,'git') as git:
            with self.assertRaises(ValueError): outputs(ROOT,'pull_request','','b'*40)
            git.assert_not_called()

    def test_push_and_pr_use_the_same_exact_source_diff(self):
        def git(_root,*args):
            if args == ('rev-parse','HEAD'): return 'b'*40
            if args == ('diff','--name-only','--no-renames','a'*40,'b'*40): return 'frontend/src/main.tsx\n'
            raise AssertionError(args)
        with patch.object(selector,'git',side_effect=git):
            actual = outputs(ROOT,'push','a'*40,'b'*40)
        self.assertTrue(actual['frontend_build'])
        self.assertFalse(actual['backend'])

    def test_openapi_dump_change_keeps_drift_gate_without_runtime_build(self):
        plan = self.plan('backend/src/bin/openapi-dump.rs')
        self.assertEqual(self.artifacts('backend/src/bin/openapi-dump.rs'), [])
        self.assertTrue(describe(plan)['openapi_check'])


    def test_automatic_generator_adapter_preserves_the_public_manual_route(self):
        package = json.loads((ROOT / 'frontend/package.json').read_bytes())
        receipt = json.loads((ROOT / 'scripts/vendor/base-quality-tooling.json').read_bytes())
        self.assertEqual(receipt['schema'], 'base-quality-tooling/v2')
        generator = receipt['frontend_generator']
        self.assertEqual(set(generator), {'source', 'file', 'contract', 'sha256'})
        self.assertEqual(generator['source'], 'scripts/generate_openapi.mjs')
        self.assertEqual(generator['file'], 'frontend/scripts/base-generate-openapi.mjs')
        self.assertEqual(generator['contract'], 'base-openapi-generation/v1')
        self.assertEqual(hashlib.sha256((ROOT / generator['file']).read_bytes()).hexdigest(), generator['sha256'])
        self.assertEqual(package['scripts']['postinstall'], 'node scripts/base-generate-openapi.mjs --schema ../openapi/openapi.yaml --output src/api/schema.d.ts --spec-env SPEC')
        self.assertEqual(package['scripts']['openapi:generate'], 'node scripts/generate-openapi.mjs')
        for name in ('Dockerfile', 'Dockerfile.umbrella'):
            path = ROOT / 'frontend' / name
            if not path.is_file(): continue
            source = path.read_text()
            generation = next(i for i, line in enumerate(source.splitlines()) if line.startswith('RUN ')
                              and ('pnpm run postinstall' in line or 'pnpm install' in line and '--ignore-scripts' not in line))
            self.assertIn('base-generate-openapi.mjs', '\n'.join(source.splitlines()[:generation]))



class RawContainerScopeTests(unittest.TestCase):
    def invoke(self, targets, selected, pins):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);script=root/'scripts/build.py';script.parent.mkdir()
            shutil.copyfile(ROOT/'scripts/build.py',script)
            for name,value in pins.items(): (root/name).write_text(value)
            with patch.object(sys,'argv',[str(script),*targets]), patch('scripts.select_ci.auto_services',return_value=selected), patch('subprocess.run') as run, contextlib.redirect_stdout(io.StringIO()):
                runpy.run_path(str(script),run_name='__main__')
                return run.call_args_list

    def test_no_change_skips_checkout_verifier_and_docker(self):
        self.assertEqual(self.invoke([],[],{}), [])

    def test_frontend_keeps_its_independent_ui_verifier(self):
        calls=self.invoke([],['frontend'],{'.base-ui-revision':'1'*40,'.namespace-base-revision':'2'*40})
        self.assertEqual(len(calls),2)
        self.assertTrue(calls[0].args[0][-1].endswith('.base-ui-revision'))
        self.assertEqual(calls[1].args[0],['docker','compose','build','frontend'])

    def test_mixed_pins_refuse_before_docker(self):
        with self.assertRaisesRegex(SystemExit,'Mixed SDK/UI pins'):
            self.invoke(['--all'],[],{'.base-ui-revision':'1'*40,'.namespace-base-revision':'2'*40})
