"""Unconnected pure owner manifest assembly. Not a runner or permission producer."""

import copy
from dataclasses import dataclass, field
import hashlib
import ipaddress
import json
import re
from typing import Mapping
from uuid import UUID

from scripts.workspace_deploy_admission import OwnerSeal, require, validate_recipe


@dataclass(frozen=True, repr=False)
class OwnerImageFacts:
    image_id: str
    default_user: str | None
    volumes: frozenset[str]
    os: str
    architecture: str


@dataclass(frozen=True, repr=False)
class OwnerRuntime:
    job_id: UUID
    attempt_id: UUID
    control_volume: str
    subnet: ipaddress.IPv4Network | None
    network_internal: bool
    users: Mapping[str, str]
    tmpfs: Mapping[str, frozenset[str]]
    image_facts: Mapping[str, OwnerImageFacts]
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
    require(type(owner) is OwnerRuntime, "owner Git source projection required")
    _verify_source_bytes(compose_bytes, seal, source, owner.job_id, owner.attempt_id)


def _verify_source_bytes(compose_bytes, seal, source, job_id, attempt_id):
    require(type(source) is OwnerComposeSource,
            "owner Git source projection required")
    require(source.job_id == job_id and source.attempt_id == attempt_id,
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


def _actual_image(name, facts, seal):
    require(type(facts) is OwnerImageFacts and facts.image_id == seal.images[name]
            and facts.os == "linux" and facts.architecture in ("amd64", "arm64")
            and type(facts.volumes) is frozenset and len(facts.volumes) <= 8
            and (facts.default_user is None or (type(facts.default_user) is str
                 and len(facts.default_user) <= 128
                 and not any(ord(c) < 32 or 127 <= ord(c) <= 159 for c in facts.default_user))),
            "actual owner image Config facts required")


def _inventory_input(payload):
    compose = payload["compose"].encode("utf-8")
    manifest = payload["manifest"].encode("utf-8")
    data = payload["seal"]
    seal = OwnerSeal(**{**data, "environment": {name: frozenset(keys)
                        for name, keys in data["environment"].items()},
                        "secret_names": frozenset(data["secret_names"])})
    data = payload["source"]
    source = OwnerComposeSource(**{**data, "job_id": UUID(data["job_id"]),
                                  "attempt_id": UUID(data["attempt_id"])})
    _verify_source_bytes(compose, seal, source, UUID(payload["job_id"]), UUID(payload["attempt_id"]))
    validate_recipe(compose, manifest, seal)
    return compose, manifest, seal, source


def read_only_inventory(payload):
    """Private read-only inspection; never an execution permit."""
    compose, manifest, seal, _ = _inventory_input(payload)
    validated = validate_recipe(compose, manifest, seal)
    facts = payload["image_facts"]
    require(set(facts) == set(validated.service_names), "incomplete image Config inventory")
    recipe = json.loads(compose)
    preparation_needed = False
    for name, data in facts.items():
        require(type(data["volumes"]) is list and len(data["volumes"]) == len(set(data["volumes"])),
                "invalid image volume projection")
        image = OwnerImageFacts(**{**data, "volumes": frozenset(data["volumes"])})
        _actual_image(name, image, seal)
        mounts = {mount["target"] for mount in recipe["services"][name].get("volumes", [])}
        require(image.volumes <= mounts, "implicit image volume not declared")
        preparation_needed |= bool(mounts) or not (
            type(image.default_user) is str
            and re.fullmatch(r"[1-9][0-9]{0,9}:[1-9][0-9]{0,9}", image.default_user)
            and all(int(value) <= 2_147_483_647 for value in image.default_user.split(":")))
    reason = ("owner_volume_user_preparation_unavailable" if preparation_needed
              else "project_secret_readback_unavailable" if validated.secret_refs
              else "owner_network_lifecycle_unavailable")
    return {"status": "held", "reason": reason}


def prepare_deployment(payload):
    """Render only from backend-owned source/image/secret projections.

    RemoteJob still must prepare actual owned volumes, validate create/start,
    and perform task/lease checks. This result is not a caller-supplied ACK.
    """
    compose, manifest, seal, source = _inventory_input(payload)
    recipe = json.loads(compose)
    facts = {name: OwnerImageFacts(**{**data, "volumes": frozenset(data["volumes"])})
             for name, data in payload["image_facts"].items()}
    users = {name: service.get("user", facts[name].default_user)
             for name, service in recipe["services"].items()}
    # None requests reuse of the EXISTING RemoteJob-owned job network; no subnet proof is invented.
    owner = OwnerRuntime(source.job_id, source.attempt_id, payload["control_volume"], None, False,
                         users, {name: frozenset(("/tmp", "/run")) for name in users}, facts,
                         payload["project_secret_values"], 4 * 1024**3, 4, 2048)
    fragment = resolve_services(compose, manifest, seal, owner, source).owner_document()
    volume_users = {}
    for service in fragment["services"].values():
        for mount in service.get("volumes", []):
            previous = volume_users.setdefault(mount["source"], service["user"])
            require(previous == service["user"], "shared volume has conflicting explicit users")
    require(set(volume_users) == set(fragment["volumes"]), "unused disposable volume")
    rendered = json.dumps(fragment, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    return {"status": "prepared", "rendered": rendered, "volume_users": volume_users}


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
    require((owner.subnet is None or (type(owner.subnet) is ipaddress.IPv4Network
            and 16 <= owner.subnet.prefixlen <= 28
            and any(owner.subnet.subnet_of(ipaddress.ip_network(pool))
                    for pool in ("10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16"))))
            and type(owner.network_internal) is bool, "owner allocated network")
    names = set(validated.service_names)
    require(not names & {"job", "transfer"}
            and not set(validated.volume_names) & {"workspace", "cargo", "sources"},
            "reserved RemoteJob resource names")
    require(set(owner.users) == set(owner.tmpfs) == set(owner.image_facts) == names,
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
        facts = owner.image_facts[name]
        _actual_image(name, facts, seal)
        user = owner.users[name]
        require(type(user) is str and re.fullmatch(r"[1-9][0-9]{0,9}:[1-9][0-9]{0,9}", user)
                and all(int(value) <= 2_147_483_647 for value in user.split(":")),
                "owner nonroot UID/GID")
        require(user == service.get("user", facts.default_user),
                "image default user requires explicit trusted preparation; no inferred UID")
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
        require(facts.volumes <= mounts, "implicit image volume not declared")
        require(service.get("read_only", True) is True, "writable root filesystem unsupported")
        environment = service.get("environment", {})
        for key in environment:
            if (name, key) in refs:
                value = owner.secret_values.get(refs[name, key])
                require(type(value) is str and len(value) <= 16 * 1024 and "\0" not in value,
                        "declared project secret value unavailable")
                environment[key] = value
        service.pop("x-forge-image-build", None)
        service.update(image=seal.images[name], environment=environment, user=user, read_only=True,
                       cap_drop=["ALL"], security_opt=["no-new-privileges:true"],
                       pull_policy="never", restart="no", networks=["job"],
                       container_name=project + "-" + name,
                       labels=dict(labels),
                       tmpfs=[path + ":rw,noexec,nosuid,mode=1777,size=16m" for path in sorted(paths)])
        for mount in service.get("volumes", []):
            mount["volume"] = {"nocopy": True}
    volumes = {name: {"name": project + "_" + name, "labels": dict(labels)}
               for name in validated.volume_names}
    network = {"name": project + "_job", "internal": owner.network_internal, "labels": dict(labels)}
    if owner.subnet is not None:
        network["ipam"] = {"config": [{"subnet": str(owner.subnet)}]}
    return PrivateServiceFragment({"name": project, "services": services,
                                   "volumes": volumes, "networks": {"job": network}})
