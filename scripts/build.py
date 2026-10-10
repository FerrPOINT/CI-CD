#!/usr/bin/env python3
"""Build standalone containers only after verifying the pinned Base checkout."""
from pathlib import Path
import subprocess
import sys

root = Path(__file__).resolve().parents[1]
base = root.parent / 'services-base'
subprocess.run([sys.executable, str(base / 'scripts/verify_base_revision.py'),
                '--base', str(base), '--revision', str(root / ('.namespace-base-revision' if (root / '.namespace-base-revision').exists() else '.base-revision'))], check=True)
compose = ['docker', 'compose']
if (root / 'docker-compose.dev.yml').exists() and not (root / 'docker-compose.yml').exists():
    compose += ['-f', 'docker-compose.dev.yml']
subprocess.run([*compose, 'build', *sys.argv[1:]], cwd=root, check=True)
