"""Regression checks for isolated CLI integration gates."""

import json
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


class CliIntegrationGateTests(unittest.TestCase):
    def test_cli_database_is_scoped_to_cli_step(self) -> None:
        workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
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
        workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
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
        self.assertIn("'.base-revision'", source_hashes)
        self.assertIn("'.namespace-base-revision'", source_hashes)


if __name__ == "__main__":
    unittest.main()
