#!/usr/bin/env python3
"""Regression checks for the runner image's Docker CLI supply-chain pin."""

from __future__ import annotations

import unittest
from pathlib import Path

DOCKERFILE = Path(__file__).resolve().parents[2] / "backend/Dockerfile.umbrella"


class BackendDockerfileTests(unittest.TestCase):
    def test_runner_cli_pin_covers_the_critical_tls_fix(self) -> None:
        dockerfile = DOCKERFILE.read_text(encoding="utf-8")
        self.assertIn("ARG DOCKER_CLI_VERSION=29.5.0", dockerfile)


if __name__ == "__main__":
    unittest.main()
