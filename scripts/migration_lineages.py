#!/usr/bin/env python3
"""Package the two released SQLx histories without rewriting applied checksums."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path


def project(backend: Path, *, input_format: str = "checkout") -> dict[str, bytes]:
    if input_format not in {"checkout", "git-blob"}:
        raise ValueError("unsupported migration input format")
    canonical = backend / "migrations"
    history = backend / "migration-lineages"
    manifest = json.loads((history / "manifest.json").read_text(encoding="utf-8"))
    if manifest["schemaVersion"] != 1:
        raise ValueError("unsupported migration lineage manifest")
    result: dict[str, bytes] = {}
    for lineage, directory in (("main", canonical), ("kubernetesV1", history / "kubernetes-v1")):
        for entry in manifest[lineage]:
            name = entry["file"]
            if Path(name).name != name or not name.endswith(".sql"):
                raise ValueError("invalid historical migration path")
            payload = (directory / name).read_bytes()
            if hashlib.sha384(payload).hexdigest() != entry["sha384"]:
                # Git text normalization can store LF while the released
                # checkout history is CRLF. Reconstruct only that declared
                # representation, and still require its exact released hash.
                if input_format == "git-blob" and b"\r" not in payload:
                    restored = payload.replace(b"\n", b"\r\n")
                    if hashlib.sha384(restored).hexdigest() == entry["sha384"]:
                        payload = restored
                if hashlib.sha384(payload).hexdigest() != entry["sha384"]:
                    raise ValueError(f"released migration checksum changed: {name}")
            version = int(name.split("_", 1)[0])
            if lineage == "main" and version >= 96:
                name = f"{version + 4:04d}_" + name.split("_", 1)[1]
            if name in result:
                raise ValueError(f"duplicate migration version: {name}")
            result[name] = payload
    # The canonical forward copies must be the exact released native SQL.
    for entry in manifest["kubernetesV1"]:
        name = entry["file"]
        forward = f"{int(name.split('_', 1)[0]) + 12:04d}_" + name.split("_", 1)[1]
        if (canonical / forward).read_bytes() != result[name]:
            raise ValueError(f"native forward migration differs: {forward}")
    for source in sorted(canonical.glob("*.sql")):
        if int(source.name.split("_", 1)[0]) >= 112:
            result[source.name] = source.read_bytes()
    versions = [int(name.split("_", 1)[0]) for name in result]
    if len(set(versions)) != len(versions):
        raise ValueError("migration lineage contains duplicate versions")
    # Released histories contain intentional numbering gaps; preserve them.
    if {path.name for path in canonical.glob("*.sql")} != {
        entry["file"] for entry in manifest["main"]
    } | {
        f"{int(entry['file'].split('_', 1)[0]) + 12:04d}_" + entry["file"].split("_", 1)[1]
        for entry in manifest["kubernetesV1"]
    } | {name for name in result if int(name.split("_", 1)[0]) >= 112}:
        raise ValueError("canonical migration inventory differs from released histories")
    return result


def reviewer_v1(backend: Path, *, input_format: str = "checkout") -> dict[str, bytes]:
    """Retain the installed Reviewer 0116; shared forward history starts at 0118."""
    projected = project(backend, input_format=input_format)
    result = {p.name: p.read_bytes() for p in (backend / "migrations").glob("*.sql")}
    manifest = json.loads((backend / "migration-lineages/manifest.json").read_text(encoding="utf-8"))
    for entry in manifest["main"]:
        name = entry["file"]
        version = int(name.split("_", 1)[0])
        projected_name = f"{version + 4:04d}_" + name.split("_", 1)[1] if version >= 96 else name
        result[name] = projected[projected_name]
    retained = {}
    for entry in manifest["reviewerV1"]:
        source, target = entry["canonicalFile"], entry["file"]
        if any(Path(name).name != name or not name.endswith(".sql") for name in (source, target)):
            raise ValueError("invalid Reviewer migration path")
        payload = result.pop(source)
        if input_format == "git-blob" and b"\r" not in payload:
            restored = payload.replace(b"\n", b"\r\n")
            if hashlib.sha384(restored).hexdigest() == entry["sha384"]:
                payload = restored
        if hashlib.sha384(payload).hexdigest() != entry["sha384"]:
            raise ValueError(f"released Reviewer migration checksum changed: {target}")
        retained[target] = payload
    result.update(retained)
    versions = [int(name.split("_", 1)[0]) for name in result]
    if len(versions) != len(set(versions)):
        raise ValueError("Reviewer migration lineage contains duplicate versions")
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", type=Path, default=Path(__file__).resolve().parents[1] / "backend")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--input-format", choices=("checkout", "git-blob"), default="checkout")
    parser.add_argument("--lineage", choices=("kubernetes-v1", "reviewer-v1"), default="kubernetes-v1")
    args = parser.parse_args()
    publisher = reviewer_v1 if args.lineage == "reviewer-v1" else project
    projected = publisher(args.backend, input_format=args.input_format)
    args.output.mkdir(parents=True, exist_ok=True)
    existing = {path.name for path in args.output.iterdir()}
    if existing - projected.keys():
        raise ValueError("output contains unexpected files; use an empty destination")
    for name, payload in projected.items():
        (args.output / name).write_bytes(payload)
    print(f"{args.lineage}: {len(projected)} checksum-verified migrations")


if __name__ == "__main__":
    main()
