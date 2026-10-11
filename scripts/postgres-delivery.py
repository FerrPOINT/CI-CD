"""Privileged isolated PostgreSQL delivery; never production/SDLC admission."""
from __future__ import annotations

import argparse
import ctypes
import fcntl
import hashlib
import json
import os
import re
import signal
import stat
import subprocess
import sys
import time
import traceback
from pathlib import Path
from urllib.parse import quote
from urllib.request import Request, HTTPRedirectHandler, ProxyHandler, build_opener
from urllib.error import HTTPError, URLError


class Blocked(RuntimeError):
    pass


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def require(condition, reason):
    if not condition:
        raise Blocked(reason)


def canonical(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':')).encode()


def digest(data):
    return hashlib.sha256(data).hexdigest()


def plain(path):
    path = Path(path)
    require(path.is_absolute() and '..' not in path.parts, 'normalized_absolute_path_required')
    for item in [path, *path.parents]:
        require(not item.is_symlink(), 'symlink_target_rejected')
    return path


def read(path, limit=128 * 1024):
    path = plain(path)
    require(path.is_file() and path.stat().st_size <= limit, 'journal_file_unavailable')
    data = path.read_bytes()
    require(len(data) <= limit, 'journal_size_exceeded')
    return json.loads(data)


def directory(path):
    path = plain(path)
    existed = path.exists()
    path.mkdir(mode=0o700, exist_ok=True)
    require(path.is_dir(), 'directory_unavailable')
    if not existed:
        sync_directory(path.parent)
    return path


def immutable(path, value):
    path = plain(path)
    data = canonical(value)
    if path.exists():
        require(path.read_bytes() == data, 'immutable_journal_conflict')
        return
    with path.open('xb') as stream:
        os.chmod(path, 0o600)
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
    sync_directory(path.parent)


def sync_directory(path):
    fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def replace(path, value):
    path = plain(path)
    temporary = path.with_name(path.name + '.next')
    require(not temporary.exists(), 'torn_pointer_requires_owner_investigation')
    immutable(temporary, value)
    os.replace(temporary, path)
    sync_directory(path.parent)


def parent_death():
    parent = os.getppid()
    require(ctypes.CDLL(None).prctl(1, signal.SIGKILL) == 0 and os.getppid() == parent,
            'coordinator_parent_unavailable')


class Owner:
    def __init__(self, packet):
        self.packet = packet
        self.policy = p = packet['policy']
        self.root = plain(packet['root'])
        self.command = packet['command']
        self.original = packet['originalOperation']
        self.project = p['projectName']
        require(re.fullmatch(r'sdlc-qa-forge-pg-[0-9a-f]{12,32}', self.project), 'disposable_target_required')
        suffix = self.project.removeprefix('sdlc-qa-forge-pg-')
        self.parent = p['networkName'].removesuffix('_qa')
        require(self.parent == 'sdlc-qa-forge-delivery-' + suffix
                and p['volumeName'] == self.parent + '_delivery-qa'
                and p['postgresVolumeName'] == self.project + '_postgres-data', 'target_owner_binding_mismatch')
        require(re.fullmatch(r'forge_test_pg_[a-z0-9_]{1,42}', p['initialDatabase']), 'disposable_source_database_required')
        self.marker = {'schema': 'forge/isolated-pg-owner/v1', 'projectId': packet['projectId'],
                       'policySha256': packet['policySha256']}
        if not packet['readOnly']:
            directory(self.root)
            self.lock = (self.root / '.lock').open('a+b')
            try:
                fcntl.flock(self.lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                raise Blocked('previous_local_writer_alive') from None
            if not (self.root / 'owner.json').exists():
                require({x.name for x in self.root.iterdir()} == {'.lock'}, 'existing_target_not_enrolled')
                for name in ('operations', 'manifests', 'backups', 'connections', 'docker-config'):
                    directory(self.root / name)
                immutable(self.root / 'docker-config/config.json', {})
                immutable(self.root / 'owner.json', self.marker)
        require(read(self.root / 'owner.json') == self.marker, 'owner_or_policy_changed')
        self.dir = self.root / 'operations' / digest(self.command['operationKey'].encode())
        self.args = argparse.Namespace(project=self.project, project_directory=plain(p['composeRoot']),
            compose_file=[plain(p['composeFile'])], env_file=[], layout='legacy',
            _docker=['/usr/local/bin/docker'], _daemon_id=p['daemonId'])
        sys.path.insert(0, str(plain(p['baseRoot'])))
        from scripts import platform_backup, platform_postgres
        self.base, self.pg = platform_backup, platform_postgres
        require(Path(self.base.__file__).resolve().is_relative_to(Path(p['baseRoot']).resolve()), 'pinned_utility_import_mismatch')
        # Bind the pinned utility consumer to bounded, fixed-daemon subprocess transport.
        self.base._run = self.base_run
        self.pg._run = self.base_run
        self.pg.sql = lambda args, database, statement: self.sql(database.name, statement)
        self.intent = None
        self.lease = None

    def env(self):
        return {'PATH': '/usr/local/bin:/usr/bin:/bin', 'DOCKER_HOST': 'unix:///var/run/docker.sock',
                'DOCKER_CONFIG': str(self.root / 'docker-config'), 'PYTHONUTF8': '1'}

    def run(self, command, data=None, extra_env=None, limit=32 * 1024 * 1024, deadline=20):
        environment = self.env()
        environment.update(extra_env or {})
        child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                 env=environment, preexec_fn=parent_death)
        try:
            out, error = child.communicate(data, timeout=deadline)
        except BaseException:
            child.kill()
            child.wait()
            raise
        require(len(out) <= limit and len(error) <= 1024 * 1024, 'command_output_exceeds_bound')
        require(child.returncode == 0, 'owner_command_failed_or_unknown')
        return out

    def base_run(self, command, *, input_stream=None, output_stream=None, text=False):
        command = list(command)
        if 'exec' in command:
            at = command.index('exec') + 1
            command[at:at] = ['-e', 'PGOPTIONS=-c statement_timeout=8000 -c lock_timeout=3000']
        data = input_stream.read(32 * 1024 * 1024 + 1) if input_stream is not None else None
        require(data is None or len(data) <= 32 * 1024 * 1024, 'backup_exceeds_supported_bound')
        result = self.run(command, data)
        if output_stream is not None:
            output_stream.write(result)
            result = None
        elif text:
            result = result.decode('utf-8')
        return subprocess.CompletedProcess(command, 0, result, '' if text else b'')

    def docker(self, *args):
        return json.loads(self.run(['/usr/local/bin/docker', *args]))

    def sql(self, database, statement):
        require(database == 'postgres' or re.fullmatch(r'forge_test_pg_[a-z0-9_]{1,42}', database), 'unknown_database_target')
        command = self.base.compose_command(self.args, 'exec', '-T', 'database', 'psql', '-X', '-qAt',
                                           '-v', 'ON_ERROR_STOP=1', '-U', 'postgres', '-d', database)
        bounded = 'SET statement_timeout=8000;\nSET lock_timeout=3000;\n' + statement
        return self.run(command, bounded.encode(), limit=2 * 1024 * 1024).decode().strip()

    def database(self, name):
        return self.base.Database('isolated-application', 'database', 'postgres', name)

    def point(self, name):
        path = self.root / (name + '.json')
        return read(path) if path.exists() else None

    def manifest(self, reference):
        require(re.fullmatch(r'[0-9a-f]{64}', reference or ''), 'manifest_reference_invalid')
        path = self.root / 'manifests' / (reference + '.json')
        value = read(path)
        require(digest(path.read_bytes()) == reference and value['projectId'] == self.packet['projectId']
                and value['policySha256'] == self.packet['policySha256'], 'manifest_identity_mismatch')
        return value

    def engine(self):
        p = self.policy
        require(self.docker('info', '--format', '{{json .ID}}') == p['daemonId'], 'daemon_identity_changed')
        network = self.docker('network', 'inspect', p['networkName'])[0]
        require(network['Internal'] and network['Labels'].get('com.docker.compose.project') == self.parent
                and network['Labels'].get('sdlc.task') == 'forge-task-delivery', 'network_not_owned_or_isolated')
        pg = self.docker('container', 'inspect', self.project + '-database-1')[0]
        labels = pg['Config']['Labels']
        require(pg['State']['Running'] and pg['Image'] == p['postgresImageId']
                and labels.get('com.docker.compose.project') == self.project
                and labels.get('com.docker.compose.service') == 'database'
                and labels.get('sdlc.task') == 'forge-task-delivery'
                and labels.get('sdlc.purpose') == 'disposable-postgres-target'
                and not pg['HostConfig']['Privileged'] and not pg['HostConfig']['PortBindings'], 'postgres_target_config_mismatch')
        require(len(pg['Mounts']) == 2
                and any(m['Type'] == 'volume' and m.get('Name') == p['postgresVolumeName'] for m in pg['Mounts'])
                and any(m['Type'] == 'bind' and not m['RW'] and m['Destination'] == '/etc/forge/pg_hba.conf'
                        and m['Source'] == self.host_path(p['hbaFile']) for m in pg['Mounts']), 'postgres_data_resource_mismatch')
        require(pg['Config']['Cmd'] == ['postgres', '-c', 'hba_file=/etc/forge/pg_hba.conf'], 'postgres_hba_config_mismatch')
        controller = self.docker('container', 'inspect', self.parent + '-qa-1')[0]
        require(controller['NetworkSettings']['Networks'][p['networkName']]['IPAddress'] == p['controllerIp'],
                'controller_address_changed')
        expected_hba = ('local all postgres trust\n'
                        f'host all postgres {p["controllerIp"]}/32 trust\n'
                        'host all forge_reader 0.0.0.0/0 scram-sha-256\n'
                        'host all forge_writer 0.0.0.0/0 scram-sha-256\n'
                        'host all all 0.0.0.0/0 reject\n'
                        'host all all ::/0 reject\n').encode()
        require(plain(p['hbaFile']).read_bytes() == expected_hba, 'postgres_write_admission_hba_changed')
        volume = self.docker('volume', 'inspect', p['postgresVolumeName'])[0]
        require(volume['Labels'].get('com.docker.compose.project') == self.project
                and volume['Labels'].get('sdlc.task') == 'forge-task-delivery', 'postgres_volume_owner_mismatch')
        require(set(pg['NetworkSettings']['Networks']) == {p['networkName']}, 'postgres_network_mismatch')
        self.pg_ip = pg['NetworkSettings']['Networks'][p['networkName']]['IPAddress']
        require(re.fullmatch(r'[0-9.]+', self.pg_ip), 'postgres_address_unavailable')
        identifiers = self.run(['/usr/local/bin/docker', 'container', 'ls', '--all', '--quiet', '--filter',
                               'label=com.docker.compose.project=' + self.project]).decode().splitlines()
        require(1 <= len(identifiers) <= 2, 'unexpected_target_container_inventory')
        for identifier in identifiers:
            actual = self.docker('container', 'inspect', identifier)[0]
            require(actual['Config']['Labels'].get('com.docker.compose.service') in ('database', 'application')
                    and actual['Config']['Labels'].get('sdlc.task') == 'forge-task-delivery', 'unknown_target_service')
            if actual['Config']['Labels'].get('com.docker.compose.service') == 'application':
                files = actual['Config']['Labels'].get('com.docker.compose.project.config_files', '').split(',')
                require(len(files) == 1 and plain(files[0]).resolve().is_relative_to(plain(p['composeRoot']).resolve()),
                        'previous_application_manifest_missing')
                read(files[0])
                self.application_compose = files[0]
        actual_id = self.sql('postgres', 'SELECT system_identifier::text FROM pg_control_system();')
        require(actual_id == p['systemIdentifier'], 'postgres_system_identifier_mismatch')
        require(self.sql('postgres', 'SHOW hba_file;') == '/etc/forge/pg_hba.conf', 'loaded_hba_path_changed')
        require(self.sql('postgres', 'SELECT count(*) FROM pg_hba_file_rules WHERE error IS NOT NULL;') == '0',
                'hba_rules_unavailable')
        if self.point('current') is None and not list((self.root / 'operations').iterdir()):
            require(len(identifiers) == 1, 'refusing_enrollment_over_existing_application')
            names = json.loads(self.sql('postgres', "SELECT COALESCE(json_agg(datname),'[]') FROM pg_database "
                "WHERE datname NOT IN ('postgres','template0','template1');"))
            require(names == [p['initialDatabase']], 'refusing_enrollment_over_existing_candidate_databases')

    def roles(self):
        roles = json.loads(self.sql('postgres', "SELECT json_agg(json_build_object('name',rolname,'login',rolcanlogin,"
            "'super',rolsuper,'createdb',rolcreatedb,'createrole',rolcreaterole,'replication',rolreplication,'bypass',rolbypassrls)) "
            "FROM pg_roles WHERE rolname NOT LIKE 'pg_%';"))
        require({x['name'] for x in roles} == {'postgres', 'forge_reader', 'forge_writer'}, 'unknown_database_role')
        for role in roles:
            if role['name'] != 'postgres':
                require(not any(role[x] for x in ('super', 'createdb', 'createrole', 'replication', 'bypass')),
                        'application_role_has_privileged_access')
        require(self.sql('postgres', "SELECT count(*) FROM pg_auth_members m "
                "JOIN pg_roles r ON r.oid=m.roleid JOIN pg_roles u ON u.oid=m.member "
                "WHERE r.rolname NOT LIKE 'pg_%' OR u.rolname NOT LIKE 'pg_%';") == '0',
                'unknown_role_membership')
        return {x['name']: x for x in roles}

    def sessions(self):
        pid = self.packet['guardPid']
        unknown = self.sql('postgres', f"SELECT count(*) FROM pg_stat_activity WHERE backend_type='client backend' "
            f"AND pid NOT IN (pg_backend_pid(),{int(pid)}) AND usename <> 'forge_reader';")
        require(unknown == '0', 'unconfirmed_previous_or_unknown_writer')

    def guard(self):
        require(self.lease is not None and time.time() < self.lease['expiresAt'], 'stale_deployment_lease')
        require(self.point('generation') == self.lease['generation'], 'stale_deployment_generation')
        key = int(self.packet['guardKey'])
        result = self.sql('postgres', f"SELECT count(*) FROM pg_locks l JOIN pg_stat_activity a ON a.pid=l.pid "
            f"WHERE l.locktype='advisory' AND l.granted AND l.pid={int(self.packet['guardPid'])} "
            f"AND l.classid={key >> 32}::oid AND l.objid={key & 0xffffffff}::oid "
            f"AND a.application_name='forge_pg_guard:{self.packet['projectId']}';")
        require(result == '1', 'deployment_guard_unavailable')
        self.engine()
        self.roles()
        require(time.time() < self.lease['expiresAt'], 'stale_deployment_lease')

    def checkpoint(self, name, payload=None):
        immutable(self.dir / (name + '.json'), {'phase': name, 'generation': self.lease['generation'],
                                                'evidence': payload})
        # QA can pause an actual checkpoint; lease and guard must still pass afterwards.
        if os.environ.get('CICD_TEST_PG_PAUSE_AT') == name:
            resume = plain(os.environ['CICD_TEST_PG_RESUME_FILE'])
            while not resume.exists():
                self.guard()
                time.sleep(0.05)
        self.guard()

    def fingerprint(self, database):
        require(int(self.sql(database, 'SELECT pg_database_size(current_database());')) <= 16 * 1024 * 1024,
                'database_exceeds_supported_bound')
        require(self.sql(database, "SELECT count(*) FROM pg_database d, LATERAL "
                "aclexplode(COALESCE(d.datacl,acldefault('d',d.datdba))) a "
                "WHERE d.datname=current_database() AND a.grantee=0 "
                "AND a.privilege_type IN ('CONNECT','CREATE','TEMPORARY');") == '0',
                'public_database_admission_not_closed')
        require(self.sql(database, "SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace "
                "WHERE n.nspname='public' AND c.relkind IN ('r','p','S','v','m','f') "
                "AND pg_get_userbyid(c.relowner)<>'postgres';") == '0', 'unknown_application_object_owner')
        evidence = self.pg.database_evidence(self.args, self.database(database))
        require(not evidence['routines'] and not evidence['triggers'], 'unsupported_writer_routines_or_triggers')
        require(self.sql(database, "SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace "
                "WHERE n.nspname='public' AND c.relkind IN ('r','p') "
                "AND has_table_privilege('forge_reader',c.oid,'INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER');") == '0'
                and self.sql(database, "SELECT has_schema_privilege('forge_reader','public','CREATE');") == 'f',
                'application_reader_can_write_or_create')
        require(self.sql(database, "SELECT has_schema_privilege('forge_writer','public','CREATE') "
                "OR has_database_privilege('forge_writer',current_database(),'CREATE,TEMPORARY') "
                "OR has_table_privilege('forge_writer','public._sqlx_migrations','INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER');") == 'f',
                'application_writer_can_create_or_mutate_history')
        tables = json.loads(self.sql(database, "SELECT COALESCE(json_agg(json_build_object('schema',n.nspname,'table',c.relname) "
            "ORDER BY n.nspname,c.relname),'[]') FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace "
            "WHERE n.nspname NOT LIKE 'pg_%' AND n.nspname <> 'information_schema' AND c.relkind IN ('r','p');"))
        require(0 < len(tables) <= 16 and all(t['schema'] == 'public' for t in tables), 'unsupported_database_catalog')
        row_hashes = {}
        for table in tables:
            identifier = self.pg.ident(table['schema']) + '.' + self.pg.ident(table['table'])
            require(int(self.sql(database, f'SELECT count(*) FROM {identifier};')) <= 512, 'row_inventory_exceeds_bound')
            rows = self.sql(database, f"SET timezone='UTC'; SELECT COALESCE(jsonb_agg(to_jsonb(t) ORDER BY to_jsonb(t)::text COLLATE \"C\"),'[]') FROM {identifier} t;")
            row_hashes[table['schema'] + '.' + table['table']] = digest(canonical(json.loads(rows)))
        history = json.loads(self.sql(database, "SELECT COALESCE(json_agg(json_build_object('version',version,"
            "'checksum',encode(checksum,'hex'),'success',success) ORDER BY version),'[]') FROM public._sqlx_migrations;"))
        require(history and all(x['success'] for x in history), 'unknown_migration_history')
        return {'schema': {'version': history[-1]['version'], 'catalogSha256': evidence['schema_hash']},
                'history': history, 'dataSha256': digest(canonical({'rows': row_hashes, 'sequences': evidence['sequences']})),
                'sequencesSha256': digest(canonical(evidence['sequences']))}

    def history(self, actual, descriptor, version):
        expected = [{'version': m['version'], 'checksum': hashlib.sha384(m['sql'].encode()).hexdigest(), 'success': True}
                    for m in descriptor['migrations'] if m['version'] <= version]
        require(actual['history'] == expected, 'applied_migration_bytes_changed_or_unknown')

    def image(self, descriptor):
        actual = self.docker('image', 'inspect', descriptor['imageId'])[0]
        require(actual['Id'] == descriptor['imageId'] and actual['Os'] == 'linux'
                and actual['Config']['Labels'].get('org.opencontainers.image.revision') == descriptor['sourceCommit']
                and not actual['Config'].get('Volumes'), 'actual_image_commit_or_config_mismatch')

    def host_path(self, local):
        local = plain(local).resolve()
        relative = local.relative_to(plain(self.policy['volumeRoot']).resolve())
        volume = self.docker('volume', 'inspect', self.policy['volumeName'])[0]
        require(volume['Labels'].get('com.docker.compose.project') == self.parent
                and volume['Labels'].get('sdlc.task') == 'forge-task-delivery', 'publication_volume_owner_mismatch')
        return str(Path(volume['Mountpoint']) / relative)

    def drain(self, source):
        self.checkpoint('drain-intent')
        if hasattr(self, 'application_compose'):
            self.run(['/usr/local/bin/docker-compose', '-p', self.project, '-f', self.application_compose, 'stop', 'application'])
        self.sql('postgres', 'BEGIN; ALTER ROLE forge_writer NOLOGIN; '
                 f'REVOKE CONNECT ON DATABASE {self.pg.ident(source)} FROM forge_writer; COMMIT;')
        self.sql('postgres', "SELECT pg_terminate_backend(pid,5000) FROM pg_stat_activity WHERE usename='forge_writer';")
        self.guard()
        self.sessions()
        require(not self.roles()['forge_writer']['login'], 'writer_drain_not_enforced')
        self.checkpoint('drained', {'sourceDatabase': source, 'rpo': 'zero_acknowledged_pre_drain_writes'})

    def create_database(self, kind):
        name = 'forge_test_pg_' + digest((self.project + self.command['operationKey']).encode())[:20] + '_' + kind
        require(self.sql('postgres', f"SELECT count(*) FROM pg_database WHERE datname={self.pg.literal(name)};") == '0',
                'database_name_already_exists_no_destructive_replace')
        self.guard()
        self.sql('postgres', f'CREATE DATABASE {self.pg.ident(name)} TEMPLATE template0;')
        self.sql('postgres', f'REVOKE ALL ON DATABASE {self.pg.ident(name)} FROM PUBLIC;')
        return name

    def restore(self, database, dump):
        self.guard()
        self.sessions()
        self.base._restore_database(self.args, self.database(database), plain(dump))
        self.guard()
        self.sessions()

    def backup(self, source, previous):
        self.checkpoint('backup-intent')
        folder = directory(self.root / 'backups' / digest(self.command['operationKey'].encode()))
        dump = folder / 'database.dump'
        require(not dump.exists(), 'backup_command_never_repeated')
        before = self.fingerprint(source)
        self.sessions()
        self.base._write_database_dump(self.args, self.database(source), dump)
        with dump.open('rb') as stream:
            os.fsync(stream.fileno())
        require(before == self.fingerprint(source), 'source_data_changed_during_backup')
        snapshot = {'schema': 'forge/isolated-pg-backup/v1', 'sourceDatabase': source,
                    'systemIdentifier': self.policy['systemIdentifier'], 'fingerprint': before,
                    'previousManifestSha256': previous, 'dumpSha256': self.base.sha256(dump),
                    'generation': self.lease['generation'], 'snapshotAt': time.time(),
                    'rpo': 'zero_acknowledged_pre_drain_writes'}
        immutable(folder / 'manifest.json', snapshot)
        self.checkpoint('backup-created', {'dumpSha256': snapshot['dumpSha256']})
        rehearsal = self.create_database('drill')
        self.checkpoint('backup-drill-intent', {'database': rehearsal})
        self.restore(rehearsal, dump)
        require(self.fingerprint(rehearsal) == before and self.fingerprint(source) == before, 'backup_restore_drill_mismatch')
        immutable(folder / 'verified.json', {'manifestSha256': digest(canonical(snapshot)), 'rehearsalDatabase': rehearsal,
                                           'fingerprint': before})
        self.checkpoint('backup-verified', {'backupManifestSha256': digest(canonical(snapshot))})
        return str(folder), snapshot

    def verified_backup(self, folder):
        folder = plain(folder)
        require(folder.resolve().is_relative_to((self.root / 'backups').resolve()), 'backup_outside_owner_root')
        snapshot = read(folder / 'manifest.json')
        verified = read(folder / 'verified.json')
        require(snapshot['schema'] == 'forge/isolated-pg-backup/v1'
                and snapshot['systemIdentifier'] == self.policy['systemIdentifier']
                and verified['manifestSha256'] == digest(canonical(snapshot))
                and verified['fingerprint'] == snapshot['fingerprint'], 'backup_not_verified_or_wrong_identity')
        require(self.base.sha256(plain(folder / 'database.dump')) == snapshot['dumpSha256'], 'backup_corrupt')
        require(self.fingerprint(snapshot['sourceDatabase']) == snapshot['fingerprint'], 'source_data_drift_after_snapshot')
        return snapshot, folder / 'database.dump'

    def migrate(self, database, descriptor):
        self.checkpoint('migration-intent', {'database': database})
        folder = directory(self.dir / 'migrations')
        for migration in descriptor['migrations']:
            file = folder / (str(migration['version']).zfill(4) + '_' + migration['description'] + '.sql')
            require(not file.exists(), 'migration_command_never_repeated')
            with file.open('xb') as stream:
                stream.write(migration['sql'].encode())
                stream.flush()
                os.fsync(stream.fileno())
        self.guard()
        self.sessions()
        self.run([self.policy['migrationBin']], extra_env={'CICD_LOCAL_DELIVERY_MODE': 'local-verification',
            'CICD_LOCAL_PG_DATABASE_URL': f'postgres://postgres@{self.pg_ip}:5432/{database}',
            'CICD_LOCAL_PG_SYSTEM_ID': self.policy['systemIdentifier'], 'CICD_LOCAL_PG_MIGRATIONS': str(folder),
            'CICD_LOCAL_PG_GUARD_PID': str(self.packet['guardPid'])})
        actual = self.fingerprint(database)
        self.history(actual, descriptor, descriptor['targetSchema']['version'])
        require(actual['schema'] == descriptor['targetSchema'], 'post_migration_schema_mismatch')
        self.checkpoint('migration-verified', actual)
        return actual

    def application(self, manifest, manifest_sha):
        self.guard()
        self.sessions()
        password = os.environ.get('CICD_LOCAL_PG_RUNTIME_PASSWORD', '')
        require(16 <= len(password) <= 128, 'protected_runtime_credential_required')
        database = manifest['database']
        self.sql(database, 'BEGIN; GRANT USAGE ON SCHEMA public TO forge_reader,forge_writer; '
                 'GRANT SELECT ON ALL TABLES IN SCHEMA public TO forge_reader; '
                 'GRANT SELECT,INSERT,UPDATE,DELETE ON ALL TABLES IN SCHEMA public TO forge_writer; '
                 'REVOKE ALL ON public._sqlx_migrations FROM forge_writer; '
                 'GRANT USAGE,SELECT ON ALL SEQUENCES IN SCHEMA public TO forge_writer; COMMIT;')
        self.sql('postgres', f'GRANT CONNECT ON DATABASE {self.pg.ident(database)} TO forge_reader;')
        require(self.sql(database, "SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace "
                "WHERE n.nspname='public' AND c.relkind IN ('r','p') "
                "AND has_table_privilege('forge_reader',c.oid,'INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER');") == '0'
                and self.sql(database, "SELECT has_schema_privilege('forge_reader','public','CREATE');") == 'f',
                'application_reader_can_write_or_create')
        connection = self.root / 'connections' / (manifest_sha + '.json')
        immutable(connection, {role + 'Url': f'postgres://forge_{role}:{quote(password,safe="")}@{self.pg_ip}:5432/{database}'
                               for role in ('reader', 'writer')})
        os.chown(connection, 65532, 65532)
        os.chmod(connection, 0o400)
        manifest_file = self.root / 'manifests' / (manifest_sha + '.json')
        os.chmod(manifest_file, 0o444)
        compose = read(self.policy['composeFile'])
        require(set(compose['services']) <= {'database', 'application'}, 'unexpected_compose_service')
        compose['services']['application'] = {'image': manifest['descriptor']['imageId'], 'pull_policy': 'never',
            'user': '65532:65532', 'read_only': True, 'cap_drop': ['ALL'], 'security_opt': ['no-new-privileges:true'],
            'pids_limit': 64, 'mem_limit': '128m', 'cpus': 0.5,
            'labels': {'sdlc.task': 'forge-task-delivery', 'sdlc.purpose': 'disposable-postgres-application'},
            'environment': {'CICD_PG_CONNECTION_FILE': '/forge/connection.json'},
            'volumes': [{'type': 'bind', 'source': self.host_path(manifest_file), 'target': '/forge/manifest.json', 'read_only': True},
                        {'type': 'bind', 'source': self.host_path(connection), 'target': '/forge/connection.json', 'read_only': True}],
            'networks': ['owner']}
        file = plain(self.policy['composeRoot']) / (digest(self.command['operationKey'].encode()) + '.json')
        immutable(file, compose)
        self.checkpoint('application-intent', {'manifestSha256': manifest_sha, 'composeFile': str(file)})
        self.run(['/usr/local/bin/docker-compose', '-p', self.project, '-f', str(file), 'up', '--detach',
                  '--no-build', '--pull', 'never', '--force-recreate', 'application'], deadline=90)
        self.checkpoint('application-started', {'manifestSha256': manifest_sha})

    def probe(self, origin, path, expected):
        observed = {'status': 'unavailable', 'httpStatus': None, 'bodySha256': None, 'observedAt': time.time()}
        try:
            with build_opener(ProxyHandler({}), NoRedirect()).open(
                    Request(origin + path, headers={'Accept': 'application/json'}), timeout=3) as response:
                body = response.read(128 * 1024 + 1)
                observed.update(httpStatus=response.status, bodySha256=digest(body))
                observed['status'] = 'verified' if len(body) <= 128 * 1024 and response.status == 200 and digest(body) == expected else 'failed'
        except HTTPError as error:
            observed.update(status='failed', httpStatus=error.code)
        except (URLError, TimeoutError, OSError):
            pass
        return observed

    def observe(self, manifest, manifest_sha, receipt):
        try:
            self.engine()
            actual = self.docker('container', 'inspect', self.project + '-application-1')[0]
            require(actual['State']['Running'] and actual['Image'] == manifest['descriptor']['imageId']
                    and actual['Config']['Labels'].get('sdlc.purpose') == 'disposable-postgres-application'
                    and actual['HostConfig']['ReadonlyRootfs'] and not actual['HostConfig']['Privileged'], 'application_identity_unavailable')
            mounts = actual['Mounts']
            expected = {'/forge/manifest.json': self.host_path(self.root / 'manifests' / (manifest_sha + '.json')),
                        '/forge/connection.json': self.host_path(self.root / 'connections' / (manifest_sha + '.json'))}
            require(len(mounts) == 2 and all(not m['RW'] and expected.get(m['Destination']) == m['Source'] for m in mounts),
                    'application_mount_identity_mismatch')
            require(set(actual['NetworkSettings']['Networks']) == {self.policy['networkName']}, 'application_network_mismatch')
            ip = actual['NetworkSettings']['Networks'][self.policy['networkName']]['IPAddress']
            origin = 'http://' + ip + ':8000'
            before = self.fingerprint(manifest['database'])
            require(before['schema'] == manifest['descriptor']['targetSchema'], 'serving_database_schema_mismatch')
            identity = digest(canonical({'database': manifest['database'], 'schemaVersion': before['schema']['version']}))
            probes = {}
            for name, path, expected_sha in [('version', '/.forge/version', manifest_sha), ('database', '/identity', identity),
                ('health', self.policy['checks']['healthPath'], self.policy['checks']['healthBodySha256']),
                ('acceptance', self.policy['checks']['acceptancePath'], self.policy['checks']['acceptanceBodySha256']),
                ('endVersion', '/.forge/version', manifest_sha)]:
                probe = self.probe(origin, path, expected_sha)
                if name == 'version':
                    for _ in range(10):
                        if probe['status'] != 'unavailable':
                            break
                        time.sleep(0.1)
                        probe = self.probe(origin, path, expected_sha)
                probes[name] = probe
            end = self.docker('container', 'inspect', self.project + '-application-1')[0]
            require(end['Id'] == actual['Id'] and end['State']['Running'] and end['Image'] == actual['Image']
                    and self.fingerprint(manifest['database']) == before, 'database_or_container_changed_during_checks')
            receipt.update(probes=probes, containerId=actual['Id'], imageId=actual['Image'], database=manifest['database'],
                           databaseFingerprint=before)
            if probes['version']['status'] == 'unavailable' or probes['endVersion']['status'] == 'unavailable':
                receipt.update(status='unknown', reason='serving_identity_unknown')
            elif any(x['status'] == 'failed' for x in probes.values()):
                receipt.update(status='failed', reason='application_check_failed')
            elif any(x['status'] == 'unavailable' for x in probes.values()):
                receipt.update(status='unavailable', reason='application_check_unavailable')
            else:
                receipt.update(status='verified', reason='isolated_image_database_application_checks_verified')
        except BaseException:
            receipt.update(status='unknown', reason='database_or_engine_outcome_unknown')
        return receipt

    def release(self, manifest, manifest_sha, receipt):
        self.guard()
        self.sessions()
        immutable(self.dir / 'verified-checks.json', receipt)
        replace(self.root / 'confirmed.json', manifest_sha)
        self.checkpoint('release-intent', {'database': manifest['database'], 'restorePreviousSnapshotAllowed': False})
        self.sql('postgres', 'BEGIN; '
            f'GRANT CONNECT ON DATABASE {self.pg.ident(manifest["database"])} TO forge_writer; '
            'ALTER ROLE forge_writer LOGIN; COMMIT;')
        self.checkpoint('writes-released', {'database': manifest['database']})

    def readback(self):
        intent = read(self.dir / 'intent.json')
        require(intent['command'] == self.command and intent['originalOperation'] == self.original
                and intent['policySha256'] == self.packet['policySha256'], 'original_input_or_policy_changed')
        receipt = read(self.dir / 'result.json') if (self.dir / 'result.json').exists() else intent['receipt']
        reconciled = read(self.dir / 'reconciled.json') if (self.dir / 'reconciled.json').exists() else None
        for item in [receipt, reconciled]:
            if item is None:
                continue
            require(item['schema'] == 'forge/local-postgres-operation/v1'
                    and item['commandSha256'] == digest(canonical(self.command))
                    and item['originalOperation'] == self.original
                    and item['generation'] == intent['lease']['generation']
                    and not item['dispatchAllowed'] and not item['sdlcAcceptanceVerified'], 'receipt_binding_mismatch')
            if item['status'] == 'verified':
                require((self.dir / 'verified-checks.json').exists() and item.get('manifestSha256')
                        and all(item['probes'][name]['status'] == 'verified' for name in ('version', 'database', 'health', 'acceptance', 'endVersion')),
                        'verified_evidence_incomplete')
        latest = reconciled or receipt
        return {'schema': 'forge/local-postgres-readback/v1', 'receipt': receipt, 'reconciledReceipt': reconciled,
                'currentManifestSha256': self.point('current'), 'confirmedManifestSha256': self.point('confirmed'),
                'reconciliationNeeded': latest['status'] == 'unknown', 'dispatchAllowed': False, 'sdlcAcceptanceVerified': False}

    def reconcile(self):
        old = self.readback()
        if not old['reconciliationNeeded']:
            return old
        self.engine()
        self.roles()
        self.sessions()
        # Only a completed release can be observed as successful. Never repeat a critical command.
        if not (self.dir / 'release-intent.json').exists():
            return old
        intent = read(self.dir / 'intent.json')
        manifest_sha = self.point('current')
        if not manifest_sha or self.point('confirmed') != manifest_sha:
            return old
        manifest = self.manifest(manifest_sha)
        if manifest['operationKey'] != self.command['operationKey'] or not self.roles()['forge_writer']['login']:
            return old
        granted = self.sql('postgres', f"SELECT has_database_privilege('forge_writer',{self.pg.literal(manifest['database'])},'CONNECT');")
        if granted != 't':
            return old
        receipt = self.observe(manifest, manifest_sha, dict(intent['receipt']))
        if receipt['status'] == 'verified':
            receipt['manifestSha256'] = manifest_sha
            immutable(self.dir / 'reconciled.json', receipt)
        return self.readback()

    def execute(self):
        if self.dir.exists():
            return self.reconcile() if self.packet['reconcile'] else self.readback()
        require(not self.packet['readOnly'] and not self.packet['reconcile'], 'original_operation_unavailable')
        operations = list((self.root / 'operations').iterdir())
        require(len(operations) <= 128, 'operation_inventory_exceeds_bound')
        for operation in operations:
            intent = read(operation / 'intent.json')
            terminal = operation / ('reconciled.json' if (operation / 'reconciled.json').exists() else 'result.json')
            require(terminal.exists() and read(terminal)['status'] != 'unknown', 'unknown_operation_holds_target')
        require(self.point('current') == self.command['expectedManifestSha256'], 'current_manifest_cas_mismatch')
        self.engine()
        self.roles()
        previous = self.point('confirmed')
        if self.command['action'] == 'deploy':
            sealed = self.packet['sealedCandidate']
            require(sealed is not None, 'sealed_candidate_unavailable')
            descriptor, candidate = sealed['descriptor'], sealed['candidate']
            source = self.manifest(previous)['database'] if previous else self.policy['initialDatabase']
            actual = self.fingerprint(source)
            require(actual['schema'] == descriptor['sourceSchema'], 'source_schema_unknown_or_incompatible')
            self.history(actual, descriptor, descriptor['sourceSchema']['version'])
            self.image(descriptor)
            restore_from = None
        else:
            require(previous is not None, 'confirmed_pair_unavailable')
            confirmed = self.manifest(previous)
            current = self.manifest(self.point('current'))
            failed_dir = self.root / 'operations' / digest(current['operationKey'].encode())
            require(not (failed_dir / 'release-intent.json').exists(), 'writes_released_previous_snapshot_restore_forbidden')
            failed = read(failed_dir / 'result.json')
            require(failed['status'] in ('failed', 'unavailable') and failed.get('databaseFingerprint'), 'failed_pair_outcome_not_known')
            require(not self.roles()['forge_writer']['login'], 'writer_not_drained_for_restore')
            self.sessions()
            require(self.fingerprint(current['database']) == failed['databaseFingerprint'], 'failed_candidate_data_drift')
            snapshot, dump = self.verified_backup(current['backupDirectory'])
            require(snapshot['previousManifestSha256'] == previous, 'backup_previous_pair_mismatch')
            descriptor, candidate = confirmed['descriptor'], confirmed['candidate']
            source = snapshot['sourceDatabase']
            self.image(descriptor)
            restore_from = (snapshot, dump)
        generation = (self.point('generation') or 0) + 1
        replace(self.root / 'generation.json', generation)
        self.lease = {'generation': generation, 'expiresAt': time.time() + self.policy['leaseSeconds'],
                      'guardPid': self.packet['guardPid'], 'systemIdentifier': self.policy['systemIdentifier']}
        self.guard()
        directory(self.dir)
        receipt = {'schema': 'forge/local-postgres-operation/v1', 'scope': 'owner_local_isolated_verification',
            'commandSha256': digest(canonical(self.command)), 'originalOperation': self.original,
            'generation': generation, 'manifestSha256': None, 'previousManifestSha256': previous,
            'status': 'unknown', 'reason': 'critical_effect_not_finalized', 'probes': {},
            'dispatchAllowed': False, 'sdlcAcceptanceVerified': False}
        self.intent = {'command': self.command, 'originalOperation': self.original,
                       'policySha256': self.packet['policySha256'], 'lease': self.lease, 'receipt': receipt,
                       'candidate': candidate, 'descriptor': descriptor, 'sourceDatabase': source}
        immutable(self.dir / 'intent.json', self.intent)
        try:
            self.drain(source)
            if restore_from is None:
                folder, snapshot = self.backup(source, previous)
            else:
                snapshot, dump = restore_from
                folder = str(dump.parent)
                self.checkpoint('restore-backup-verified', {'dumpSha256': snapshot['dumpSha256']})
            target = self.create_database('restore' if restore_from else 'candidate')
            self.checkpoint('restore-intent' if restore_from else 'candidate-copy-intent', {'database': target})
            self.restore(target, Path(folder) / 'database.dump')
            require(self.fingerprint(target) == snapshot['fingerprint'], 'restored_snapshot_rows_or_schema_mismatch')
            self.checkpoint('restore-verified' if restore_from else 'candidate-copy-verified', self.fingerprint(target))
            if restore_from is None:
                try:
                    fingerprint = self.migrate(target, descriptor)
                except Blocked as error:
                    if str(error) == 'owner_command_failed_or_unknown':
                        self.sessions()
                        current_fp = self.fingerprint(target)
                        # The SQLx transaction left the verified source catalog intact.
                        if current_fp == snapshot['fingerprint']:
                            manifest_sha = self.publish(candidate, descriptor, target, folder, snapshot,
                                                        current_fp, False)
                            receipt.update(status='failed', reason='migration_failed_source_snapshot_preserved', database=target,
                                           databaseFingerprint=current_fp, manifestSha256=manifest_sha)
                            self.checkpoint('migration-failed', current_fp)
                            immutable(self.dir / 'result.json', receipt)
                            return self.readback()
                    raise
            else:
                fingerprint = self.fingerprint(target)
            manifest_sha = self.publish(candidate, descriptor, target, folder, snapshot, fingerprint, restore_from is not None)
            manifest = self.manifest(manifest_sha)
            receipt['manifestSha256'] = manifest_sha
            self.application(manifest, manifest_sha)
            receipt = self.observe(manifest, manifest_sha, receipt)
            if receipt['status'] == 'verified':
                self.release(manifest, manifest_sha, receipt)
            immutable(self.dir / 'result.json', receipt)
        except BaseException as error:
            immutable(self.dir / 'diagnostic.json', {'class': type(error).__name__,
                'reason': str(error) if isinstance(error, Blocked) else 'owner_effect_unknown',
                'frames': [{'function': frame.name, 'line': frame.lineno} for frame in traceback.extract_tb(error.__traceback__)]})
            # Leave the original unknown intent; no automatic dangerous retry or release.
        return self.readback()

    def publish(self, candidate, descriptor, target, folder, snapshot, fingerprint, restored):
        manifest = {'schema': 'forge/isolated-postgres-manifest/v1', 'projectId': self.packet['projectId'],
            'operationKey': self.command['operationKey'], 'policySha256': self.packet['policySha256'],
            'candidate': candidate, 'descriptor': descriptor, 'database': target,
            'systemIdentifier': self.policy['systemIdentifier'], 'generation': self.lease['generation'],
            'backupDirectory': folder, 'sourceSnapshot': snapshot['fingerprint'],
            'databaseFingerprint': fingerprint, 'restored': restored}
        manifest_sha = digest(canonical(manifest))
        immutable(self.root / 'manifests' / (manifest_sha + '.json'), manifest)
        replace(self.root / 'current.json', manifest_sha)
        return manifest_sha


def main():
    packet = json.loads(sys.stdin.buffer.read(256 * 1024 + 1))
    try:
        owner = Owner(packet)
        result = owner.execute()
    except BaseException as error:
        reason = str(error) if isinstance(error, Blocked) else 'owner_command_rejected_or_unavailable'
        result = {'schema': 'forge/local-postgres-rejection/v1', 'status': 'blocked', 'reason': reason,
                  'dispatchAllowed': False, 'sdlcAcceptanceVerified': False}
    sys.stdout.buffer.write(canonical(result) + b'\n')


if __name__ == '__main__':
    main()
