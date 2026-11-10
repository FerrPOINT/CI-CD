import hashlib
import json
import shutil
import tempfile
import unittest
from pathlib import Path

from migration_lineages import project, reviewer_v1


class MigrationLineagesTest(unittest.TestCase):
    backend = Path(__file__).resolve().parents[1] / "backend"

    def test_installed_reviewer_history_preserves_checksum_and_shared_future(self):
        files = reviewer_v1(self.backend)
        self.assertEqual(hashlib.sha384(files["0116_pull_task_and_comments.sql"]).hexdigest(),
                         "ba466b28c6fb6ec05ededcbd71022365fe048f944d781cc7abfaa10691ac0a3b6ea852152a327bd1ac2adf15f8c1c7eb")
        self.assertEqual(files["0117_environment_purpose.sql"],
                         (self.backend / "migrations/0116_environment_purpose.sql").read_bytes())
        self.assertNotIn("0117_pull_task_and_comments.sql", files)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            shutil.copytree(self.backend / "migrations", root / "migrations")
            shutil.copytree(self.backend / "migration-lineages", root / "migration-lineages")
            (root / "migrations/0118_future.sql").write_bytes(b"SELECT 1;\n")
            self.assertEqual(reviewer_v1(root)["0118_future.sql"], b"SELECT 1;\n")
            (root / "migrations/0117_pull_task_and_comments.sql").write_bytes(b"SELECT 2;\n")
            with self.assertRaisesRegex(ValueError, "Reviewer migration checksum changed"):
                reviewer_v1(root)

    def test_native_applied_history_and_shared_future(self):
        files = project(self.backend)
        manifest = json.loads((self.backend / "migration-lineages/manifest.json").read_text())
        for entry in [entry for entry in manifest["main"] if int(entry["file"].split("_")[0]) <= 95] + manifest["kubernetesV1"]:
            self.assertEqual(hashlib.sha384(files[entry["file"]]).hexdigest(), entry["sha384"])
        self.assertEqual(files["0100_shared_space_lifecycle.sql"], (self.backend / "migrations/0096_shared_space_lifecycle.sql").read_bytes())
        self.assertEqual(len(files), len(list((self.backend / "migrations").glob("*.sql"))))

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
