#!/usr/bin/env python3
"""Regression checks for the runner image's Docker CLI supply-chain pin."""

from __future__ import annotations

import unittest
import re
import shlex
from pathlib import Path

DOCKERFILE = Path(__file__).resolve().parents[2] / "backend/Dockerfile.umbrella"


class BackendDockerfileTests(unittest.TestCase):
    def test_dependency_and_production_graphs_build_only_the_runtime_binaries(self) -> None:
        dockerfile = DOCKERFILE.read_text(encoding="utf-8")
        shipped = set(re.findall(r"COPY --from=build /app/target/release/([A-Za-z0-9_-]+)", dockerfile))
        builds = re.findall(r"/usr/local/cargo/bin/cargo build ([^\n\\]+)", dockerfile)
        self.assertEqual(len(builds), 2)
        self.assertTrue(shipped)
        for build in builds:
            arguments = shlex.split(build)
            self.assertEqual({arguments[i + 1] for i, value in enumerate(arguments) if value == "--bin"}, shipped)
            self.assertIn("--locked", arguments)
            self.assertIn("--release", arguments)

    def test_runner_cli_pin_covers_the_critical_tls_fix(self) -> None:
        dockerfile = DOCKERFILE.read_text(encoding="utf-8")
        self.assertIn("ARG DOCKER_CLI_VERSION=29.5.0", dockerfile)


if __name__ == "__main__":
    unittest.main()
