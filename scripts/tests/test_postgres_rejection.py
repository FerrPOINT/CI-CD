"""PostgreSQL owner rejection remains readable when a pointer is unavailable."""
import importlib.util
import io
import json
import sys
import tempfile
import types
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]


class PostgresRejectionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        spec = importlib.util.spec_from_file_location('postgres_delivery_rejection', ROOT / 'scripts/postgres-delivery.py')
        cls.module = importlib.util.module_from_spec(spec)
        # No fixture invokes the Linux constructor or its advisory lock.
        platform_imports = {'fcntl': types.ModuleType('fcntl')} if sys.platform == 'win32' else {}
        with patch.dict(sys.modules, platform_imports):
            spec.loader.exec_module(cls.module)

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='forge-rejection-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / 'operations').mkdir()
        (self.root / 'current.json').write_text(json.dumps('a' * 64), encoding='utf-8')
        (self.root / 'confirmed.json').write_text(json.dumps('b' * 64), encoding='utf-8')

    def reject(self):
        root, module = self.root, self.module

        class FixtureOwner(module.Owner):
            def __init__(self, packet):
                self.root = root
                self.dir = root / 'operations' / 'new-operation'
                self.packet = {'readOnly': False, 'reconcile': False}
                self.command = {'expectedManifestSha256': 'c' * 64}

        output = io.BytesIO()
        streams = types.SimpleNamespace(stdin=types.SimpleNamespace(buffer=io.BytesIO(b'{}')),
                                        stdout=types.SimpleNamespace(buffer=output))
        before = {p.name: p.read_bytes() for p in root.iterdir() if p.is_file()}
        with patch.object(module, 'Owner', FixtureOwner), patch.object(module, 'sys', streams):
            module.main()
        self.assertEqual(before, {p.name: p.read_bytes() for p in root.iterdir() if p.is_file()})
        result = json.loads(output.getvalue())
        self.assertEqual(result['schema'], 'forge/local-postgres-rejection/v1')
        self.assertEqual(result['status'], 'blocked')
        self.assertFalse(result['dispatchAllowed'])
        self.assertFalse(result['sdlcAcceptanceVerified'])
        return result

    def assert_unavailable(self, result):
        self.assertEqual(result['reason'], 'manifest_pointer_unavailable')
        self.assertNotIn('currentManifestSha256', result)
        self.assertNotIn('confirmedManifestSha256', result)

    def test_corrupt_current_pointer_keeps_a_json_rejection(self):
        (self.root / 'current.json').write_text('{', encoding='utf-8')
        self.assert_unavailable(self.reject())

    def test_corrupt_confirmed_pointer_does_not_export_a_partial_pair(self):
        (self.root / 'confirmed.json').write_text('{', encoding='utf-8')
        self.assert_unavailable(self.reject())

    def test_oversized_pointer_keeps_a_bounded_json_rejection(self):
        (self.root / 'current.json').write_bytes(b' ' * (128 * 1024 + 1))
        self.assert_unavailable(self.reject())

    def test_unreadable_pointer_keeps_a_json_rejection(self):
        read = self.module.read

        def unavailable(path, *args, **kwargs):
            if path.name == 'current.json':
                raise PermissionError('fixture pointer unavailable')
            return read(path, *args, **kwargs)

        with patch.object(self.module, 'read', unavailable):
            self.assert_unavailable(self.reject())

    def test_valid_pair_preserves_the_original_rejection_and_hashes(self):
        result = self.reject()
        self.assertEqual(result['reason'], 'current_manifest_cas_mismatch')
        self.assertEqual(result['currentManifestSha256'], 'a' * 64)
        self.assertEqual(result['confirmedManifestSha256'], 'b' * 64)

    def test_absent_pointers_remain_distinct_from_unreadable_pointers(self):
        (self.root / 'current.json').unlink()
        (self.root / 'confirmed.json').unlink()
        result = self.reject()
        self.assertEqual(result['reason'], 'current_manifest_cas_mismatch')
        self.assertIsNone(result['currentManifestSha256'])
        self.assertIsNone(result['confirmedManifestSha256'])


if __name__ == '__main__':
    unittest.main()
