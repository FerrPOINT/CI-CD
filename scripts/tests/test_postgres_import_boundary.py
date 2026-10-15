"""Privileged imports execute only the compiled Base helper closure."""
import json
import importlib.util
from pathlib import Path
import py_compile
import re
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
BASE = ROOT.parent / 'services-base'


class PostgresImportBoundaryTests(unittest.TestCase):
    def check_import(self, shadow=None, bytecode=False):
        with tempfile.TemporaryDirectory(prefix='forge-pg-import-') as temporary:
            root = Path(temporary)
            bundle = root / 'base'
            source = (ROOT / 'backend/src/task_delivery/postgres.rs').read_text(encoding='utf-8')
            files = re.findall(r'pinned_base_helper!\("([^\"]+)"\)', source)
            self.assertTrue(files)
            for name in [*files, 'Cargo.toml']:
                target = bundle / name
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(BASE / name, target)
            marker = root / 'unreviewed-import'
            if shadow:
                (bundle / shadow).write_text(
                    'from pathlib import Path\n'
                    f'Path({str(marker)!r}).write_text("executed")\n'
                    'raise RuntimeError("unreviewed import executed")\n',
                    encoding='utf-8',
                )
            if bytecode:
                poison = root / 'unreviewed.py'
                poison.write_text(
                    'from pathlib import Path\n'
                    f'Path({str(marker)!r}).write_text("executed")\n'
                    'raise RuntimeError("unreviewed bytecode executed")\n',
                    encoding='utf-8',
                )
                cached = importlib.util.cache_from_source(str(bundle / 'scripts/_operator_compat.py'))
                py_compile.compile(str(poison), cfile=cached, doraise=True,
                                   invalidation_mode=py_compile.PycInvalidationMode.UNCHECKED_HASH)
            owner_root = root / 'owner'
            owner_root.mkdir()
            compose = root / 'compose.json'
            compose.write_text('{}', encoding='utf-8')
            project = 'sdlc-qa-forge-pg-0123456789ab'
            parent = 'sdlc-qa-forge-delivery-0123456789ab'
            packet = {
                'projectId': '00000000-0000-0000-0000-000000000001',
                'root': str(owner_root), 'readOnly': True,
                'command': {'operationKey': 'import-boundary'},
                'originalOperation': {}, 'policySha256': 'a' * 64,
                'policy': {
                    'projectName': project, 'networkName': parent + '_qa',
                    'volumeName': parent + '_delivery-qa',
                    'postgresVolumeName': project + '_postgres-data',
                    'initialDatabase': 'forge_test_pg_import',
                    'baseRoot': str(bundle), 'composeRoot': str(root),
                    'composeFile': str(compose), 'daemonId': 'fixture-daemon',
                },
            }
            (owner_root / 'owner.json').write_text(json.dumps({
                'schema': 'forge/isolated-pg-owner/v1',
                'projectId': packet['projectId'], 'policySha256': packet['policySha256'],
            }), encoding='utf-8')
            request = root / 'request.json'
            request.write_text(json.dumps(packet), encoding='utf-8')
            program = '''import importlib.util,json,sys,types
if sys.platform == 'win32': sys.modules['fcntl']=types.ModuleType('fcntl')
spec=importlib.util.spec_from_file_location('pg_owner',sys.argv[1])
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
owner=module.Owner(json.load(open(sys.argv[2],encoding='utf-8')))
print(owner.base.__file__)
'''
            result = subprocess.run(
                [sys.executable, '-I', '-B', '-c', program,
                 str(ROOT / 'scripts/postgres-delivery.py'), str(request)],
                cwd=root, capture_output=True, text=True, timeout=15,
            )
            self.assertFalse(marker.exists(), result.stderr)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn('pdlc_operator', result.stdout)

    def test_pinned_namespace_loads_from_another_cwd(self):
        self.check_import()

    def test_unpinned_package_initializer_is_not_executed(self):
        self.check_import('scripts/__init__.py')

    def test_unpinned_module_cannot_shadow_the_namespace(self):
        self.check_import('scripts.py')

    def test_unpinned_bytecode_cannot_replace_a_pinned_source(self):
        self.check_import(bytecode=True)


if __name__ == '__main__':
    unittest.main()
