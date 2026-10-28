"""Seal/verify a same-candidate frontend dist; verification needs no Node install."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import shlex
import shutil
import stat
import subprocess
from pathlib import Path

SCHEMA = 'base-frontend-artifact/v1'
MAX_BYTES = 1024**3
MAX_FILES = 10000
METADATA_BYTES = 4 * 1024**2


class FrontendMismatch(ValueError):
    """Stable diagnostic code without configuration values or secret content."""

    def __init__(self, code, message):
        super().__init__(message)
        self.code = code


def file_hash(path, *, max_bytes=None):
    before = path.stat()
    limit = before.st_size if max_bytes is None else max_bytes
    if type(limit) is not int or limit < 0 or before.st_size > limit:
        raise ValueError('Frontend file exceeds its byte budget')
    result = hashlib.sha256()
    size = 0
    with path.open('rb') as stream:
        while block := stream.read(min(1024 * 1024, limit - size + 1)):
            size += len(block)
            if size > limit:
                raise ValueError('Frontend file grew beyond its byte budget')
            result.update(block)
    after = path.stat()
    stamp = lambda value: (value.st_dev, value.st_ino, value.st_size, value.st_mtime_ns, value.st_ctime_ns)
    if size != before.st_size or stamp(before) != stamp(after):
        raise ValueError('Frontend file changed during inspection')
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
    try:
        metadata = path.lstat()
    except FileNotFoundError:
        return False
    return stat.S_ISLNK(metadata.st_mode) or bool(getattr(metadata, 'st_file_attributes', 0) & 0x400)


def checked_path(path):
    path = Path(path)
    if any(linked(parent) for parent in (path, *path.parents)):
        raise ValueError('Frontend payload path cannot contain links or junctions')
    return path


def copy_file(source, target, expected):
    """Bound bytes before writing; only the trusted original digest can publish."""
    source, target = checked_path(source), checked_path(target)
    length, expected_hash = expected['bytes'], expected['sha256']
    if (type(length) is not int or not 0 <= length <= MAX_BYTES
            or not isinstance(expected_hash, str) or not re.fullmatch(r'[a-f0-9]{64}', expected_hash)):
        raise ValueError('Invalid frontend transfer budget or digest')
    before = source.stat()
    if not stat.S_ISREG(before.st_mode) or before.st_size != length:
        raise ValueError('Frontend payload changed before bounded transfer')
    target.parent.mkdir(parents=True, exist_ok=True)
    checked_path(target)
    remaining, digest, created = length, hashlib.sha256(), None
    try:
        with source.open('rb') as incoming, target.open('xb') as outgoing:
            created = os.fstat(outgoing.fileno())
            while chunk := incoming.read(min(1024 * 1024, remaining + 1)):
                if len(chunk) > remaining:
                    raise ValueError('Frontend payload grew beyond its transfer budget')
                outgoing.write(chunk)
                digest.update(chunk)
                remaining -= len(chunk)
        after = source.stat()
        stamp = lambda value: (value.st_dev, value.st_ino, value.st_size, value.st_mtime_ns, value.st_ctime_ns)
        if remaining or digest.hexdigest() != expected_hash or stamp(before) != stamp(after):
            raise ValueError('Frontend payload changed during bounded transfer')
    except BaseException:
        if created is not None:
            try:
                current = target.lstat()
                if (current.st_dev, current.st_ino) == (created.st_dev, created.st_ino):
                    target.unlink()
            except FileNotFoundError:
                pass
        raise


def copy_payload(source, target, files):
    source, target = Path(source), Path(target)
    if (not isinstance(files, dict) or len(files) > MAX_FILES
            or any(not isinstance(row, dict) or type(row.get('bytes')) is not int or row['bytes'] < 0 for row in files.values())
            or sum(row['bytes'] for row in files.values()) > MAX_BYTES):
        raise ValueError('Frontend payload exceeds its transfer budget')
    for name, expected in files.items():
        if (not isinstance(name, str) or name.startswith('/') or '\\' in name or ':' in name
                or any(ord(c) < 32 or ord(c) == 127 for c in name)
                or any(part in ('', '.', '..') for part in name.split('/'))):
            raise ValueError('Unsafe frontend payload path')
        copy_file(source / name, target / name, expected)


def identity(root, node, pnpm, environment):
    root = root.resolve()
    if Path(git(root, 'rev-parse', '--show-toplevel')).resolve() != root:
        raise ValueError('Frontend artifact requires the product Git root')
    if git(root, 'status', '--porcelain', '--untracked-files=normal'):
        raise FrontendMismatch('source_dirty', 'Frontend artifact requires a clean candidate source')
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
    pin_name = '.base-ui-revision'
    references = package.get('dependencies', {})
    pinned_uri = isinstance(references, dict) and any(
        isinstance(references.get(name), str) and references[name].startswith('file:../../.local/sdk/')
        for name in ('@sdlc/ui', '@pdlc/ui'))
    if 'frontend_ui_pin_file' in quality or pinned_uri:
        # New explicit contracts require the coherent Base tooling package;
        # legacy standalone copies keep their original stdlib-only path.
        if __package__:
            from .quality_support import ui_pin_name, local_ui_directory
        else:
            from quality_support import ui_pin_name, local_ui_directory
        pin_name = ui_pin_name(quality)
    pin = root / pin_name
    ui = pin.read_text(encoding='utf-8').strip() if pin.is_file() else None
    if ui is None and product != 'services-base' or ui is not None and not re.fullmatch(r'[a-f0-9]{40}', ui):
        raise ValueError('Exact Base UI pin required for the consumer')
    if 'frontend_ui_pin_file' in quality or pinned_uri:
        local_ui_directory(package, ui)
    values = {name: value for name, value in environment.items() if name.startswith('VITE_') or name in {
        'NODE_ENV', 'NODE_OPTIONS', 'BROWSERSLIST', 'BROWSERSLIST_ENV', 'SOURCE_DATE_EPOCH'}}
    if environment.get('SPEC'):
        spec = Path(environment['SPEC'])
        spec = spec if spec.is_absolute() else root / 'frontend' / spec
        if linked(spec) or not spec.resolve().is_relative_to(root) or not spec.is_file():
            raise ValueError('SPEC must be a regular product-owned generation input')
        values['SPEC'] = {'path': spec.resolve().relative_to(root).as_posix(), 'sha256': file_hash(spec)}
    identity = {'schema': SCHEMA, 'product': product, 'source_revision': revision, 'base_ui_revision': ui,
            'node_version': node, 'pnpm_version': pnpm,
            'build_platform': {'os': platform.system().lower(), 'architecture': platform.machine().lower()},
            'build_environment_sha256': hashlib.sha256(encoded(values)).hexdigest()}
    if 'frontend_ui_pin_file' in quality:
        identity['base_ui_pin_file'] = pin_name
    return identity


def payload(directory):
    checked_path(directory)
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
        records[name] = {'bytes': length, 'sha256': file_hash(path, max_bytes=length)}
    if not records.get('index.html', {}).get('bytes'):
        raise ValueError('Frontend payload requires a nonempty index.html')
    return records


def seal(root, dist, output, *, node, pnpm, environment=None, prepared=False):
    root, dist, output = Path(root).resolve(), Path(dist), Path(output)
    environment = os.environ if environment is None else environment
    before = identity(root, node, pnpm, environment)
    if output.exists():
        raise ValueError('Choose a new frontend artifact destination')
    prepared_dist = root.parent / '.local/frontend' / root.name / 'source' / root.name / 'frontend/dist'
    owned_dist = dist.resolve() == prepared_dist.resolve() if prepared else dist.resolve().is_relative_to(root)
    if not output.resolve().is_relative_to(root / '.local') or not owned_dist:
        raise ValueError('Source/output must stay in the product and its owned .local directory')
    files = payload(dist)
    checked_path(output).mkdir(parents=True)
    copy_payload(dist, output / 'dist', files)
    if payload(output / 'dist') != files or identity(root, node, pnpm, environment) != before:
        raise ValueError('Frontend source or payload changed during publication')
    manifest = {**before, 'files': files}
    if len(encoded(manifest)) > METADATA_BYTES:
        raise ValueError('Frontend manifest exceeds its byte budget')
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
            or not path.is_file() or path.stat().st_size > METADATA_BYTES):
        raise FrontendMismatch('manifest_digest', 'Frontend manifest digest differs from the trusted producer')
    with checked_path(path).open('rb') as stream:
        raw = stream.read(METADATA_BYTES + 1)
    if len(raw) > METADATA_BYTES or hashlib.sha256(raw).hexdigest() != expected_sha256:
        raise FrontendMismatch('manifest_digest', 'Frontend manifest digest differs from the trusted producer')
    manifest = json.loads(raw)
    expected = identity(Path(root), node, pnpm, os.environ if environment is None else environment)
    if not isinstance(manifest, dict) or set(manifest) != set(expected) | {'files'}:
        raise ValueError('Unsupported frontend artifact manifest')
    for name, value in expected.items():
        if manifest.get(name) != value:
            raise FrontendMismatch(name, 'Frontend candidate, pin, toolchain or build environment differs')
    try:
        actual_payload = payload(artifact / 'dist')
    except ValueError as error:
        raise FrontendMismatch('payload', str(error)) from error
    if {p.name for p in artifact.iterdir()} != {'manifest.json', 'dist', 'SHA256SUMS'} or actual_payload != manifest['files']:
        raise FrontendMismatch('payload', 'Frontend payload is missing, corrupt or contains undeclared files')
    sums = artifact / 'SHA256SUMS'
    expected_sums = checksums(manifest, expected_sha256)
    with checked_path(sums).open('rb') as stream:
        actual_sums = stream.read(len(expected_sums) + 1)
    if actual_sums != expected_sums:
        raise FrontendMismatch('checksums', 'Frontend checksum list differs from the trusted manifest')
    return manifest


def workflow_jobs(content):
    sections = re.split(r'\n  ([A-Za-z0-9_-]+):\n', content.split('\njobs:\n', 1)[-1])
    return dict(zip(sections[1::2], sections[2::2], strict=True))


def check_generation_ci(root, producer):
    """Git cannot prove parity of ignored generated files; retain their drift gate."""
    root = Path(root)
    path = checked_path(root / '.quality-gate.json')
    if path.stat().st_size > METADATA_BYTES:
        raise ValueError('Product generation metadata exceeds its byte budget')
    quality = json.loads(path.read_text(encoding='utf-8-sig'))
    profile = quality.get('frontend_generation')
    if profile is None:
        return
    names = quality.get('frontend_generated')
    if (not isinstance(profile, dict) or profile.get('schema') != 'base-frontend-generation/v1'
            or not isinstance(names, list) or not names or len(names) > MAX_FILES):
        raise ValueError('Explicit generated output policy required')
    for name in names:
        if (not isinstance(name, str) or not name or '\\' in name or ':' in name
                or any(ord(c) < 32 or ord(c) == 127 for c in name)
                or Path(name).is_absolute() or name != Path(name).as_posix()
                or any(part in ('.', '..') for part in Path(name).parts)):
            raise ValueError('Unsafe generated output policy path')
    full = ['frontend/' + name for name in ('pnpm-lock.yaml', *names)]
    tracked = set(git(root, '--literal-pathspecs', 'ls-files', '-z', '--', *full).split('\0')) - {''}
    if full[0] not in tracked:
        raise ValueError('Frontend lockfile must remain tracked')
    remaining = [name for name in full[1:] if name not in tracked]
    ignored = set()
    if remaining:
        result = subprocess.run(['git', '-C', str(root), 'check-ignore', '-z', '--stdin'],
                                input=('\0'.join(remaining) + '\0').encode('utf-8'), capture_output=True)
        if result.returncode not in (0, 1):
            raise ValueError('Cannot inspect generated output Git policy')
        ignored = set(result.stdout.decode('utf-8').split('\0')) - {''}
        if set(remaining) != ignored:
            raise ValueError('Generated output must be tracked or explicitly ignored')
    producers = {'ci.yml': producer}
    release = root / '.github/workflows/release.yml'
    if release.is_file():
        producers['release.yml'] = workflow_jobs(release.read_text(encoding='utf-8')).get('frontend', '')
    for filename, job in producers.items():
        commands = set()
        for line in job.splitlines():
            line = line.strip().removeprefix('- run: ').removeprefix('run: ')
            if line.startswith(('git ', 'test ', 'pnpm ')):
                # A failed non-final command in an && list does not trigger
                # bash errexit. These mandatory checks must stand alone.
                commands.add(tuple(shlex.split(line)))
        required = set().union(*(set(row[3:]) for row in commands
                                 if row[:3] == ('git', 'ls-files', '--error-unmatch')))
        compared = set().union(*(set(row[3:]) for row in commands
                                 if row[:3] == ('git', 'diff', '--exit-code')))
        if 'pnpm-lock.yaml' not in required & compared:
            raise ValueError(filename + ': lockfile requires Git drift checks')
        for name, full_name in zip(names, full[1:], strict=True):
            if full_name in tracked:
                if name not in required & compared:
                    raise ValueError(filename + ': tracked generated output requires Git drift checks')
                continue
            if name in required:
                raise ValueError(filename + ': ignored generated output cannot be required as tracked')
            if ('pnpm', 'openapi:check') not in commands:
                raise ValueError(filename + ': ignored generated output needs independent OpenAPI parity')
            guards = {('test', '-f', name), ('test', '!', '-L', name), ('test', '-s', name)}
            if not guards <= commands:
                raise ValueError(filename + ': generated output requires regular nonempty file guards')
            if ('git', 'check-ignore', '-q', '--', name) not in commands:
                raise ValueError(filename + ': ignored output requires its explicit Git ignore guard')


def check_ci(root):
    """Lint the canonical handoff job shape, not arbitrary YAML semantics."""
    content = (Path(root) / '.github/workflows/ci.yml').read_text(encoding='utf-8')
    jobs = workflow_jobs(content)
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
    check_generation_ci(root, producer)
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
