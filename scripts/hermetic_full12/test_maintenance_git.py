"""Private Git admission tests using tiny public synthetic blobs only."""
import ast
from contextlib import contextmanager
import copy
import hashlib
import io
import json
import os
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

import host as h
import maintenance_git as m
import run as gate


@contextmanager
def fixture(qualification='published_exact_commit'):
    # These are public test strings, not implementations or encoded private source.
    payloads = {name: ('public-unit-fixture:' + name + '\r\n').encode() for name in m.EXPECTED_FILES}
    files = {name: {'blob': hashlib.sha1(b'blob ' + str(len(raw)).encode() + b'\0' + raw).hexdigest(),
                    'sha256': hashlib.sha256(raw).hexdigest()} for name, raw in payloads.items()}
    pin = {'schema': 'forge/private-maintenance-pin/v2', 'repository': m.REPOSITORY,
           'commit': 'a' * 40, 'ref': 'refs/heads/maintenance-reviewed',
           'qualification': qualification, 'files': files}
    def git(_, *args):
        if args == ('remote', 'get-url', 'origin'):
            return ('https://github.com/' + m.REPOSITORY + '.git\n').encode()
        if args == ('rev-parse', '--verify', 'a' * 40 + '^{commit}'):
            return ('a' * 40 + '\n').encode()
        if args == ('rev-parse', '--is-shallow-repository'):
            return b'false\n'
        if args == ('rev-parse', '--verify', 'refs/remotes/origin/maintenance-reviewed^{commit}'):
            return ('a' * 40 + '\n').encode()
        if args == ('ls-tree', '-z', 'a' * 40, '--', *sorted(files)):
            return b''.join(('100644 blob ' + files[name]['blob'] + '\t' + name + '\0').encode() for name in sorted(files))
        if args[:2] == ('cat-file', 'blob'):
            return next(payloads[name] for name in files if files[name]['blob'] == args[2])
        raise AssertionError('unexpected synthetic Git invocation')
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory) / 'pin.json'
        path.write_text(json.dumps(pin), encoding='ascii')
        with (patch.object(m, 'PIN', path), patch.object(m, 'EXPECTED_FILES', copy.deepcopy(files)),
              patch.object(m, 'CANDIDATE_COMMIT', pin['commit']), patch.object(m, 'CANDIDATE_REF', pin['ref'])):
            yield Path(directory), pin, payloads, git


class MaintenanceGitTests(unittest.TestCase):
    @staticmethod
    def candidate_git(original):
        def git(root, *args):
            if args == ('check-ref-format', 'refs/heads/maintenance-reviewed'):
                return b''
            if args == ('ls-remote', '--exit-code', 'origin', 'refs/heads/maintenance-reviewed'):
                return b'a' * 40 + b'\trefs/heads/maintenance-reviewed\n'
            return original(root, *args)
        return git

    def test_candidate_metadata_without_activating_or_materializing(self):
        with fixture() as (root, pin, _, git):
            m.PIN.write_text('{"commit": null}', encoding='ascii')
            before = m.PIN.read_bytes()
            call = Mock(side_effect=self.candidate_git(git))
            candidate = m.qualify(root, 'a' * 40, 'refs/heads/maintenance-reviewed', call)
            self.assertEqual(candidate, pin)
            self.assertEqual(m.PIN.read_bytes(), before)
            self.assertEqual(list(root.iterdir()), [m.PIN])
            reads = [c.args[1:] for c in call.call_args_list]
            self.assertEqual(reads.count(('ls-remote', '--exit-code', 'origin',
                                          'refs/heads/maintenance-reviewed')), 2)
            self.assertEqual(reads[-1], reads[2])
            with self.assertRaises(m.QualificationFailure):
                m.preflight()

    def test_candidate_invalid_selectors_never_call_git(self):
        for commit, ref in [('main', 'refs/heads/maintenance-reviewed'),
                            ('A' * 40, 'refs/heads/maintenance-reviewed'),
                            ('a' * 40, 'refs/tags/maintenance-reviewed'),
                            ('a' * 40, 'refs/heads/*'), ('a' * 40, '--upload-pack=evil'),
                            ('a' * 40, 'refs/heads/secret\nother'),
                            ('a' * 40, 'refs/heads/' + 'a' * 257)]:
            with self.subTest(commit=commit, ref=ref):
                git = Mock()
                with self.assertRaises(m.QualificationFailure):
                    m.qualify(Path('.'), commit, ref, git)
                git.assert_not_called()

    def reject_candidate(self, change):
        with fixture() as (root, _, _, original):
            git = self.candidate_git(original)
            call = Mock(side_effect=lambda checkout, *args: change(args, lambda: git(checkout, *args)))
            with self.assertRaises(m.QualificationFailure) as error:
                m.qualify(root, 'a' * 40, 'refs/heads/maintenance-reviewed', call)
            self.assertEqual(str(error.exception), 'maintenance_pin_unqualified')
            self.assertEqual(list(root.iterdir()), [m.PIN])
            return [c.args[1:] for c in call.call_args_list]

    def test_candidate_foreign_origin_before_network(self):
        reads = self.reject_candidate(lambda a, normal: b'https://evil.invalid/private' if a[0] == 'remote' else normal())
        self.assertFalse(any(a[0] == 'ls-remote' for a in reads))

    def test_candidate_malformed_git_ref_refused(self):
        def changed(args, normal):
            if args[0] == 'check-ref-format':
                raise RuntimeError('private Git error')
            return normal()
        self.assertEqual(len(self.reject_candidate(changed)), 1)

    def test_candidate_unpublished_changed_or_ambiguous_tip_refused(self):
        for raw in (b'', b'b' * 40 + b'\trefs/heads/maintenance-reviewed\n',
                    b'a' * 40 + b'\trefs/heads/other\n',
                    (b'a' * 40 + b'\trefs/heads/maintenance-reviewed\n') * 2):
            with self.subTest(raw=raw):
                reads = self.reject_candidate(lambda a, normal: raw if a[0] == 'ls-remote' else normal())
                self.assertFalse(any(a[0] == 'cat-file' for a in reads))

    def test_candidate_moved_during_readback_refused(self):
        calls = 0
        def changed(args, normal):
            nonlocal calls
            if args[0] == 'ls-remote':
                calls += 1
                if calls == 2:
                    return b'b' * 40 + b'\trefs/heads/maintenance-reviewed\n'
            return normal()
        self.reject_candidate(changed)
        self.assertEqual(calls, 2)

    def test_candidate_preserves_tree_history_and_raw_hash_guards(self):
        for command, replacement in (
                ('ls-tree', lambda b: b.replace(b'100644', b'120000')),
                ('ls-tree', lambda b: b.split(b'\0', 1)[1]),
                ('cat-file', lambda b: b.replace(b'\r\n', b'\n')),
                ('rev-parse', lambda b: b'false' if b.strip() == b'a' * 40 else b'true')):
            with self.subTest(command=command):
                self.reject_candidate(lambda a, normal: replacement(normal()) if a[0] == command else normal())

    def test_candidate_cli_outputs_only_metadata_and_no_writes(self):
        with fixture() as (root, pin, _, git), patch.object(m, 'qualification_git',
                return_value=self.candidate_git(git)), patch('sys.stdout', new_callable=io.StringIO) as output:
            before = m.PIN.read_bytes()
            with patch('sys.argv', ['maintenance_git.py', 'qualify', str(root), 'a' * 40,
                                    'refs/heads/maintenance-reviewed']):
                self.assertEqual(m.main(), 0)
            self.assertEqual(json.loads(output.getvalue()), pin)
            self.assertNotIn('public-unit-fixture', output.getvalue())
            self.assertEqual(m.PIN.read_bytes(), before)

    def test_candidate_cli_failure_never_prints_private_error(self):
        with (patch.object(m, 'qualification_git', return_value=Mock(side_effect=RuntimeError(
                'TOKEN SQL PRIVATE PATH'))), patch('sys.stdout', new_callable=io.StringIO) as output,
                patch('sys.argv', ['maintenance_git.py', 'qualify', '.', m.CANDIDATE_COMMIT,
                                   m.CANDIDATE_REF])):
            self.assertEqual(m.main(), 1)
        self.assertEqual(output.getvalue(), 'MAINTENANCE_PIN_UNQUALIFIED\n')

    def test_candidate_git_shared_absolute_deadline_and_no_replace(self):
        with patch.object(m.time, 'monotonic', side_effect=[10, 11, 12, 13]), patch.object(
                h, 'command', return_value=b'metadata') as run:
            git = m.qualification_git(20)
            self.assertEqual(git(Path('checkout'), 'check-ref-format', 'refs/heads/reviewed'), b'metadata')
            git(Path('checkout'), 'remote', 'get-url', 'origin')
        self.assertEqual([c.kwargs['timeout'] for c in run.call_args_list], [10, 8])
        self.assertEqual(run.call_args.args[0][:3], ['git', '--no-replace-objects', '--no-optional-locks'])

    def test_candidate_git_expired_budget_no_subprocess(self):
        with patch.object(m.time, 'monotonic', return_value=20), patch.object(h, 'command') as run:
            with self.assertRaises(m.QualificationFailure):
                m.qualification_git(20)(Path('.'), 'remote', 'get-url', 'origin')
        run.assert_not_called()

    def test_candidate_git_late_nonzero_or_oversize_refused(self):
        for raw, finish in ((b'ok', 20), (RuntimeError('PRIVATE'), 11), (b'x' * (2**20 + 1), 11)):
            with self.subTest(finish=finish), patch.object(
                    m.time, 'monotonic', side_effect=[10, finish]), patch.object(h, 'command',
                    side_effect=raw if isinstance(raw, Exception) else None, return_value=raw):
                with self.assertRaises(m.QualificationFailure):
                    m.qualification_git(20)(Path('.'), 'remote', 'get-url', 'origin')

    def test_current_pin_exact_reviewed_safety_successor(self):
        pin = json.loads(m.PIN.read_bytes())
        self.assertEqual(pin['commit'], '43d02057d96b326b4ea077388277e6064602ef61')
        self.assertEqual(pin['ref'], 'refs/heads/fix/maintenance-admission-and-installer-20261010')
        self.assertEqual(pin['qualification'], 'published_exact_commit')
        self.assertEqual(m.CANDIDATE_COMMIT, pin['commit'])
        self.assertEqual(pin['files'], m.EXPECTED_FILES)
        self.assertEqual(m.preflight(), pin)

    def test_missing_pin_no_git_no_materialization(self):
        with fixture(qualification='pending_reviewed_safety_successor'), tempfile.TemporaryDirectory() as directory, patch.object(h, 'git') as git:
            with self.assertRaises(m.QualificationFailure):
                h.maintenance(Path(directory), Path(directory) / 'private', {})
            git.assert_not_called()
            self.assertEqual(list(Path(directory).iterdir()), [])

    def test_job_missing_pin_before_guard_checkout_resource_effects(self):
        with fixture(qualification='pending_reviewed_safety_successor'), patch.object(h, 'hosted_guard') as guard, patch.object(h, 'checkout_proof') as checkout, patch.object(h, 'reclaim') as reclaim, patch.object(gate, 'start_daemon') as daemon, patch.object(h, 'wall_budget'):
            with self.assertRaises(m.QualificationFailure):
                gate.run_job('A')
        for operation in (guard, checkout, reclaim, daemon):
            operation.assert_not_called()
        self.assertIsNone(h.BOOTSTRAP_DEADLINE)

    def test_preflight_stdout_fixed_no_private_payload(self):
        output = io.StringIO()
        with fixture(qualification='pending_reviewed_safety_successor'), patch('sys.argv', ['maintenance_git.py', 'preflight']), patch('sys.stdout', output):
            self.assertEqual(m.main(), 1)
        self.assertEqual(output.getvalue(), 'MAINTENANCE_PIN_UNQUALIFIED\n')

    def test_qualified_exact_commit_blob_hash_readback(self):
        with fixture() as (root, pin, raw, git):
            proof, payloads = m.read_payloads(root, git)
            self.assertEqual(payloads, raw)
            self.assertEqual(proof, {key: pin[key] for key in ('repository', 'commit', 'ref', 'files')})
            m.proof_matches(proof)

    def test_exact_bytes_materialized_distinct_from_product_sdk(self):
        with fixture() as (root, pin, raw, git), patch.object(h, 'git', side_effect=git):
            proof, _ = m.read_payloads(root, git)
            destination = h.maintenance(root, root / 'private-checkout', proof)
            self.assertEqual(destination, root / 'maintenance')
            for name, value in raw.items():
                self.assertEqual((destination / name).read_bytes(), value)

    def reject_pin(self, change):
        with fixture() as (root, pin, _, git):
            change(pin)
            m.PIN.write_text(json.dumps(pin), encoding='ascii')
            call = Mock(side_effect=git)
            with self.assertRaises(m.QualificationFailure):
                m.read_payloads(root, call)
            call.assert_not_called()

    def test_moving_ref_rejected(self):
        self.reject_pin(lambda p: p.update(commit='main'))

    def test_wrong_repository_rejected(self):
        self.reject_pin(lambda p: p.update(repository='other/private'))

    def test_extra_path_rejected(self):
        self.reject_pin(lambda p: p['files'].update({'../secret': {}}))

    def test_changed_hash_contract_rejected(self):
        self.reject_pin(lambda p: p['files']['scripts/compose_helpers.py'].update(sha256='0' * 64))

    def test_unknown_policy_rejected(self):
        self.reject_pin(lambda p: p.update(qualification='task_changes'))

    def test_wrong_candidate_commit_or_ref_rejected_before_git(self):
        for changed in ({'commit': 'b' * 40}, {'ref': 'refs/heads/other'},
                        {'ref': 'refs/tags/maintenance-reviewed'}, {'ref': None}):
            with self.subTest(changed=changed):
                self.reject_pin(lambda p: p.update(changed))

    def test_closed_v2_schema_rejected_before_git(self):
        for changed in ({'schema': 'forge/private-maintenance-pin/v1'}, {'extra': True},
                        {'qualification': 'pending_published_ref_readback'}):
            with self.subTest(changed=changed):
                self.reject_pin(lambda p: p.update(changed))

    def test_qualify_other_candidate_with_same_files_never_calls_git(self):
        with fixture():
            for commit, ref in (('b' * 40, 'refs/heads/maintenance-reviewed'),
                                ('a' * 40, 'refs/heads/other')):
                with self.subTest(commit=commit, ref=ref):
                    git = Mock()
                    with self.assertRaises(m.QualificationFailure):
                        m.qualify(Path('.'), commit, ref, git)
                    git.assert_not_called()

    def test_fetched_branch_mismatch_or_missing_before_payload_read(self):
        for failure in (b'b' * 40 + b'\n', RuntimeError('PRIVATE REF ERROR')):
            with self.subTest(failure=type(failure).__name__), fixture() as (root, _, _, original):
                def changed(checkout, *args):
                    if args == ('rev-parse', '--verify', 'refs/remotes/origin/maintenance-reviewed^{commit}'):
                        if isinstance(failure, Exception):
                            raise failure
                        return failure
                    return original(checkout, *args)
                git = Mock(side_effect=changed)
                with self.assertRaises(m.QualificationFailure):
                    m.read_payloads(root, git)
                self.assertFalse(any(c.args[1] in ('cat-file', 'ls-tree') for c in git.call_args_list))
                self.assertEqual(list(root.iterdir()), [m.PIN])

    def test_unapproved_717_cannot_qualify_before_git(self):
        git = Mock()
        with self.assertRaises(m.QualificationFailure):
            m.qualify(Path('.'), '7170d3cc412fc3788a242be4470819fda26f56fc', m.CANDIDATE_REF, git)
        git.assert_not_called()

    def test_incomplete_reviewed_packet_cannot_qualify_or_preflight(self):
        for field in ('blob', 'sha256'):
            with self.subTest(field=field), fixture() as (root, _, _, git):
                m.EXPECTED_FILES['scripts/compose_helpers.py'][field] = None
                call = Mock(side_effect=git)
                with self.assertRaises(m.QualificationFailure):
                    m.qualify(root, 'a' * 40, 'refs/heads/maintenance-reviewed', call)
                with self.assertRaises(m.QualificationFailure):
                    m.preflight()
                call.assert_not_called()

    def test_reviewed_safety_digest_binds_unchanged_consumers_without_fallback(self):
        with fixture() as (root, pin, _, git):
            proof, _ = m.read_payloads(root, git)
            for consumer in ('native', 'observer'):
                module = SimpleNamespace(SDK_SHA256='old-unqualified-hash', consumer=consumer)
                h.bind_maintenance(module, proof)
                self.assertEqual(module.SDK_SHA256, pin['files']['scripts/compose_helpers.py']['sha256'])
                changed = copy.deepcopy(proof)
                changed['ref'] = 'refs/heads/other'
                with self.assertRaises(m.QualificationFailure):
                    h.bind_maintenance(module, changed)
                self.assertEqual(module.SDK_SHA256, pin['files']['scripts/compose_helpers.py']['sha256'])

    def test_pending_safety_packet_cannot_rebind_consumer(self):
        module = SimpleNamespace(SDK_SHA256='unchanged')
        with fixture(qualification='pending_reviewed_safety_successor'):
            with self.assertRaises(m.QualificationFailure):
                h.bind_maintenance(module, {})
        self.assertEqual(module.SDK_SHA256, 'unchanged')

    def test_rebinding_is_before_original_load_sdk_and_smoke_binding(self):
        tree = ast.parse((h.HERE / 'run.py').read_bytes())
        job = ast.unparse(next(n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == 'run_job'))
        smoke = ast.unparse(next(n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == 'smoke'))
        self.assertLess(job.index('h.bind_maintenance(q, maintenance_proof)'), job.index('q.load_sdk(maintenance)'))
        self.assertLess(smoke.index("h.bind_maintenance(base, admission['maintenance_git'])"), smoke.index('binding.bind()'))
        self.assertIn('h.require(base.SDK_SHA256 == q.SDK_SHA256)', smoke)

    def reject_git(self, predicate, replacement):
        with fixture() as (root, _, _, git):
            def changed(checkout, *args):
                value = git(checkout, *args)
                return replacement(value) if predicate(args) else value
            with self.assertRaises(m.QualificationFailure):
                m.read_payloads(root, changed)

    def test_wrong_commit_readback_rejected(self):
        self.reject_git(lambda a: a[:2] == ('rev-parse', '--verify'), lambda _: b'b' * 40)

    def test_shallow_checkout_rejected(self):
        self.reject_git(lambda a: a == ('rev-parse', '--is-shallow-repository'), lambda _: b'true')

    def test_wrong_remote_rejected(self):
        self.reject_git(lambda a: a[0] == 'remote', lambda _: b'https://private-token@evil.invalid/repo')

    def test_wrong_blob_rejected(self):
        self.reject_git(lambda a: a[0] == 'ls-tree', lambda b: b.replace(b' blob ', b' blob 0'))

    def test_symlink_tree_rejected(self):
        self.reject_git(lambda a: a[0] == 'ls-tree', lambda b: b.replace(b'100644', b'120000'))

    def test_missing_file_rejected(self):
        self.reject_git(lambda a: a[0] == 'ls-tree', lambda b: b.split(b'\0', 1)[1])

    def test_raw_byte_normalization_not_allowed(self):
        self.reject_git(lambda a: a[0] == 'cat-file', lambda b: b.replace(b'\r\n', b'\n'))

    def test_git_error_redacted(self):
        with fixture() as (root, _, _, git), patch('sys.stdout', new_callable=io.StringIO) as output:
            with self.assertRaises(m.QualificationFailure) as failure:
                m.read_payloads(root, Mock(side_effect=RuntimeError('TOKEN SQL PRIVATE PATH')))
            self.assertEqual(str(failure.exception), 'maintenance_pin_unqualified')
            self.assertEqual(output.getvalue(), '')
            self.assertEqual(h.safe_error('bootstrap', failure.exception)['dependency'], 'maintenance_pin_unqualified')

    def test_changed_proof_no_write(self):
        with fixture() as (root, pin, _, git), patch.object(h, 'git', side_effect=git):
            proof, _ = m.read_payloads(root, git)
            proof['commit'] = 'b' * 40
            with self.assertRaises(ValueError):
                h.maintenance(root, root, proof)
            self.assertFalse((root / 'maintenance').exists())

    def test_workflow_shared_private_checkout_full_history_original_budget(self):
        workflow = (h.HERE.parents[1] / '.github/workflows/forge-hermetic-full12.yml').read_text()
        self.assertEqual(workflow.count('python3 -B controls/scripts/hermetic_full12/maintenance_git.py preflight'), 3)
        self.assertEqual(workflow.count('path: services-base\n          fetch-depth: 0'), 3)
        self.assertEqual(workflow.count('Exact authenticated product SDK\n        timeout-minutes: 2'), 3)
        self.assertEqual(workflow.count('token: ${{ secrets.SERVICES_BASE_TOKEN }}'), 3)
        self.assertNotIn('set -x', workflow)

    def test_qualification_before_materialization_daemon_and_build(self):
        source = ast.unparse(next(node for node in ast.parse((h.HERE / 'run.py').read_bytes()).body
                                 if isinstance(node, ast.FunctionDef) and node.name == 'run_job'))
        self.assertLess(source.index('h.m.read_payloads('), source.index('root.mkdir('))
        self.assertLess(source.index('h.m.read_payloads('), source.index('daemon = start_daemon('))
        self.assertIn("'maintenance_git': maintenance_proof", source)


class ConsumerHistoryTests(unittest.TestCase):
    @staticmethod
    def git(_, *args):
        if args == ('rev-parse', '--is-shallow-repository'):
            return b'false\n'
        parents = {'HEAD': ('a' * 40, h.CONTROLS_PARENT),
                   h.CONTROLS_PARENT: (h.CONTROLS_PARENT, h.DELEGATION_CONTROLS),
                   h.DELEGATION_CONTROLS: (h.DELEGATION_CONTROLS, h.RESOURCE_CONTROLS),
                   h.RESOURCE_CONTROLS: (h.RESOURCE_CONTROLS, h.DIAGNOSTIC_CONTROLS),
                   h.DIAGNOSTIC_CONTROLS: (h.DIAGNOSTIC_CONTROLS, h.MAINTENANCE_CONTROLS),
                   h.MAINTENANCE_CONTROLS: (h.MAINTENANCE_CONTROLS, h.SAFETY_CONTROLS),
                   h.SAFETY_CONTROLS: (h.SAFETY_CONTROLS, h.QUALIFICATION_CONTROLS),
                   h.QUALIFICATION_CONTROLS: (h.QUALIFICATION_CONTROLS, h.PUBLIC_CONTROLS),
                   h.PUBLIC_CONTROLS: (h.PUBLIC_CONTROLS, h.SOURCE)}
        if args[:4] == ('rev-list', '--parents', '-n', '1'):
            return (' '.join(parents[args[4]]) + '\n').encode()
        if args == ('diff', '--name-status', h.CONTROLS_PARENT, 'HEAD'):
            return b'M\tscripts/hermetic_full12/maintenance-pin.json\n'
        if args == ('diff', '--name-status', h.SOURCE, 'HEAD'):
            return b'A\tscripts/hermetic_full12/maintenance-pin.json\n'
        raise AssertionError('unexpected synthetic Git invocation')

    def test_exact_normal_successor_history_admitted(self):
        with patch.object(h, 'git', side_effect=self.git):
            h.controls_history(Path('controls'))
        self.assertEqual(h.CONTROLS_PARENT, '3f366b5c73c1386f3d710f4dd94dc1c0ac074bc6')
        self.assertEqual(h.DELEGATION_CONTROLS, '3a9bbafd45c98b0458d1eddf98908275896476a0')
        self.assertEqual(h.RESOURCE_CONTROLS, 'eb91d4f1a3f3e0f3731c38a861db68ae8a0d952a')
        self.assertEqual(h.DIAGNOSTIC_CONTROLS, '1310958a4b5cf68642a6ce8d4466ef5ff1e64f31')
        self.assertEqual(h.MAINTENANCE_CONTROLS, 'b9375dc4b4f070b0f6cff1733228f1a6865b1368')
        self.assertEqual(h.SAFETY_CONTROLS, '4d97c54f35b485224a8229495763658fcbbd40e9')
        self.assertEqual(h.QUALIFICATION_CONTROLS, '632ea8347602fe4af22e7369790ed9c75e53f76a')
        self.assertEqual(h.PUBLIC_CONTROLS, '1dbedf85242c3540b70005ce5f0c20badb682c41')

    def test_shallow_wrong_parent_or_extra_merge_parent_rejected(self):
        for selector, raw in (
                (('rev-parse', '--is-shallow-repository'), b'true\n'),
                (('rev-list', '--parents', '-n', '1', 'HEAD'), ('a' * 40 + ' ' + h.SOURCE).encode()),
                (('rev-list', '--parents', '-n', '1', 'HEAD'), ('a' * 40 + ' ' + h.CONTROLS_PARENT + ' ' + h.SOURCE).encode()),
                (('rev-list', '--parents', '-n', '1', h.CONTROLS_PARENT), (h.CONTROLS_PARENT + ' ' + h.SOURCE).encode()),
                (('rev-list', '--parents', '-n', '1', h.MAINTENANCE_CONTROLS), (h.MAINTENANCE_CONTROLS + ' ' + h.SOURCE).encode()),
                (('rev-list', '--parents', '-n', '1', h.SAFETY_CONTROLS), (h.SAFETY_CONTROLS + ' ' + h.SOURCE).encode()),
                (('rev-list', '--parents', '-n', '1', h.QUALIFICATION_CONTROLS), (h.QUALIFICATION_CONTROLS + ' ' + h.SOURCE).encode()),
                (('rev-list', '--parents', '-n', '1', h.PUBLIC_CONTROLS), (h.PUBLIC_CONTROLS + ' ' + h.CONTROLS_PARENT).encode())):
            with self.subTest(selector=selector), patch.object(h, 'git', side_effect=lambda root, *a:
                    raw if a == selector else self.git(root, *a)):
                with self.assertRaises(ValueError):
                    h.controls_history(Path('controls'))

    def test_consumer_delta_and_public_history_path_guards(self):
        for baseline, raw in ((h.CONTROLS_PARENT, b''),
                              (h.CONTROLS_PARENT, b'M\tbackend/src/main.rs\n'),
                              (h.CONTROLS_PARENT, b'M\tscripts/hermetic_full12/tools.Dockerfile\n'),
                              (h.CONTROLS_PARENT, b'D\tscripts/hermetic_full12/maintenance-pin.json\n'),
                              (h.CONTROLS_PARENT, b'A\tscripts/hermetic_full12/private.py\n'),
                              (h.SOURCE, b'M\tbackend/Cargo.lock\n'),
                              (h.SOURCE, b'A\t.local/private.py\n')):
            with self.subTest(baseline=baseline, raw=raw), patch.object(h, 'git', side_effect=lambda root, *a:
                    raw if a == ('diff', '--name-status', baseline, 'HEAD') else self.git(root, *a)):
                with self.assertRaises(ValueError):
                    h.controls_history(Path('controls'))

    def test_workflow_and_host_bind_only_new_branch(self):
        workflow = (h.HERE.parents[1] / '.github/workflows/forge-hermetic-full12.yml').read_text()
        self.assertEqual(h.BRANCH, 'build-only/forge-delegation-full12-20261010')
        self.assertIn('branches: [' + h.BRANCH + ']', workflow)
        self.assertEqual(workflow.count("github.ref == 'refs/heads/" + h.BRANCH + "'"), 3)
        self.assertNotIn('forge-public-safe-full12-25be-20261009', workflow)

    def test_git_does_not_apply_replacement_objects(self):
        with patch.object(h, 'command', return_value=b'') as command:
            h.git(Path('controls'), 'rev-parse', 'HEAD')
        self.assertIn('--no-replace-objects', command.call_args.args[0])


if __name__ == '__main__':
    unittest.main()
