"""Regression checks for isolated CLI integration gates."""

import hashlib
import json
import shlex
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


class ManualGitReceiveGateTests(unittest.TestCase):
    def test_ordinary_ci_builds_only_server_runner_and_contract_targets(self) -> None:
        workflow = (ROOT / '.github/workflows/ci.yml').read_text(encoding='utf-8')
        backend = workflow.split('\n  backend:\n', 1)[1].split('\n  frontend:\n', 1)[0]
        build = backend.split('- name: Build changed application after isolated units', 1)[1].split('- name:', 1)[0]
        command = next(line.strip().removeprefix('run: ') for line in build.splitlines() if 'run: cargo build' in line)
        self.assertEqual(shlex.split(command), [
            'cargo', 'build', '--locked', '-p', 'cicd-server', '--bin', 'cicd-server',
            '--bin', 'forge-runner', '--bin', 'openapi-dump',
        ])
        condition = "if: needs.changes.outputs.backend_build == 'true'"
        self.assertIn(condition, build)
        contract = backend.split('- name: openapi drift gate', 1)[1]
        self.assertIn(condition, contract)
        self.assertIn('debug/openapi-dump', contract)
        release = (ROOT / '.github/workflows/release.yml').read_text(encoding='utf-8')
        self.assertIn('cargo build --locked --release --workspace', release)

    def test_postgres_receive_races_remain_explicit_and_isolated(self) -> None:
        workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        backend = workflow.split("\n  backend:\n", 1)[1].split("\n  frontend:\n", 1)[0]
        receive = workflow.split("\n  git-receive:\n", 1)[1]
        self.assertNotIn("services:", backend)
        self.assertNotIn("--test integration_db", backend)
        self.assertIn("cargo test --locked -p cicd-domain --lib", backend)
        self.assertIn("cargo test --locked --manifest-path tests/cache-protocol/Cargo.toml --lib", backend)
        self.assertIn("github.event_name == 'workflow_dispatch' && inputs.git_receive_only", receive)
        self.assertIn("services:", receive)
        self.assertIn("cargo clippy --locked --features integration --test integration_db -- -D warnings", receive)
        self.assertIn("--test integration_db git_sat_receive:: -- --test-threads=1", receive)
        self.assertIn("test result: ok. 4 passed; 0 failed; 0 ignored;", receive)
        for name in (
            "cancelled_http_receive_keeps_native_child_and_namespace_lease_until_exit",
            "concurrent_bootstrap_and_receive_finish_around_waiting_namespace_archive",
        ):
            self.assertIn(name, receive)
        self.assertEqual(receive.count("run: python3 ../scripts/vendor/base_rust_target_cache.py"), 1)


class CliIntegrationGateTests(unittest.TestCase):
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
