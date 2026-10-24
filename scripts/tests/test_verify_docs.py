#!/usr/bin/env python3
"""Regression tests for documentation drift checks."""

from __future__ import annotations

import importlib.util
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / "verify_docs.py"
SPEC = importlib.util.spec_from_file_location("verify_docs", SCRIPT)
assert SPEC and SPEC.loader
verify_docs = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(verify_docs)


class CapabilityStatusRegressionTests(unittest.TestCase):
    def setUp(self) -> None:
        verify_docs.problems.clear()

    def test_negative_readme_fixtures_preserve_caller_inputs(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            readme = root / 'README.md'
            original = b'# protected caller README\n'
            readme.write_bytes(original)
            os.utime(readme, ns=(1_000_000_000, 1_000_000_000))
            before = readme.stat().st_mtime_ns
            with patch.object(verify_docs, 'ROOT', root):
                for check in (self.test_readme_contract_rejects_missing_safe_proof,
                              self.test_readme_contract_rejects_mobile_gallery_images):
                    verify_docs.problems.clear()
                    check()
            self.assertEqual(readme.read_bytes(), original)
            self.assertEqual(readme.stat().st_mtime_ns, before)

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

    def test_readme_contract_rejects_missing_safe_proof(self) -> None:
        with tempfile.TemporaryDirectory() as temporary, patch.object(verify_docs, 'ROOT', Path(temporary)):
            readme = Path(temporary) / 'README.md'
            readme.write_text("# Forge\n<a name=\"overview\"></a>\n", encoding="utf-8")
            verify_docs.check_readme_contract()
            self.assertTrue(
                any(problem.startswith("README lacks required safe visual proof: 06-pipeline-detail.png")
                    for problem in verify_docs.problems)
            )

    def test_readme_contract_rejects_mobile_gallery_images(self) -> None:
        with tempfile.TemporaryDirectory() as temporary, patch.object(verify_docs, 'ROOT', Path(temporary)):
            readme = Path(temporary) / 'README.md'
            readme.write_text("# Forge\n![Mobile](docs/screenshots/375x812/wide.png)\n", encoding='utf-8')
            verify_docs.check_readme_contract()
            self.assertTrue(any("must not include mobile screenshots" in item for item in verify_docs.problems))



class ValidatorSourceRoutingTests(unittest.TestCase):
    """Exercise the packaged planner and its actual CI projection, without builders."""
    SOURCE = 'scripts/verify_docs.py'
    OWNER = 'tooling:quality-routes'
    MODULE = 'test_verify_docs'

    def plan_validator(self, paths, profile='build'):
        from unittest.mock import patch
        from scripts.vendor import base_quality_selection as selector
        root = Path(__file__).resolve().parents[2]
        def head(repository, *arguments):
            self.assertEqual(repository, root)
            self.assertEqual(arguments, ('rev-parse', 'HEAD'))
            return 'a' * 40
        with patch.object(selector, 'git', side_effect=head):
            return selector.plan(root, profile=profile, changed=paths)

    def test_validator_and_unit_paths_choose_exact_source_checks(self):
        for path in (self.SOURCE, 'scripts/tests/' + self.MODULE + '.py'):
            for profile in ('fast', 'unit', 'build'):
                with self.subTest(path=path, profile=profile):
                    candidate = self.plan_validator([path], profile)
                    expected = {self.OWNER}
                    if path == self.SOURCE and profile == 'fast':
                        expected.add('docs:contracts')
                    self.assertEqual({gate['id'] for gate in candidate['gates']}, expected)
                    gate = next(g for g in candidate['gates'] if g['id'] == self.OWNER)
                    self.assertEqual(gate['argv'].count('scripts.tests.' + self.MODULE), 1)
                    self.assertFalse(any(g.get('kind') == 'artifact' for g in candidate['gates']))

    def test_mixed_application_and_unknown_neighbor_keep_builds(self):
        for paths in ([self.SOURCE, 'backend/src/main/java/example/Production.java'],
                      [self.SOURCE, self.SOURCE.replace('.py', '_unreviewed.py')]):
            with self.subTest(paths=paths):
                candidate = self.plan_validator(paths)
                self.assertIn('backend', candidate['components'])
                self.assertTrue(any(g.get('kind') == 'artifact' for g in candidate['gates']))

    def test_ci_projection_keeps_validator_only_out_of_application_jobs(self):
        from unittest.mock import patch
        from scripts.vendor import base_quality_selection as selector
        root = Path(__file__).resolve().parents[2]
        for paths, backend in (([self.SOURCE], 'false'),
                ([self.SOURCE, 'backend/src/main/java/example/Production.java'], 'true')):
            with self.subTest(paths=paths):
                def changed(repository, *arguments):
                    self.assertEqual(arguments, ('diff', '--name-only', '--no-renames', 'a' * 40, 'b' * 40))
                    return '\n'.join(paths)
                with patch.object(selector, 'git', side_effect=changed):
                    result = selector.ci_outputs('pull_request', 'a' * 40, 'b' * 40, repo=root)
                self.assertEqual(result['backend_build'], backend)
                self.assertEqual(result['frontend_build'], 'false')


if __name__ == "__main__":
    unittest.main()
