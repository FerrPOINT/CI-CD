#!/usr/bin/env python3
"""Documentation verification for Forge CI/CD.

Checks: markdown link/anchor integrity, canonical naming (ADR-0009),
status taxonomy, orphan docs, current-state cross-check, screenshot manifest,
and router/OpenAPI path drift.

Usage: python3 scripts/verify_docs.py [--all | --links --anchors --canonical --status-labels --orphan-docs --current-state --screenshots --manifest --openapi-routes --api-doc-routes]
Exit code 0 = clean; non-zero with a report otherwise.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import struct
import sys
from urllib.parse import unquote
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
    (r"frontend/src/shared/api/generated", "use frontend/src/api/schema.d.ts (ADR-0009)"),
    (r"\brunner_leases\b", "use job_leases (ADR-0009)"),
    (r"\bpipeline_run_id\b", "use pipeline_id (ADR-0009)"),
    (r"\bjob_run_id\b", "use job_id + execution_attempts (ADR-0009)"),
    (r"shared/api/generated", "use frontend/src/api/schema.d.ts for current frontend contract"),
]
# Docs where historic mentions are allowed (they document the deprecation itself)
CANONICAL_ALLOWLIST = {
    "docs/adr/0009-canonical-registry.md",
    "docs/IMPLEMENTATION_CONTRACTS.md",
    "docs/DOCUMENTATION_GOVERNANCE.md",
    "docs/adr/0006-postgresql-outbox.md",
}

STATUS_TOKENS = ("Current verified", "Configuration only", "Target approved", "Deprecated/historical")
BACKEND_ROUTE_FILES = (
    "backend/src/api.rs",
    "backend/src/platform.rs",
    "backend/src/git_host.rs",
    "backend/src/pulls.rs",
)
FORBIDDEN_STALE_STATUS = [
    (r"В MVP задачи переводятся вручную через API, CLI или Dashboard", "current execution uses embedded runner; manual transitions are historical/manual-job only"),
    (r"Automation — configuration only", "schedules/outgoing webhooks are Current verified MVP; only notifications/inbound handlers are configuration-only"),
    (r"Identity — storage only", "identity has conditional auth/RBAC enforcement when CICD_AUTH_SECRET is non-empty"),
    (r"Outbox worker и runner API -- Target", "outbox worker is current MVP; only external runner API is target"),
    (r"Нет auth/RBAC: Spoofing на всех API", "auth/RBAC is conditional, not absent"),
    (r"Отсутствует rate limiting", "route-class in-process rate limiting exists; distributed/proxy limiting remains target"),
    (r"Membership и tenant isolation отсутствуют", "project_memberships are current; tenant membership/isolation remains target"),
    (r"project membership/scoped PAT ещё target", "project_memberships are current; tenant isolation/scoped PAT remains target"),
    (r"enforcement всё ещё coarse global-role", "project membership enforcement is current when auth is enabled"),
    (r"реализовать project RBAC", "project RBAC is current; remaining target is tenant isolation/scoped credentials/session policy"),
    (r"retry отдельной job[^.\n]*удаляет[^.\n]*job_logs", "job retry now creates execution_attempts and preserves old logs"),
    (r"retry отдельной job[^.\n]*очищает старые", "job retry now preserves previous attempt logs"),
    (r"полноценной истории попыток ещё нет", "execution_attempts are current MVP"),
]

problems: list[str] = []


def fail(msg: str) -> None:
    problems.append(msg)


def md_files() -> list[Path]:
    files = sorted(DOCS.rglob("*.md")) + [ROOT / "README.md"]
    if (ROOT / "CONTRIBUTING.md").exists():
        files.append(ROOT / "CONTRIBUTING.md")
    return files


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def check_links() -> None:
    for p in md_files():
        text = read_text(p)
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


def slugify_heading(raw: str) -> str:
    raw = re.sub(r"\s+\{#[^}]+}\s*$", "", raw)
    raw = re.sub(r"`([^`]*)`", r"\1", raw)
    raw = re.sub(r"\[([^\]]+)\]\([^)]+\)", r"\1", raw)
    raw = raw.strip().lower()
    out: list[str] = []
    for ch in raw:
        if ch.isalnum() or ch == "_":
            out.append(ch)
        elif ch.isspace() or ch == "-":
            out.append("-")
    return "".join(out).strip("-")


def heading_anchors(path: Path) -> set[str]:
    anchors: set[str] = set()
    counts: dict[str, int] = {}
    for line in read_text(path).splitlines():
        m = re.match(r"^(#{1,6})\s+(.+?)\s*#*$", line)
        if not m:
            continue
        explicit = re.search(r"\s+\{#([^}]+)}\s*$", m.group(2))
        base = explicit.group(1) if explicit else slugify_heading(m.group(2))
        if not base:
            continue
        count = counts.get(base, 0)
        counts[base] = count + 1
        anchors.add(base if count == 0 else f"{base}-{count}")
    return anchors


def check_anchors() -> None:
    cache: dict[Path, set[str]] = {}
    for p in md_files():
        text = read_text(p)
        rel = p.relative_to(ROOT).as_posix()
        for target in re.findall(r"\]\(([^)]+)\)", text):
            before_hash, sep, fragment = target.partition("#")
            if not sep or not fragment:
                continue
            if before_hash.startswith(("http://", "https://", "mailto:")):
                continue
            dest = p if before_hash == "" else (p.parent / before_hash).resolve()
            if dest.suffix.lower() != ".md":
                continue
            if not dest.exists():
                continue  # reported by check_links
            anchors = cache.setdefault(dest, heading_anchors(dest))
            decoded = unquote(fragment)
            candidates = {decoded, decoded.lower(), slugify_heading(decoded)}
            if anchors.isdisjoint(candidates):
                fail(f"broken anchor: {rel} -> {target}")


def check_canonical() -> None:
    for p in md_files():
        rel = p.relative_to(ROOT).as_posix()
        if rel in CANONICAL_ALLOWLIST or "/plans/" in rel:
            continue
        text = read_text(p)
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
    readme = read_text(ROOT / "README.md")
    for token in STATUS_TOKENS[:3]:
        if token not in readme and "Current verified" not in readme:
            fail("README lacks capability status legend")
    for p in md_files():
        rel = p.relative_to(ROOT).as_posix()
        text = read_text(p)
        for pattern, hint in FORBIDDEN_STALE_STATUS:
            for m in re.finditer(pattern, text):
                line = text.count("\n", 0, m.start()) + 1
                fail(f"stale status phrase at {rel}:{line} ({hint})")


def check_orphans() -> None:
    indexed = set()
    docs_root = DOCS.resolve()
    for p in md_files():
        text = read_text(p)
        for target in re.findall(r"\]\(([^)#]+?)(?:#[^)]*)?\)", text):
            if target.startswith(("http://", "https://", "mailto:")):
                continue
            dest = (p.parent / target).resolve()
            if dest.suffix.lower() == ".md" and (dest == docs_root or docs_root in dest.parents):
                indexed.add(dest)
        for ref in re.findall(r"docs/[A-Za-z0-9_./-]+\.md", text):
            indexed.add((ROOT / ref).resolve())
    stubs = 0
    for p in sorted(DOCS.glob("*.md")):
        if p.name in {"AGENTS.md"}:
            continue
        if p.resolve() not in indexed:
            text = read_text(p)
            if text.startswith("# Moved") or "Redirect" in text[:200]:
                stubs += 1
                continue
            fail(f"orphan doc (not referenced anywhere): docs/{p.name}")


def check_current_state() -> None:
    p = DOCS / "CURRENT_STATE.md"
    if not p.exists():
        fail("docs/CURRENT_STATE.md missing")
        return
    text = read_text(p)
    if not re.search(r"Снято:\s*`20\d{2}-\d{2}-\d{2}`", text):
        fail("CURRENT_STATE.md lacks ISO capture date")
    # routes in router must match documented current-state marker
    router = ROOT / "frontend/src/app/router.tsx"
    if router.exists():
        n = len(re.findall(r"path: '/", read_text(router)))
        documented = re.search(r"Frontend:\s*(\d+)\s+маршрут", text)
        if not documented and n:
            fail(f"CURRENT_STATE route count missing: router has {n} paths")
        elif documented and int(documented.group(1)) != n:
            fail(f"CURRENT_STATE route count drift: documented {documented.group(1)}, router has {n}")


def check_screenshots() -> None:
    readme = read_text(ROOT / "README.md")
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


def manifest_screenshot_entries() -> list[str]:
    manifest = DOCS / "assets/screens/manifest.md"
    if not manifest.exists():
        return []
    text = read_text(manifest)
    return [m.group(2) for m in re.finditer(r"\((\.\./)+screenshots/([^)]+\.png)\)", text)]


def png_dimensions(path: Path) -> tuple[int, int] | None:
    with path.open("rb") as f:
        if f.read(8) != b"\x89PNG\r\n\x1a\n":
            fail(f"not a PNG file: {path.relative_to(ROOT).as_posix()}")
            return None
        header = f.read(8)
        if len(header) != 8:
            fail(f"truncated PNG header: {path.relative_to(ROOT).as_posix()}")
            return None
        length, chunk_type = struct.unpack(">I4s", header)
        if chunk_type != b"IHDR" or length < 8:
            fail(f"PNG missing IHDR: {path.relative_to(ROOT).as_posix()}")
            return None
        data = f.read(8)
        if len(data) != 8:
            fail(f"truncated PNG IHDR: {path.relative_to(ROOT).as_posix()}")
            return None
        width, height = struct.unpack(">II", data)
        return width, height


def check_manifest() -> None:
    manifest = DOCS / "assets/screens/manifest.md"
    if not manifest.exists():
        return  # manifest arrives at Gate 5
    text = read_text(manifest)
    entries = manifest_screenshot_entries()
    seen: set[str] = set()
    for name in entries:
        if name in seen:
            fail(f"duplicate manifest screenshot reference: {name}")
        seen.add(name)
        if not (DOCS / "screenshots" / name).exists():
            fail(f"manifest references missing file: {name}")
    for line_no, line in enumerate(text.splitlines(), start=1):
        m = re.match(r"\|\s*\[[^\]]+\]\((?:\.\./)+screenshots/([^)]+\.png)\)\s*\|.*\|\s*(\d+)[×x](\d+)\s*\|\s*$", line)
        if not m:
            continue
        name = m.group(1)
        expected = (int(m.group(2)), int(m.group(3)))
        path = DOCS / "screenshots" / name
        if not path.exists():
            continue
        actual = png_dimensions(path)
        if actual and actual != expected:
            fail(
                f"manifest dimension drift at {manifest.relative_to(ROOT).as_posix()}:{line_no}: "
                f"{name} says {expected[0]}x{expected[1]}, actual {actual[0]}x{actual[1]}"
            )
    count = len(seen)
    readme = read_text(ROOT / "README.md")
    m = re.search(r"Полный визуальный реестр\s*\((\d+)\s+скрин", readme)
    if m and int(m.group(1)) != count:
        fail(f"README visual registry count drift: documented {m.group(1)}, manifest has {count}")


def extract_router_paths() -> set[str]:
    paths: set[str] = set()
    for rel in BACKEND_ROUTE_FILES:
        src = ROOT / rel
        if not src.exists():
            continue
        text = read_text(src)
        paths.update(re.findall(r"\.route\(\s*\"([^\"]+)\"", text))
    return paths


def extract_openapi_paths() -> set[str]:
    spec = ROOT / "openapi/openapi.yaml"
    if not spec.exists():
        fail("openapi/openapi.yaml missing")
        return set()
    paths: set[str] = set()
    in_paths = False
    for line in read_text(spec).splitlines():
        if line == "paths:":
            in_paths = True
            continue
        if in_paths and line and not line.startswith((" ", "/")):
            break
        if not in_paths:
            continue
        m = re.match(r"^  (/[^:]+):\s*$", line)
        if m:
            paths.add(m.group(1))
    return paths


def check_openapi_routes() -> None:
    router_paths = extract_router_paths()
    openapi_paths = extract_openapi_paths()
    for path in sorted(router_paths - openapi_paths):
        fail(f"router path missing from OpenAPI: {path}")


def check_api_doc_routes() -> None:
    api_doc = DOCS / "API.md"
    if not api_doc.exists():
        fail("docs/API.md missing")
        return
    text = read_text(api_doc)
    for path in sorted(extract_router_paths()):
        if path not in text:
            fail(f"backend route missing from API.md: {path}")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--all", action="store_true")
    for name in (
        "links",
        "anchors",
        "canonical",
        "status-labels",
        "orphan-docs",
        "current-state",
        "screenshots",
        "manifest",
        "openapi-routes",
        "api-doc-routes",
    ):
        ap.add_argument(f"--{name}", action="store_true")
    args = ap.parse_args()
    checks = {
        "links": check_links,
        "anchors": check_anchors,
        "canonical": check_canonical,
        "status-labels": check_status_labels,
        "orphan-docs": check_orphans,
        "current-state": check_current_state,
        "screenshots": check_screenshots,
        "manifest": check_manifest,
        "openapi-routes": check_openapi_routes,
        "api-doc-routes": check_api_doc_routes,
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
