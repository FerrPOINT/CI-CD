#!/usr/bin/env python3
"""The wrapper must select a complete profile and refuse an unknown project."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT=Path(__file__).resolve().parents[2]

@unittest.skipUnless(os.name == 'posix' and shutil.which('bash'), 'requires POSIX shell')
class BaseStandBackupTests(unittest.TestCase):
    def invoke(self, project):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)
            runner=path/'python3'
            runner.write_text("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$RECEIPT\"\n")
            runner.chmod(0o700)
            env=dict(os.environ,PATH=str(path)+os.pathsep+os.environ['PATH'],
                SDLC_WORKSPACE_DIR=str(path),SDLC_PROJECT=project,SDLC_DOCKER_CONTEXT='selected',
                SDLC_SIGNING_KEY=str(path/'preserved.pem'),RECEIPT=str(path/'args'))
            result=subprocess.run(['bash',str(ROOT/'scripts/backup-stand.sh'),str(path/'backup.tar.gz')],env=env,capture_output=True)
            return result.returncode,(path/'args').read_text().splitlines() if (path/'args').exists() else []
    def test_complete_profile_and_quiescence_forwarded(self):
        code,args=self.invoke('sdlc1')
        self.assertEqual(code,0)
        for required in ('--workspace-profile','--quiesce','--signing-key','--docker-context'):
            self.assertIn(required,args)
        self.assertNotIn('--skip-file-volumes',args)
    def test_unknown_project_does_not_invoke_backup(self):
        code,args=self.invoke('old-demo')
        self.assertNotEqual(code,0)
        self.assertEqual(args,[])

if __name__=='__main__':unittest.main()
