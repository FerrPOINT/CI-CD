"""Export the real runner/workspace boundary without compiling the unchanged control plane."""

import argparse
import hashlib
import json
import subprocess
from pathlib import Path

import tomllib

DEPENDENCIES = (
    "anyhow",
    "axum",
    "chrono",
    "clap",
    "libc",
    "reqwest",
    "serde",
    "serde_json",
    "thiserror",
    "tokio",
    "uuid",
)


def value(item):
    if isinstance(item, dict):
        return (
            "{"
            + ", ".join(json.dumps(k) + " = " + value(v) for k, v in item.items())
            + "}"
        )
    return json.dumps(item)


def check_lock(root):
    root = Path(root)
    original = tomllib.loads((root / "source-lock.toml").read_text())["package"]
    selected = tomllib.loads((root / "Cargo.lock").read_text())["package"]
    identities = {
        (p["name"], p["version"], p.get("source"), p.get("checksum")) for p in original
    }
    for package in selected:
        if package["name"] == "forge-runner-standalone" and not package.get("source"):
            continue
        identity = (
            package["name"],
            package["version"],
            package.get("source"),
            package.get("checksum"),
        )
        if identity not in identities:
            raise ValueError(
                "Standalone resolution changed a pinned package: " + package["name"]
            )
    return {
        "status": "PASS",
        "packages": len(selected),
        "dependency_versions_changed": False,
    }


def prepare(repository, revision, output):
    repository = Path(repository).resolve()
    output = Path(output).resolve()
    if output.exists():
        raise ValueError("Export target already exists")

    def blob(path):
        return subprocess.check_output(
            ["git", "-C", str(repository), "show", revision + ":" + path]
        )

    manifest = tomllib.loads(blob("backend/Cargo.toml").decode())
    lock_bytes = blob("backend/Cargo.lock")
    packages = tomllib.loads(lock_bytes.decode())["package"]
    root = next(p for p in packages if p["name"] == manifest["package"]["name"])
    domain_manifest = tomllib.loads(blob("backend/domain/Cargo.toml").decode())
    domain_package = next(p for p in packages if p["name"] == "cicd-domain")
    dependencies = {}
    names = (*DEPENDENCIES, "sha1", "sha2", "utoipa", "url")
    for name in names:
        owner = root if name in manifest["dependencies"] else domain_package
        spec = manifest["dependencies"].get(
            name, domain_manifest["dependencies"].get(name)
        )
        if isinstance(spec, dict) and spec.get("workspace"):
            spec = manifest["workspace"]["dependencies"][name]
        spec = {"version": spec} if isinstance(spec, str) else dict(spec)
        matches = [
            p
            for p in packages
            if p["name"] == spec.get("package", name)
            and (
                name in owner["dependencies"]
                or name + " " + p["version"] in owner["dependencies"]
            )
        ]
        if len(matches) != 1:
            raise ValueError("Ambiguous original dependency: " + name)
        spec["version"] = "=" + matches[0]["version"]
        dependencies[name] = spec
    runner = blob("backend/src/bin/forge-runner.rs")
    workspace = blob("backend/src/runner_workspace.rs")
    output.mkdir(parents=True)
    (output / "src").mkdir()
    (output / "src/main.rs").write_bytes(
        b"extern crate self as cicd;\npub mod runner_workspace;\npub use cicd_domain as domain;\n"
        + runner
    )
    (output / "src/runner_workspace.rs").write_bytes(workspace)
    paths = subprocess.check_output(
        ["git", "-C", str(repository), "ls-tree", "-r", "--name-only", revision],
        text=True,
    ).splitlines()
    hashes = {}
    for source in paths:
        if source.startswith(("backend/domain/", "backend/src/runner_workspace/")):
            relative = source.removeprefix("backend/")
            target = output / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            data = blob(source)
            target.write_bytes(data)
            hashes[source] = hashlib.sha256(data).hexdigest()
    (output / "LICENSE").write_bytes(blob("backend/LICENSE"))
    (output / "source-lock.toml").write_bytes(lock_bytes)
    (output / "Cargo.lock").write_bytes(lock_bytes)
    version = manifest["package"]["version"]
    if isinstance(version, dict) and version.get("workspace"):
        version = manifest["workspace"]["package"]["version"]
    cargo = (
        '[package]\nname = "forge-runner-standalone"\nversion = '
        + value(version)
        + '\nedition = "2024"\nrust-version = "1.88"\n\n[workspace]\nmembers = ["domain"]\n\n[workspace.package]\nversion = '
        + value(version)
        + '\nedition = "2024"\nrust-version = "1.88"\nlicense-file = "LICENSE"\nrepository = '
        + value(manifest["workspace"]["package"]["repository"])
        + "\n\n[workspace.dependencies]\n"
    )
    cargo += "".join(
        name + " = " + value(spec) + "\n" for name, spec in dependencies.items()
    )
    cargo += '\n[[bin]]\nname = "forge-runner"\npath = "src/main.rs"\n\n[dependencies]\ncicd-domain = {path = "domain"}\n'
    cargo += "".join(
        name + " = " + value(spec) + "\n"
        for name, spec in dependencies.items()
        if name not in {"utoipa", "url"}
    )
    cargo += "\n[profile.release]\ndebug = 0\ncodegen-units = 16\nlto = false\n"
    (output / "Cargo.toml").write_text(cargo)
    receipt = {
        "source_revision": revision,
        "runner_source_sha256": hashlib.sha256(runner).hexdigest(),
        "workspace_source_sha256": hashlib.sha256(workspace).hexdigest(),
        "workspace_export_sha256": hashlib.sha256(workspace).hexdigest(),
        "supporting_source_hashes": hashes,
        "recipe": "standalone-runner-and-owned-workspace-v1",
        "omitted_module": "unchanged control-plane HTTP, database and embedded runner modules only",
    }
    (output / "source-receipt.json").write_text(json.dumps(receipt, indent=2))
    return receipt


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository")
    parser.add_argument("--revision")
    parser.add_argument("--output", required=True)
    parser.add_argument("--check-lock", action="store_true")
    args = parser.parse_args()
    result = (
        check_lock(args.output)
        if args.check_lock
        else prepare(args.repository, args.revision, args.output)
    )
    print(json.dumps(result))
