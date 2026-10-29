"""Product selection and complete manual acceptance remain independent."""
import hashlib
import json
import re
import unittest
from pathlib import Path
from unittest.mock import patch

from scripts.vendor import base_quality_selection as selector

ROOT = Path(__file__).resolve().parents[2]


def job(text, name):
    return re.search(r'^  '+name+r':\n.*?(?=^  [a-zA-Z0-9_-]+:\n|\Z)', text, re.M | re.S)[0]


class QualityRouteTests(unittest.TestCase):
    def test_every_automatic_consumer_uses_the_selected_exact_head(self):
        ci = (ROOT/'.github/workflows/ci.yml').read_text(encoding='utf-8')
        paths = re.findall(r'^          path: CI-CD\n(.*?)(?=^      -|^  [a-zA-Z0-9_-]+:|\Z)', ci, re.M | re.S)
        self.assertEqual(len(paths), 4)
        for checkout in paths:
            self.assertIn('ref: ${{ github.event.pull_request.head.sha || github.sha }}', checkout)

    def plan(self, changed, profile='build', **options):
        with patch.object(selector, 'git', return_value='a' * 40):
            return selector.plan(ROOT, profile, changed=changed, **options)

    def test_docs_and_no_change_have_no_application_producers(self):
        self.assertEqual(self.plan([])['gates'], [])
        for profile in ('fast', 'unit', 'build'):
            selected = self.plan(['docs/ARCHITECTURE.md'], profile)
            self.assertFalse(any(g['component'] in ('frontend', 'backend') for g in selected['gates']))

    def test_one_frontend_does_not_select_backend(self):
        selected = self.plan(['frontend/src/pages/search/search.tsx'])
        self.assertEqual(selected['components'], ['frontend'])
        self.assertIn('frontend:build', [g['id'] for g in selected['gates']])

    def test_frontend_test_and_config_keep_units_without_production_compile(self):
        for name in ('frontend/src/pages/search/search.test.tsx', 'frontend/vitest.config.ts',
                     'frontend/src/test-setup.ts'):
            with self.subTest(path=name):
                gates = [g['id'] for g in self.plan([name])['gates']]
                self.assertIn('frontend:tests', gates)
                self.assertNotIn('frontend:build', gates)

    def test_backend_and_sdk_do_not_select_frontend(self):
        for name in ('backend/domain/src/entities.rs', '.base-revision'):
            selected = self.plan([name])
            self.assertEqual(selected['components'], ['backend'])
            self.assertIn('backend:build', [g['id'] for g in selected['gates']])

    def test_ui_pin_is_independent_and_lockfile_invalidates_its_producer(self):
        for name in ('.base-ui-revision', 'frontend/pnpm-lock.yaml'):
            selected = self.plan([name])
            self.assertEqual(selected['components'], ['frontend'])
            self.assertIn('frontend:build', [g['id'] for g in selected['gates']])
        for name in ('ci.yml', 'release.yml'):
            text = (ROOT/'.github/workflows'/name).read_text(encoding='utf-8')
            frontend = job(text, 'frontend')
            backend = job(text, 'backend')
            self.assertIn('cat .base-ui-revision', frontend)
            self.assertIn('--revision .base-ui-revision', frontend)
            self.assertNotIn('cat .base-revision', frontend)
            self.assertIn('cat .base-revision', backend)
            self.assertNotIn('cat .base-ui-revision', backend)

    def test_unknown_path_expands_and_explicit_all_keeps_builds(self):
        unknown = self.plan(['new-runtime-input.conf'])
        self.assertEqual(set(unknown['components']), {'docs', 'frontend', 'backend', 'tooling'})
        explicit = self.plan([], components=['docs', 'frontend', 'backend', 'tooling'])
        self.assertTrue({'frontend:build', 'backend:build'} <= {g['id'] for g in explicit['gates']})

    def test_full_backend_is_manual_and_unit_does_not_compile_all_targets(self):
        unit = self.plan(['backend/domain/src/entities.rs'], 'unit')
        native = [g for g in unit['gates'] if g['component'] == 'backend']
        self.assertEqual(len(native), 1)
        self.assertIn('cargo test --locked -p cicd-domain --lib', native[0]['argv'][-1])
        self.assertNotIn('--workspace', native[0]['argv'][-1])
        release = self.plan(['backend/domain/src/entities.rs'], 'release')
        full = next(g for g in release['gates'] if g['id'] == 'backend:quality')
        self.assertIn('cargo test --locked --workspace', full['argv'][-1])

    def test_new_native_artifacts_are_not_opted_into_reuse(self):
        artifacts = [gate for gate in selector.load(ROOT)['gates'] if gate.get('kind') == 'artifact']
        self.assertEqual({gate['id'] for gate in artifacts}, {'frontend:build', 'backend:build'})
        self.assertTrue(all(gate.get('cacheable') is False for gate in artifacts))
        backend = next(gate for gate in artifacts if gate['id'] == 'backend:build')
        self.assertEqual(backend['artifact_outputs'], ['cicd-server', 'forge-runner'])
        self.assertNotIn('--workspace', backend['argv'][-1])
        self.assertNotIn('find ', backend['argv'][-1])

    def test_test_configuration_is_separate_and_keeps_the_old_inventory(self):
        vite = (ROOT/'frontend/vite.config.ts').read_text(encoding='utf-8')
        vitest = (ROOT/'frontend/vitest.config.ts').read_text(encoding='utf-8')
        self.assertNotIn('  test: {', vite)
        for field in ("environment: 'jsdom'", 'maxWorkers: 4',
                      "include: ['src/**/*.{test,spec}.{ts,tsx}']", "setupFiles: ['./src/test-setup.ts']"):
            self.assertIn(field, vitest)
        self.assertIn("import viteConfig from './vite.config'", vitest)

    def test_packaged_selector_has_exact_pin_and_registered_generator(self):
        receipt = json.loads((ROOT/'scripts/vendor/base-quality-tooling.json').read_text(encoding='utf-8'))
        self.assertEqual(receipt['revision'], (ROOT/'.quality-tooling-revision').read_text().strip())
        self.assertEqual(receipt['sha256'], hashlib.sha256((ROOT/'scripts/vendor/base_quality_selection.py').read_bytes()).hexdigest())
        self.assertEqual(set(receipt), {'sha256', 'frontend_generator', 'schema', 'source', 'repository', 'revision'})
        changes = job((ROOT/'.github/workflows/ci.yml').read_text(encoding='utf-8'), 'changes')
        # Execute the actual in-workflow metadata verifier against this product.
        code = re.search(r"          python3 - <<'PY'\n(.*?)^          PY$", changes, re.M | re.S)[1]
        code = '\n'.join(line[10:] for line in code.splitlines())
        with patch('pathlib.Path', side_effect=lambda path: ROOT if str(path) == 'CI-CD' else Path(path)):
            exec(compile(code, '<ci-selector-verifier>', 'exec'), {})
        self.assertNotIn('rust_target_cache', changes)
        self.assertNotIn('frontend_artifact', changes)

    def test_automatic_routes_retain_static_and_compatibility_checks(self):
        ci = (ROOT/'.github/workflows/ci.yml').read_text(encoding='utf-8')
        self.assertNotIn('services:', ci)
        self.assertNotIn('playwright install', ci)
        self.assertNotIn('cargo test --locked --workspace', ci)
        self.assertNotIn('--all-targets', ci)
        self.assertIn('git rev-parse HEAD', job(ci, 'changes'))
        for command in ('pnpm openapi:check', 'pnpm exec tsc --noEmit', 'pnpm test:unit',
                        'pnpm lint', 'pnpm openapi:compat'):
            self.assertIn(command, job(ci, 'frontend'))
        self.assertIn("if: needs.changes.outputs.frontend_build == 'true'", job(ci, 'frontend'))
        backend = job(ci, 'backend')
        self.assertIn("if: needs.changes.outputs.backend_build == 'true'", backend)
        self.assertIn('cargo check --locked -p cicd-server --bin cicd-server --bin forge-runner', backend)
        self.assertNotIn('\n  minimum-rust:', ci)
        self.assertEqual(ci.count('uses: dtolnay/rust-toolchain@stable'), 1)
        self.assertEqual(ci.count('uses: Swatinem/rust-cache@v2'), 1)

    def test_manual_acceptance_preserves_complete_original_jobs(self):
        release = (ROOT/'.github/workflows/release.yml').read_text(encoding='utf-8')
        self.assertEqual(release.split('\non:\n',1)[1].split('\n\n',1)[0].strip(), 'workflow_dispatch:')
        self.assertEqual(set(re.findall(r'^  ([a-zA-Z0-9_-]+):$', release.split('jobs:\n',1)[1], re.M)),
                         {'docs','backend','frontend','minimum-rust'})
        self.assertIn('cargo clippy --locked --workspace --all-targets -- -D warnings', release)
        self.assertIn('cargo check --locked --workspace --all-targets', release)
        self.assertIn('pnpm test:all', release)
        self.assertNotIn('run: pnpm build', release)
        self.assertIn('run: pnpm exec tsc --noEmit && pnpm exec vite build', release)
        self.assertIn('python ../../services-base/scripts/check_ui_consumer.py', release)
        self.assertIn('pnpm theme:check http://127.0.0.1:4173', release)


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



class CssInputOwnershipTests(unittest.TestCase):
    def setUp(self):
        import importlib.util
        from pathlib import Path
        from unittest.mock import patch
        self.root = Path(__file__).resolve().parents[2]
        spec = importlib.util.spec_from_file_location('css_input_selection', self.root / 'scripts/vendor/base_quality_selection.py')
        self.selection = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.selection)
        current = patch.object(self.selection, 'git', return_value='a' * 40)
        current.start()
        self.addCleanup(current.stop)

    def test_css_fixture_is_unit_only_and_called_by_public_commands(self):
        import json
        manifest = self.selection.load(self.root)
        path = 'frontend/scripts/tailwind-source-inputs.test.mjs'
        build = next(g for g in manifest['gates'] if g['id'] == 'frontend:build')
        self.assertTrue(self.selection.matches(path, manifest['frontend_unit_inputs']))
        self.assertTrue(self.selection.matches(path, build['exclude_inputs']))
        package = json.loads((self.root / 'frontend/package.json').read_text(encoding='utf-8'))
        self.assertTrue(package['scripts']['test:unit'].startswith('node --test scripts/tailwind-source-inputs.test.mjs && vitest run '))
        self.assertEqual(package['scripts']['test:all'], 'node --test scripts/tailwind-source-inputs.test.mjs && vitest run')

    def test_css_fixture_skips_application_build_but_real_css_keeps_it(self):
        path = 'frontend/scripts/tailwind-source-inputs.test.mjs'
        for profile in ('fast', 'unit', 'build'):
            with self.subTest(profile=profile):
                plan = self.selection.plan(self.root, profile=profile, changed=[path])
                self.assertIn('frontend:tests', [g['id'] for g in plan['gates']])
                self.assertFalse(any(g.get('kind') == 'artifact' for g in plan['gates']))
        mixed = self.selection.plan(self.root, profile='build', changed=[path, 'frontend/src/index.css'])
        self.assertIn('frontend:build', [g['id'] for g in mixed['gates']])
        self.assertNotIn('backend', mixed['components'])

    def test_ci_css_fixture_preserves_production_and_backend_selection(self):
        from unittest.mock import patch
        path = 'frontend/scripts/tailwind-source-inputs.test.mjs'
        for changed, build in ((path+'\n', 'false'), (path+'\nfrontend/src/index.css\n', 'true')):
            with self.subTest(build=build), patch.object(self.selection, 'git', return_value=changed):
                values = self.selection.ci_outputs('push', 'a'*40, 'b'*40, repo=self.root)
            self.assertEqual(values, {'backend':'false','frontend':'true','backend_build':'false','frontend_build':build})


if __name__ == '__main__':
    unittest.main()


class StandalonePrerequisiteTests(unittest.TestCase):
    def test_independent_units_and_static_precede_artifact(self):
        import importlib.util
        import tempfile
        from unittest.mock import patch

        spec = importlib.util.spec_from_file_location(
            "java_prerequisite_selection", ROOT / "scripts/vendor/base_quality_selection.py")
        selection = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(selection)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)

            def gate(identifier, component, tier):
                return {
                    "id": identifier, "component": component, "test_tier": tier,
                    "profiles": ["build"], "argv": ["fixture-never-executed"], "cwd": ".",
                    "inputs": ["source.txt"], "cacheable": False,
                    **({"kind": "artifact", "outputs": [".local/ready"]} if tier == "artifact" else {}),
                }

            manifest = {
                "schema": "base-quality-product/v1", "product": "fixture",
                "components": {"app": ["source.txt"], "docs": ["docs/**"]},
                "gates": [gate("bundle:build", "app", "artifact"),
                          gate("source:unit", "app", "unit"), gate("docs:static", "docs", "static")],
            }
            (root / ".quality-gate.json").write_text(json.dumps(manifest), encoding="utf-8")
            (root / "source.txt").write_text("never compiled\n", encoding="utf-8")
            with patch.object(selection, "git", return_value="a" * 40):
                candidate = selection.plan(root, "build", components=["app", "docs"])
            self.assertEqual([gate["id"] for gate in candidate["gates"]],
                             ["source:unit", "docs:static", "bundle:build"])
