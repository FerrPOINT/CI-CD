#!/usr/bin/env python3
"""Generate a CycloneDX-lite SBOM (THIRD_PARTY policy, CISA Minimum Elements).

Usage: python3 scripts/generate_sbom.py [--out docs/assets/sbom.json] [--check]
Sources: backend/Cargo.lock (cargo crates), frontend/package.json (npm deps).
The output is a flat component inventory with licenses where known; a full
CycloneDX 1.5 document can be produced by cargo-cyclonedx / cyclonedx-npm
later (Target), this satisfies the Minimum Elements floor today.
"""
from __future__ import annotations

import argparse
import datetime
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT_DEFAULT = ROOT / "docs/assets/sbom.json"

NPM_LICENSE_OVERRIDES = {
    # pnpm license fields can be noisy; keep explicit decisions here.
}


def cargo_components() -> list[dict]:
    lock = (ROOT / "backend/Cargo.lock").read_text()
    blocks = re.split(r"\n\n", lock)
    out = []
    for block in blocks:
        m_name = re.search(r'name = "([^"]+)"', block)
        m_ver = re.search(r'version = "([^"]+)"', block)
        if not (m_name and m_ver):
            continue
        name, version = m_name.group(1), m_ver.group(1)
        if name in ("cicd-server", "cicd-domain", "cicd-migrate", "cicd-cli"):
            continue  # first-party
        out.append(
            {
                "type": "library",
                "name": name,
                "version": version,
                "purl": f"pkg:cargo/{name}@{version}",
                "license": "unknown",
            }
        )
    return out


def npm_components() -> list[dict]:
    pkg = json.loads((ROOT / "frontend/package.json").read_text())
    out = []
    for section in ("dependencies", "devDependencies"):
        for name, spec in pkg.get(section, {}).items():
            version = re.sub(r"[\^~>=< ]", "", spec.split("||")[0]) or "unknown"
            out.append(
                {
                    "type": "library",
                    "name": name,
                    "version": version,
                    "purl": f"pkg:npm/{name}@{version}",
                    "license": NPM_LICENSE_OVERRIDES.get(name, "unknown"),
                    "scope": "required" if section == "dependencies" else "development",
                }
            )
    return out


def build_document(timestamp: str | None = None) -> dict:
    components = cargo_components() + npm_components()
    if timestamp is None:
        timestamp = (
            datetime.datetime.now(datetime.timezone.utc)
            .isoformat(timespec="seconds")
            .replace("+00:00", "Z")
        )
    return {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "serialNumber": "urn:uuid:00000000-0000-0000-0000-000000000000",
        "version": 1,
        "metadata": {
            "timestamp": timestamp,
            "component": {
                "type": "application",
                "name": "forge-ci-cd",
                "version": "0.1.0",
            },
            "properties": [
                {"name": "forge:sbom:note", "value": "CycloneDX-lite (CISA Minimum Elements); licenses marked unknown pending full toolchain"}
            ],
        },
        "components": components,
    }


def render(doc: dict) -> str:
    return json.dumps(doc, indent=2) + "\n"


def display_path(path: Path) -> str:
    try:
        return path.relative_to(ROOT).as_posix()
    except ValueError:
        return str(path)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=Path, default=OUT_DEFAULT)
    parser.add_argument("--check", action="store_true", help="fail if the committed SBOM is stale")
    args = parser.parse_args()

    out = args.out
    if not out.is_absolute():
        out = ROOT / out

    if args.check:
        if not out.exists():
            print(f"SBOM drift: {display_path(out)} is missing; run scripts/generate_sbom.py")
            return 1
        try:
            current = json.loads(out.read_text(encoding="utf-8"))
        except json.JSONDecodeError as error:
            print(f"SBOM drift: {display_path(out)} is invalid JSON: {error}")
            return 1
        timestamp = current.get("metadata", {}).get("timestamp")
        expected = build_document(timestamp if isinstance(timestamp, str) else None)
        expected_text = render(expected)
        current_text = render(current)
        if current_text != expected_text:
            print(f"SBOM drift: {display_path(out)} is stale; run scripts/generate_sbom.py")
            return 1
        print(f"SBOM: {display_path(out)} is up to date")
        return 0

    doc = build_document()
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(render(doc), encoding="utf-8")
    print(f"SBOM: {len(doc['components'])} components -> {display_path(out)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
