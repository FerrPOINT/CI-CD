#!/usr/bin/env python3
"""Package the two released SQLx histories without rewriting applied checksums."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path


def project(backend: Path) -> dict[str, bytes]:
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


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", type=Path, default=Path(__file__).resolve().parents[1] / "backend")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    projected = project(args.backend)
    args.output.mkdir(parents=True, exist_ok=True)
    existing = {path.name for path in args.output.iterdir()}
    if existing - projected.keys():
        raise ValueError("output contains unexpected files; use an empty destination")
    for name, payload in projected.items():
        (args.output / name).write_bytes(payload)
    print(f"kubernetes-v1: {len(projected)} checksum-verified migrations")


if __name__ == "__main__":
    main()
