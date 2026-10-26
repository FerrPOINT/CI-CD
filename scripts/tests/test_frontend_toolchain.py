import unittest
import re
import shlex
import json
from pathlib import Path
from scripts.vendor import base_quality_selection as selection


ROOT = Path(__file__).resolve().parents[2]


class FrontendToolchainTests(unittest.TestCase):
    def test_closed_postinstall_uses_the_same_schema_layout_in_prepared_and_docker_paths(self):
        manifest = selection.load(ROOT)
        profile = manifest['frontend_generation']
        self.assertIn('openapi/**', profile['inputs'])
        self.assertIn('frontend/scripts/base-generate-openapi.mjs', profile['inputs'])
        for gate in manifest['gates']:
            if gate.get('prepare_frontend'):
                self.assertEqual(gate['descriptor_input_contract'], 'prepared-frontend-generation-context-v1')
        for name in ('Dockerfile', 'Dockerfile.umbrella'):
            source = (ROOT / 'frontend' / name).read_text(encoding='utf8')
            stage = source.split('AS generated\n', 1)[1].split('\nFROM ', 1)[0]
            self.assertLess(stage.index('COPY CI-CD/frontend/scripts/base-generate-openapi.mjs'), stage.index('pnpm run postinstall'))
            self.assertIn('COPY CI-CD/openapi/openapi.yaml /openapi/openapi.yaml', stage)
            self.assertIn('ENV SPEC=/openapi/openapi.yaml', source)

    def test_tracked_openapi_output_keeps_the_generators_canonical_lf(self):
        attributes = (ROOT / '.gitattributes').read_text(encoding='utf-8').splitlines()
        self.assertIn('frontend/src/api/schema.d.ts text eol=lf', attributes)
        schema = (ROOT / 'frontend/src/api/schema.d.ts').read_bytes()
        self.assertNotIn(b'\r\n', schema)

    def test_agent_presence_scope_excludes_lint_and_other_components(self):
        manifest = selection.load(ROOT)
        actual = {gate['id']: gate.get('environment_input_contract')
                  for gate in manifest['gates'] if gate.get('environment_input_contract') is not None}
        self.assertEqual(actual, {
            'frontend:types': 'frontend-agent-presence-v1',
            'frontend:tests': 'frontend-agent-presence-v1',
            'frontend:build': 'frontend-agent-presence-v1',
        })

    def test_production_presence_requires_closed_source_and_root_ignore_input(self):
        manifest = json.loads((ROOT / '.quality-gate.json').read_text(encoding='utf-8'))
        gate = next(value for value in manifest['gates'] if value['id'] == 'frontend:build')
        self.assertEqual(gate['kind'], 'artifact')
        self.assertEqual(gate.get('artifact_input_contract'), 'git-source-v1')
        self.assertIn('.gitignore', gate['inputs'])

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
