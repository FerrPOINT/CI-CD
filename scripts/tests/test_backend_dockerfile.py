#!/usr/bin/env python3
"""Regression checks for runner pins and compile-time delivery witnesses."""

from __future__ import annotations

import unittest
import re
import shlex
from pathlib import Path

DOCKERFILE = Path(__file__).resolve().parents[2] / "backend/Dockerfile.umbrella"


def staged_paths(dockerfile: str, required: set[str]) -> set[str]:
    staged = set()
    for line in dockerfile.replace("\\\n", " ").splitlines():
        if not line.startswith("COPY services-base/"):
            continue
        *sources, destination = shlex.split(line)[1:]
        for source in sources:
            for path in required:
                if path == source:
                    target = destination + Path(source).name if destination.endswith("/") else destination
                elif path.startswith(source.rstrip("/") + "/"):
                    target = destination.rstrip("/") + path[len(source.rstrip("/")):]
                else:
                    continue
                if target == "/" + path:
                    staged.add(path)
    return staged


class BackendDockerfileTests(unittest.TestCase):
    def test_all_embedded_base_helpers_are_staged_at_the_include_path(self) -> None:
        root = DOCKERFILE.parents[1]
        source = (root / "backend/src/task_delivery/postgres.rs").read_text(encoding="utf-8")
        required = set(re.findall(r'include_bytes!\("(?:\.\./)+(services-base/[^\"]+)"\)', source))
        required.update("services-base/" + path for path in re.findall(r'pinned_base_helper!\("([^\"]+)"\)', source))
        self.assertTrue(required, "The privileged coordinator must pin its Base helpers")
        for name in ("Dockerfile", "Dockerfile.umbrella"):
            dockerfile = (root / "backend" / name).read_text(encoding="utf-8")
            with self.subTest(dockerfile=name):
                self.assertFalse(
                    required - staged_paths(dockerfile, required),
                    "Docker build must stage every embedded Base helper at its include path",
                )

    def test_missing_transitive_copy_or_wrong_destination_is_detected(self) -> None:
        required = {"services-base/scripts/platform_namespace.py", "services-base/base-operator/src/pdlc_operator/backup/postgres.py"}
        copies = "COPY services-base/scripts /services-base/scripts\nCOPY services-base/base-operator/src /services-base/base-operator/src\n"
        self.assertEqual(staged_paths(copies, required), required)
        self.assertEqual(staged_paths(copies.splitlines()[0], required), {"services-base/scripts/platform_namespace.py"})
        self.assertEqual(staged_paths(copies.replace(" /services-base/", " /wrong/"), required), set())

    def test_explicit_files_are_supported_without_accepting_a_sibling(self) -> None:
        required = {"services-base/scripts/platform_namespace.py", "services-base/scripts/_platform_namespace_contract.py"}
        copies = "COPY services-base/scripts/platform_namespace.py /services-base/scripts/\n"
        self.assertEqual(staged_paths(copies, required), {"services-base/scripts/platform_namespace.py"})

    def test_runner_cli_pin_covers_the_critical_tls_fix(self) -> None:
        dockerfile = DOCKERFILE.read_text(encoding="utf-8")
        self.assertIn("ARG DOCKER_CLI_VERSION=29.5.0", dockerfile)


if __name__ == "__main__":
    unittest.main()
