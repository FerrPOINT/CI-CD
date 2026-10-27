"""Real PowerShell/Git delivery boundary; explicit manual integration."""
import hashlib
import json
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

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

