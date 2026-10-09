"""Private parent retention adapter. Importing it never prepares or launches QA."""
import copy
import hashlib
import os
from pathlib import Path
import re
import shutil


CAPABILITIES = ['CHOWN', 'DAC_OVERRIDE', 'FOWNER']
DISPOSABLE = ('target', 'scratch')


def parent_class(sdk, q):
    class ParentSession(sdk.ComposeHelper):
        def record(self, phase):
            data = dict(version=2, project=self.project, task=self.task, purpose=self.purpose,
                        daemon_id=self.identity, manifest=str(self.path), manifest_sha256=self.manifest_hash,
                        docker=self.docker, cleanup_id=self.cleanup_id, created_at=self.created_at,
                        updated_at=sdk.utc_now(), owner_pid=self.owner_pid, owner_host=self.owner_host,
                        phase=phase, sdk_sha256=q.SDK_SHA256,
                        retention_adapter_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest())
            q.atomic(self.journal, data)
            self.registry.mkdir(parents=True, exist_ok=True)
            sdk.restrict(self.registry)
            pointer = self.registry / (q.digest(str(self.journal).encode()) + '.json')
            if phase in ('cleaned', 'disposable-cleaned'):
                pointer.unlink(missing_ok=True)
            else:
                q.atomic(pointer, {'journal': str(self.journal)})

        def write_reviewed(self, manifest):
            if set(manifest) != {'services', 'networks', 'volumes'}:
                raise ValueError('Unexpected parent manifest scope')
            qa = manifest['services']['qa']
            if ('privileged' in qa or qa.get('user') != '0:0'
                    or qa.get('cap_drop') != ['ALL'] or qa.get('cap_add') != CAPABILITIES):
                raise ValueError('Parent requires reviewed explicit minimum capabilities')
            super().write(manifest['services'], volumes=manifest['volumes'], networks=manifest['networks'])

    return ParentSession


def parent_manifest(q, original, config):
    """Keep original limits/images/env and bind target outside the external cache."""
    n = q.plain(config['native_root']).resolve()
    cache = q.plain(config['cache_root']).resolve()
    sources = n / 'sources'
    if (cache == n or cache.is_relative_to(n / 'disposable') or n.is_relative_to(cache)
            or any(cache == protected or cache.is_relative_to(protected) or protected.is_relative_to(cache)
                   for protected in (sources, n / 'output', n / 'children'))):
        raise ValueError('External cache cannot overlap disposable/source roots')
    for name in ('forge_git_sha', 'base_git_sha'):
        if not re.fullmatch('[a-f0-9]{40}', config[name]):
            raise ValueError('Exact Git SHA export required')
    manifest = copy.deepcopy(original)
    qa = manifest['services']['qa']
    if qa.pop('privileged', None) is not True:
        raise ValueError('Original privileged QA basis differs; explicit review required')
    if any(key in qa for key in ('cap_add', 'cap_drop', 'user')):
        raise ValueError('Original QA capability basis differs')
    qa.update(user='0:0', cap_drop=['ALL'], cap_add=CAPABILITIES[:])
    env = qa['environment']
    if env['CARGO_HOME'] != '/cache/cargo' or env['CARGO_TARGET_DIR'] != '/cache/target/rust-gates':
        raise ValueError('Original dependency/target layout changed')
    mounts = qa['volumes']
    by_target = {item['target']: item for item in mounts}
    if len(by_target) != len(mounts):
        raise ValueError('Duplicate parent mount target')
    for target, source in (('/work/CI-CD', sources / 'CI-CD'),
                           ('/work/services-base', sources / 'services-base'), ('/cache', cache)):
        expected = by_target[target]
        if expected.get('type') != 'bind' or q.plain(expected['source']).resolve() != source:
            raise ValueError('Original parent source/cache mount differs')
        if target != '/cache' and expected.get('read_only') is not True:
            raise ValueError('Git exported sources must remain read-only')
    additions = [
        (n / 'disposable/target', '/cache/target', False),
        (n / 'disposable/scratch', '/tmp/forge-qa', False),
        (sources / 'CI-CD/scripts/native_qa_compose_client.py', '/usr/local/bin/docker-compose', True),
        (n / 'transport.json', '/etc/forge-native-qa-transport.json', True),
    ]
    for source, target, read_only in additions:
        if target in by_target:
            raise ValueError('Reviewed retention mount already exists')
        q.plain(source)
        mounts.append({'type': 'bind', 'source': str(source), 'target': target,
                       'read_only': read_only, 'bind': {'create_host_path': False}})
    env['TMPDIR'] = '/tmp/forge-qa'
    return manifest


def disposable_identity(q, config):
    n = q.plain(config['native_root']).resolve()
    if not re.fullmatch('sdlc-qa-forge-delivery-[a-f0-9]{12,32}', config['project']):
        raise ValueError('Exact owned parent project required')
    if not re.fullmatch('[a-f0-9]{40}', config['forge_git_sha']):
        raise ValueError('Exact Forge Git SHA required')
    return {'version': 1, 'native_root': str(n), 'project': config['project'],
            'forge_git_sha': config['forge_git_sha'], 'paths': list(DISPOSABLE)}


def create_disposable(q, config):
    """Prepare-only action, called once after an explicit future slot grant."""
    identity = disposable_identity(q, config)
    root = Path(identity['native_root']) / 'disposable'
    root.mkdir(exist_ok=False)
    q.atomic(root / 'ownership.json', identity, exclusive=True)
    for name in DISPOSABLE:
        (root / name).mkdir(exist_ok=False)
    return q.digest((root / 'ownership.json').read_bytes())


def cleanup_disposable(q, config, ownership_sha256, inventory, parent_journal, nested_journals):
    """Immediate finally cleanup only; not an age-based recovery or wildcard prune."""
    if os.name != 'posix':
        raise ValueError('Disposable native cleanup requires Linux')
    identity = disposable_identity(q, config)
    n = Path(identity['native_root'])
    projects = {config[key] for key in ('project', 'pg_project', 'oci_project')}
    if len(projects) != 3 or set(inventory) != projects:
        raise ValueError('Exact three-project independent inventory required')
    if any(set(state) != {'containers', 'networks', 'volumes'} or any(state.values())
           for state in inventory.values()):
        raise ValueError('Disposable target still has live/unknown Compose resources')
    for path in [parent_journal, *nested_journals]:
        path = q.plain(path).resolve()
        if not path.is_relative_to(n) or path.name != 'journal.json':
            raise ValueError('Unowned disposable cleanup journal')
        journal = q.read(path)
        if (journal.get('version') != 2 or journal.get('phase') != 'cleaned'
                or journal.get('daemon_id') != q.DAEMON_ID or journal.get('project') not in projects
                or journal.get('task') != q.TASK or journal.get('sdk_sha256') != q.SDK_SHA256
                or not re.fullmatch('[a-f0-9]{32}', journal.get('cleanup_id', ''))):
            raise ValueError('Actual v2 cleanup has not completed')
        manifest = path.parent / 'compose.json'
        if (journal.get('manifest') != str(manifest)
                or q.digest(q.plain(manifest).read_bytes()) != journal.get('manifest_sha256')):
            raise ValueError('Actual v2 cleanup manifest changed')
    if q.read(parent_journal)['project'] != config['project']:
        raise ValueError('Parent v2 cleanup journal mismatch')
    root = q.plain(n / 'disposable')
    marker = root / 'ownership.json'
    if q.digest(marker.read_bytes()) != ownership_sha256 or q.read(marker) != identity:
        raise ValueError('Disposable ownership proof changed')
    if set(path.name for path in root.iterdir()) != {*DISPOSABLE, 'ownership.json'}:
        raise ValueError('Unknown disposable path; cleanup refused')
    protected = [q.plain(config['cache_root']).resolve(), n / 'sources', n / 'output']
    for name in DISPOSABLE:
        target = q.plain(root / name).resolve()
        if target.parent != root or any(target == path or path.is_relative_to(target) or target.is_relative_to(path)
                                        for path in protected):
            raise ValueError('Disposable path overlaps protected content')
        for current, directories, files in os.walk(target, followlinks=False):
            for child in [*directories, *files]:
                q.plain(Path(current) / child)
    for name in DISPOSABLE:
        shutil.rmtree(root / name)
    q.atomic(root / 'cleanup.json', {'status': 'cleaned', 'paths': list(DISPOSABLE),
                                   'ownership_sha256': ownership_sha256})
    return {'status': 'cleaned', 'paths': list(DISPOSABLE), 'external_cache_deleted': False}
