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
                self.assertNotIn('--exclude=**/vitest.config.ts', copies(build)[0])
                self.assertNotIn('--exclude=vite.config.ts', copies(build)[0])
                self.assertIn('pnpm test:unit && pnpm exec tsc --noEmit', unit)
                self.assertIn('from=unit,source=/tmp/frontend-unit-check', build)

    def test_frontend_test_config_selection_skips_compile_and_keeps_production_neighbors(self):
        spec = importlib.util.spec_from_file_location('forge_frontend_selector', ROOT / 'scripts/vendor/base_quality_selection.py')
        selection = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(selection)
        manifest = selection.load(ROOT)
        contract = next(gate for gate in manifest['gates'] if gate['id'] == 'tooling:build-contracts')
        for consumed in ('frontend/vite.config.ts', 'frontend/vitest.config.ts', 'frontend/tsconfig.json',
                         'frontend/Dockerfile', 'frontend/Dockerfile.umbrella'):
            self.assertTrue(selection.matches(consumed, contract['inputs']), consumed)
        build = next(gate for gate in manifest['gates'] if gate['id'] == 'frontend:build')
        path = 'frontend/vitest.config.ts'
        self.assertTrue(selection.matches(path, build['exclude_inputs']))
        self.assertTrue(selection.matches(path, manifest['frontend_unit_inputs']))
        cases = [([path], False), (['frontend/src/test-setup.ts'], False)]
        cases.extend(([neighbor], True) for neighbor in ('frontend/vite.config.ts',
                     'frontend/src/main.tsx', 'frontend/tsconfig.json', 'frontend/package.json',
                     'frontend/pnpm-lock.yaml', 'frontend/nested/vitest.config.ts'))
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

class ServerCliMetadataTests(unittest.TestCase):
    def test_server_graph_keeps_unselected_cli_metadata_and_a_compilation_guard(self):
        import re
        import shlex
        import tomllib
        from pathlib import Path

        product = Path(__file__).resolve().parents[2]
        backend = product / 'backend'
        workspace = tomllib.loads((backend / 'Cargo.toml').read_text(encoding='utf-8'))
        cli = tomllib.loads((backend / 'cli/Cargo.toml').read_text(encoding='utf-8'))
        self.assertEqual(cli['bin'], [{'name': 'cicd-cli', 'path': 'src/main.rs'}])
        pending, seen = [backend], set()
        while pending:
            directory = pending.pop().resolve()
            if directory in seen:
                continue
            seen.add(directory)
            manifest = tomllib.loads((directory / 'Cargo.toml').read_text(encoding='utf-8'))
            for table in [manifest, *manifest.get('target', {}).values()]:
                for kind in ('dependencies', 'build-dependencies'):
                    for name, dependency in table.get(kind, {}).items():
                        if not isinstance(dependency, dict):
                            continue
                        owner = directory
                        if dependency.get('workspace'):
                            dependency = workspace['workspace']['dependencies'][name]
                            owner = backend
                        if isinstance(dependency, dict) and 'path' in dependency:
                            path = (owner / dependency['path']).resolve()
                            if path.is_relative_to(backend):
                                pending.append(path)
        self.assertNotIn((backend / 'cli').resolve(), seen,
                         'A production CLI dependency requires a new source closure')
        self.assertTrue((backend / 'cli/src/main.rs').is_file(), 'Real CLI remains available separately')
        for name in ('Dockerfile', 'Dockerfile.umbrella'):
            with self.subTest(recipe=name):
                text = (backend / name).read_text(encoding='utf-8')
                copied = [item for line in text.splitlines() if line.startswith('COPY ')
                          for item in shlex.split(line)[1:-1] if item.startswith('CI-CD/backend/cli')]
                self.assertEqual(copied, ['CI-CD/backend/cli/Cargo.toml'])
                self.assertIn('compile_error!("metadata-only cicd-cli must not be compiled");', text)
                self.assertNotRegex(text, r'rm -rf --[^\n]*\bcli/src\b')
                self.assertNotIn('> cli/src/lib.rs', text, 'Do not introduce an empty auto-library')
                shipped = set(re.findall(r'COPY --from=build /app/target/release/([A-Za-z0-9_-]+)', text))
                self.assertEqual(shipped, {'cicd-server', 'forge-runner'})
                builds = re.findall(r'(?:/usr/local/cargo/bin/)?cargo build ([^\n\\]+)', text)
                self.assertEqual(len(builds), 2)
                for command in builds:
                    args = shlex.split(command)
                    self.assertEqual({args[i + 1] for i, value in enumerate(args) if value == '--bin'}, shipped)
                    self.assertIn('--locked', args)
                    self.assertIn('--release', args)



class RustCiJobReuseTests(unittest.TestCase):
    @staticmethod
    def job(name):
        import re
        source = (ROOT / '.github/workflows/ci.yml').read_text(encoding='utf-8')
        match = re.search(r'^  ' + re.escape(name) + r':\n(.*?)(?=^  [a-zA-Z][\w-]*:|\Z)',
                          source, re.M | re.S)
        if match is None:
            raise AssertionError('Missing CI job: ' + name)
        return match.group(1)

    def test_one_workspace_owns_rust_preparation_and_all_original_checks(self):
        backend = self.job('backend')
        summary = self.job('minimum-rust')
        self.assertIn("needs: changes", backend)
        self.assertIn("if: needs.changes.outputs.backend == 'true'", backend)
        self.assertEqual(backend.count('uses: dtolnay/rust-toolchain@stable'), 1)
        self.assertEqual(backend.count('toolchain: 1.88.0'), 1)
        self.assertEqual(backend.count('uses: Swatinem/rust-cache@v2'), 1)
        self.assertEqual(backend.count('run: python3 ../scripts/vendor/base_rust_target_cache.py'), 1)
        self.assertEqual(backend.count('uses: actions/checkout@'), 2)
        self.assertNotIn('uses:', summary)
        self.assertNotIn('cargo ', summary)
        self.assertIn('needs: [changes, backend]', summary)
        self.assertIn("if: always() && needs.changes.outputs.backend == 'true'", summary)

    def test_minimum_check_keeps_scope_environment_and_unit_failure_order(self):
        backend = self.job('backend')
        source = (ROOT / '.github/workflows/ci.yml').read_text(encoding='utf-8')
        minimum = 'env -u SQLX_OFFLINE cargo check --locked --workspace --all-targets'
        self.assertEqual(source.count('cargo check --locked --workspace --all-targets'), 1)
        self.assertIn(minimum, backend)
        self.assertLess(backend.index('cargo test --locked'), backend.index(minimum))
        self.assertLess(backend.index(minimum), backend.index('run: cargo build --locked'))
        self.assertIn("if: needs.changes.outputs.backend_build == 'true'", backend)
        self.assertIn('SQLX_OFFLINE: "true"', backend)
        self.assertNotIn('continue-on-error:', backend)

    def test_legacy_status_rejects_failure_cancellation_skip_and_missing_result(self):
        import os
        import shutil
        import subprocess
        if os.name == 'nt':
            # Use Git Bash explicitly; never launch the Windows WSL bash shim.
            bash = Path(os.environ.get('ProgramFiles', 'C:/Program Files')) / 'Git/bin/bash.exe'
            if not bash.is_file():
                self.skipTest('Git Bash is unavailable; run this shell contract on Linux')
        else:
            bash = shutil.which('bash')
            if not bash:
                self.skipTest('Bash is unavailable')
        summary = self.job('minimum-rust')
        self.assertIn('shell: bash', summary)
        self.assertIn('BACKEND_RESULT: ${{ needs.backend.result }}', summary)
        commands = [line.strip()[5:] for line in summary.splitlines()
                    if line.strip().startswith('run: ')]
        self.assertEqual(len(commands), 1)
        for result in ('success', 'failure', 'cancelled', 'skipped', '', 'unknown'):
            with self.subTest(result=result):
                process = subprocess.run([str(bash), '--noprofile', '--norc', '-c', commands[0]],
                    env={**os.environ, 'BACKEND_RESULT': result}, capture_output=True, timeout=10)
                self.assertEqual(process.returncode == 0, result == 'success')

if __name__ == "__main__":
    unittest.main()
