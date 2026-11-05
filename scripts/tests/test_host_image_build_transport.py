"""Exercise real Git/archive/pipe transport without SSH or a build daemon."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from uuid import uuid4


HELPER = Path(__file__).resolve().parents[1] / 'host_image_build.py'
WORKER = r'''
import hashlib, json, os, sys, time
if sys.argv[1] == 'reject':
    os.close(0)
    time.sleep(0.3)
    print('fixture worker refused: admission busy', file=sys.stderr, flush=True)
    sys.exit(73)
request = json.loads(sys.stdin.buffer.readline())
context = sys.stdin.buffer.read()
assert len(context) == request['contextBytes']
assert hashlib.sha256(context).hexdigest() == request['contextSha256']
print(json.dumps({'schema': 'forge/host-image-build-result/v1',
    'sourceCommit': request['sourceCommit'],
    'publishedImage': {'imageRef': 'example.test/tools@sha256:' + '1' * 64}}))
'''
CALLER = r'''
import importlib.util, json, signal, sys
from types import SimpleNamespace
spec = importlib.util.spec_from_file_location('selected_helper', sys.argv[1])
helper = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helper)
helper.ssh = lambda *args: [sys.executable, sys.argv[2], sys.argv[3]]
# Signal delivery is qualified separately on Linux. This regression exercises
# real child pipe ownership on both Windows and Linux without installing handlers.
helper.signal = SimpleNamespace(SIGTERM=signal.SIGTERM, SIGINT=signal.SIGINT,
    SIGHUP=getattr(signal, 'SIGHUP', signal.SIGTERM), signal=lambda *args: None)
try:
    helper.execute({}, None, json.loads(sys.argv[4]))
except Exception as error:
    print(type(error).__name__ + ': ' + str(error), file=sys.stderr)
    sys.exit(1)
'''


class HostImageBuildTransportTests(unittest.TestCase):
    def attempt(self, mode):
        with tempfile.TemporaryDirectory(prefix='forge-pipe-test-') as temporary:
            root = Path(temporary)
            workspace = root / 'source checkout'
            workspace.mkdir()
            worker = root / 'worker.py'
            worker.write_text(WORKER, encoding='utf-8')
            (workspace / 'tracked.txt').write_bytes(b'x' * (1024 * 1024))
            subprocess.run(['git', 'init', '--quiet', str(workspace)], check=True)
            subprocess.run(['git', '-C', str(workspace), 'add', 'tracked.txt'], check=True)
            subprocess.run(['git', '-C', str(workspace), '-c', 'user.name=Fixture',
                            '-c', 'user.email=fixture@example.test', 'commit', '--quiet',
                            '-m', 'fixture'], check=True)
            commit = subprocess.check_output(['git', '-C', str(workspace), 'rev-parse', 'HEAD'],
                                             text=True).strip()
            request = {name: str(uuid4()) for name in ('jobId', 'attemptId', 'leaseId')}
            request.update(generation=1, sourceCommit=commit, spec={})
            result = subprocess.run([sys.executable, '-B', '-c', CALLER, str(HELPER),
                                     str(worker), mode, json.dumps(request)], cwd=workspace,
                                    capture_output=True, text=True, timeout=20)
            self.assertFalse(list(workspace.glob('.forge-build-*')))
            return result

    def test_early_worker_refusal_is_flushed_before_cleanup(self):
        result = self.attempt('reject')
        self.assertEqual(result.returncode, 1)
        self.assertIn('RuntimeError: Worker closed build input', result.stderr)
        self.assertIn('fixture worker refused: admission busy', result.stderr)
        self.assertNotIn('Published ', result.stdout)

    def test_complete_context_still_reaches_the_worker(self):
        result = self.attempt('accept')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('Published example.test/tools@sha256:', result.stdout)


if __name__ == '__main__':
    unittest.main()
