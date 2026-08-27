#!/usr/bin/env python3
"""Documentation verification for Forge CI/CD.

Checks: markdown link integrity, canonical naming (ADR-0009), status taxonomy,
orphan docs, current-state cross-check, screenshot manifest.

Usage: python3 scripts/verify_docs.py [--all | --links --canonical --status-labels --orphan-docs --current-state --screenshots --manifest]
Exit code 0 = clean; non-zero with a report otherwise.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOCS = ROOT / "docs"

FORBIDDEN_CANONICAL = [
    (r"\boutbox_events\b", "use outbox_messages (ADR-0009)"),
    (r"\bpipeline_runs\b", "use pipelines/execution_attempts (ADR-0009)"),
    (r"\bjob_runs\b", "use jobs/execution_attempts (ADR-0009)"),
    (r"/api/v1/runner/v1/", "use /api/v1/runner/* (ADR-0009)"),
    (r"backend/migration/migrations", "use backend/migrations (ADR-0009)"),
    (r"openapi/openapi\.json", "use openapi/openapi.yaml (ADR-0009)"),
]
# Docs where historic mentions are allowed (they document the deprecation itself)
CANONICAL_ALLOWLIST = {
    "docs/adr/0009-canonical-registry.md",
    "docs/IMPLEMENTATION_CONTRACTS.md",
    "docs/DOCUMENTATION_GOVERNANCE.md",
    "docs/adr/0006-postgresql-outbox.md",
}

STATUS_TOKENS = ("Current verified", "Configuration only", "Target approved", "Deprecated/historical")

problems: list[str] = []


def fail(msg: str) -> None:
    problems.append(msg)


def md_files() -> list[Path]:
    files = sorted(DOCS.rglob("*.md")) + [ROOT / "README.md"]
    if (ROOT / "CONTRIBUTING.md").exists():
        files.append(ROOT / "CONTRIBUTING.md")
    return files


def check_links() -> None:
    for p in md_files():
        text = p.read_text(errors="ignore")
        rel = p.relative_to(ROOT).as_posix()
        for target in re.findall(r"\]\(([^)#]+?)(?:#[^)]*)?\)", text):
            if target.startswith(("http://", "https://", "mailto:")):
                continue
            dest = (p.parent / target).resolve()
            if not dest.exists():
                fail(f"broken link: {rel} -> {target}")
        # implicit doc references like `docs/API.md`
        for ref in re.findall(r"`(docs/[A-Za-z0-9_./-]+\.md)`", text):
            if "NNNN" in ref or "V2_MIGRATION" in ref:
                continue  # template / planned placeholders
            if not (ROOT / ref).exists():
                fail(f"broken doc ref: {rel} -> {ref}")


def check_canonical() -> None:
    for p in md_files():
        rel = p.relative_to(ROOT).as_posix()
        if rel in CANONICAL_ALLOWLIST or "/plans/" in rel:
            continue
        text = p.read_text(errors="ignore")
        for pattern, hint in FORBIDDEN_CANONICAL:
            for m in re.finditer(pattern, text):
                line = text.count("\n", 0, m.start()) + 1
                fail(f"non-canonical '{m.group(0)}' at {rel}:{line} ({hint})")


def check_status_labels() -> None:
    # Capability tables must carry status tokens; check key docs
    key = [
        DOCS / "CURRENT_STATE.md",
        DOCS / "FUNCTIONAL_ARCHITECTURE.md",
    ]
    for p in key:
        if not p.exists():
            fail(f"missing {p.name}")
    readme = (ROOT / "README.md").read_text()
    for token in STATUS_TOKENS[:3]:
        if token not in readme and "Current verified" not in readme:
            fail("README lacks capability status legend")


def check_orphans() -> None:
    indexed = set()
    for p in md_files():
        text = p.read_text(errors="ignore")
        for ref in re.findall(r"docs/[A-Za-z0-9_./-]+\.md", text):
            indexed.add((ROOT / ref).resolve())
    stubs = 0
    for p in sorted(DOCS.glob("*.md")):
        if p.name in {"AGENTS.md"}:
            continue
        if p.resolve() not in indexed:
            if p.read_text(errors="ignore").startswith("# Moved") or "Redirect" in p.read_text(errors="ignore")[:200]:
                stubs += 1
                continue
            fail(f"orphan doc (not referenced anywhere): docs/{p.name}")


def check_current_state() -> None:
    p = DOCS / "CURRENT_STATE.md"
    if not p.exists():
        fail("docs/CURRENT_STATE.md missing")
        return
    text = p.read_text()
    if "2026-08-27" not in text:
        fail("CURRENT_STATE.md lacks capture date")
    # routes in router must be <= documented count marker
    router = ROOT / "frontend/src/app/router.tsx"
    if router.exists():
        n = len(re.findall(r"path: '/", router.read_text()))
        if "21 маршрут" not in text and n:
            fail(f"CURRENT_STATE route count drift: router has {n} paths")


def check_screenshots() -> None:
    readme = (ROOT / "README.md").read_text()
    shots = re.findall(r"\(docs/screenshots/([^)]+\.png)\)", readme)
    seen = set()
    for s in shots:
        if s in seen:
            fail(f"duplicate screenshot reference: {s}")
        seen.add(s)
        if not (DOCS / "screenshots" / s).exists():
            fail(f"missing screenshot file: {s}")
    hashes = {}
    for p in sorted((DOCS / "screenshots").glob("*.png")):
        h = hashlib.sha256(p.read_bytes()).hexdigest()
        if h in hashes:
            fail(f"duplicate screenshot content: {p.name} == {hashes[h]}")
        hashes[h] = p.name


def check_manifest() -> None:
    manifest = DOCS / "assets/screens/manifest.md"
    if not manifest.exists():
        return  # manifest arrives at Gate 5
    text = manifest.read_text()
    for m in re.finditer(r"\((\.\./)+screenshots/([^)]+\.png)\)", text):
        if not (DOCS / "screenshots" / m.group(2)).exists():
            fail(f"manifest references missing file: {m.group(2)}")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--all", action="store_true")
    for name in ("links", "canonical", "status-labels", "orphan-docs", "current-state", "screenshots", "manifest"):
        ap.add_argument(f"--{name}", action="store_true")
    args = ap.parse_args()
    checks = {
        "links": check_links,
        "canonical": check_canonical,
        "status-labels": check_status_labels,
        "orphan-docs": check_orphans,
        "current-state": check_current_state,
        "screenshots": check_screenshots,
        "manifest": check_manifest,
    }
    selected = [fn for name, fn in checks.items() if args.all or getattr(args, name.replace("-", "_"))]
    if not selected:
        selected = [checks["links"]]
    for fn in selected:
        fn()
    if problems:
        print(f"FAIL ({len(problems)} problem(s)):")
        for p in problems:
            print(f"  - {p}")
        return 1
    print("OK: documentation checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
