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
        if source is not None:
            continue
        identity = package.get('id')
        if not isinstance(identity, str) or not identity or identity.startswith('-') or any(c in identity for c in '\0\r\n'):
            raise ValueError('Cargo local package identity is invalid')
        local.add(identity)
    return sorted(local)


def reset_local_artifacts(*, runner=None):
    run = runner or subprocess.run
    result = run(['cargo', 'metadata', '--locked', '--all-features', '--format-version', '1'],
                 check=True, capture_output=True, text=True, timeout=120)
    packages = local_packages(json.loads(result.stdout))
    if packages:
        command = ['cargo', 'clean', '--locked']
        for identity in packages:
            command.extend(['--package', identity])
        run(command, check=True, timeout=120)
    return len(packages)


if __name__ == '__main__':
    count = reset_local_artifacts()
    print(f'Reset {count} local crates; compiled registry/git dependencies retained')
