"""Pure closed-recipe validation, not an executor, sandbox or authority producer.

The seal is a trusted owner projection, never deserialized from job/agent input.
No secrets are resolved and no Docker, filesystem or subprocess IO is performed.
Only JSON Compose documents are supported in this initial validation-only slice.
"""

from dataclasses import dataclass
import hashlib
import json
import math
import re
from typing import Mapping


MAX_BYTES = 256 * 1024
NAME = re.compile(r"[a-z][a-z0-9_-]{0,62}\Z", re.ASCII)
ENV_NAME = re.compile(r"[A-Z][A-Z0-9_]{0,127}\Z", re.ASCII)
IMAGE = re.compile(r"sha256:[a-f0-9]{64}\Z", re.ASCII)
COMMIT = re.compile(r"[a-f0-9]{40}\Z", re.ASCII)
SECRET = re.compile(r"\$\{([A-Z][A-Z0-9_]{0,127})\}\Z", re.ASCII)


class Rejected(ValueError):
    """Safe diagnostic; never includes repository values or protected secrets."""


@dataclass(frozen=True)
class OwnerSeal:
    source_commit: str
    compose_sha256: str
    images_manifest_sha256: str
    images: Mapping[str, str]
    environment: Mapping[str, frozenset[str]]
    secret_names: frozenset[str]


@dataclass(frozen=True)
class ValidatedRecipe:
    compose_sha256: str
    images_manifest_sha256: str
    service_names: tuple[str, ...]
    volume_names: tuple[str, ...]
    secret_refs: tuple[tuple[str, str, str], ...]


def require(condition, reason):
    if not condition:
        raise Rejected(reason)


def _pairs(items):
    result = {}
    for name, value in items:
        require(name not in result, "duplicate JSON key")
        result[name] = value
    return result


def _constant(_):
    raise Rejected("nonfinite JSON number")


def _document(data):
    require(type(data) is bytes and 0 < len(data) <= MAX_BYTES, "document size")
    try:
        value = json.loads(data.decode("utf-8"), object_pairs_hook=_pairs,
                           parse_constant=_constant)
    except (ValueError, UnicodeError, RecursionError):
        raise Rejected("invalid closed JSON document") from None
    pending = [(value, 0)]
    while pending:
        item, depth = pending.pop()
        require(depth <= 16, "document nesting")
        if type(item) is dict:
            pending.extend((v, depth + 1) for v in item.values())
        elif type(item) is list:
            pending.extend((v, depth + 1) for v in item)
        elif type(item) is str:
            require(not any(0xD800 <= ord(c) <= 0xDFFF for c in item), "invalid Unicode scalar")
    return value


def _fields(value, required, optional=()):
    require(type(value) is dict and set(required) <= value.keys()
            and value.keys() <= set(required) | set(optional), "unsupported recipe fields")


def _mounts(mounts, volumes):
    require(type(mounts) is list and len(mounts) <= 8, "mount bound")
    targets = set()
    for mount in mounts:
        _fields(mount, ("type", "source", "target"), ("read_only",))
        require(mount["type"] == "volume" and type(mount["source"]) is str
                and mount["source"] in volumes, "only declared disposable volumes")
        target = mount["target"]
        require(type(target) is str and 1 < len(target) <= 256 and target.startswith("/")
                and not any(c in target for c in ("\\", "$", ":"))
                and all(part and part not in (".", "..") for part in target[1:].split("/"))
                and not any(ord(c) < 32 or ord(c) == 127 for c in target), "mount target")
        require(not any(target == root or target.startswith(root + "/")
                        for root in ("/proc", "/sys", "/dev", "/run", "/var/run"))
                and not target.endswith(".sock") and target not in targets, "unsafe mount target")
        require(type(mount.get("read_only", False)) is bool, "mount read-only type")
        targets.add(target)


def _environment(name, values, seal):
    require(type(values) is dict and len(values) <= 32, "environment bound")
    refs = []
    for key, value in values.items():
        require(ENV_NAME.fullmatch(key) and key in seal.environment.get(name, ())
                and not key.startswith(("DOCKER_", "CICD_RUNNER_")), "environment key not permitted")
        require(type(value) is str and len(value) <= 4096
                and not any(ord(c) < 32 or ord(c) == 127 for c in value), "environment value")
        secret = SECRET.fullmatch(value)
        if secret:
            require(secret[1] in seal.secret_names, "secret reference not permitted")
            refs.append((name, key, secret[1]))
        else:
            require("$" not in value, "unsupported environment interpolation")
    return refs


def _dependencies(services):
    remaining = set(services)
    while remaining:
        ready = {name for name in remaining
                 if not set(services[name].get("depends_on", [])) & remaining}
        require(ready, "cyclic service dependencies")
        remaining -= ready


def validate_recipe(compose_bytes, images_manifest_bytes, seal):
    """Compare untrusted bytes with owner-sealed inputs; never grants execution."""
    require(type(seal) is OwnerSeal and COMMIT.fullmatch(seal.source_commit), "owner source pin")
    compose = _document(compose_bytes)
    manifest = _document(images_manifest_bytes)
    compose_hash = hashlib.sha256(compose_bytes).hexdigest()
    manifest_hash = hashlib.sha256(images_manifest_bytes).hexdigest()
    require(compose_hash == seal.compose_sha256
            and manifest_hash == seal.images_manifest_sha256, "sealed bytes mismatch")
    _fields(manifest, ("schema", "sourceCommit", "images"))
    require(manifest["schema"] == "forge/workspace-images/v1"
            and manifest["sourceCommit"] == seal.source_commit, "manifest source binding")
    _fields(compose, ("services",), ("volumes",))
    services, volumes = compose["services"], compose.get("volumes", {})
    require(type(services) is dict and 1 <= len(services) <= 8, "service bound")
    require(type(volumes) is dict and len(volumes) <= 8, "volume bound")
    require(all(NAME.fullmatch(name) and definition == {} and type(definition) is dict
                for name, definition in volumes.items()), "only owned default-driver volumes")
    require(type(manifest["images"]) is dict and set(services) == set(manifest["images"])
            and manifest["images"] == dict(seal.images), "sealed service image registry")
    total_memory, total_cpu, total_pids = 0, 0.0, 0
    refs = []
    for name, service in services.items():
        require(NAME.fullmatch(name), "service name")
        _fields(service, ("image", "mem_limit", "cpus", "pids_limit"),
                ("environment", "volumes", "depends_on", "read_only"))
        image = service["image"]
        require(type(image) is str and IMAGE.fullmatch(image)
                and image == seal.images[name], "only exact sealed image IDs")
        memory, cpu, pids = service["mem_limit"], service["cpus"], service["pids_limit"]
        require(type(memory) is int and 16 * 1024**2 <= memory <= 2 * 1024**3, "memory bound")
        require(type(cpu) in (int, float) and 0.1 <= cpu <= 2 and math.isfinite(cpu), "CPU bound")
        require(type(pids) is int and 16 <= pids <= 512, "PID bound")
        require(type(service.get("read_only", False)) is bool, "read-only type")
        total_memory += memory
        total_cpu += cpu
        total_pids += pids
        depends = service.get("depends_on", [])
        require(type(depends) is list and len(depends) <= 7
                and all(type(item) is str and item in services and item != name for item in depends)
                and len(set(depends)) == len(depends), "service dependencies")
        _mounts(service.get("volumes", []), volumes)
        refs.extend(_environment(name, service.get("environment", {}), seal))
    require(total_memory <= 4 * 1024**3 and total_cpu <= 4 and total_pids <= 2048,
            "aggregate resource bound")
    _dependencies(services)
    return ValidatedRecipe(compose_hash, manifest_hash, tuple(sorted(services)),
                           tuple(sorted(volumes)), tuple(sorted(refs)))
