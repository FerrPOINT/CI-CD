"""Regression checks for isolated CLI integration gates."""

import hashlib
import importlib.util
import json
import re
import shlex
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]


class CliIntegrationGateTests(unittest.TestCase):
    def test_frontend_test_config_keeps_the_existing_policy_separate_from_vite(self):
        frontend = ROOT / 'frontend'
        vite = (frontend / 'vite.config.ts').read_text(encoding='utf-8')
        vitest = (frontend / 'vitest.config.ts').read_text(encoding='utf-8')
        self.assertNotIn('vitest/config', vite)
        self.assertNotRegex(vite, r'\btest\s*:')
        self.assertIn("import { defineConfig, mergeConfig } from 'vitest/config'", vitest)
        self.assertIn("import viteConfig from './vite.config'", vitest)
        self.assertRegex(vitest, r'export default mergeConfig\(\s*viteConfig,\s*defineConfig\(')
        test = re.search(r'test: \{(.*?)\n    \}', vitest, re.S)
        self.assertIsNotNone(test)
        self.assertEqual([line.strip() for line in test[1].splitlines() if line.strip()], [
            "environment: 'jsdom',", 'maxWorkers: 4,',
            "include: ['src/**/*.{test,spec}.{ts,tsx}'],",
            "setupFiles: ['./src/test-setup.ts'],",
        ])
        includes = json.loads((frontend / 'tsconfig.json').read_text())['include']
        self.assertIn('vite.config.ts', includes)
        self.assertIn('vitest.config.ts', includes)

    def test_frontend_test_config_stays_in_unit_copy_and_out_of_application_copy(self):
        for name in ('Dockerfile', 'Dockerfile.umbrella'):
            with self.subTest(recipe=name):
                text = (ROOT / 'frontend' / name).read_text(encoding='utf-8')
                unit = text.split('FROM dependencies AS unit\n', 1)[1].split('FROM dependencies AS build\n', 1)[0]
                build = text.split('FROM dependencies AS build\n', 1)[1].split('\nFROM nginx:', 1)[0]
                copies = lambda stage: [shlex.split(line) for line in stage.splitlines()
                                        if line.startswith('COPY ') and 'CI-CD/frontend/ ./' in line]
                self.assertEqual(len(copies(unit)), 1)
                self.assertEqual(len(copies(build)), 1)
                self.assertFalse(any('vitest.config.ts' in arg for arg in copies(unit)[0]))
                self.assertIn('--exclude=vitest.config.ts', copies(build)[0])
                self.assertIn('--exclude=**/vitest.config.ts', copies(build)[0])
                self.assertNotIn('--exclude=vite.config.ts', copies(build)[0])
                self.assertIn('pnpm test:unit && pnpm exec tsc --noEmit', unit)
                self.assertIn('from=unit,source=/tmp/frontend-unit-check', build)

    def test_frontend_test_config_selection_skips_compile_and_keeps_production_neighbors(self):
        spec = importlib.util.spec_from_file_location('forge_frontend_selector', ROOT / 'scripts/vendor/base_quality_selection.py')
        selection = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(selection)
        manifest = selection.load(ROOT)
        build = next(gate for gate in manifest['gates'] if gate['id'] == 'frontend:build')
        path = 'frontend/vitest.config.ts'
        self.assertTrue(selection.matches(path, build['exclude_inputs']))
        self.assertTrue(selection.matches(path, manifest['frontend_unit_inputs']))
        cases = [([path], False), (['frontend/src/test-setup.ts'], False)]
        cases.extend(([neighbor], True) for neighbor in ('frontend/vite.config.ts',
                     'frontend/src/main.tsx', 'frontend/tsconfig.json', 'frontend/package.json',
                     'frontend/pnpm-lock.yaml'))
        cases.extend([([path, 'frontend/vite.config.ts'], True),
                      ([path, 'backend/src/main.rs'], False)])
        for paths, compile_frontend in cases:
            with self.subTest(paths=paths), patch.object(selection, 'git', return_value='\n'.join(paths)):
                gates = {gate['id'] for gate in selection.plan(ROOT, profile='build', changed=paths)['gates']}
                self.assertIn('frontend:tests', gates)
                self.assertEqual('frontend:build' in gates, compile_frontend)
                for event in ('push', 'pull_request'):
                    outputs = selection.ci_outputs(event, 'a' * 40, 'b' * 40, repo=ROOT)
                    self.assertEqual(outputs['frontend'], 'true')
                    self.assertEqual(outputs['frontend_build'], str(compile_frontend).lower())
                    self.assertEqual(outputs['backend'], str('backend/src/main.rs' in paths).lower())

    def test_build_contract_test_edits_select_only_the_static_tooling_gate(self):
        spec = importlib.util.spec_from_file_location('forge_packaged_selector', ROOT / 'scripts/vendor/base_quality_selection.py')
        selection = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(selection)
        for path in ('scripts/tests/test_backend_dockerfile.py', 'scripts/tests/test_ci_contract_checks.py'):
            with self.subTest(path=path):
                plan = selection.plan(ROOT, profile='build', changed=[path])
                self.assertEqual([gate['id'] for gate in plan['gates']], ['tooling:build-contracts'])
        for path, required in (('backend/src/main.rs', 'backend:build'),
                               ('frontend/src/main.tsx', 'frontend:build')):
            self.assertIn(required, [gate['id'] for gate in selection.plan(ROOT, profile='build', changed=[path])['gates']])
        unknown = selection.plan(ROOT, profile='build', changed=['scripts/tests/future_unknown.py'])
        self.assertTrue({'backend:build', 'frontend:build'} <= {gate['id'] for gate in unknown['gates']})
        workflow = (ROOT / '.github/workflows/ci.yml').read_text(encoding='utf-8')
        docs = workflow.split('\n  docs:\n', 1)[1].split('\n  minimum-rust:\n', 1)[0]
        self.assertIn('scripts.tests.test_backend_dockerfile scripts.tests.test_ci_contract_checks.CliIntegrationGateTests', docs)

    def test_ordinary_build_selects_runtime_and_openapi_bins_without_workspace_defaults(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        step = workflow.split("- name: Build changed application after isolated units", 1)[1].split("- name:", 1)[0]
        command = re.search(r"run: (cargo build [^\n]+)", step)
        self.assertIsNotNone(command)
        arguments = shlex.split(command[1])
        self.assertNotIn("--workspace", arguments)
        self.assertIn("--locked", arguments)
        self.assertEqual({arguments[i + 1] for i, arg in enumerate(arguments) if arg == "--bin"},
                         {"cicd-server", "forge-runner", "openapi-dump"})

    def test_cli_database_is_scoped_to_cli_step(self) -> None:
        workflow = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        backend = workflow.split("\n  backend:\n", 1)[1].split("\n  frontend:\n", 1)[0]
        job_env = backend.split("\n    env:\n", 1)[1].split("\n    steps:\n", 1)[0]
        self.assertIn("/forge_test_cicd", job_env)
        self.assertNotIn("forge_test_cli", job_env)
        self.assertIn("createdb -h 127.0.0.1 -p 5432 -U forge_owner forge_test_cli", backend)
        cli_step = backend.split("- name: CLI tests (real PostgreSQL)", 1)[1].split("- name:", 1)[0]
        self.assertIn("/forge_test_cli", cli_step)
        self.assertIn(
            "cargo test --locked -p cicd-cli --features integration --test cli_real_api -- --test-threads=1",
            cli_step,
        )
        self.assertIn(
            "cargo clippy --locked -p cicd-cli --all-targets --features integration -- -D warnings",
            backend,
        )
        self.assertIn("cargo test --locked --features integration --test integration_db -- --test-threads=1", backend)

    def test_docs_job_runs_cli_gate_regressions(self) -> None:
        workflow = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        docs = workflow.split("\n  docs:\n", 1)[1].split("\n  backend:\n", 1)[0]
        self.assertIn("scripts.tests.test_ci_contract_checks", docs)


@unittest.skipUnless(shutil.which("pwsh"), "PowerShell 7 is required for native QA preflight")
class DeliveryPinPreflightTests(unittest.TestCase):
    def selected_pin(self, namespace: bool) -> str:
        with tempfile.TemporaryDirectory(prefix="forge-pin-preflight-") as directory:
            root = Path(directory)
            repo, base = root / "CI-CD", root / "services-base"
            (repo / "scripts").mkdir(parents=True)
            (base / "scripts").mkdir(parents=True)
            script = repo / "scripts/verify-task-delivery.ps1"
            script.write_bytes((ROOT / "scripts/verify-task-delivery.ps1").read_bytes())
            (repo / ".base-revision").write_text("legacy-sdk\n", encoding="ascii")
            if namespace:
                (repo / ".namespace-base-revision").write_text("namespace-sdk\n", encoding="ascii")
            # Stop at the Base contract boundary, before any Docker operation.
            marker = base / "selected.json"
            (base / "scripts/verify_base_revision.py").write_text(
                "import json,sys\nfrom pathlib import Path\n"
                "pin=Path(sys.argv[sys.argv.index('--revision')+1])\n"
                "Path(__file__).resolve().parents[1].joinpath('selected.json').write_text(json.dumps(pin.name))\n"
                "raise SystemExit(37)\n",
                encoding="utf-8",
            )
            result = subprocess.run(
                [shutil.which("pwsh"), "-NoProfile", "-File", str(script), "-BaseRoot", str(base),
                 "-RustImage", "sha256:" + "1" * 64, "-PostgresImage", "sha256:" + "2" * 64,
                 "-TargetCache", "unused-target", "-CargoCache", "unused-cargo", "-RustupCache", "unused-rustup"],
                capture_output=True, text=True, timeout=30,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("Pinned Base validation failed", result.stderr)
            return json.loads(marker.read_text(encoding="utf-8"))

    def test_namespace_pin_is_used_before_any_docker_operation(self):
        self.assertEqual(self.selected_pin(True), ".namespace-base-revision")

    def test_legacy_checkout_keeps_its_base_pin(self):
        self.assertEqual(self.selected_pin(False), ".base-revision")

    def test_delivery_source_hashes_cover_both_sdk_pin_files(self):
        script = (ROOT / "scripts/verify-task-delivery.ps1").read_text(encoding="utf-8")
        source_hashes = script.split("function SourceHashes", 1)[1].split("$before = SourceHashes", 1)[0]
        self.assertIn("$hashes['CI-CD/' + $selectedPinName]", source_hashes)

    def test_source_hashes_include_the_selected_pin_even_when_untracked(self):
        script = (ROOT / "scripts/verify-task-delivery.ps1").read_text(encoding="utf-8")
        function = script.split("function SourceHashes {", 1)[1].split("\n$before = SourceHashes", 1)[0]
        with tempfile.TemporaryDirectory(prefix="forge-source-hashes-") as directory:
            root = Path(directory)
            repo, base = root / "CI-CD", root / "services-base"
            repo.mkdir()
            base.mkdir()
            (repo / "scripts").mkdir()
            (base / "scripts").mkdir()
            (repo / ".base-revision").write_text("legacy-sdk\n", encoding="ascii")
            (repo / ".namespace-base-revision").write_text("selected-sdk\n", encoding="ascii")
            subprocess.run(["git", "init", "-q", str(repo)], check=True)
            subprocess.run(["git", "-C", str(repo), "add", ".base-revision"], check=True)
            subprocess.run(["git", "init", "-q", str(base)], check=True)
            selected_pin = repo / ".namespace-base-revision"
            ps_quote = lambda value: "'" + str(value).replace("'", "''") + "'"
            command = (
                f"$repo = {ps_quote(repo)}; $sdk = {ps_quote(base)}; "
                f"$basePin = {ps_quote(selected_pin)}; function SourceHashes {{{function}; "
                "SourceHashes | ConvertTo-Json -Depth 3 -Compress"
            )
            result = subprocess.run(
                [shutil.which("pwsh"), "-NoProfile", "-Command", command],
                capture_output=True, text=True, timeout=30,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            hashes = json.loads(result.stdout)
            self.assertEqual(
                hashes["CI-CD/.namespace-base-revision"],
                hashlib.sha256(selected_pin.read_bytes()).hexdigest().upper(),
            )


if __name__ == "__main__":
    unittest.main()
