"""Seal/verify a same-candidate frontend dist; verification needs no Node install."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
from pathlib import Path

SCHEMA = 'base-frontend-artifact/v1'
MAX_BYTES = 1024**3
MAX_FILES = 10000


def file_hash(path):
    result = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            result.update(block)
    return result.hexdigest()


def encoded(value):
    return (json.dumps(value, sort_keys=True, indent=2) + '\n').encode()


def checksums(manifest, manifest_sha256):
    rows = [manifest_sha256 + '  manifest.json\n']
    rows.extend(record['sha256'] + '  dist/' + name + '\n' for name, record in sorted(manifest['files'].items()))
    return ''.join(rows).encode()


def git(root, *arguments):
    return subprocess.check_output(['git', '-C', str(root), *arguments], stderr=subprocess.PIPE,
                                   text=True).strip()


def linked(path):
    return path.is_symlink() or getattr(path, 'is_junction', lambda: False)()


def identity(root, node, pnpm, environment):
    root = root.resolve()
    if Path(git(root, 'rev-parse', '--show-toplevel')).resolve() != root:
        raise ValueError('Frontend artifact requires the product Git root')
    if git(root, 'status', '--porcelain', '--untracked-files=normal'):
        raise ValueError('Frontend artifact requires a clean candidate source')
    revision = git(root, 'rev-parse', 'HEAD')
    if not re.fullmatch(r'[a-f0-9]{40}', revision):
        raise ValueError('Exact candidate revision required')
    quality = json.loads((root / '.quality-gate.json').read_text(encoding='utf-8-sig'))
    product = quality.get('product', '')
    if quality.get('schema') != 'base-quality-product/v1' or not re.fullmatch(r'[A-Za-z0-9_-]+', product):
        raise ValueError('Product-owned quality identity required')
    package = json.loads((root / 'frontend/package.json').read_text(encoding='utf-8'))
    if package.get('packageManager') != 'pnpm@' + pnpm:
        raise ValueError('Actual pnpm must match the product packageManager')
    if not re.fullmatch(r'\d+\.\d+\.\d+', node):
        raise ValueError('Exact Node version required')
    pin = root / '.base-ui-revision'
    ui = pin.read_text(encoding='utf-8').strip() if pin.is_file() else None
    if ui is None and product != 'services-base' or ui is not None and not re.fullmatch(r'[a-f0-9]{40}', ui):
        raise ValueError('Exact Base UI pin required for the consumer')
    values = {name: value for name, value in environment.items() if name.startswith('VITE_') or name in {
        'NODE_ENV', 'NODE_OPTIONS', 'BROWSERSLIST', 'BROWSERSLIST_ENV', 'SOURCE_DATE_EPOCH'}}
    if environment.get('SPEC'):
        spec = Path(environment['SPEC'])
        spec = spec if spec.is_absolute() else root / 'frontend' / spec
        if linked(spec) or not spec.resolve().is_relative_to(root) or not spec.is_file():
            raise ValueError('SPEC must be a regular product-owned generation input')
        values['SPEC'] = {'path': spec.resolve().relative_to(root).as_posix(), 'sha256': file_hash(spec)}
    return {'schema': SCHEMA, 'product': product, 'source_revision': revision, 'base_ui_revision': ui,
            'node_version': node, 'pnpm_version': pnpm,
            'build_platform': {'os': platform.system().lower(), 'architecture': platform.machine().lower()},
            'build_environment_sha256': hashlib.sha256(encoded(values)).hexdigest()}


def payload(directory):
    if linked(directory) or not directory.is_dir():
        raise ValueError('Frontend payload must be a regular directory, without links')
    records, size = {}, 0
    for path in sorted(directory.rglob('*')):
        if linked(path):
            raise ValueError('Frontend payload cannot contain links')
        if path.is_dir():
            continue
        if not path.is_file():
            raise ValueError('Frontend payload requires regular files')
        name = path.relative_to(directory).as_posix()
        if ('\\' in name or ':' in name or any(ord(char) < 32 or ord(char) == 127 for char in name)
                or any(part in ('', '.', '..') for part in name.split('/'))):
            raise ValueError('Unsafe frontend payload path')
        length = path.stat().st_size
        size += length
        if size > MAX_BYTES or len(records) >= MAX_FILES:
            raise ValueError('Frontend payload exceeds the artifact budget')
        records[name] = {'bytes': length, 'sha256': file_hash(path)}
    if not records.get('index.html', {}).get('bytes'):
        raise ValueError('Frontend payload requires a nonempty index.html')
    return records


def seal(root, dist, output, *, node, pnpm, environment=None):
    root, dist, output = Path(root).resolve(), Path(dist), Path(output)
    environment = os.environ if environment is None else environment
    before = identity(root, node, pnpm, environment)
    if output.exists():
        raise ValueError('Choose a new frontend artifact destination')
    if not output.resolve().is_relative_to(root / '.local') or not dist.resolve().is_relative_to(root):
        raise ValueError('Source/output must stay in the product and its owned .local directory')
    files = payload(dist)
    output.mkdir(parents=True)
    for name in files:
        target = output / 'dist' / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(dist / name, target)
    if payload(output / 'dist') != files or identity(root, node, pnpm, environment) != before:
        raise ValueError('Frontend source or payload changed during publication')
    manifest = {**before, 'files': files}
    path = output / 'manifest.json'
    with path.open('xb') as stream:
        stream.write(encoded(manifest))
    manifest_sha = file_hash(path)
    (output / 'SHA256SUMS').write_bytes(checksums(manifest, manifest_sha))
    return {'manifest_sha256': manifest_sha, 'checksums_sha256': file_hash(output / 'SHA256SUMS'),
            'source_revision': before['source_revision'],
            'payload_bytes': sum(record['bytes'] for record in files.values()), 'files': len(files)}


def verify(root, artifact, expected_sha256, *, node, pnpm, environment=None):
    artifact = Path(artifact)
    path = artifact / 'manifest.json'
    if (not re.fullmatch(r'[a-f0-9]{64}', expected_sha256) or linked(artifact) or linked(path)
            or not path.is_file() or path.stat().st_size > 4 * 1024**2 or file_hash(path) != expected_sha256):
        raise ValueError('Frontend manifest digest differs from the trusted producer')
    manifest = json.loads(path.read_text(encoding='utf-8'))
    expected = identity(Path(root), node, pnpm, os.environ if environment is None else environment)
    if not isinstance(manifest, dict) or set(manifest) != set(expected) | {'files'}:
        raise ValueError('Unsupported frontend artifact manifest')
    if any(manifest.get(name) != value for name, value in expected.items()):
        raise ValueError('Frontend candidate, pin, toolchain or build environment differs')
    if {p.name for p in artifact.iterdir()} != {'manifest.json', 'dist', 'SHA256SUMS'} or payload(artifact / 'dist') != manifest['files']:
        raise ValueError('Frontend payload is missing, corrupt or contains undeclared files')
    sums = artifact / 'SHA256SUMS'
    if linked(sums) or sums.read_bytes() != checksums(manifest, expected_sha256):
        raise ValueError('Frontend checksum list differs from the trusted manifest')
    return manifest


def check_ci(root):
    """Lint the canonical handoff job shape, not arbitrary YAML semantics."""
    content = (Path(root) / '.github/workflows/ci.yml').read_text(encoding='utf-8')
    sections = re.split(r'\n  ([A-Za-z0-9_-]+):\n', content.split('\njobs:\n', 1)[-1])
    jobs = dict(zip(sections[1::2], sections[2::2], strict=True))
    if not {'frontend', 'frontend-package'} <= jobs.keys():
        raise ValueError('Frontend CI requires producer and packaging jobs')
    producer, consumer = jobs['frontend'], jobs['frontend-package']
    node = re.findall(r'\n          node-version: [\"\']?(\d+\.\d+\.\d+)', producer)
    if len(node) != 1:
        raise ValueError('Frontend CI must declare one exact Node version')
    for job in (producer, consumer):
        if '          ref: ${{ github.sha }}' not in job or '--node-version ' + node[0] not in job:
            raise ValueError('Frontend handoff must preserve candidate SHA and Node version')
    compile_steps = [match.start() for match in re.finditer(r'pnpm (?:exec vite build|platform:compile)', producer)]
    seal_step = producer.find(' seal --root ')
    if len(compile_steps) != 1 or seal_step < compile_steps[0] or 'name: tested-frontend-dist' not in producer:
        raise ValueError('Seal/publish must follow the single application compilation')
    if any(command in consumer for command in ('pnpm install', 'npm install', 'vite build', 'platform:compile',
                                               'actions/setup-node', 'pnpm/action-setup', 'corepack')):
        raise ValueError('Frontend packaging must not install or compile the application')
    if ('    needs: [changes, frontend]' not in consumer or 'actions/download-artifact@' not in consumer
            or 'name: tested-frontend-dist' not in consumer or ' verify --root ' not in consumer
            or consumer.find(' verify --root ') > consumer.find('docker buildx build')
            or '--build-arg FRONTEND_CHECKSUMS_SHA256=' not in consumer
            or '${{ needs.frontend.outputs.manifest_sha256 }}' not in consumer
            or '${{ needs.frontend.outputs.checksums_sha256 }}' not in consumer):
        raise ValueError('Frontend packaging requires the verified upstream artifact identity')
    environment = r'(?:VITE_[A-Za-z0-9_]+|NODE_ENV|NODE_OPTIONS|BROWSERSLIST|BROWSERSLIST_ENV|SOURCE_DATE_EPOCH)'
    if re.search(r'^          ' + environment + r':', producer, re.MULTILINE):
        raise ValueError('Build environment must be job-scoped so seal observes the same inputs')
    for name, value in re.findall(r'^      (' + environment + r'): (.+)$', producer, re.MULTILINE):
        if '\n      ' + name + ': ' + value not in consumer:
            raise ValueError('Frontend packaging must preserve producer build environment')
    return {'status': 'PASS', 'node_version': node[0], 'application_compile_steps': 1,
            'packaging_installs_or_compiles': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('seal', 'verify', 'check-ci'))
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--dist', type=Path)
    parser.add_argument('--artifact', type=Path)
    parser.add_argument('--manifest-sha256')
    parser.add_argument('--node-version')
    parser.add_argument('--pnpm-version')
    args = parser.parse_args()
    if args.action == 'check-ci':
        print(json.dumps(check_ci(args.root)))
        return
    if args.artifact is None or not args.node_version or not args.pnpm_version:
        parser.error('--artifact, --node-version and --pnpm-version are required for handoff')
    options = {'node': args.node_version, 'pnpm': args.pnpm_version}
    if args.action == 'seal':
        if args.dist is None:
            parser.error('--dist is required for seal')
        for command, expected in ((['node', '--version'], 'v' + args.node_version),
                                  ([shutil.which('pnpm') or 'pnpm', '--version'], args.pnpm_version)):
            if subprocess.check_output(command, text=True).strip() != expected:
                raise ValueError('Actual Node/pnpm differs from the declared build toolchain')
        result = seal(args.root, args.dist, args.artifact, **options)
        if os.environ.get('GITHUB_OUTPUT'):
            with Path(os.environ['GITHUB_OUTPUT']).open('a', encoding='utf-8') as stream:
                stream.write('manifest_sha256=' + result['manifest_sha256'] + '\n')
                stream.write('checksums_sha256=' + result['checksums_sha256'] + '\n')
    else:
        if args.manifest_sha256 is None:
            parser.error('--manifest-sha256 is required for verify')
        result = verify(args.root, args.artifact, args.manifest_sha256, **options)
        result = {'status': 'VERIFIED', 'source_revision': result['source_revision'],
                  'manifest_sha256': args.manifest_sha256, 'files': len(result['files']),
                  'checksums_sha256': file_hash(args.artifact / 'SHA256SUMS')}
    print(json.dumps(result))


if __name__ == '__main__':
    main()
