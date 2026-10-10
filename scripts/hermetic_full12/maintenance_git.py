"""Metadata-only public contract; private bytes stay in authenticated Git/ephemeral VM."""
import hashlib
import json
from pathlib import Path
import re
import sys
import time

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
    return _read_payloads(checkout, git, pin)


def _read_payloads(checkout, git, pin):
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


def qualify(checkout, commit, ref, git):
    """Read-only candidate metadata, never activation of the checked-in pin."""
    try:
        require(isinstance(commit, str) and re.fullmatch('[a-f0-9]{40}', commit))
        require(isinstance(ref, str) and ref.startswith('refs/heads/'))
        require(len(ref) <= 256 and re.fullmatch('[A-Za-z0-9_./-]+', ref))
        git(checkout, 'check-ref-format', ref)
        # Authenticate the origin before any network call. No URL supplied by caller.
        remote = git(checkout, 'remote', 'get-url', 'origin').decode().strip()
        require(remote in ('https://github.com/' + REPOSITORY + '.git',
                           'https://github.com/' + REPOSITORY,
                           'git@github.com:' + REPOSITORY + '.git'))
        advertised = (commit + '\t' + ref + '\n').encode('ascii')
        require(git(checkout, 'ls-remote', '--exit-code', 'origin', ref) == advertised)
        pin = {'schema': 'forge/private-maintenance-pin/v1', 'repository': REPOSITORY,
               'commit': commit, 'qualification': 'published_exact_commit', 'files': EXPECTED_FILES}
        _read_payloads(checkout, git, pin)
        require(git(checkout, 'ls-remote', '--exit-code', 'origin', ref) == advertised)
        return pin
    except Exception:
        raise QualificationFailure() from None


def qualification_git(deadline):
    from host import command

    def git(checkout, *args):
        try:
            remaining = deadline - time.monotonic()
            require(remaining > 0)
            raw = command(['git', '--no-replace-objects', '--no-optional-locks',
                           '-C', str(checkout), *args], timeout=remaining)
            require(len(raw) <= 2**20 and time.monotonic() < deadline)
            return raw
        except Exception:
            raise QualificationFailure() from None
    return git


def proof_matches(proof):
    pin = preflight()
    require(proof == {'repository': REPOSITORY, 'commit': pin['commit'], 'files': pin['files']})


def main():
    try:
        if len(sys.argv) == 5 and sys.argv[1] == 'qualify':
            pin = qualify(Path(sys.argv[2]), sys.argv[3], sys.argv[4],
                          qualification_git(time.monotonic() + 60))
            print(json.dumps(pin, sort_keys=True, indent=2))
            return 0
        require(sys.argv[1:] == ['preflight'])
        preflight()
        print('MAINTENANCE_PIN_QUALIFIED')
        return 0
    except Exception:
        print('MAINTENANCE_PIN_UNQUALIFIED')
        return 1


if __name__ == '__main__':
    sys.exit(main())
