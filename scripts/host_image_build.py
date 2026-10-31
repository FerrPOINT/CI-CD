#!/usr/bin/env python3
"""Transport a typed repository build to the operator-owned rootless worker."""
import argparse
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


def connection(path):
    data = json.loads(path.read_text())
    if set(data) != {"schema", "host", "port", "user", "identityFile", "knownHostsFile"}:
        raise ValueError("Closed build connection required")
    if data["schema"] != "forge/build-host-connection/v1" or data["user"] != "root":
        raise ValueError("Unsupported operator connection")
    import ipaddress
    ipaddress.ip_address(data["host"])
    if type(data["port"]) is not int or not 1 <= data["port"] <= 65535:
        raise ValueError("Invalid operator port")
    for name in ["identityFile", "knownHostsFile"]:
        if not Path(data[name]).is_absolute():
            raise ValueError("Absolute operator credential paths required")
    return data


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
    readback = parser.add_mutually_exclusive_group()
    readback.add_argument("--status", nargs=2, metavar=("ATTEMPT", "GENERATION"))
    readback.add_argument("--cancel", nargs=2, metavar=("ATTEMPT", "GENERATION"))
    readback.add_argument("--health", action="store_true")
    args = parser.parse_args()
    data = connection(args.connection)
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
            json.loads(result.stdout)
            sys.stdout.buffer.write(result.stdout)
        else:
            execute(data, key, json.loads(args.request))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, RuntimeError, subprocess.SubprocessError) as error:
        print("Typed image build failed: " + str(error), file=sys.stderr)
        raise SystemExit(1)
