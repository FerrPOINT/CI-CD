#!/usr/bin/env python3
"""Regression coverage for the Base stand backup payload."""

from __future__ import annotations

import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "backup-stand.sh"


class BaseStandBackupTests(unittest.TestCase):
    def test_backups_cover_all_databases_and_persistent_payloads(self) -> None:
        content = SCRIPT.read_text(encoding="utf-8")
        for database in ("task-tracker", "wiki", "fleet-control", "cicd", "java-agent"):
            self.assertIn(f"[{database}]", content)
        self.assertIn("sdlc-local_tt_uploads", content)
        self.assertIn("sdlc-local_ja_chromium", content)
        self.assertNotIn("sdlc-local_ja_data", content)


if __name__ == "__main__":
    unittest.main()
