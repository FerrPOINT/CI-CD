#!/usr/bin/env python3
"""Regression tests for documentation drift checks."""

from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "verify_docs.py"
SPEC = importlib.util.spec_from_file_location("verify_docs", SCRIPT)
assert SPEC and SPEC.loader
verify_docs = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(verify_docs)


class CapabilityStatusRegressionTests(unittest.TestCase):
    def setUp(self) -> None:
        verify_docs.problems.clear()

    def test_canonical_docs_do_not_describe_shipped_post_wave4_features_as_target(self) -> None:
        assertions = {
            "docs/CURRENT_STATE.md": (
                "resumable artifact sessions остаются target",
                "profile/keyring/YAML/NDJSON",
                "off-site/PITR/monthly drill остаются target",
            ),
            "docs/DELIVERY_ARCHITECTURE.md": (
                "Profiles, keyring, generated DTO/client, request tracing, NDJSON",
                "OTLP, alerting и корреляция API--CLI не реализованы",
            ),
            "docs/API.md": (
                "нет resumable/chunked artifact sessions",
                "one-shot artifact upload",
            ),
        }
        for relative_path, forbidden in assertions.items():
            content = (verify_docs.ROOT / relative_path).read_text(encoding="utf-8")
            for phrase in forbidden:
                self.assertNotIn(phrase, content, f"stale current-state claim in {relative_path}")


if __name__ == "__main__":
    unittest.main()
