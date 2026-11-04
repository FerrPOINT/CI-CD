import hashlib
import io
import subprocess
import tarfile
import json
import shutil
import tempfile
import unittest
from pathlib import Path

from migration_lineages import project


class MigrationLineagesTest(unittest.TestCase):
    backend = Path(__file__).resolve().parents[1] / "backend"

    def test_native_applied_history_and_shared_future(self):
        files = project(self.backend)
        manifest = json.loads((self.backend / "migration-lineages/manifest.json").read_text())
        for entry in [entry for entry in manifest["main"] if int(entry["file"].split("_")[0]) <= 95] + manifest["kubernetesV1"]:
            self.assertEqual(hashlib.sha384(files[entry["file"]]).hexdigest(), entry["sha384"])
        self.assertEqual(files["0100_shared_space_lifecycle.sql"], (self.backend / "migrations/0096_shared_space_lifecycle.sql").read_bytes())
        self.assertEqual(len(files), len(list((self.backend / "migrations").glob("*.sql"))))

    def test_git_blob_transport_restores_only_manifest_declared_bytes(self):
        expected = project(self.backend)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = subprocess.check_output([
                "git", "-C", str(self.backend.parent), "archive", "HEAD",
                "backend/migrations", "backend/migration-lineages",
            ])
            with tarfile.open(fileobj=io.BytesIO(archive)) as source:
                source.extractall(root, filter="data")
            exported = root / "backend"
            # PDLC4's qualified export restores immutable Git SQL blobs after
            # archive extraction, preserving its already-applied LF history.
            for sql in exported.rglob("*.sql"):
                sql.write_bytes(subprocess.check_output([
                    "git", "-C", str(self.backend.parent), "show",
                    "HEAD:" + sql.relative_to(root).as_posix(),
                ]))
            with self.assertRaisesRegex(ValueError, "checksum changed"):
                project(exported)
            self.assertEqual(project(exported, input_format="git-blob"), expected)
            changed = exported / "migrations/0001_bootstrap_v1.sql"
            changed.write_bytes(changed.read_bytes() + b"SELECT 99;")
            with self.assertRaisesRegex(ValueError, "checksum changed"):
                project(exported, input_format="git-blob")

    def test_unknown_input_format_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "input format"):
            project(self.backend, input_format="guess")

    def test_historical_tampering_is_rejected_and_future_bytes_shared(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            shutil.copytree(self.backend / "migrations", root / "migrations")
            shutil.copytree(self.backend / "migration-lineages", root / "migration-lineages")
            future = root / "migrations/9999_future.sql"
            future.write_bytes(b"SELECT 1;\n")
            self.assertEqual(project(root)[future.name], future.read_bytes())
            original = root / "migration-lineages/kubernetes-v1/0096_kubernetes_environment_target.sql"
            original.write_bytes(original.read_bytes() + b"\n")
            with self.assertRaisesRegex(ValueError, "checksum changed"):
                project(root)


if __name__ == "__main__":
    unittest.main()
