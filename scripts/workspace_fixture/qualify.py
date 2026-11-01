"""Run integrated workspace bridge on disposable owned services, under Base slot."""
import argparse
import hashlib
import ipaddress
import json
import os
import socket
import subprocess
import sys
import uuid
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--base-tooling', type=Path, required=True)
parser.add_argument('--runtime-receipt', type=Path, required=True)
parser.add_argument('--postgres-image', required=True)
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--docker-context', default='default')
parser.add_argument('--r1-source', type=Path)
parser.add_argument('--windows-cli-receipt', type=Path)
parser.add_argument('--test-python', type=Path)
args = parser.parse_args()
TOOLING = args.base_tooling.resolve(strict=True)
sys.path.insert(0, str(TOOLING))
sys.path.insert(0, str(TOOLING / 'scripts'))
from compose_helpers import ComposeHelper

scripts = Path(__file__).resolve().parent
root = args.output.resolve()
root.mkdir(parents=True, exist_ok=True)
assembled = json.loads(args.runtime_receipt.read_text(encoding='utf-8'))
image = assembled['image']
docker = ['docker', '--context', args.docker_context]
projects = ['sdlc-qa-bridge-target-' + uuid.uuid4().hex[:12],
    'sdlc-qa-bridge-control-' + uuid.uuid4().hex[:12]]
subnets = [ipaddress.ip_network('10.253.43.0/28'), ipaddress.ip_network('10.253.44.0/28')]
ids = subprocess.check_output(docker + ['network','ls','-q'], text=True).split()
for network in json.loads(subprocess.check_output(docker + ['network','inspect',*ids])):
    for entry in network['IPAM'].get('Config') or []:
        existing = ipaddress.ip_network(entry['Subnet'])
        assert not any(existing.version == subnet.version and existing.overlaps(subnet) for subnet in subnets)
output = root / projects[1]
output.mkdir(parents=True)
data = {name: str(uuid.uuid4()) for name in ['workspace','target','repository','environment','profile',
    'project','registry','namespace','tracker','tracker_project','task','actor','other_actor',
    'session','other_session','group','marker','forge_instance','binding_operation']}
data.update(target_project=projects[0],candidate=assembled['candidate'],auth_secret='synthetic-'+uuid.uuid4().hex)
selected_windows = (args.r1_source, args.windows_cli_receipt, args.test_python)
windows_helper = all(value is not None for value in selected_windows)
if any(value is not None for value in selected_windows) and not windows_helper:
    parser.error('Select R1 source, qualified Windows CLI receipt and tools Python together')
if windows_helper:
    data.update(windows_helper=True, task_key='R1-7',
                repository_url='https://pdlc.acm-ai.ru/git/r1/r1.git')
    args.r1_source = args.r1_source.resolve(strict=True)
    args.test_python = args.test_python.resolve(strict=True)
    cli = args.windows_cli_receipt.resolve(strict=True).with_name('cicd-cli.exe')
    producer = json.loads(args.windows_cli_receipt.read_text(encoding='utf-8'))
    assert hashlib.sha256(cli.read_bytes()).hexdigest() == producer['sha256']
    data.update(cli_executable=str(cli), cli_sha256=hashlib.sha256(cli.read_bytes()).hexdigest())
(output/'identities.json').write_text(json.dumps(data))
(output/'tracker-token').write_text('synthetic-tracker-reader')
script = {'type':'bind','source':str(scripts/'services.py'),
    'target':'/_qa/services.py','read_only':True}
inputs = {'type':'bind','source':str(output),'target':'/fixture-input','read_only':True}

def service(memory, cpu, **values):
    return {'image':image,'mem_limit':memory,'memswap_limit':memory,'cpus':cpu,
        'pids_limit':128,'networks':['selected'],**values}


def loopback_port(container_port):
    # Keep the published endpoint stable through Compose stop/start. Docker's
    # anonymous published ports are reassigned when the container starts again.
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        port = listener.getsockname()[1]
    return f'127.0.0.1:{port}:{container_port}'

with ComposeHelper(project=projects[0],task='development-workspace-local-qualification',
        purpose='retained-runtime-for-workspace-bridge',docker=docker,
        directory=root/'compose'/projects[0]) as target:
    app = service('128m',.25,entrypoint=['python3'],command=['/_qa/services.py','application'],
        volumes=[script,inputs,{'type':'volume','source':'data','target':'/data'}],
        healthcheck={'test':['CMD','python3','-c',"import urllib.request; urllib.request.urlopen('http://127.0.0.1:8080').read()"],
            'interval':'2s','timeout':'2s','retries':20})
    init = service('128m',.25,user='0:0',entrypoint=['python3'],command=['-c','import os; os.chown("/data",10001,10001)'],
        volumes=[{'type':'volume','source':'data','target':'/data'}])
    if windows_helper:
        app['ports'] = [loopback_port(8080)]
    target.write({'application':app,'init':init},volumes={'data':{}},
        networks={'selected':{'ipam':{'config':[{'subnet':str(subnets[0])}]}}})
    subprocess.run(target.run('init'),check=True)
    subprocess.run(target.command+['up','-d','--wait','application'],check=True)
    if windows_helper:
        address = subprocess.check_output(target.command+['port','application','8080'], text=True).strip()
        assert address.startswith('127.0.0.1:')
        data['application_url'] = 'http://' + address
    with ComposeHelper(project=projects[1],task='development-workspace-local-qualification',
            purpose='integrated-forge-workspace-bridge',docker=docker,
            directory=root/'compose'/projects[1]) as control:
        pg = service('512m',.5,image=args.postgres_image,
            environment={'POSTGRES_USER':'forge_owner','POSTGRES_PASSWORD':'synthetic_fixture',
                'POSTGRES_DB':'forge_test_bridge'},
            volumes=[{'type':'volume','source':'database','target':'/var/lib/postgresql/data'}],
            healthcheck={'test':['CMD-SHELL','pg_isready -U forge_owner -d forge_test_bridge'],
                'interval':'2s','timeout':'2s','retries':30})
        shared={'type':'volume','source':'controller','target':'/controller'}
        socket={'type':'bind','source':'/var/run/docker.sock','target':'/var/run/docker.sock','read_only':True}
        target_file={'type':'bind','source':str(target.directory),'target':'/target-fixture','read_only':True}
        controller=service('768m',1,user='0:0',environment={
            'CICD_DATABASE_URL':'postgres://forge_owner:synthetic_fixture@db:5432/forge_test_bridge',
            'CICD_BIND':'0.0.0.0:22801','CICD_ROLES':'api,worker','CICD_AUTH_SECRET':data['auth_secret'],
            'CICD_EMBEDDED_RUNNER_ENABLED':'false','CICD_GIT_ROOT':'/controller/git',
            'CICD_ARTIFACTS_DIR':'/controller/artifacts',
            'CICD_DEVELOPMENT_WORKSPACE_CATALOG_ROOT':'/controller/catalog',
            'CICD_DEVELOPMENT_WORKSPACE_OPERATOR':'/opt/pdlc-workspace/bin/pdlc-dev-workspace-operator',
            'CICD_NAMESPACE__INSTANCE_ID':data['forge_instance'],
            'CICD_NAMESPACE__REGISTRY_INSTANCE_ID':data['registry'],
            'CICD_NAMESPACE__TRACKER_INSTANCE_ID':data['tracker'],
            'CICD_NAMESPACE__TRACKER_URL':'http://tracker:8080',
            'CICD_NAMESPACE__TRACKER_TOKEN_FILE':'/fixture-input/tracker-token'},
            volumes=[shared,socket,target_file,inputs],
            healthcheck={'test':['CMD','python3','-c',"import urllib.request; urllib.request.urlopen('http://127.0.0.1:22801/api/v1/readiness').read()"],
                'interval':'2s','timeout':'2s','retries':40})
        tracker=service('128m',.25,entrypoint=['python3'],command=['/_qa/services.py','tracker'],
            volumes=[script,inputs])
        if windows_helper:
            controller['ports'] = [loopback_port(22801)]
        probe=service('512m',.5,user='0:0',entrypoint=['/opt/pdlc-workspace/bin/python'],
            command=['-B','/probe/probe.py'],networks=['selected','target'],
            volumes=[shared,socket,target_file,inputs,
                {'type':'bind','source':str(scripts/'probe.py'),'target':'/probe/probe.py','read_only':True},
                {'type':'bind','source':str(output),'target':'/evidence'}])
        control.write({'db':pg,'controller':controller,'tracker':tracker,'probe':probe},
            volumes={'database':{},'controller':{}},
            networks={'selected':{'ipam':{'config':[{'subnet':str(subnets[1])}]}},
                'target':{'external':True,'name':projects[0]+'_selected'}},
            daemon_bind_sources=('/var/run/docker.sock',))
        subprocess.run(control.command+['up','-d','--wait','db'],check=True)
        subprocess.run(control.command+['exec','-T','db','psql','-U','forge_owner','-d','forge_test_bridge',
            '-v','ON_ERROR_STOP=1','-c','CREATE ROLE forge_runtime NOLOGIN;'],check=True)
        data['database_container']=subprocess.check_output(control.command+['ps','-q','db'],text=True).strip()
        (output/'identities.json').write_text(json.dumps(data))
        try:
            subprocess.run(control.command+['up','-d','--wait','tracker','controller'],check=True)
            subprocess.run(control.command+['exec','-T','controller','sha256sum','-c',
                                           '/opt/forge-runtime/SHA256SUMS'],check=True,stdout=subprocess.DEVNULL)
            data['controller_container']=subprocess.check_output(control.command+['ps','-q','controller'],text=True).strip()
            if windows_helper:
                address = subprocess.check_output(control.command+['port','controller','22801'], text=True).strip()
                assert address.startswith('127.0.0.1:')
                data['api_url'] = 'http://' + address
            (output/'identities.json').write_text(json.dumps(data))
            subprocess.run(control.run('probe'),check=True)
            if windows_helper:
                env = {**os.environ, 'R1_WORKSPACE_INTEGRATION_FIXTURE': str(output/'windows-helper-fixture.json')}
                subprocess.run([str(args.test_python), '-B', '-m', 'unittest', 'discover', '-s', 'scripts/pdlc',
                                '-p', 'test_workspace_integration.py', '-v'],
                               cwd=args.r1_source, env=env, check=True, timeout=240)
                baseline = json.loads((output/'runtime-baseline.json').read_text())
                actual = json.loads(subprocess.check_output(docker+['inspect',baseline['containerId']]))[0]
                assert actual['Id'] == baseline['containerId'] and actual['State']['Running'] is True
                assert actual['Image'] == baseline['imageId'] and actual['Mounts'] == baseline['mounts']
                result = json.loads((output/'result.json').read_text())
                result.update(windowsHelperRealCli=True, lostReplyReadbackAndReplay=True,
                              r1_source=subprocess.check_output(['git','rev-parse','HEAD'],cwd=args.r1_source,text=True).strip())
                (output/'result.json').write_text(json.dumps(result,indent=2)+'\n')
        finally:
            with (output/'controller.log').open('wb') as log:
                subprocess.run(control.command+['logs','--no-color','controller'],stdout=log,stderr=subprocess.STDOUT,check=True)
        print((output/'result.json').read_text())
