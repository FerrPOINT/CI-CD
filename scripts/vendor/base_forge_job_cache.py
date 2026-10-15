"""Registered read-only public Cargo seeds; job writes remain attempt-private.

This module grants neither Docker access nor volume lifecycle authority. The
operator registers immutable payloads; Forge/Gateway enforce typed attachments.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import sys
import tempfile
import time
from pathlib import Path, PurePosixPath

SCHEMA = "base-forge-cache-registry/v1"
SEED_SCHEMA = "base-forge-cargo-seed/v1"
TARGET = "/opt/sdlc/cargo-seed"
DESTINATION = "/usr/local/cargo/registry"
FIELDS = ("workspace", "project", "daemon", "image", "base_revision", "lock_sha256", "platform")
HEX = re.compile(r"[a-f0-9]{64}\Z")
REVISION = re.compile(r"[a-f0-9]{40}\Z")
PROJECT = re.compile(r"[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}\Z")


def regular(path, limit=None):
    path = Path(path)
    metadata = path.lstat()
    if path.is_symlink() or getattr(metadata, "st_file_attributes", 0) & 0x400 or not path.is_file():
        raise ValueError("Cache evidence must be a regular file without links")
    if limit is not None and metadata.st_size > limit:
        raise ValueError("Cache evidence exceeds its byte limit")
    return path.read_bytes()


def cache_key(identity):
    if not isinstance(identity, dict) or set(identity) != set(FIELDS) or not all(isinstance(value, str) for value in identity.values()):
        raise ValueError("Cache identity fields differ from the protocol")
    if identity["workspace"] not in {"pdlc1", "pdlc2", "pdlc3"}:
        raise ValueError("Unknown cache workspace")
    if not PROJECT.fullmatch(identity["project"]):
        raise ValueError("Cache requires the database project UUID")
    if not re.fullmatch(r"[A-Za-z0-9_.:-]{1,256}", identity["daemon"]):
        raise ValueError("Cache requires the verified daemon identity")
    if not re.fullmatch(r"sha256:[a-f0-9]{64}", identity["image"]):
        raise ValueError("Cache requires the immutable tool image ID")
    if not REVISION.fullmatch(identity["base_revision"]) or not HEX.fullmatch(identity["lock_sha256"]):
        raise ValueError("Cache requires exact SDK and lock inputs")
    if identity["platform"] not in {"linux/amd64", "linux/arm64"}:
        raise ValueError("Unknown cache tool platform")
    packet = SCHEMA + "\n" + "\n".join(identity[name] for name in FIELDS) + "\n"
    return hashlib.sha256(packet.encode("ascii")).hexdigest()


def registry(value, maximum_bytes):
    """Validate all bindings and charge each immutable seed once, across projects."""
    if not isinstance(value, dict) or value.get("schema") != SCHEMA or type(maximum_bytes) is not int or maximum_bytes <= 0:
        raise ValueError("Unknown cache registry or budget")
    if set(value) != {"schema", "seeds", "bindings"}:
        raise ValueError("Unknown cache registry fields")
    seeds, bindings = value.get("seeds"), value.get("bindings")
    if not isinstance(seeds, dict) or not isinstance(bindings, list) or len(seeds) > 64 or len(bindings) > 256:
        raise ValueError("Cache registration count exceeds its limit")
    names, total, keys = set(), 0, set()
    for manifest_sha, seed in seeds.items():
        if not isinstance(manifest_sha, str) or not HEX.fullmatch(manifest_sha) or not isinstance(seed, dict) or set(seed) != {"volume", "bytes"}:
            raise ValueError("Invalid immutable seed registration")
        if seed["volume"] != "pdlc-cargo-seed-" + manifest_sha or seed["volume"] in names:
            raise ValueError("Seed volume name/content identity mismatch")
        if type(seed["bytes"]) is not int or not 0 < seed["bytes"] <= maximum_bytes:
            raise ValueError("Seed payload exceeds its byte budget")
        names.add(seed["volume"])
        total += seed["bytes"]
    if total > maximum_bytes:
        raise ValueError("Registered seed pool exceeds its global budget")
    for binding in bindings:
        if not isinstance(binding, dict) or set(binding) != {*FIELDS, "key", "seed"}:
            raise ValueError("Invalid cache binding fields")
        key = cache_key({name: binding[name] for name in FIELDS})
        if binding["key"] != key or key in keys or binding["seed"] not in seeds:
            raise ValueError("Cache binding identity/seed mismatch")
        keys.add(key)
    return value


def read_registry(path, expected_sha256, maximum_bytes):
    if not HEX.fullmatch(expected_sha256):
        raise ValueError("Registry requires an operator-pinned SHA-256")
    payload = regular(path, 1024 * 1024)
    if hashlib.sha256(payload).hexdigest() != expected_sha256:
        raise ValueError("Cache registry changed after operator registration")
    return registry(json.loads(payload), maximum_bytes)


def binding_for(value, identity):
    key = cache_key(identity)
    binding = next((item for item in value["bindings"] if item["key"] == key), None)
    if binding is None:
        return None
    return {**binding, **value["seeds"][binding["seed"]]}


def seed_labels(manifest_sha256, size):
    return {"pdlc.cache.owner": "operator", "pdlc.cache.schema": SEED_SCHEMA,
            "pdlc.cache.manifest-sha256": manifest_sha256, "pdlc.cache.bytes": str(size)}


def volume_matches(record, resource):
    labels = resource.get("Labels") or {}
    return (resource.get("Name") == record["volume"] and
            all(labels.get(name) == value for name, value in seed_labels(record["seed"], record["bytes"]).items()) and
            resource.get("Driver") == "local" and not resource.get("Options") and
            not labels.get("pdlc.gateway.workspace") and not labels.get("com.docker.compose.project"))


def public_path(name):
    """Seed only public crates.io data, never Cargo config/credentials or targets."""
    path = PurePosixPath(name)
    if path.as_posix() != name or path.is_absolute() or any(part in {"", ".", ".."} for part in path.parts) or "\\" in name:
        return False
    parts = path.parts
    if len(parts) < 3 or parts[0] not in {"cache", "index"} or not re.fullmatch(r"index\.crates\.io-[a-f0-9]{16}", parts[1]):
        return False
    if any(part in {".git", "credentials", "credentials.toml", "target"} for part in parts[2:]):
        return False
    if parts[0] == "cache":
        return len(parts) == 3 and bool(re.fullmatch(r"[A-Za-z0-9_.+-]+\.crate", parts[2]))
    if parts[0] == "index":
        return parts[2:] == ("config.json",) or (parts[2] == ".cache" and len(parts) >= 4)
    return False


def verified_seed(source, expected_sha256, maximum_bytes):
    source = Path(source)
    if source.is_symlink() or getattr(source.lstat(), "st_file_attributes", 0) & 0x400:
        raise ValueError("Seed root links are forbidden")
    payload = regular(source / "seed.json", 8 * 1024 * 1024)
    if not HEX.fullmatch(expected_sha256) or hashlib.sha256(payload).hexdigest() != expected_sha256:
        raise ValueError("Seed manifest hash differs from its registration")
    manifest = json.loads(payload)
    if manifest.get("schema") != SEED_SCHEMA or not isinstance(manifest.get("files"), dict) or not 0 < len(manifest["files"]) <= 100000:
        raise ValueError("Invalid seed file manifest")
    declared = manifest["files"]
    actual, size = {}, 0
    for path in sorted(source.rglob("*")):
        metadata = path.lstat()
        if path.is_symlink() or getattr(metadata, "st_file_attributes", 0) & 0x400:
            raise ValueError("Seed payload links/junctions are forbidden")
        if path.is_dir():
            continue
        name = path.relative_to(source).as_posix()
        if name == "seed.json":
            continue
        if not public_path(name):
            raise ValueError("Seed contains data outside the public Cargo namespace")
        size += metadata.st_size
        if size > maximum_bytes:
            raise ValueError("Seed payload exceeds its registered byte limit")
        payload = regular(path, maximum_bytes)
        if name.endswith("/config.json") and name.startswith("index/"):
            config = json.loads(payload)
            if (set(config) - {"dl", "api", "auth-required"} or config.get("auth-required") or
                    config.get("dl") != "https://static.crates.io/crates" or config.get("api") != "https://crates.io"):
                raise ValueError("Seed index is not the public crates.io registry")
        actual[name] = hashlib.sha256(payload).hexdigest()
    if actual != declared or size != maximum_bytes:
        raise ValueError("Seed payload hash/size differs from its manifest")
    return actual


def materialize(source, destination, expected_sha256, maximum_bytes):
    """Verify before writing; preserve an existing private registry on any miss."""
    started = time.monotonic()
    destination = Path(destination)
    try:
        files = verified_seed(source, expected_sha256, maximum_bytes)
        if destination.is_symlink() or getattr(destination.lstat(), "st_file_attributes", 0) & 0x400 or any(destination.iterdir()):
            raise ValueError("Private registry is nonempty or is a link; it was preserved")
        for name in files:
            target = destination.joinpath(*PurePosixPath(name).parts)
            target.parent.mkdir(parents=True, exist_ok=True)
            # Do not hardlink a job-writable file into the trusted operator seed.
            partial = None
            try:
                with tempfile.NamedTemporaryFile(prefix=".seed-partial-", dir=target.parent, delete=False) as output, (Path(source) / name).open("rb") as incoming:
                    partial = Path(output.name)
                    shutil.copyfileobj(incoming, output)
                # Atomic, no overwrite. Both links are private; neither points into the seed.
                os.link(partial, target)
            finally:
                if partial is not None:
                    partial.unlink(missing_ok=True)
        return {"status": "SEEDED", "files": len(files), "bytes": maximum_bytes,
                "seconds": time.monotonic() - started}
    except (OSError, ValueError, KeyError, TypeError) as exc:
        return {"status": "MISS", "reason": str(exc) if isinstance(exc, ValueError) else type(exc).__name__,
                "seconds": time.monotonic() - started}


def export_seed(registry_root, output, maximum_bytes):
    """Publish public downloads/index once; never include unpacked sources or auth."""
    registry_root, output = Path(registry_root), Path(output)
    if type(maximum_bytes) is not int or maximum_bytes <= 0 or output.exists():
        raise ValueError("Seed export requires a new output and positive byte budget")
    root_metadata = registry_root.lstat()
    if registry_root.is_symlink() or getattr(root_metadata, "st_file_attributes", 0) & 0x400:
        raise ValueError("Registry root links are forbidden")
    selected, total = {}, 0
    for kind in ("cache", "index"):
        folder = registry_root / kind
        if not folder.exists():
            continue
        if folder.is_symlink() or getattr(folder.lstat(), "st_file_attributes", 0) & 0x400:
            raise ValueError("Registry folder links are forbidden")
        for public in folder.iterdir():
            if not re.fullmatch(r"index\.crates\.io-[a-f0-9]{16}", public.name):
                continue
            if public.is_symlink() or getattr(public.lstat(), "st_file_attributes", 0) & 0x400:
                raise ValueError("Public registry links are forbidden")
            for path in public.rglob("*"):
                if path.is_symlink() or getattr(path.lstat(), "st_file_attributes", 0) & 0x400:
                    raise ValueError("Public registry payload links are forbidden")
                if not path.is_file():
                    continue
                relative = path.relative_to(registry_root).as_posix()
                if not public_path(relative):
                    raise ValueError("Unexpected file in the public registry namespace")
                payload = regular(path, maximum_bytes)
                selected[relative] = hashlib.sha256(payload).hexdigest()
                total += len(payload)
                if total > maximum_bytes:
                    raise ValueError("Public download seed exceeds its export budget")
    if not selected:
        raise ValueError("Public dependency cache is empty")
    output.mkdir(parents=True)
    for name in selected:
        target = output / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(registry_root / name, target)
    payload = (json.dumps({"schema": SEED_SCHEMA, "files": selected}, sort_keys=True, separators=(",", ":")) + "\n").encode()
    (output / "seed.json").write_bytes(payload)
    sha = hashlib.sha256(payload).hexdigest()
    verified_seed(output, sha, total)
    return {"manifest_sha256": sha, "bytes": total, "files": len(selected),
            "volume": "pdlc-cargo-seed-" + sha}


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    if argv[:1] == ["export"]:
        parser = argparse.ArgumentParser(description="Export only public Cargo downloads; no Docker/runtime writes")
        parser.add_argument("--registry", type=Path, required=True)
        parser.add_argument("--output", type=Path, required=True)
        parser.add_argument("--budget", type=int, required=True)
        args = parser.parse_args(argv[1:])
        print(json.dumps(export_seed(args.registry, args.output, args.budget), sort_keys=True))
        return 0
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--manifest-sha256", required=True)
    parser.add_argument("--bytes", type=int, required=True)
    parser.add_argument("--destination", type=Path, default=Path(DESTINATION))
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)
    report = materialize(args.source, args.destination, args.manifest_sha256, args.bytes)
    print("Forge dependency cache: " + json.dumps(report, sort_keys=True), file=sys.stderr, flush=True)
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if command:
        os.execvpe(command[0], command, os.environ)
    return 0 if report["status"] == "SEEDED" else 1


if __name__ == "__main__":
    raise SystemExit(main())
