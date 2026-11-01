"""Real HTTP/CLI -> PostgreSQL worker -> Base operator -> Docker fixture."""
import base64
import hashlib
import hmac
import json
import os
import subprocess
import time
import uuid
from pathlib import Path
from urllib.error import HTTPError
from urllib.request import Request, urlopen

from pdlc_operator.development_workspace.inventory import inventory
from pdlc_operator.development_workspace.profile import profile_hash

data = json.loads(Path('/fixture-input/identities.json').read_text())
root = Path('/controller/catalog')
target_root = root / 'targets' / data['target']
target_root.mkdir(parents=True, mode=0o700)
profile = {'apiVersion': 'pdlc.dev/workspace/v1', 'name': 'bridge-fixture',
    'repositories': [{'key': 'app', 'path': 'sources/app', 'sourceSelection': 'explicit-commit'}],
    'tools': [{'name': 'git', 'location': 'local', 'version': '>=2.0', 'lockfiles': []}],
    'services': [{'key': 'application', 'dependsOn': []}],
    'storage': [{'key': 'data', 'retainOnStop': True}],
    'checks': [{'key': 'unit', 'kind': 'backend', 'executor': 'forge',
        'commands': ['python -m unittest'], 'requiresRuntime': False}]}
target = inventory({'id': data['target'], 'adapter': 'compose', 'dockerContext': 'default',
    'project': data['target_project'], 'composeFile': '/target-fixture/compose.json'}, profile)
environment = {'adapter': 'compose', 'environmentId': data['environment'], 'projectId': data['project'],
    'url': 'http://application:8080'}
canonical = lambda value: json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()
public = {'id': data['target'], 'repository_id': data['repository'], 'configuration_id': data['project'],
    'profile_id': data['profile'], 'environment_id': data['environment'],
    'environment_hash': hashlib.sha256(canonical(environment)).hexdigest(),
    'name': 'Disposable bridge fixture', 'kind': 'existing', 'public_origin': None,
    'profile_hash': profile_hash(profile), 'profile': profile,
    'repositories': [{'key': 'app', 'repository_id': data['repository'],
        'clone_url': 'https://fixture.invalid/app.git'}]}
for name, value in [('public.json', public), ('profile.json', profile), ('runtime.json', target)]:
    (target_root / name).write_bytes(canonical(value))

def sql(statement):
    return subprocess.check_output(['docker','exec','-i',data['database_container'],'psql',
        '-X','-v','ON_ERROR_STOP=1','-U','forge_owner','-d','forge_test_bridge','-tAc',statement], text=True).strip()

binding = canonical({'schema_version':1,'namespace':{'registry_instance_id':data['registry'],
    'namespace_id':data['namespace']},'resource':{'kind':'git_group',
    'instance_id':data['forge_instance'],'resource_id':data['group']},
    'operation_id':data['binding_operation'],'generation':1,'state':'active','create_spec':None}).decode()
sql(f"""
INSERT INTO users(id,username,role) VALUES ('{data['actor']}','fixture-owner','admin'),
 ('{data['other_actor']}','fixture-second','admin');
INSERT INTO sessions(id,user_id,refresh_token_hash,expires_at,family_id) VALUES
 ('{data['session']}','{data['actor']}','fixture-session-one',now()+interval '1 hour','{data['session']}'),
 ('{data['other_session']}','{data['other_actor']}','fixture-session-two',now()+interval '1 hour','{data['other_session']}');
INSERT INTO repositories(id,name,visibility) VALUES ('{data['repository']}','bridge-source','private');
INSERT INTO git_groups(id,slug,name) VALUES ('{data['group']}','bridge-fixture','Bridge fixture');
UPDATE repository_catalog SET group_id='{data['group']}' WHERE id='{data['repository']}';
INSERT INTO forge_namespace_bindings(resource_id,registry_instance_id,namespace_id,generation,state,command)
 VALUES ('{data['group']}','{data['registry']}','{data['namespace']}',1,'active','{binding}');
INSERT INTO forge_workspace_projects(registry_instance_id,namespace_id,tracker_instance_id,tracker_project_id,
 generation,name,project_key,state,observed_at) VALUES ('{data['registry']}','{data['namespace']}',
 '{data['tracker']}','{data['tracker_project']}',1,'Fixture','FIXTURE','active',now());
INSERT INTO projects(id,name,repository_url,repository_id) VALUES ('{data['project']}','Bridge fixture',
 '{data.get('repository_url', 'https://fixture.invalid/app.git')}','{data['repository']}');
INSERT INTO environments(id,project_id,name,url) VALUES ('{data['environment']}','{data['project']}',
 'Disposable Compose','http://application:8080');
""")

def token(actor, session):
    encode = lambda value: base64.urlsafe_b64encode(canonical(value)).rstrip(b'=')
    value = encode({'alg':'HS256','typ':'JWT'}) + b'.' + encode({'sub':actor,'sid':session,
        'role':'admin','ver':0,'iat':int(time.time()),'exp':int(time.time())+1800})
    return (value + b'.' + base64.urlsafe_b64encode(hmac.new(data['auth_secret'].encode(),
        value, hashlib.sha256).digest()).rstrip(b'=')).decode()

owner = token(data['actor'],data['session'])
second = token(data['other_actor'],data['other_session'])
base = 'http://controller:22801'
path = '/api/v1/development-workspaces/' + data['workspace']

def request(method, route, value=None, auth=owner, expected=200):
    native = Request(base + route, data=None if value is None else canonical(value), method=method,
        headers={'Authorization':'Bearer '+auth, 'Content-Type':'application/json'})
    try:
        response = urlopen(native, timeout=15)
    except HTTPError as error:
        response = error
    body = json.load(response)
    assert response.status == expected, (method,route,response.status,body)
    return body

def cli(*args):
    env = {**os.environ, 'CICD_API_TOKEN': owner, 'CICD_CONFIG': '/nonexistent-fixture-profile.toml'}
    # The SDK allows plain HTTP only on loopback. Run the actual packaged CLI
    # inside its controller; Docker reads the synthetic token from its private
    # process environment, never from a command-line value.
    command = ['docker','exec','--env','CICD_API_TOKEN','--env','CICD_CONFIG',
        data['controller_container'],'/usr/local/bin/cicd-cli','--api-url',
        'http://127.0.0.1:22801','--output','json','dev-workspace',*args]
    result = subprocess.run(command, env=env, capture_output=True, text=True, timeout=30)
    assert result.returncode == 0, (args,result.stderr)
    return json.loads(result.stdout)

def wait(operation):
    for _ in range(40):
        result = request('GET',path+'/operations/'+operation)
        if result['state'] in ['succeeded','failed','unknown']:
            assert result['state'] == 'succeeded', result
            return result
        time.sleep(.5)
    raise AssertionError('Worker did not finish original operation '+operation)

registration = str(uuid.uuid4())
cli('register','--project',data['project'],'--workspace',data['workspace'],'--operation-id',registration,
    '--target',data['target'],'--name','Bridge fixture')
registered = wait(registration)
assert request('GET',path)['workspace']['runtime'] == 'running'
assert len(cli('list','--project',data['project'])['items']) == 1
assert cli('profiles','--project',data['project'])['items'][0]['id'] == data['target']

def operation(action, **parameters):
    version = request('GET',path)['workspace']['version']
    identity = str(uuid.uuid4())
    body = {'operation_id':identity,'expected_version':version,'command':{'action':action,**parameters}}
    request('POST',path+'/operations',body)
    return identity, body, wait(identity)

begin, begin_body, _ = operation('begin_session',task=data.get('task_key', 'FIXTURE-7'))
version = request('GET',path)['workspace']['version']
for auth, action in [(second, {'action':'begin_session','task':data.get('task_key', 'FIXTURE-7')}),
                     (owner, {'action':'stop'})]:
    request('POST',path+'/operations',{'operation_id':str(uuid.uuid4()),
        'expected_version':version,'command':action},auth=auth,expected=409)
operation('end_session',session_id=begin)
stop, stop_body, stopped = operation('stop')
assert request('GET',path)['workspace']['runtime'] == 'stopped'
assert request('POST',path+'/operations',stop_body) == stopped
changed = {**stop_body,'command':{'action':'start'}}
request('POST',path+'/operations',changed,expected=409)
version = request('GET',path)['workspace']['version']
start = str(uuid.uuid4())
cli('start','--workspace',data['workspace'],'--operation-id',start,'--expected-version',str(version))
wait(start)
assert request('GET',path)['workspace']['runtime'] == 'running'
with urlopen('http://application:8080',timeout=10) as response:
    assert json.load(response)['marker'] == data['marker']
assert inventory({'id':data['target'],'adapter':'compose','dockerContext':'default',
    'project':data['target_project'],'composeFile':'/target-fixture/compose.json'},profile) == target
termination = json.loads(subprocess.check_output(['docker','inspect',target['services']['application']['containerId']]))[0]
assert termination['State']['Running'] is True
result = {'candidate':data['candidate'],'registration':registration,'session':begin,'stop':stop,'start':start,
    'realHttpCliWorkerOperator':True,'oneWriterAndBusyStop':True,'originalKeyReplay':True,
    'changedIntentDenied':True,'dataAndContainerIdentityPreserved':True,'fixtureOnly':True,
    'productionAcceptance':False}
Path('/evidence/result.json').write_text(json.dumps(result,indent=2)+'\n')
if data.get('windows_helper'):
    host = {'fixtureOnly': True, 'apiUrl': data['api_url'], 'applicationUrl': data['application_url'],
            'workspaceId': data['workspace'], 'projectId': data['project'],
            'repositoryId': data['repository'], 'environmentId': data['environment'],
            'ownerToken': owner, 'secondToken': second, 'marker': data['marker'],
            'cliExecutable': data['cli_executable'], 'cliSha256': data['cli_sha256']}
    Path('/evidence/windows-helper-fixture.json').write_text(json.dumps(host))
print(json.dumps(result))
