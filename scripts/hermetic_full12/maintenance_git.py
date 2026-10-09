"""Metadata-only public contract; private bytes stay in authenticated Git/ephemeral VM."""
import hashlib
import json
from pathlib import Path
import re
import sys

PIN = Path(__file__).with_name('maintenance-pin.json')
REPOSITORY = 'FerrPOINT/services-base'
EXPECTED_FILES = {
    'scripts/compose_helpers.py': {
        'blob': '6f03edc996ad660384b46e97d298ccf419697c65',
        'sha256': '2569882c3bb6a8b86ebb42367254720c5f802d5423bc98ef02c6fe3283e0874f'},
    'scripts/local_resource_cleanup.py': {
        'blob': 'aed6edc8ab5af6ec656ae8c591b034a2663eec76',
        'sha256': 'babc3fbba5094608b03a34a27fecf512fa917950b1ae55636277dd33741031e1'},
    'scripts/install_resource_cleanup.ps1': {
        'blob': '63e3b543caf002a62d67d123beb5eb30ec66ed94',
        'sha256': '33fe120af1854034220d940095d04774d660ffba1da47d2f9daa5e244cafae18'},
}


class QualificationFailure(ValueError):
    def __init__(self):
        super().__init__('maintenance_pin_unqualified')


def require(value):
    if not value:
        raise QualificationFailure()


def preflight():
    try:
        require(not PIN.is_symlink() and PIN.stat().st_size < 16384)
        pin = json.loads(PIN.read_bytes())
        require(set(pin) == {'schema', 'repository', 'commit', 'qualification', 'files'})
        require(pin['schema'] == 'forge/private-maintenance-pin/v1')
        require(pin['repository'] == REPOSITORY and pin['files'] == EXPECTED_FILES)
        require(isinstance(pin['commit'], str) and re.fullmatch('[a-f0-9]{40}', pin['commit']))
        require(pin['qualification'] == 'published_exact_commit')
        return pin
    except Exception:
        raise QualificationFailure() from None


def read_payloads(checkout, git):
    pin = preflight()  # Never attempt Git/network/materialization on a missing pin.
    try:
        remote = git(checkout, 'remote', 'get-url', 'origin').decode().strip()
        require(remote in ('https://github.com/' + REPOSITORY + '.git',
                           'https://github.com/' + REPOSITORY,
                           'git@github.com:' + REPOSITORY + '.git'))
        commit = pin['commit']
        require(git(checkout, 'rev-parse', '--verify', commit + '^{commit}').decode().strip() == commit)
        require(git(checkout, 'rev-parse', '--is-shallow-repository').strip() == b'false')
        entries = git(checkout, 'ls-tree', '-z', commit, '--', *sorted(EXPECTED_FILES)).split(b'\0')
        tree = {}
        for entry in entries:
            if not entry:
                continue
            metadata, name = entry.split(b'\t')
            mode, kind, blob = metadata.decode('ascii').split()
            name = name.decode('ascii')
            require(mode == '100644' and kind == 'blob' and name not in tree)
            tree[name] = blob
        require(tree == {name: item['blob'] for name, item in EXPECTED_FILES.items()})
        payloads = {}
        for name, item in EXPECTED_FILES.items():
            raw = git(checkout, 'cat-file', 'blob', item['blob'])
            require(0 < len(raw) <= 2**20)
            require(hashlib.sha1(b'blob ' + str(len(raw)).encode() + b'\0' + raw).hexdigest() == item['blob'])
            require(hashlib.sha256(raw).hexdigest() == item['sha256'])
            payloads[name] = raw
        return {'repository': REPOSITORY, 'commit': commit, 'files': pin['files']}, payloads
    except Exception:
        raise QualificationFailure() from None


def proof_matches(proof):
    pin = preflight()
    require(proof == {'repository': REPOSITORY, 'commit': pin['commit'], 'files': pin['files']})


def main():
    try:
        require(sys.argv[1:] == ['preflight'])
        preflight()
        print('MAINTENANCE_PIN_QUALIFIED')
        return 0
    except QualificationFailure:
        print('MAINTENANCE_PIN_UNQUALIFIED')
        return 1


if __name__ == '__main__':
    sys.exit(main())
