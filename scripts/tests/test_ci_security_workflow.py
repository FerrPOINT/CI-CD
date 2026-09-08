#!/usr/bin/env python3
"""Regression coverage for the blocking CI supply-chain gate."""

from __future__ import annotations

import unittest
from pathlib import Path

WORKFLOW = Path(__file__).resolve().parents[2] / ".github/workflows/ci.yml"


class SecurityWorkflowTests(unittest.TestCase):
    def test_security_job_runs_all_documented_blocking_gates(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")
        security_job = workflow.split("\n  security:\n", 1)[1].split("\n  docs:\n", 1)[0]

        for command in (
            "cargo install cargo-audit --locked",
            "if cargo tree -i sqlx-mysql --edges features --target all 2>/dev/null | grep -q .; then exit 1; fi",
            "if cargo tree -i rsa --target all 2>/dev/null | grep -q .; then exit 1; fi",
            "cargo audit --ignore RUSTSEC-2023-0071",
            "pnpm install --frozen-lockfile",
            "pnpm audit --audit-level high",
            "python3 scripts/scan_secrets.py",
            "python3 scripts/generate_sbom.py --check",
            "scripts/scan_container_images.sh",
        ):
            self.assertIn(command, security_job)


if __name__ == "__main__":
    unittest.main()
