#!/usr/bin/env python3
"""Transport a typed repository build to the operator-owned rootless worker."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import uuid

MAX_CONTEXT = 512 * 1024**2
MAX_RESULT = 2 * 1024**2


def connection(path, *, raw=None):
    data = json.loads(path.read_bytes() if raw is None else raw)
    fields = {"schema", "host", "port", "user", "identityFile", "knownHostsFile"}
    version = data.get("schema")
    required = fields if version == "forge/build-host-connection/v1" else fields | {"id", "hostKeySha256"}
    if set(data) != required:
        raise ValueError("Closed build connection required")
    if version not in ("forge/build-host-connection/v1", "forge/build-host-connection/v2") or data["user"] != "root":
        raise ValueError("Unsupported operator connection")
    import ipaddress
    ipaddress.ip_address(data["host"])
    if type(data["port"]) is not int or not 1 <= data["port"] <= 65535:
        raise ValueError("Invalid operator port")
    for name in ["identityFile", "knownHostsFile"]:
        if not Path(data[name]).is_absolute():
            raise ValueError("Absolute operator credential paths required")
    if version == "forge/build-host-connection/v2":
        import re
        if not re.fullmatch(r"[a-z][a-z0-9-]{0,62}", data["id"]):
            raise ValueError("Invalid build host ID")
        if fingerprint(Path(data["knownHostsFile"])) != data["hostKeySha256"]:
            raise ValueError("Build host SSH fingerprint changed")
    return data


def fingerprint(path):
    rows = [line.split() for line in path.read_text().splitlines()
            if line.strip() and not line.lstrip().startswith("#")]
    if len(rows) != 1 or len(rows[0]) != 3 or rows[0][0].startswith("@"):
        raise ValueError("One explicit SSH host key required")
    key = base64.b64decode(rows[0][2], validate=True)
    return "SHA256:" + base64.b64encode(hashlib.sha256(key).digest()).decode().rstrip("=")


def private_directory(path):
    path = Path(path)
    if not path.is_absolute() or path.resolve(strict=True) != path:
        raise ValueError("Canonical private binding directory required")
    stat = path.stat()
    if not path.is_dir() or stat.st_uid != os.geteuid() or stat.st_mode & 0o077:
        raise ValueError("Build binding is not private owner state")


def pinned_connection(directory):
    private_directory(directory)
    marker = directory / "binding.json"
    if marker.is_symlink() or marker.stat().st_size > 65536 or marker.stat().st_mode & 0o077:
        raise ValueError("Private bounded build binding required")
    metadata = json.loads(marker.read_text())
    if (set(metadata) != {"schema", "hostId", "endpoint", "sshHostKeyFingerprint",
                          "configurationSha256", "files"}
            or metadata["schema"] != "forge/build-host-binding/v1"
            or set(metadata["files"]) != {"identity", "known_hosts", "connection.json"}):
        raise ValueError("Unknown build binding")
    terminal = directory / "terminal.json"
    for name, digest in metadata["files"].items():
        path = directory / name
        if name == "identity" and terminal.exists() and not path.exists():
            continue
        if path.is_symlink() or path.parent != directory or not path.is_file() or path.stat().st_mode & 0o077:
            raise ValueError("Build binding file redirected or missing")
        if hashlib.sha256(path.read_bytes()).hexdigest() != digest:
            raise ValueError("Build binding bytes changed")
    data = connection(directory / "connection.json")
    if (data["host"], data["port"], data["user"]) != tuple(metadata["endpoint"]):
        raise ValueError("Build binding endpoint changed")
    if fingerprint(Path(data["knownHostsFile"])) != metadata["sshHostKeyFingerprint"]:
        raise ValueError("Build binding host key changed")
    return data


def pin_connection(source, directory, expected=None):
    directory = Path(directory)
    if directory.exists():
        data = pinned_connection(directory)
        if expected and json.loads((directory / "binding.json").read_text())["configurationSha256"] != expected:
            raise ValueError("Retained build connection differs from original intent")
        return data
    private_directory(directory.parent)
    raw = source.read_bytes()
    data = connection(source, raw=raw)
    if expected and hashlib.sha256(raw).hexdigest() != expected:
        raise ValueError("Selected build connection differs from operator intent")
    if len(raw) > 65536:
        raise ValueError("Build connection exceeds bound")
    with tempfile.TemporaryDirectory(prefix=".build-host-", dir=directory.parent) as temporary:
        stage = Path(temporary)
        stage.chmod(0o700)
        files = {"identity": Path(data["identityFile"]).read_bytes(),
                 "known_hosts": Path(data["knownHostsFile"]).read_bytes()}
        if any(not value or len(value) > 65536 for value in files.values()):
            raise ValueError("Build credentials exceed bound")
        selected = dict(data, identityFile=str(directory / "identity"),
                        knownHostsFile=str(directory / "known_hosts"))
        files["connection.json"] = json.dumps(selected, sort_keys=True).encode()
        for name, value in files.items():
            path = stage / name
            with path.open("xb") as stream:
                stream.write(value); stream.flush(); os.fsync(stream.fileno())
            path.chmod(0o600)
        metadata = {"schema": "forge/build-host-binding/v1",
                    "hostId": data.get("id", "connection-" + hashlib.sha256(raw).hexdigest()[:16]),
                    "endpoint": [data["host"], data["port"], data["user"]],
                    "sshHostKeyFingerprint": fingerprint(stage / "known_hosts"),
                    "configurationSha256": hashlib.sha256(raw).hexdigest(),
                    "files": {name: hashlib.sha256(value).hexdigest() for name, value in files.items()}}
        with (stage / "binding.json").open("x") as stream:
            json.dump(metadata, stream); stream.flush(); os.fsync(stream.fileno())
        (stage / "binding.json").chmod(0o600)
        os.rename(stage, directory)
        parent = os.open(directory.parent, os.O_DIRECTORY)
        try:
            os.fsync(parent)
        finally:
            os.close(parent)
        # TemporaryDirectory cleanup accepts the atomically moved-away staging name.
    return pinned_connection(directory)


def terminal_readback(directory):
    path = directory / "terminal.json"
    if not path.exists():
        return None
    if path.is_symlink() or path.stat().st_size > MAX_RESULT or path.stat().st_mode & 0o077:
        raise ValueError("Invalid retained terminal readback")
    value = json.loads(path.read_bytes())
    if value.get("schema") != "forge/host-image-build-status/v1" or value.get("phase") not in ("complete", "failed"):
        raise ValueError("Retained build is not terminal")
    return value


def retain_terminal(directory, value):
    if value.get("phase") not in ("complete", "failed") or value.get("ownerAlive") is not False:
        return
    path = directory / "terminal.json"
    payload = json.dumps(value, sort_keys=True).encode()
    if path.exists():
        if json.loads(path.read_bytes()) != value:
            raise ValueError("Retained terminal readback changed")
    else:
        with path.open("xb") as stream:
            stream.write(payload); stream.flush(); os.fsync(stream.fileno())
        path.chmod(0o600)
    parent = os.open(directory, os.O_DIRECTORY)
    try:
        os.fsync(parent)
        (directory / "identity").unlink(missing_ok=True)
        os.fsync(parent)
    finally:
        os.close(parent)


def ssh(data, private_key, operation):
    return ["ssh", "-F", "/dev/null", "-T", "-o", "BatchMode=yes",
            "-o", "StrictHostKeyChecking=yes", "-o", "IdentitiesOnly=yes",
            "-o", "IdentityAgent=none", "-o", "ConnectTimeout=10",
            "-o", "ServerAliveInterval=10", "-o", "ServerAliveCountMax=3",
            "-o", "UserKnownHostsFile=" + data["knownHostsFile"],
            "-i", str(private_key), "-p", str(data["port"]),
            data["user"] + "@" + data["host"], operation]


def request_identity(value):
    required = {"jobId", "attemptId", "leaseId", "generation", "sourceCommit", "spec"}
    if set(value) != required:
        raise ValueError("Closed typed execution identity required")
    for name in ["jobId", "attemptId", "leaseId"]:
        if str(uuid.UUID(value[name])) != value[name]:
            raise ValueError("Invalid execution UUID")
    import re
    if not re.fullmatch("(?:[a-f0-9]{40}|[a-f0-9]{64})", value["sourceCommit"]):
        raise ValueError("Exact source commit required")
    if type(value["generation"]) is not int or value["generation"] <= 0:
        raise ValueError("Fencing generation required")


def execute(data, key, value):
    request_identity(value)
    workspace = Path.cwd().resolve()
    actual = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    if actual != value["sourceCommit"]:
        raise ValueError("Checkout differs from the offered commit")
    # The archive contains tracked bytes from the exact commit, never runner
    # credentials, .git/config or unrelated files left in the workspace.
    with tempfile.TemporaryDirectory(prefix=".forge-build-", dir=workspace) as directory:
        directory = Path(directory)
        archive = directory / "source.tar"
        subprocess.run(["git", "archive", "--format=tar", "--output", str(archive),
                        value["sourceCommit"]], check=True)
        size = archive.stat().st_size
        if not 0 < size <= MAX_CONTEXT:
            raise ValueError("Repository context exceeds transport bound")
        digest = hashlib.sha256()
        with archive.open("rb") as stream:
            for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                digest.update(chunk)
        request = {**value, "schema": "forge/host-image-build/v1",
                   "contextBytes": size, "contextSha256": digest.hexdigest()}
        output = directory / "result.json"
        with output.open("wb") as stream:
            child = subprocess.Popen(ssh(data, key, "build"), stdin=subprocess.PIPE,
                                     stdout=stream)
            def cancel(signum, _frame):
                # The server retains custody and the shared slot until worker
                # readback; the controller separately checks that final receipt.
                subprocess.run(ssh(data, key, "cancel " + value["attemptId"] + " "
                                   + str(value["generation"])),
                               stdout=subprocess.DEVNULL, timeout=15, check=True)
                if child.poll() is None:
                    child.terminate()
                raise SystemExit(128 + signum)
            for signum in [signal.SIGTERM, signal.SIGINT, signal.SIGHUP]:
                signal.signal(signum, cancel)
            try:
                child.stdin.write((json.dumps(request) + "\n").encode())
                with archive.open("rb") as source:
                    shutil.copyfileobj(source, child.stdin, 1024 * 1024)
                child.stdin.close()
                status = child.wait()
            except BrokenPipeError as error:
                # An early refusal closes stdin before its diagnostic reaches
                # SSH stderr. Let that child finish before cleanup terminates it.
                try:
                    child.stdin.close()
                except BrokenPipeError:
                    pass
                status = child.wait(timeout=15)
                raise RuntimeError(
                    f"Worker closed build input before accepting context (exit {status}); "
                    "retain its receipt"
                ) from error
            finally:
                if child.poll() is None:
                    child.terminate()
                    child.wait(timeout=15)
        if status != 0:
            raise RuntimeError("Worker refused or failed the build; retain its receipt")
        if output.stat().st_size > MAX_RESULT:
            raise ValueError("Worker result exceeds bound")
        result = json.loads(output.read_text())
        if result["schema"] != "forge/host-image-build-result/v1" or result["sourceCommit"] != value["sourceCommit"]:
            raise ValueError("Worker result source differs")
        print("Published " + result["publishedImage"]["imageRef"], flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--connection", type=Path, default=Path("/run/forge-build-host/connection.json"))
    parser.add_argument("--request")
    parser.add_argument("--pin", type=Path)
    parser.add_argument("--binding", type=Path)
    parser.add_argument("--expect-connection")
    readback = parser.add_mutually_exclusive_group()
    readback.add_argument("--status", nargs=2, metavar=("ATTEMPT", "GENERATION"))
    readback.add_argument("--cancel", nargs=2, metavar=("ATTEMPT", "GENERATION"))
    readback.add_argument("--health", action="store_true")
    args = parser.parse_args()
    if args.pin:
        if args.binding or args.request or args.status or args.cancel or args.health:
            raise ValueError("Pin is a separate owner operation")
        pin_connection(args.connection, args.pin, args.expect_connection)
        return
    data = pinned_connection(args.binding) if args.binding else connection(args.connection)
    if args.binding and (args.status or args.cancel):
        retained = terminal_readback(args.binding)
        if retained is not None:
            attempt, generation = args.status or args.cancel
            if retained.get("attemptId") != attempt or retained.get("generation") != int(generation):
                raise ValueError("Retained terminal identity differs")
            print(json.dumps(retained))
            return
    with tempfile.TemporaryDirectory(prefix="forge-build-ssh-") as temporary:
        key = Path(temporary) / "identity"
        key.write_bytes(Path(data["identityFile"]).read_bytes())
        key.chmod(0o600)
        if args.health:
            result=subprocess.run(ssh(data,key,"health"),stdout=subprocess.PIPE,check=True)
            if len(result.stdout)>4096:
                raise ValueError("Health response exceeds bound")
            json.loads(result.stdout)
            sys.stdout.buffer.write(result.stdout)
        elif args.status or args.cancel:
            attempt, generation = args.status or args.cancel
            if str(uuid.UUID(attempt)) != attempt or int(generation) <= 0:
                raise ValueError("Canonical readback identity required")
            result = subprocess.run(ssh(data, key, ("cancel" if args.cancel else "status") + " " + attempt + " " + str(int(generation))),
                                    stdout=subprocess.PIPE, check=True)
            if len(result.stdout) > MAX_RESULT:
                raise ValueError("Worker receipt exceeds bound")
            value = json.loads(result.stdout)
            if args.binding:
                if (value.get("phase") != "absent"
                        and (value.get("attemptId") != attempt or value.get("generation") != int(generation))):
                    raise ValueError("Worker readback identity differs")
                retain_terminal(args.binding, value)
            sys.stdout.buffer.write(result.stdout)
        else:
            execute(data, key, json.loads(args.request))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, RuntimeError, subprocess.SubprocessError) as error:
        print("Typed image build failed: " + str(error), file=sys.stderr)
        raise SystemExit(1)
