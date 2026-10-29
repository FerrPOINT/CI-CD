#!/usr/bin/env python3
"""Select standalone container services before any pinned Source preparation."""
from pathlib import Path
import subprocess
import sys

root = Path(__file__).resolve().parents[1]
previous_path = sys.path[:]
try:
    sys.path.insert(0, str(root))
    from scripts.select_build import SERVICES
    from scripts.select_build import auto_services
finally:
    sys.path[:] = previous_path

targets = sys.argv[1:]
if not targets:
    targets = auto_services(root)
elif targets == ['--all']:
    targets = list(SERVICES)
elif not set(targets) <= set(SERVICES) or len(set(targets)) != len(targets):
    raise SystemExit('Choose declared services, --all, or no arguments for changed services')

if targets:
    base = root.parent / 'services-base'
    revision = '.namespace-base-revision' if (root / '.namespace-base-revision').exists() else '.base-revision'
    if targets == ['frontend'] and (root / '.base-ui-revision').is_file():
        revision = '.base-ui-revision'
    if 'frontend' in targets and set(targets) - {'frontend'} and (root / '.base-ui-revision').is_file():
        if (root / revision).read_text().strip() != (root / '.base-ui-revision').read_text().strip():
            raise SystemExit('Mixed SDK/UI pins require the qualified multi-pin candidate route; no Docker build started')
    subprocess.run([sys.executable, str(base / 'scripts/verify_base_revision.py'),
                    '--base', str(base), '--revision', str(root / revision)], check=True)
    compose = ['docker', 'compose']
    if (root / 'docker-compose.dev.yml').exists() and not (root / 'docker-compose.yml').exists():
        compose += ['-f', 'docker-compose.dev.yml']
    subprocess.run([*compose, 'build', *targets], cwd=root, check=True)
else:
    print('NO_BUILD_SERVICES_SELECTED: no producer, preparation or image qualification; choose an explicit service for cold bootstrap')
