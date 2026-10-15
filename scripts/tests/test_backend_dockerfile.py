#!/usr/bin/env python3
"""Regression checks for runner pins and compile-time delivery witnesses."""

from __future__ import annotations

import unittest
import re
import shlex
from pathlib import Path

DOCKERFILE = Path(__file__).resolve().parents[2] / "backend/Dockerfile.umbrella"


class BackendDockerfileTests(unittest.TestCase):
    def test_all_embedded_base_helpers_are_staged_at_the_include_path(self) -> None:
        root = DOCKERFILE.parents[1]
        source = (root / "backend/src/task_delivery/postgres.rs").read_text(encoding="utf-8")
        required = set(re.findall(r'include_bytes!\("(?:\.\./)+(services-base/[^\"]+)"\)', source))
        self.assertTrue(required, "The privileged coordinator must pin its Base helpers")
        for name in ("Dockerfile", "Dockerfile.umbrella"):
            dockerfile = (root / "backend" / name).read_text(encoding="utf-8")
            staged = set()
            for line in dockerfile.replace("\\\n", " ").splitlines():
                if line.startswith("COPY services-base/"):
                    *sources, destination = shlex.split(line)[1:]
                    staged.update((source, destination + Path(source).name) for source in sources)
            with self.subTest(dockerfile=name):
                self.assertFalse(
                    {(path, "/" + path) for path in required} - staged,
                    "Docker build must stage every embedded Base helper at its include path",
                )

    def test_runner_cli_pin_covers_the_critical_tls_fix(self) -> None:
        dockerfile = DOCKERFILE.read_text(encoding="utf-8")
        self.assertIn("ARG DOCKER_CLI_VERSION=29.5.0", dockerfile)


if __name__ == "__main__":
    unittest.main()
