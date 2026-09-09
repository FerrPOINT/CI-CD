#!/usr/bin/env python3
"""Regression checks for observability rules and runbook semantics."""

from __future__ import annotations

import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


class ObservabilityContractTests(unittest.TestCase):
    def test_no_runner_alert_respects_embedded_runner_mode(self) -> None:
        rules = (ROOT / "deploy/observability/forge-alerts.yaml").read_text(encoding="utf-8")
        self.assertIn(
            'expr: forge_runners_online == 0 and forge_runner_embedded_enabled == 0',
            rules,
        )

    def test_runbook_explains_embedded_runner_exception(self) -> None:
        runbook = (ROOT / "docs/runbooks/OBSERVABILITY.md").read_text(encoding="utf-8")
        self.assertIn("forge_runner_embedded_enabled == 0", runbook)
        self.assertIn("CICD_EMBEDDED_RUNNER_ENABLED=true", runbook)


if __name__ == "__main__":
    unittest.main()
