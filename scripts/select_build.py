"""Map the existing Base plan to product container services, without running gates."""
from pathlib import Path
from scripts.vendor import base_quality_selection as selector

ROOT = Path(__file__).resolve().parents[1]
SERVICE_GATES = {'backend': 'backend:build', 'runner': 'backend:build', 'frontend': 'frontend:build'}
NON_CONTAINER_GATES = frozenset([])
SERVICES = tuple(SERVICE_GATES)


def build_services(plan, manifest):
    inputs = manifest['container_inputs']
    if (not isinstance(inputs, dict) or set(inputs) != set(SERVICES) or
            any(not isinstance(patterns, list) or not patterns or
                any(not isinstance(pattern, str) or not pattern for pattern in patterns)
                for patterns in inputs.values())):
        raise ValueError('Review the container service input map before preparation')
    artifacts = {gate['id'] for gate in plan['gates'] if gate.get('kind') == 'artifact'}
    if artifacts - (set(SERVICE_GATES.values()) | NON_CONTAINER_GATES):
        raise ValueError('Review the container mapping for the new artifact producer')
    return [name for name in SERVICES if SERVICE_GATES[name] in artifacts or any(
        selector.matches(path, inputs[name]) for path in plan['changed'])]


def auto_services(root=ROOT):
    manifest = selector.load(root)
    plan = selector.plan(root, profile='build', base='HEAD', manifest=manifest)
    return build_services(plan, manifest)
