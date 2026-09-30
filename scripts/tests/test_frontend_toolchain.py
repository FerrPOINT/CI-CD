import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


class FrontendToolchainTests(unittest.TestCase):
    def test_umbrella_pins_pnpm_before_frozen_install(self):
        dockerfile = (ROOT / 'frontend/Dockerfile.umbrella').read_text(encoding='utf-8')
        pin = dockerfile.index('corepack prepare pnpm@10.28.1 --activate')
        install = dockerfile.index('pnpm install --frozen-lockfile')
        self.assertLess(pin, install)
        self.assertNotIn('--no-frozen-lockfile', dockerfile)


if __name__ == '__main__':
    unittest.main()
