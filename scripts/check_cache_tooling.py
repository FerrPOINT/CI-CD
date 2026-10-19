"""Check standalone cache artifacts without fetching a new SDK checkout."""

import fnmatch
import hashlib
import json
import re
from pathlib import Path

import tomllib


def check_build_scope(root):
    root = Path(root).resolve()
    manifest = json.loads((root / ".quality-gate.json").read_text(encoding="utf-8"))
    compiled = []
    for relative in ("runner_cache.rs", "task_delivery/postgres.rs"):
        owner = root / "backend/src" / relative
        owner_inputs = []
        for source in re.findall(r'include_(?:str|bytes)!\("([^"]+)"\)', owner.read_text(encoding="utf-8")):
            path = (owner.parent / source).resolve()
            # External Base helpers have their own pinned SDK input contract.
            # This guard covers product-local literal embedded script inputs.
            if path.is_relative_to(root) and path.relative_to(root).parts[0] == "scripts":
                owner_inputs.append(path.relative_to(root).as_posix())
        if not owner_inputs:
            raise ValueError("Embedded script owner has no reviewed product inputs: " + relative)
        compiled.extend(owner_inputs)
    artifact = next(gate for gate in manifest["gates"] if gate["id"] == "backend:build")
    for path in compiled:
        def matches(patterns):
            return any(fnmatch.fnmatchcase(path, pattern) for pattern in patterns)
        if not matches(manifest["components"]["backend"]) or matches(manifest["components"]["frontend"]):
            raise ValueError("Compiled cache input must select backend only: " + path)
        if not matches(artifact["inputs"]) or matches(artifact.get("exclude_inputs", [])):
            raise ValueError("Compiled cache input is absent from artifact identity: " + path)


def check(root):
    check_build_scope(root)
    directory = Path(root) / "scripts/vendor"
    receipt = json.loads((directory / "base-forge-cache-tooling.json").read_text(encoding="utf-8"))
    expected = {"base_forge_job_cache.py": "scripts/forge_job_cache.py",
                "base-build-cache-policy.json": "deploy/build-cache-policy.json"}
    if (receipt.get("schema") != "base-forge-cache-tooling/v1"
            or receipt.get("repository") != "https://github.com/FerrPOINT/services-base"
            or not re.fullmatch(r"[a-f0-9]{40}", receipt.get("revision", ""))
            or set(receipt.get("files", {})) != set(expected)):
        raise ValueError("Unknown exact cache tooling package")
    for name, source in expected.items():
        record = receipt["files"][name]
        if record.get("source") != source or hashlib.sha256((directory / name).read_bytes()).hexdigest() != record.get("sha256"):
            raise ValueError("Packaged cache tooling source/hash differs")
    policy = json.loads((directory / "base-build-cache-policy.json").read_text(encoding="utf-8"))
    if (policy.get("schema") != "base-build-cache-policy/v1"
            or type(policy.get("forge_seed_pool_bytes")) is not int
            or not 0 < policy["forge_seed_pool_bytes"] <= policy["download_pool_bytes"]):
        raise ValueError("Cache seed sub-budget differs from the shared pool")
    backend = Path(root) / "backend"
    root_lock = tomllib.loads((backend / "Cargo.lock").read_text(encoding="utf-8"))
    application = next(item for item in root_lock["package"] if item["name"] == "cicd-server")
    harness = tomllib.loads((backend / "tests/cache-protocol/Cargo.toml").read_text(encoding="utf-8"))
    for name, constraint in harness["dependencies"].items():
        version = constraint["version"] if isinstance(constraint, dict) else constraint
        dependency = next(item for item in application["dependencies"] if item == name or item.startswith(name + " "))
        candidates = [item["version"] for item in root_lock["package"] if item["name"] == name]
        actual = dependency.split()[1] if " " in dependency else (candidates[0] if len(candidates) == 1 else None)
        if version != "=" + str(actual):
            raise ValueError("Cache unit target dependency differs from the application lock: " + name)
    workspace = tomllib.loads((backend / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]
    if harness["package"]["edition"] != str(workspace["edition"]) or harness["package"]["rust-version"] != str(workspace["rust-version"]):
        raise ValueError("Cache unit target toolchain differs from the application")
    return receipt["revision"]


if __name__ == "__main__":
    print("Cache tooling package verified: " + check(Path(__file__).resolve().parents[1]))
