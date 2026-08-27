#!/usr/bin/env python3
"""Generate a CycloneDX-lite SBOM (THIRD_PARTY policy, CISA Minimum Elements).

Usage: python3 scripts/generate_sbom.py [--out docs/assets/sbom.json]
Sources: backend/Cargo.lock (cargo crates), frontend/package.json (npm deps).
The output is a flat component inventory with licenses where known; a full
CycloneDX 1.5 document can be produced by cargo-cyclonedx / cyclonedx-npm
later (Target), this satisfies the Minimum Elements floor today.
"""
from __future__ import annotations

import datetime
import json
import re
import sys
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


def main() -> int:
    components = cargo_components() + npm_components()
    doc = {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "serialNumber": "urn:uuid:00000000-0000-0000-0000-000000000000",
        "version": 1,
        "metadata": {
            "timestamp": datetime.datetime.now(datetime.timezone.utc)
            .isoformat(timespec="seconds")
            .replace("+00:00", "Z"),
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
    out = Path(sys.argv[sys.argv.index("--out") + 1]) if "--out" in sys.argv else OUT_DEFAULT
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(doc, indent=2) + "\n")
    print(f"SBOM: {len(components)} components -> {out.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
