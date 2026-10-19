"""A release must keep the standalone graph without repeating domain units."""
import shlex
import tomllib
import unittest
from pathlib import Path

from scripts.vendor import base_quality_selection as selection

ROOT = Path(__file__).resolve().parents[2]
PROTOCOL = 'cargo test --locked --manifest-path tests/cache-protocol/Cargo.toml --lib'


class BackendReleaseOwnership(unittest.TestCase):
    def gates(self, profile):
        return selection.plan(ROOT, profile=profile, changed=['backend/domain/src/lib.rs'])['gates']

    def test_release_executes_domain_once_and_retains_the_separate_protocol_workspace(self):
        gates = {gate['id']: gate for gate in self.gates('release')}
        self.assertIn('backend:quality', gates)
        self.assertNotIn('backend:unit', gates)
        self.assertIn('backend:cache-protocol-unit', gates)
        commands = [gate['argv'][-1] for gate in gates.values() if gate['id'].startswith('backend:')]
        for command in ('cargo fmt --all -- --check', 'cargo test --locked --workspace', PROTOCOL):
            self.assertEqual(1, sum(text.count(command) for text in commands), command)
        protocol = gates['backend:cache-protocol-unit']
        original = next(gate for gate in selection.load(ROOT)['gates'] if gate['id'] == 'backend:unit')
        self.assertEqual(PROTOCOL, protocol['argv'][-1])
        for field in ('argv', 'cwd', 'tools', 'sdk_revision', 'platforms', 'inputs', 'cacheable', 'test_tier'):
            expected = original[field][:-1] if field == 'argv' else original[field]
            actual = protocol[field][:-1] if field == 'argv' else protocol[field]
            self.assertEqual(expected, actual, field)
        quality = gates['backend:quality']
        self.assertTrue(quality['external_state'])
        self.assertIn('CICD_TEST_DATABASE_URL', quality['requires_env'])
        self.assertIn('cargo clippy --locked --workspace --all-targets -- -D warnings', quality['argv'][-1])
        self.assertNotIn('--exclude', shlex.split(quality['argv'][-1]))
        self.assertFalse(quality['cacheable'])
        manifest = tomllib.loads((ROOT / 'backend/tests/cache-protocol/Cargo.toml').read_text())
        self.assertIn('workspace', manifest, 'The standalone protocol suite is not in --workspace tests')
        workspace = tomllib.loads((ROOT / 'backend/Cargo.toml').read_text())['workspace']
        self.assertIn('domain', workspace['members'])
        self.assertNotIn('domain', workspace.get('exclude', []))

    def test_ordinary_profiles_keep_both_small_unit_suites_in_the_existing_owner(self):
        for profile in ('fast', 'unit', 'build'):
            with self.subTest(profile=profile):
                gates = {gate['id']: gate for gate in self.gates(profile)}
                self.assertIn('backend:unit', gates)
                self.assertNotIn('backend:quality', gates)
                self.assertNotIn('backend:cache-protocol-unit', gates)
                self.assertEqual('cargo fmt --all -- --check; cargo test --locked -p cicd-domain --lib; ' + PROTOCOL,
                                 gates['backend:unit']['argv'][-1])

    def test_integrated_quality_keeps_the_existing_full_workspace_contract(self):
        for profile in ('integrated', 'integration'):
            with self.subTest(profile=profile):
                gates = {gate['id']: gate for gate in self.gates(profile)}
                self.assertIn('backend:quality', gates)
                self.assertNotIn('backend:unit', gates)
                self.assertNotIn('backend:cache-protocol-unit', gates)
                self.assertIn('cargo test --locked --workspace', gates['backend:quality']['argv'][-1])

    def test_contract_changes_have_a_static_owner_without_application_builds(self):
        path = 'scripts/tests/test_backend_release_ownership.py'
        gates = selection.plan(ROOT, profile='build', changed=[path])['gates']
        self.assertEqual(['tooling:build-contracts'], [gate['id'] for gate in gates])
        self.assertEqual('none', gates[0]['execution_slot'])
        for consumed in (path, '.quality-gate.json', 'backend/Cargo.toml', 'backend/tests/cache-protocol/Cargo.toml',
                         'scripts/vendor/base_quality_selection.py'):
            self.assertTrue(selection.matches(consumed, gates[0]['inputs']), consumed)
        self.assertIn('scripts.tests.test_backend_release_ownership', gates[0]['argv'])
        self.assertIn('scripts.tests.test_backend_release_ownership', (ROOT / '.github/workflows/ci.yml').read_text())


if __name__ == '__main__':
    unittest.main()
