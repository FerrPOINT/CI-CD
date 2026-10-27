#!/usr/bin/env python3
"""Build standalone containers only after verifying the pinned Base checkout."""
from pathlib import Path
import subprocess
import sys

root = Path(__file__).resolve().parents[1]
base = root.parent / 'services-base'
targets = sys.argv[1:]
if not targets:
    sys.path.insert(0, str(root))
    from scripts.select_ci import auto_services
    targets = auto_services(root)
elif targets == ['--all']:
    targets = ['backend', 'frontend']
elif not set(targets) <= {'backend', 'frontend'} or len(set(targets)) != len(targets):
    raise SystemExit('Choose backend/frontend, --all, or no arguments for changed services')

revision = '.namespace-base-revision' if (root / '.namespace-base-revision').exists() else '.base-revision'
if targets == ['frontend'] and (root / '.base-ui-revision').is_file():
    revision = '.base-ui-revision'
if targets and set(targets) == {'backend', 'frontend'} and (root / '.base-ui-revision').is_file():
    if (root / revision).read_text().strip() != (root / '.base-ui-revision').read_text().strip():
        raise SystemExit('Mixed SDK/UI pins require the qualified multi-pin candidate route; no Docker build started')
if targets:
    subprocess.run([sys.executable, str(base / 'scripts/verify_base_revision.py'),
                '--base', str(base), '--revision', str(root / revision)], check=True)
    compose = ['docker', 'compose']
    if (root / 'docker-compose.dev.yml').exists() and not (root / 'docker-compose.yml').exists():
        compose += ['-f', 'docker-compose.dev.yml']
    subprocess.run([*compose, 'build', *targets], cwd=root, check=True)
else:
    print('NO_BUILD_SERVICES_SELECTED: no container producer, preparation or image qualification; use an explicit service for cold bootstrap')
