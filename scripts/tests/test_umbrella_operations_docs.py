#!/usr/bin/env python3
"""Regression coverage for Base umbrella operational entrypoints."""

from __future__ import annotations

import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
RUNBOOK = ROOT / "docs/runbooks/OBSERVABILITY.md"
OPERATIONS = ROOT / "docs/OPERATIONS.md"


class UmbrellaOperationsDocsTests(unittest.TestCase):
    def test_observability_runbook_uses_the_workspace_compose_file(self) -> None:
        content = RUNBOOK.read_text(encoding="utf-8")
        self.assertIn("/opt/dev/sdlc/docker-compose.local.yml", content)
        self.assertNotIn("-f docker-compose.local.yml --profile observability", content)

    def test_backup_docs_show_the_explicit_umbrella_helper_arguments(self) -> None:
        content = OPERATIONS.read_text(encoding="utf-8")
        self.assertIn("--project-dir /opt/dev/sdlc", content)
        self.assertIn("--compose-file /opt/dev/sdlc/docker-compose.local.yml", content)


if __name__ == "__main__":
    unittest.main()
