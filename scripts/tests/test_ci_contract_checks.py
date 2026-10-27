"""Regression checks for isolated integration fixture environments and CI gates."""

import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


class CliIntegrationGateTests(unittest.TestCase):
    def test_full_release_keeps_addressed_mode_in_one_manual_callee(self) -> None:
        release = (ROOT / '.github/workflows/release.yml').read_text(encoding='utf-8')
        qualification = (ROOT / '.github/workflows/qualification.yml').read_text(encoding='utf-8')
        self.assertIn('uses: ./.github/workflows/qualification.yml', release)
        self.assertIn('git_receive_only: false', release)
        self.assertNotIn('concurrency:', release)
        self.assertIn('workflow_dispatch:', qualification)
        self.assertIn('workflow_call:', qualification)
        self.assertIn('git_receive_only:', qualification)

    def test_sdk_guards_share_one_verified_checkout_before_rust_setup(self) -> None:
        workflow = (ROOT / '.github/workflows/ci.yml').read_text(encoding='utf-8')
        docs = workflow.split('\n  docs:\n', 1)[1].split('\n  backend:\n', 1)[0]
        backend = workflow.split('\n  backend:\n', 1)[1].split('\n  frontend:\n', 1)[0]
        self.assertNotIn('repository: FerrPOINT/services-base', docs)
        self.assertEqual(backend.count('repository: FerrPOINT/services-base'), 1)
        verification = backend.index('name: Verify pinned Base checkout')
        rust = backend.index('uses: dtolnay/rust-toolchain@')
        for guard in ['Pinned SDK import boundary regressions', 'Native QA Compose ownership regressions']:
            self.assertLess(verification, backend.index('name: ' + guard))
            self.assertLess(backend.index('name: ' + guard), rust)

    def test_cli_database_is_scoped_to_cli_step(self) -> None:
        workflow = (ROOT / ".github/workflows/qualification.yml").read_text(encoding="utf-8")
        backend = workflow.split("\n  backend:\n", 1)[1].split("\n  frontend:\n", 1)[0]
        job_env = backend.split("\n    env:\n", 1)[1].split("\n    steps:\n", 1)[0]
        self.assertIn("/forge_test_cicd", job_env)
        self.assertNotIn("forge_test_cli", job_env)
        self.assertIn("createdb -h 127.0.0.1 -p 5432 -U forge_owner forge_test_cli", backend)
        cli_step = backend.split("- name: CLI tests (real PostgreSQL)", 1)[1].split("- name:", 1)[0]
        self.assertIn("/forge_test_cli", cli_step)
        self.assertIn(
            "cargo test --locked -p cicd-cli --features integration --test cli_real_api -- --test-threads=1",
            cli_step,
        )
        self.assertIn(
            "cargo clippy --locked -p cicd-cli --all-targets --features integration -- -D warnings",
            backend,
        )
        self.assertIn("cargo test --locked --features integration --test integration_db -- --test-threads=1", backend)


    def test_docs_job_runs_cli_gate_regressions(self) -> None:
        workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        docs = workflow.split("\n  docs:\n", 1)[1].split("\n  backend:\n", 1)[0]
        self.assertIn("scripts.tests.test_ci_contract_checks", docs)


class RepositorySyncFixtureEnvironmentTests(unittest.TestCase):
    def test_namespace_hosted_sync_fixtures_have_explicit_public_git_origin(self) -> None:
        workflow = (ROOT / ".github/workflows/qualification.yml").read_text(encoding="utf-8")
        backend = workflow.split("\n  backend:\n", 1)[1].split("\n  frontend:\n", 1)[0]
        job_env = backend.split("\n    env:\n", 1)[1].split("\n    steps:\n", 1)[0]
        # Namespace-bound hosted readback needs an origin even when the fixture
        # never makes a network request. Keep it in the PostgreSQL job scope.
        self.assertIn("CICD_NAMESPACE__INSTANCE_ID:", job_env)
        self.assertIn("CICD_NAMESPACE__REGISTRY_INSTANCE_ID:", job_env)
        self.assertIn("CICD_NAMESPACE__PUBLIC_GIT_ORIGIN: https://forge-qa.invalid", job_env)





if __name__ == "__main__":
    unittest.main()
