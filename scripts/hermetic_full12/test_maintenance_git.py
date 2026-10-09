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
import unittest
from unittest.mock import Mock, patch

import host as h
import maintenance_git as m
import run as gate


@contextmanager
def fixture():
    # These are public test strings, not implementations or encoded private source.
    payloads = {name: ('public-unit-fixture:' + name + '\r\n').encode() for name in m.EXPECTED_FILES}
    files = {name: {'blob': hashlib.sha1(b'blob ' + str(len(raw)).encode() + b'\0' + raw).hexdigest(),
                    'sha256': hashlib.sha256(raw).hexdigest()} for name, raw in payloads.items()}
    pin = {'schema': 'forge/private-maintenance-pin/v1', 'repository': m.REPOSITORY,
           'commit': 'a' * 40, 'qualification': 'published_exact_commit', 'files': files}
    def git(_, *args):
        if args == ('remote', 'get-url', 'origin'):
            return ('https://github.com/' + m.REPOSITORY + '.git\n').encode()
        if args == ('rev-parse', '--verify', 'a' * 40 + '^{commit}'):
            return ('a' * 40 + '\n').encode()
        if args == ('rev-parse', '--is-shallow-repository'):
            return b'false\n'
        if args == ('ls-tree', '-z', 'a' * 40, '--', *sorted(files)):
            return b''.join(('100644 blob ' + files[name]['blob'] + '\t' + name + '\0').encode() for name in sorted(files))
        if args[:2] == ('cat-file', 'blob'):
            return next(payloads[name] for name in files if files[name]['blob'] == args[2])
        raise AssertionError('unexpected synthetic Git invocation')
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory) / 'pin.json'
        path.write_text(json.dumps(pin), encoding='ascii')
        with patch.object(m, 'PIN', path), patch.object(m, 'EXPECTED_FILES', copy.deepcopy(files)):
            yield Path(directory), pin, payloads, git


class MaintenanceGitTests(unittest.TestCase):
    def test_current_pin_explicitly_missing(self):
        pin = json.loads(m.PIN.read_bytes())
        self.assertIsNone(pin['commit'])
        self.assertEqual(pin['qualification'], 'missing_published_exact_commit')
        with self.assertRaises(m.QualificationFailure):
            m.preflight()

    def test_missing_pin_no_git_no_materialization(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(h, 'git') as git:
            with self.assertRaises(m.QualificationFailure):
                h.maintenance(Path(directory), Path(directory) / 'private', {})
            git.assert_not_called()
            self.assertEqual(list(Path(directory).iterdir()), [])

    def test_job_missing_pin_before_guard_checkout_resource_effects(self):
        with patch.object(h, 'hosted_guard') as guard, patch.object(h, 'checkout_proof') as checkout, patch.object(h, 'reclaim') as reclaim, patch.object(gate, 'start_daemon') as daemon, patch.object(h, 'wall_budget'):
            with self.assertRaises(m.QualificationFailure):
                gate.run_job('A')
        for operation in (guard, checkout, reclaim, daemon):
            operation.assert_not_called()
        self.assertIsNone(h.BOOTSTRAP_DEADLINE)

    def test_preflight_stdout_fixed_no_private_payload(self):
        output = io.StringIO()
        with patch('sys.argv', ['maintenance_git.py', 'preflight']), patch('sys.stdout', output):
            self.assertEqual(m.main(), 1)
        self.assertEqual(output.getvalue(), 'MAINTENANCE_PIN_UNQUALIFIED\n')

    def test_qualified_exact_commit_blob_hash_readback(self):
        with fixture() as (root, pin, raw, git):
            proof, payloads = m.read_payloads(root, git)
            self.assertEqual(payloads, raw)
            self.assertEqual(proof, {key: pin[key] for key in ('repository', 'commit', 'files')})
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


if __name__ == '__main__':
    unittest.main()
