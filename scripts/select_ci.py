"""Thin Forge CI/container adapter over the exact Base product plan."""
import os
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from scripts import select_build
from scripts.vendor import base_quality_selection as selector

RUST_COMPONENTS = {'backend', 'cli', 'delivery', 'pg-migrate', 'migration'}


def describe(plan):
    gates = {gate['id'] for gate in plan['gates']}
    return {
        'backend': bool(set(plan['components']) & RUST_COMPONENTS),
        'pinned_imports': 'pinned-imports' in plan['components'],
        'server_checks': 'backend' in plan['components'],
        'frontend': 'frontend' in plan['components'],
        'backend_build': 'backend:build' in gates,
        'cli_build': 'cli:build' in gates,
        'delivery_build': 'delivery:build' in gates,
        'pg_migrate_build': 'pg-migrate:build' in gates,
        'migration_build': 'migration:build' in gates,
        'frontend_build': 'frontend:build' in gates,
        'openapi_check': 'backend:build' in gates or any(selector.matches(path,
            ['openapi/**', 'backend/src/bin/openapi-dump.rs']) for path in plan['changed']),
    }


def build_services(plan):
    return select_build.build_services(plan, selector.load(ROOT))


def auto_services(root=ROOT):
    return select_build.auto_services(root)


def outputs(root, event, base, head):
    manifest = selector.load(root)
    if event not in ('pull_request', 'push') or not re.fullmatch('[0-9a-f]{40}', head) or head == '0' * 40:
        raise ValueError('Choose a supported event and exact candidate head')
    if event == 'push' and base == '0' * 40:
        plan = selector.plan(root, profile='build', changed=[], components=list(manifest['components']), manifest=manifest)
    else:
        if not re.fullmatch('[0-9a-f]{40}', base) or base == '0' * 40:
            raise ValueError('Choose an exact comparison base before preparation')
        paths = selector.git(root, 'diff', '--name-only', '--no-renames', base, head).splitlines()
        plan = selector.plan(root, profile='build', changed=paths, manifest=manifest)
    return describe(plan)


if __name__ == '__main__':
    values = outputs(ROOT, *(os.environ.get(name, '') for name in ('EVENT_NAME', 'BASE_SHA', 'HEAD_SHA')))
    result = ''.join(f'{name}={str(value).lower()}\n' for name, value in values.items())
    destination = os.environ.get('GITHUB_OUTPUT')
    if destination:
        with open(destination, 'a', encoding='utf-8', newline='') as stream:
            stream.write(result)
    else:
        print(result, end='')
