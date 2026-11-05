"""Original build endpoint remains owned by its attempt after route changes."""
import base64
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("transport", Path(__file__).parents[1] / "host_image_build.py")
transport = importlib.util.module_from_spec(spec)
spec.loader.exec_module(transport)


class BuildBindingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.root.chmod(0o700)
        self.key = self.root / "source-key"
        self.key.write_bytes(b"private-fixture-key")
        self.hosts = self.root / "source-known-hosts"
        self.hosts.write_text("192.0.2.10 ssh-ed25519 " + base64.b64encode(b"first public host key").decode() + "\n")
        self.source = self.root / "source.json"
        self.data = {"schema": "forge/build-host-connection/v2", "id": "builder-a",
                     "host": "192.0.2.10", "port": 22, "user": "root",
                     "identityFile": str(self.key), "knownHostsFile": str(self.hosts),
                     "hostKeySha256": transport.fingerprint(self.hosts)}
        self.write_source()
        self.binding = self.root / "attempt-host"

    def tearDown(self):
        self.temp.cleanup()

    def write_source(self):
        self.source.write_text(json.dumps(self.data))

    def test_route_change_does_not_replace_original_host_or_credentials(self):
        selected = transport.pin_connection(self.source, self.binding)
        self.data.update(id="builder-b", host="192.0.2.20")
        self.write_source()
        self.key.write_bytes(b"replacement-key")
        retained = transport.pin_connection(self.source, self.binding)
        self.assertEqual(retained, selected)
        self.assertEqual(retained["host"], "192.0.2.10")
        self.assertEqual(Path(retained["identityFile"]).read_bytes(), b"private-fixture-key")
        metadata = json.loads((self.binding / "binding.json").read_text())
        self.assertEqual(metadata["hostId"], "builder-a")
        self.assertEqual(metadata["endpoint"], ["192.0.2.10", 22, "root"])

    def test_selected_config_hash_is_checked_before_any_binding(self):
        with self.assertRaisesRegex(ValueError, "operator intent"):
            transport.pin_connection(self.source, self.binding, "0" * 64)
        self.assertFalse(self.binding.exists())
        expected = hashlib.sha256(self.source.read_bytes()).hexdigest()
        transport.pin_connection(self.source, self.binding, expected)
        with self.assertRaisesRegex(ValueError, "original intent"):
            transport.pin_connection(self.source, self.binding, "1" * 64)

    def test_credentials_are_released_only_after_terminal_owner_is_gone(self):
        transport.pin_connection(self.source, self.binding)
        value = {"schema": "forge/host-image-build-status/v1", "phase": "running", "ownerAlive": True}
        transport.retain_terminal(self.binding, value)
        self.assertTrue((self.binding / "identity").exists())
        self.assertIsNone(transport.terminal_readback(self.binding))
        value.update(phase="failed", ownerAlive=False)
        transport.retain_terminal(self.binding, value)
        self.assertFalse((self.binding / "identity").exists())
        self.assertEqual(transport.terminal_readback(self.binding), value)
        self.assertEqual(transport.pinned_connection(self.binding)["host"], "192.0.2.10")

    def test_corruption_or_symlink_never_retargets_to_current_connection(self):
        transport.pin_connection(self.source, self.binding)
        selected = self.binding / "known_hosts"
        selected.write_text("corrupted")
        with self.assertRaisesRegex(ValueError, "bytes changed"):
            transport.pinned_connection(self.binding)
        selected.unlink()
        selected.symlink_to(self.hosts)
        with self.assertRaisesRegex(ValueError, "redirected"):
            transport.pinned_connection(self.binding)

    def test_previous_connection_schema_is_read_without_enabling_remote_defaults(self):
        self.data = {k: v for k, v in self.data.items() if k not in ("id", "hostKeySha256")}
        self.data["schema"] = "forge/build-host-connection/v1"
        self.write_source()
        self.assertEqual(transport.connection(self.source)["host"], "192.0.2.10")
        self.assertEqual(transport.pin_connection(self.source, self.binding)["host"], "192.0.2.10")

    def test_fingerprint_and_directory_permissions_fail_closed(self):
        self.data["hostKeySha256"] = "SHA256:wrong"
        self.write_source()
        with self.assertRaisesRegex(ValueError, "fingerprint"):
            transport.pin_connection(self.source, self.binding)
        self.root.chmod(0o755)
        with self.assertRaisesRegex(ValueError, "private"):
            transport.pin_connection(self.source, self.binding)


if __name__ == "__main__":
    unittest.main()
