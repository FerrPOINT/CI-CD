"""Unconnected pure owner manifest assembly. Not a runner or permission producer."""

import copy
from dataclasses import dataclass, field
import hashlib
import ipaddress
import json
import re
from typing import Mapping
from uuid import UUID

from scripts.workspace_deploy_admission import require, validate_recipe


@dataclass(frozen=True, repr=False)
class OwnerRuntime:
    job_id: UUID
    attempt_id: UUID
    control_volume: str
    subnet: ipaddress.IPv4Network
    network_internal: bool
    users: Mapping[str, str]
    tmpfs: Mapping[str, frozenset[str]]
    image_volumes: Mapping[str, frozenset[str]]
    secret_values: Mapping[str, str]
    memory_bytes: int
    cpus: float
    pids: int


@dataclass(frozen=True)
class OwnerComposeSource:
    """Private backend projection of VerifiedComposeSource, not request authority."""
    job_id: UUID
    attempt_id: UUID
    source_commit: str
    compose_path: str
    blob_oid: str
    sha256: str
    plan_sha256: str


def verify_compose_source(compose_bytes, seal, owner, source):
    require(type(source) is OwnerComposeSource and type(owner) is OwnerRuntime,
            "owner Git source projection required")
    require(source.job_id == owner.job_id and source.attempt_id == owner.attempt_id,
            "Compose source attempt mismatch")
    require(type(source.source_commit) is str and type(source.plan_sha256) is str
            and source.source_commit == seal.source_commit
            and re.fullmatch(r"[0-9a-f]{40}", source.source_commit)
            and re.fullmatch(r"[0-9a-f]{64}", source.plan_sha256),
            "Compose source commit/plan mismatch")
    require(type(source.compose_path) is str
            and not any(ord(char) < 32 or 127 <= ord(char) <= 159
                        or 0xD800 <= ord(char) <= 0xDFFF for char in source.compose_path)
            and len(source.compose_path.encode("utf-8")) <= 1024
            and not any(char in source.compose_path for char in "\\:")
            and all(part and part not in (".", "..") and not part.startswith("-")
                    for part in source.compose_path.split("/")), "Compose source path")
    require(type(compose_bytes) is bytes and 0 < len(compose_bytes) <= 256 * 1024,
            "Compose source byte bound")
    require(source.sha256 == seal.compose_sha256 == hashlib.sha256(compose_bytes).hexdigest()
            and source.blob_oid == hashlib.sha1(
                b"blob " + str(len(compose_bytes)).encode("ascii") + b"\0" + compose_bytes,
                usedforsecurity=False).hexdigest(), "Compose source bytes differ from Git blob")


@dataclass(frozen=True)
class PrivateServiceFragment:
    _document: dict = field(repr=False)

    def owner_document(self):
        """Private control only; RemoteJob.save_manifest must escape literals once."""
        return copy.deepcopy(self._document)


def resolve_services(compose_bytes, manifest_bytes, seal, owner, source):
    """Requires actual owner readback inputs; performs no IO or lifecycle effects."""
    verify_compose_source(compose_bytes, seal, owner, source)
    validated = validate_recipe(compose_bytes, manifest_bytes, seal)
    compose = json.loads(compose_bytes)
    require(type(owner) is OwnerRuntime and type(owner.job_id) is UUID
            and type(owner.attempt_id) is UUID and owner.job_id.int and owner.attempt_id.int,
            "owner execution identity")
    require(type(owner.control_volume) is str
            and re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,127}", owner.control_volume),
            "owner control volume identity")
    require(type(owner.subnet) is ipaddress.IPv4Network
            and 16 <= owner.subnet.prefixlen <= 28
            and any(owner.subnet.subnet_of(ipaddress.ip_network(pool))
                    for pool in ("10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16"))
            and type(owner.network_internal) is bool, "owner allocated network")
    names = set(validated.service_names)
    require(not names & {"job", "transfer"}
            and not set(validated.volume_names) & {"workspace", "cargo", "sources"},
            "reserved RemoteJob resource names")
    require(set(owner.users) == set(owner.tmpfs) == set(owner.image_volumes) == names,
            "complete owner service policy required")
    services = compose["services"]
    require(type(owner.memory_bytes) is int and type(owner.cpus) in (int, float)
            and type(owner.pids) is int
            and 0 < sum(item["mem_limit"] for item in services.values()) <= owner.memory_bytes <= 4 * 1024**3
            and 0 < sum(item["cpus"] for item in services.values()) <= owner.cpus <= 4
            and 0 < sum(item["pids_limit"] for item in services.values()) <= owner.pids <= 2048,
            "owner resource quota")
    project = "sdlc-build-job-" + owner.attempt_id.hex
    labels = {"sdlc.task": "job-" + str(owner.job_id), "sdlc.purpose": "cicd-job",
              "sdlc.job": str(owner.job_id), "sdlc.attempt": str(owner.attempt_id),
              "sdlc.control-volume": owner.control_volume}
    refs = {(service, key): secret for service, key, secret in validated.secret_refs}
    for name, service in services.items():
        user = owner.users[name]
        require(type(user) is str and re.fullmatch(r"[1-9][0-9]{0,9}:[1-9][0-9]{0,9}", user)
                and all(int(value) <= 2_147_483_647 for value in user.split(":")),
                "owner nonroot UID/GID")
        paths = owner.tmpfs[name]
        require(type(paths) is frozenset and len(paths) <= 4
                and all(type(path) is str and re.fullmatch(r"/[A-Za-z0-9_/-]+", path)
                        and all(part and part not in (".", "..") for part in path[1:].split("/"))
                        and not any(path == root or path.startswith(root + "/")
                                    for root in ("/proc", "/sys", "/dev")) for path in paths),
                "owner tmpfs policy")
        mounts = {mount["target"] for mount in service.get("volumes", [])}
        require(not mounts & paths and not any(
            a != b and (a.startswith(b + "/") or b.startswith(a + "/"))
            for a in mounts | paths for b in mounts | paths), "overlapping owner mount targets")
        require(type(owner.image_volumes[name]) is frozenset
                and owner.image_volumes[name] <= mounts, "implicit image volume not declared")
        require(service.get("read_only", True) is True, "writable root filesystem unsupported")
        environment = service.get("environment", {})
        for key in environment:
            if (name, key) in refs:
                value = owner.secret_values.get(refs[name, key])
                require(type(value) is str and len(value) <= 16 * 1024 and "\0" not in value,
                        "declared project secret value unavailable")
                environment[key] = value
        service.update(environment=environment, user=user, read_only=True,
                       cap_drop=["ALL"], security_opt=["no-new-privileges:true"],
                       pull_policy="never", restart="no", networks=["job"],
                       container_name=project + "-" + name,
                       labels=dict(labels),
                       tmpfs=[path + ":rw,noexec,nosuid,size=16m" for path in sorted(paths)])
        for mount in service.get("volumes", []):
            mount["volume"] = {"nocopy": True}
    volumes = {name: {"name": project + "_" + name, "labels": dict(labels)}
               for name in validated.volume_names}
    network = {"name": project + "_job", "internal": owner.network_internal,
               "labels": dict(labels), "ipam": {"config": [{"subnet": str(owner.subnet)}]}}
    return PrivateServiceFragment({"name": project, "services": services,
                                   "volumes": volumes, "networks": {"job": network}})
