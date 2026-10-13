import runpy
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


BUILD_SCRIPT = Path(__file__).resolve().parents[2] / "scripts/build.py"


class BuildPinTests(unittest.TestCase):
    def invoke(self, targets, pins, *, dev_compose=False, reject=False):
        with tempfile.TemporaryDirectory() as directory:
            product = Path(directory) / "product"
            script = product / "scripts/build.py"
            script.parent.mkdir(parents=True)
            shutil.copyfile(BUILD_SCRIPT, script)
            for name, value in pins.items():
                (product / name).write_text(value, encoding="utf-8")
            if dev_compose:
                (product / "docker-compose.dev.yml").write_text("services: {}")
            with patch.object(sys, "argv", [str(script), *targets]):
                with patch("subprocess.run") as command:
                    if reject:
                        command.side_effect = subprocess.CalledProcessError(1, "verify")
                        with self.assertRaises(subprocess.CalledProcessError):
                            runpy.run_path(str(script), run_name="__main__")
                        self.assertEqual(command.call_count, 1)
                        return
                    runpy.run_path(str(script), run_name="__main__")
                    self.assertEqual(command.call_count, 2)
                    verifier, build = command.call_args_list
                    args = verifier.args[0]
                    self.assertTrue(verifier.kwargs["check"])
                    self.assertEqual(
                        args[args.index("--base") + 1],
                        str(product.parent / "services-base"),
                    )
                    selected = Path(args[args.index("--revision") + 1])
                    self.assertEqual(selected.parent, product)
                    compose = ["docker", "compose"]
                    if dev_compose:
                        compose += ["-f", "docker-compose.dev.yml"]
                    self.assertEqual(build.args[0], [*compose, "build", *targets])
                    self.assertEqual(build.kwargs, {"cwd": product, "check": True})
                    return selected.name

    def test_frontend_uses_independent_ui_pin(self):
        self.assertEqual(
            self.invoke(
                ["frontend"],
                {
                    ".base-ui-revision": "1" * 40,
                    ".namespace-base-revision": "2" * 40,
                    ".base-revision": "3" * 40,
                },
            ),
            ".base-ui-revision",
        )

    def test_backend_keeps_namespace_sdk_pin(self):
        self.assertEqual(
            self.invoke(
                ["backend"],
                {
                    ".base-ui-revision": "1" * 40,
                    ".namespace-base-revision": "2" * 40,
                    ".base-revision": "3" * 40,
                },
            ),
            ".namespace-base-revision",
        )

    def test_legacy_frontend_falls_back_to_namespace_pin(self):
        self.assertEqual(
            self.invoke(
                ["frontend"],
                {".namespace-base-revision": "2" * 40, ".base-revision": "3" * 40},
            ),
            ".namespace-base-revision",
        )

    def test_ui_pin_does_not_require_namespace_pin(self):
        self.assertEqual(
            self.invoke(
                ["frontend"],
                {".base-ui-revision": "1" * 40, ".base-revision": "3" * 40},
                dev_compose=True,
            ),
            ".base-ui-revision",
        )

    def test_backend_without_namespace_keeps_legacy_pin(self):
        self.assertEqual(
            self.invoke(
                ["backend"], {".base-ui-revision": "1" * 40, ".base-revision": "3" * 40}
            ),
            ".base-revision",
        )

    def test_frontend_without_new_pins_keeps_legacy_pin(self):
        self.assertEqual(
            self.invoke(["frontend"], {".base-revision": "3" * 40}), ".base-revision"
        )

    def test_verification_failure_never_starts_docker_build(self):
        self.invoke(["frontend"], {".base-ui-revision": "1" * 40}, reject=True)


if __name__ == "__main__":
    unittest.main()
