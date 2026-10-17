"""Reset local crate artifacts once after restoring a job-private Cargo target."""

import json
import subprocess


def local_packages(metadata):
    packages = metadata.get('packages') if isinstance(metadata, dict) else None
    if not isinstance(packages, list):
        raise ValueError('Cargo metadata must contain a package inventory')
    local = set()
    for package in packages:
        if not isinstance(package, dict) or 'source' not in package:
            raise ValueError('Cargo package source is missing')
        source = package['source']
        if source is not None and (not isinstance(source, str) or not source):
            raise ValueError('Cargo package source is invalid')
        name = package.get('name')
        if (not isinstance(name, str) or not name or name.startswith('-')
                or any(c in name for c in '\0\r\n /\\:@')):
            raise ValueError('Cargo package name is invalid')
        if source is None:
            local.add(name)
    return sorted(local)


def reset_local_artifacts(*, runner=None):
    run = runner or subprocess.run
    result = run(['cargo', 'metadata', '--locked', '--all-features', '--format-version', '1'],
                 check=True, capture_output=True, text=True, timeout=120)
    metadata = json.loads(result.stdout)
    packages = local_packages(metadata)
    external = {p['name'] for p in metadata['packages'] if p['source'] is not None}
    collisions = sorted(set(packages) & external)
    if collisions:
        print('Cargo clean also resets same-name external crates: ' + ', '.join(collisions))
    if packages:
        command = ['cargo', 'clean', '--locked']
        for name in packages:
            command.extend(['--package', name])
        run(command, check=True, timeout=120)
    return len(packages)


if __name__ == '__main__':
    count = reset_local_artifacts()
    print(f'Reset {count} local crate names; external crates with other names retained')
