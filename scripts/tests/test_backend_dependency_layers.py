"""Product Docker source graph controls; native qualification remains separate."""
import json
import re
import shlex
from pathlib import Path
import tomllib
import unittest

ROOT=Path(__file__).resolve().parents[2]
PRODUCT='CI-CD'
RECIPES=(ROOT/'backend/Dockerfile',ROOT/'backend/Dockerfile.umbrella')
UNIT_COMMAND='cargo test --locked --release -p cicd-domain --lib'
SDK=('sdlc-shared', 'sdlc-auth-core', 'sdlc-cli-core', 'sdlc-telemetry')

def graph(text):
    result={}
    for stage in re.split(r'(?m)^FROM ',text)[1:]:
        first,body=stage.split('\n',1);fields=first.split()
        name=fields[-1] if len(fields)==3 and fields[1].upper()=='AS' else 'runtime'
        result[name]=({fields[0]}|set(re.findall(r'(?:--from=|\bfrom=)([a-zA-Z0-9_-]+)',body)),body.replace('\\\n',' '))
    return result

def packages():
    root=tomllib.loads((ROOT/'backend/Cargo.toml').read_text(encoding='utf-8'))
    return {member:tomllib.loads((ROOT/'backend'/member/'Cargo.toml').read_text(encoding='utf-8'))
            for member in root['workspace']['members']}

class BackendDependencyLayerTests(unittest.TestCase):
    def test_unit_graph_is_real_and_does_not_inherit_server_preparation(self):
        for recipe in RECIPES:
            with self.subTest(recipe=recipe.name):
                stages=graph(recipe.read_text(encoding='utf-8'))
                self.assertTrue({'manifests','unit-dependencies','dependencies','unit'}<=stages.keys())
                seen,pending=set(),['unit']
                while pending:
                    name=pending.pop()
                    if name in seen or name not in stages:continue
                    seen.add(name);pending.extend(stages[name][0])
                self.assertNotIn('dependencies',seen)
                self.assertFalse(any('cargo build' in stages[name][1] for name in seen))
                unit=stages['unit'][1]
                self.assertIn(UNIT_COMMAND,unit);self.assertNotIn('--no-run',unit)
                self.assertIn("printf 'passed",unit)
                self.assertIn(UNIT_COMMAND+' --no-run',stages['unit-dependencies'][1])
                self.assertNotIn('rust-unit-check',stages['unit-dependencies'][1])
                full=next(body for name,(_,body) in stages.items() if name in ('build','builder'))
                self.assertIn('from=unit,source=/tmp/rust-unit-check',full)
                self.assertIn('COPY CI-CD/backend/domain ./domain',unit); self.assertNotIn('cache-protocol',unit)

    def test_stable_product_metadata_and_cleaned_stubs_cover_real_packages(self):
        expected={value['package']['name'] for value in packages().values()}
        for recipe in RECIPES:
            with self.subTest(recipe=recipe.name):
                stages=graph(recipe.read_text(encoding='utf-8'))
                self.assertIn('manifests',stages)
                for name in ('manifests','unit-dependencies','dependencies'):
                    for line in stages[name][1].splitlines():
                        if line.startswith('COPY ') and '--from=' not in line:
                            for source in line.split()[1:-1]:
                                if source.startswith(('backend/',PRODUCT+'/backend/')):
                                    self.assertTrue(source.endswith(('Cargo.toml','Cargo.lock*')),line)
                for name in ('unit-dependencies','dependencies'):
                    body=stages[name][1]
                    self.assertIn('cargo clean --release',body)
                    self.assertEqual(set(re.findall(r'-p ([a-zA-Z0-9_-]+)',body.split('cargo clean',1)[-1])),expected)

    def test_mutable_downloads_workers_and_original_sdk_contract(self):
        for recipe in RECIPES:
            with self.subTest(recipe=recipe.name):
                text=recipe.read_text(encoding='utf-8').replace('\\\n',' ')
                self.assertIn('ARG CARGO_BUILD_JOBS=2',text)
                self.assertIn('CARGO_REGISTRY_CACHE_ID=pdlc-operator-downloads-v1-cargo188',text)
                self.assertIn('id=${CARGO_REGISTRY_CACHE_ID}-${TARGETARCH}',text)
                self.assertNotIn('CARGO_TARGET_DIR=',text)
                for value in re.findall(r'--mount=type=cache,([^\s]+)',text):
                    target=re.search(r'(?:^|,)target=([^,]+)',value)[1]
                    self.assertIn(target,('/usr/local/cargo/registry','/.__pdlc_build_slot__'))
                for line in text.splitlines():
                    if line.startswith('RUN ') and 'cargo ' in line:self.assertIn('target=/.__pdlc_build_slot__',line)
                for crate in SDK:self.assertIn('COPY services-base/crates/'+crate+' /services-base/crates/'+crate,text)

    def test_context_policy_preserves_required_roots_and_live_private_exclusions(self):
        for recipe in RECIPES:
            with self.subTest(recipe=recipe.name):
                path=recipe.with_name(recipe.name+'.dockerignore');self.assertTrue(path.is_file())
                rules=[line for line in path.read_text(encoding='utf-8').splitlines() if line and not line.startswith('#')]
                self.assertEqual(rules[0],'**')
                positives=[line for line in rules if line.startswith('!')]
                self.assertEqual(len(positives),len(set(positives)))
                self.assertIn('!CI-CD/backend/**',positives)
                for parent,barrier,child in (('!CI-CD/','CI-CD/*','!CI-CD/backend/'),('!services-base/','services-base/*','!services-base/Cargo.toml'),('!services-base/crates/','services-base/crates/*','!services-base/crates/'+SDK[0]+'/')):
                    self.assertLess(rules.index(parent),rules.index(barrier));self.assertLess(rules.index(barrier),rules.index(child))
                for crate in SDK:self.assertIn('!services-base/crates/'+crate+'/**',positives)
                if not (PRODUCT=='fleet-control' and recipe.name=='Dockerfile'):
                    expected={'!'+PRODUCT+'/','!'+PRODUCT+'/backend/','!'+PRODUCT+'/backend/**',
                              '!services-base/','!services-base/Cargo.toml','!services-base/crates/'}
                    for crate in SDK:
                        expected.update(('!services-base/crates/'+crate+'/','!services-base/crates/'+crate+'/**'))
                    self.assertEqual(set(positives),expected)
                    for parent,barrier,child in (('!services-base/','services-base/*','!services-base/Cargo.toml'),
                                                 ('!services-base/crates/','services-base/crates/*','!services-base/crates/'+SDK[0]+'/')):
                        self.assertLess(rules.index(parent),rules.index(barrier))
                        self.assertLess(rules.index(barrier),rules.index(child))
                    for crate in SDK:
                        self.assertGreater(rules.index('!services-base/crates/'+crate+'/'),rules.index('services-base/crates/*'))
                last=max(rules.index(line) for line in positives)
                for private in ('**/.git','**/.local','**/.env','**/.env.*','**/target','**/node_modules','**/coverage','**/test-results'):
                    self.assertGreater(rules.index(private),last)
                policy=ROOT.parent/'.dockerignore'
                if (ROOT.parent/'source-workspace.json').is_file() and policy.is_file():
                    tail=policy.read_text(encoding='utf-8').split('**/.git',1)[1]
                    for private in tail.splitlines():
                        if private and not private.startswith('#'):self.assertGreater(rules.index(private),last)

    def test_all_actual_runtime_package_consumers_have_real_sources(self):
        values=packages();by_name={value['package']['name']:member for member,value in values.items()}
        seen,pending=set(),['server' if PRODUCT=='fleet-control' else 'cicd-server']
        while pending:
            name=pending.pop()
            if name in seen or name not in by_name:continue
            seen.add(name);pending.extend(values[by_name[name]].get('dependencies',{}))
        for recipe in RECIPES:
            stages=graph(recipe.read_text(encoding='utf-8'))
            self.assertIn('builder' if PRODUCT=='fleet-control' else 'build',stages)
            full=stages['builder' if PRODUCT=='fleet-control' else 'build'][1]
            prefix='backend/' if PRODUCT=='fleet-control' and recipe.name=='Dockerfile' else PRODUCT+'/backend/'
            for name in seen:
                member=by_name[name]
                source, destination = ((prefix+'src', './src') if member=='.'
                                       else (prefix+member, './'+member))
                copies = [shlex.split(line) for line in full.splitlines() if line.startswith('COPY ')]
                self.assertTrue(any(row[-2:] == [source, destination] for row in copies), name)

    def test_actual_delivery_targets_and_guard_have_one_owner(self):
        for recipe in RECIPES:
            stages=graph(recipe.read_text(encoding='utf-8'));self.assertIn('build',stages)
            full=stages['build'][1]
            self.assertIn('cargo build --locked --release -p cicd-server --bin cicd-server --bin forge-runner',full)
            self.assertNotIn('-p cicd-cli',full);self.assertNotIn('--bin openapi-dump',full)
            for binary in ('cicd-server','forge-runner'):self.assertIn('/app/target/release/'+binary,stages['runtime'][1])
            for token in ('ARG DOCKER_CLI_VERSION=29.5.0','git-lfs','USER cicd','/var/lib/forge/git','/workspaces'):
                self.assertIn(token,stages['runtime'][1])
        self.assertIn('cargo test --locked -p cicd-cli --features integration',(ROOT/'.github/workflows/release.yml').read_text(encoding='utf-8'))
        manifest=json.loads((ROOT/'.quality-gate.json').read_bytes())
        module='scripts.tests.test_backend_dependency_layers'
        for profile in ('fast','unit','build','release'):
            owners=[gate for gate in manifest['gates'] if profile in gate['profiles'] and module in gate['argv']]
            self.assertEqual(len(owners),1);self.assertEqual(owners[0]['execution_slot'],'none')
        self.assertIn(module,(ROOT/'.github/workflows/ci.yml').read_text(encoding='utf-8'))
