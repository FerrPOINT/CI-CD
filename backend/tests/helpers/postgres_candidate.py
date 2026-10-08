"""Actual runner build/rehearsal for isolated protocol QA; no upstream admission."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

c = json.loads(Path('build.json').read_text())
sys.path.insert(0, '/work/services-base')
from scripts import platform_backup as backup, platform_postgres as pg

args = argparse.Namespace(project=c['project'], project_directory=Path(c['composeRoot']),
    compose_file=[Path(c['composeFile'])], env_file=[], layout='legacy',
    _docker=['/usr/local/bin/docker'], _daemon_id=c['daemonId'])

def run(command, **kw):
    # Desktop Compose startup is outside the protocol's critical-effect deadlines.
    deadline = 120 if command[:2] == ['docker', 'build'] else 60
    return subprocess.run(command, check=True, timeout=deadline, **kw)

def sql(db, statement):
    return run(backup.compose_command(args, 'exec', '-T', 'database', 'psql', '-X', '-qAt',
        '-v', 'ON_ERROR_STOP=1', '-U', 'postgres', '-d', db), input=statement,
        text=True, capture_output=True).stdout.strip()

def database(name):
    return backup.Database('fixture', 'database', 'postgres', name)

def schema(db):
    evidence = pg.database_evidence(args, database(db))
    return {'version': int(sql(db, 'SELECT max(version) FROM public._sqlx_migrations WHERE success;')),
            'catalogSha256': evidence['schema_hash']}

source = schema(c['sourceDatabase'])
migrations = c['migrations']
target = source
if c['mode'] != 'A':
    rehearsal = c['rehearsalDatabase']
    sql('postgres', f'CREATE DATABASE {pg.ident(rehearsal)} TEMPLATE template0;')
    dump = Path('source.dump').absolute()
    backup._write_database_dump(args, database(c['seedDatabase']), dump)
    backup._restore_database(args, database(rehearsal), dump)
    folder = Path('rehearsal-migrations').absolute()
    folder.mkdir()
    for m in migrations:
        (folder / f"{m['version']:04}_{m['description']}.sql").write_text(m['sql'])
    run([c['migrationBin']], env={**os.environ, 'CICD_LOCAL_DELIVERY_MODE':'local-verification',
        'CICD_LOCAL_PG_DATABASE_URL': f"postgres://postgres@{c['ip']}:5432/{rehearsal}",
        'CICD_LOCAL_PG_MIGRATIONS':str(folder), 'CICD_LOCAL_PG_SYSTEM_ID':c['systemIdentifier']})
    target = schema(rehearsal)
    sql('postgres', f'DROP DATABASE {pg.ident(rehearsal)};')
    dump.unlink()
    if c['mode'] == 'M':
        # Intentionally bad pending catalog; genuine runner artifact, negative case only.
        migrations[1]['sql'] = 'ALTER TABLE public.records ADD COLUMN id bigint NULL;'
        migrations[1]['sha256'] = hashlib.sha256(migrations[1]['sql'].encode()).hexdigest()
commit = run(['git','rev-parse','HEAD'], capture_output=True, text=True).stdout.strip()
run(['docker','build','--pull=false','--label','org.opencontainers.image.revision='+commit,
     '--iidfile','image-id','.'])
descriptor = {'schema':'forge/postgres-candidate/v1','dataProtocol':'isolated_postgres_shadow_v1',
    'imageId':Path('image-id').read_text().strip(),'sourceCommit':commit,'sourceSchema':source,
    'targetSchema':target,'readableSchemaVersions':[target['version']],'migrations':migrations}
Path('product.txt').write_text(json.dumps(descriptor, sort_keys=True, separators=(',',':')))
