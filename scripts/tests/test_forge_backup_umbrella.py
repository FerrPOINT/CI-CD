#!/usr/bin/env python3
"""Regression coverage for Base umbrella backup service selection."""

from __future__ import annotations

import subprocess
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/forge_backup.py"


class UmbrellaBackupServiceTests(unittest.TestCase):
    def test_dry_run_targets_the_actual_umbrella_service_names(self) -> None:
        result = subprocess.run(
            [
                sys.executable,
                str(SCRIPT),
                "backup",
                "--project-dir",
                "/opt/dev/sdlc",
                "--compose-file",
                "/opt/dev/sdlc/docker-compose.local.yml",
                "--env-file",
                "/opt/dev/sdlc/.env",
                "--postgres-service",
                "cicd-postgres",
                "--backend-service",
                "cicd-backend",
                "--frontend-service",
                "cicd-frontend",
                "--dry-run",
                "--skip-git-fsck",
                "--backup-dir",
                "/tmp/forge-umbrella-backup-test",
            ],
            check=True,
            text=True,
            capture_output=True,
        )
        self.assertIn("ps -q cicd-backend", result.stdout)
        self.assertIn("stop cicd-frontend cicd-backend", result.stdout)
        self.assertIn("exec -T cicd-postgres pg_dump", result.stdout)


if __name__ == "__main__":
    unittest.main()
