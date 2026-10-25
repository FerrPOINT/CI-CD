import unittest
import re
import shlex
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


class FrontendToolchainTests(unittest.TestCase):
    def test_agent_presence_is_explicit_and_limited_to_types_and_unit(self):
        manifest = json.loads((ROOT / '.quality-gate.json').read_text(encoding='utf-8'))
        actual = {gate['id']: gate.get('environment_input_contract')
                  for gate in manifest['gates'] if gate.get('environment_input_contract') is not None}
        self.assertEqual(actual, {
            'frontend:types': 'frontend-agent-presence-v1',
            'frontend:tests': 'frontend-agent-presence-v1',
        })

    def check_frozen_install(self, dockerfile):
        pin = dockerfile.index('corepack prepare pnpm@10.28.1 --activate')
        installs = list(re.finditer(r'\bpnpm\s+install\b[^\n]*', dockerfile))
        self.assertTrue(installs)
        for install in installs:
            self.assertLess(pin, install.start())
            self.assertIn('--frozen-lockfile', shlex.split(install.group()))
        self.assertNotIn('--no-frozen-lockfile', dockerfile)

    def test_umbrella_pins_pnpm_before_frozen_install(self):
        self.check_frozen_install((ROOT / 'frontend/Dockerfile.umbrella').read_text(encoding='utf-8'))

    def test_store_option_does_not_hide_a_missing_frozen_lock(self):
        source = (ROOT / 'frontend/Dockerfile.umbrella').read_text(encoding='utf-8')
        with self.assertRaises(AssertionError):
            self.check_frozen_install(source.replace('--frozen-lockfile', ''))

    def test_install_before_the_pin_is_rejected(self):
        source = (ROOT / 'frontend/Dockerfile.umbrella').read_text(encoding='utf-8')
        with self.assertRaises(AssertionError):
            self.check_frozen_install('RUN pnpm install --frozen-lockfile\n' + source)


if __name__ == '__main__':
    unittest.main()
