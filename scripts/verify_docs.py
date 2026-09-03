#!/usr/bin/env python3
"""Documentation verification for Forge CI/CD.

Checks: markdown link/anchor integrity, canonical naming (ADR-0009),
status taxonomy, orphan docs, current-state cross-check, readiness migration
examples, frontend CI contract, screenshot manifest/route coverage, plans status
notes, and router/OpenAPI path drift.

Usage: python3 scripts/verify_docs.py [--all | --links --anchors --canonical --status-labels --orphan-docs --current-state --readiness-examples --frontend-contracts --screenshots --manifest --plans --openapi-routes --api-doc-routes]
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
PLANS = ROOT / "plans"

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
    "backend/src/runner_protocol.rs",
    "backend/src/git_host.rs",
    "backend/src/pulls.rs",
)
FRONTEND_REQUIRED_SCRIPTS = {
    "dev",
    "build",
    "test",
    "lint",
    "e2e",
    "e2e:install",
    "seed:evidence",
    "shoot:evidence",
    "openapi:generate",
    "openapi:check",
    "openapi:compat",
}
FRONTEND_SCRIPT_TOOL_DEPS = {
    "build": ("typescript", "vite"),
    "test": ("vitest",),
    "lint": ("@eslint/js", "eslint", "eslint-plugin-react-hooks", "globals", "typescript-eslint"),
    "e2e": ("@axe-core/playwright", "@playwright/test"),
    "e2e:install": ("@playwright/test",),
    "openapi:check": ("openapi-typescript",),
    "openapi:compat": ("yaml",),
    "openapi:generate": ("openapi-typescript",),
}
PNPM_BUILTINS = {
    "add",
    "audit",
    "config",
    "dlx",
    "env",
    "exec",
    "install",
    "remove",
    "setup",
    "store",
    "update",
}
FORBIDDEN_STALE_STATUS = [
    (r"В MVP задачи переводятся вручную через API, CLI или Dashboard", "current execution uses embedded runner; manual transitions are historical/manual-job only"),
    (r"Automation — configuration only", "schedules/outgoing webhooks/in_app/SSE notifications are Current verified MVP; only inbound handlers/external adapters remain target"),
    (r"Identity — storage only", "identity has conditional auth/RBAC enforcement when CICD_AUTH_SECRET is non-empty"),
    (r"Outbox worker и runner API -- Target", "outbox worker is current MVP; only external runner API is target"),
    (r"Нет auth/RBAC: Spoofing на всех API", "auth/RBAC is conditional, not absent"),
    (r"Отсутствует rate limiting", "route-class in-process rate limiting exists; distributed/proxy limiting remains target"),
    (r"Membership и tenant isolation отсутствуют", "project_memberships are current; tenant membership/isolation remains target"),
    (r"project membership/scoped PAT ещё target", "project_memberships and scoped PAT are current; tenant isolation remains target"),
    (r"enforcement всё ещё coarse global-role", "project membership enforcement is current when auth is enabled"),
    (r"всё ещё используют coarse global-role policy", "project membership enforcement is current when auth is enabled"),
    (r"реализовать project RBAC", "project RBAC is current; remaining target is tenant isolation/scoped credentials/session policy"),
    (r"production session/logout policy", "refresh logout/revoke is current; cookie/CSRF/session-family policy remains target"),
    (r"POST /api/v1/auth/logout\s+— target", "auth logout refresh revocation is current"),
    (r"Server-side session revocation is Phase 2", "frontend logout calls the server-side revoke endpoint"),
    (r"logout/revoke policy для sessions", "refresh logout/revoke is current; cookie/CSRF/session-family policy remains target"),
    (r"Уже выданный access JWT оста[её]тся действительным", "access JWT is session-bound and protected routes reject revoked/rotated sessions"),
    (r"immediate access-token invalidation", "session-bound access invalidation is current; cookie/CSRF/session-family policy remains target"),
    (r"немедленная инвалидизация access token[^.\n]*оста[её]тся target", "session-bound access invalidation is current; cookie/CSRF/session-family policy remains target"),
    (r"Git-level RBAC пока target", "Git Smart HTTP project checks are current; tenant isolation/scoped Git credentials remain target"),
    (r"Git/delivery binding", "Git project binding is current; delivery binding remains target"),
    (r"Git repository binding и production", "Git repository URL binding is current; tenant-bound Git mapping and production session policy remain target"),
    (r"/api/v1/git/\{repo\}/", "Git Smart HTTP routes are mounted at /git/{repo}/..., not under /api/v1"),
    (r"не опирается на JWT/PAT", "Git Smart HTTP accepts JWT/PAT when CICD_AUTH_SECRET is configured"),
    (r"Нет repository/project authorization", "Git Smart HTTP now checks linked project membership for private/write operations"),
    (r"repo-level policy отсутствует", "Git Smart HTTP now has project-linked read/write policy"),
    (r"tenant/scoped PAT", "scoped PAT is current MVP; remaining target is tenant isolation/service-account/scoped Git credentials"),
    (r"scoped PAT/SAT", "scoped PAT is current MVP; remaining target is service-account tokens and tenant/scoped Git credentials"),
    (r"Scoped PAT, tenant isolation", "scoped PAT is current MVP; tenant isolation remains target"),
    (r"Tenant scope, scoped PAT", "scoped PAT is current MVP; tenant scope remains target"),
    (r"tenant boundary, scoped PAT", "scoped PAT is current MVP; tenant boundary remains target"),
    (r"Scoped tokens, pepper/HMAC storage", "scoped PAT is current MVP; pepper/HMAC/service-account token hardening remains target"),
    (r"scopes/pepper/rotation остаются target", "PAT scopes are current MVP; pepper/rotation remain target"),
    (r"Нет scopes, pepper/HMAC storage, revoke reason и project boundary", "PAT scopes/project boundary are current MVP; pepper/HMAC/revoke reason remain target"),
    (r"пустой CICD_GIT_TOKEN разрешает", "empty CICD_GIT_TOKEN only disables the legacy shared token; auth-secret mode still enforces Git credentials"),
    (r"retry отдельной job[^.\n]*удаляет[^.\n]*job_logs", "job retry now creates execution_attempts and preserves old logs"),
    (r"retry отдельной job[^.\n]*очищает старые", "job retry now preserves previous attempt logs"),
    (r"полноценной истории попыток ещё нет", "execution_attempts are current MVP"),
    (r"log pagination/search и [^.\n]*оста[её]тся", "bounded log pagination/search is current MVP; command spans remain target"),
    (r"pagination/search оста[её]тся target", "bounded log pagination/search is current MVP; command spans remain target"),
    (r"pagination/search.*target diagnostic logs", "bounded log pagination/search is current MVP; command spans remain target"),
    (r"sender/SSE delivery нет", "in_app/SSE notification delivery is current MVP; external adapters remain target"),
    (r"notifications sender/SSE delivery", "in_app/SSE notification delivery is current MVP; external adapters remain target"),
    (r"notification sender отсутств", "in_app/SSE notification delivery is current MVP; external adapters remain target"),
    (r"notifications sender не реализован", "in_app/SSE notification delivery is current MVP; external adapters remain target"),
    (r"SSE не реализован", "notification SSE stream is current MVP; external adapters remain target"),
    (r"email/Slack/SSE sender", "SSE notification stream is current MVP; email/Slack adapters remain target"),
    (r"Доставки нет", "in_app/SSE notification delivery is current MVP; external adapters remain target"),
    (r"Delivery history/replay/dead letters остаются target", "bounded delivery history/requeue is current MVP; production lease/dead-letter policy remains target"),
    (r"Replay, dead letters и полная история доставок остаются target", "bounded delivery history/requeue is current MVP; production lease/dead-letter policy remains target"),
    (r"история доставок остаются target", "bounded delivery history/requeue is current MVP; clarify remaining scheduler or dead-letter target scope"),
    (r"full deliveries table/replay ещё target", "bounded outbox attempt history/requeue is current MVP; production delivery snapshots/leases remain target"),
    (r"delivery history, audited replay/dead letters", "bounded delivery history/requeue is current MVP; audited full dead-letter workflow remains target"),
    (r"env \(`CICD_`\) → typed config \(в процессе\)", "backend server now uses typed RuntimeConfig; package split remains target"),
    (r"full typed config target", "backend RuntimeConfig is current; remaining target is package split/production hardening"),
    (r"сейчас — прямое чтение env", "server startup and AppState now use typed RuntimeConfig"),
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


def latest_migration_version() -> int | None:
    migration_dir = ROOT / "backend/migrations"
    versions: list[int] = []
    for p in sorted(migration_dir.glob("*.sql")):
        m = re.match(r"^(\d{4})_", p.name)
        if m:
            versions.append(int(m.group(1)))
    if not versions:
        fail("no SQLx migrations found under backend/migrations")
        return None
    return max(versions)


def check_readiness_examples() -> None:
    api_doc = DOCS / "API.md"
    if not api_doc.exists():
        fail("docs/API.md missing")
        return
    latest = latest_migration_version()
    if latest is None:
        return
    text = read_text(api_doc)
    section = re.search(
        r"#### GET /api/v1/readiness(?P<body>.*?)(?:\n#### |\n### |\Z)",
        text,
        re.DOTALL,
    )
    if not section:
        fail("docs/API.md lacks /api/v1/readiness section")
        return
    response = re.search(
        r"\*\*Response 200:\*\*\s*```json\s*(?P<json>\{.*?\})\s*```",
        section.group("body"),
        re.DOTALL,
    )
    if not response:
        fail("docs/API.md lacks readiness Response 200 JSON example")
        return
    try:
        payload = json.loads(response.group("json"))
    except json.JSONDecodeError as exc:
        line = text.count("\n", 0, section.start("body") + response.start("json")) + exc.lineno
        fail(f"docs/API.md readiness Response 200 JSON is invalid near line {line}: {exc.msg}")
        return
    migrations = payload.get("migrations")
    if not isinstance(migrations, dict):
        fail("docs/API.md readiness Response 200 lacks migrations object")
        return
    for field in ("latest_applied_version", "latest_required_version"):
        documented = migrations.get(field)
        if documented != latest:
            fail(
                f"readiness Response 200 example drift: "
                f"{field} says {documented!r}, latest migration is {latest}"
            )


def frontend_package() -> dict:
    path = ROOT / "frontend/package.json"
    if not path.exists():
        fail("frontend/package.json missing")
        return {}
    try:
        data = json.loads(read_text(path))
    except json.JSONDecodeError as exc:
        fail(f"frontend/package.json is invalid JSON near line {exc.lineno}: {exc.msg}")
        return {}
    if not isinstance(data, dict):
        fail("frontend/package.json must be a JSON object")
        return {}
    return data


def package_section(data: dict, key: str) -> dict:
    value = data.get(key, {})
    if not isinstance(value, dict):
        fail(f"frontend/package.json {key} must be an object")
        return {}
    return value


def semver_major(spec: object) -> str | None:
    if not isinstance(spec, str):
        return None
    match = re.search(r"\d+", spec)
    return match.group(0) if match else None


def semver_major_minor(spec: object) -> str | None:
    if not isinstance(spec, str):
        return None
    match = re.search(r"\d+(?:\.\d+)?", spec)
    return match.group(0) if match else None


def frontend_dependency_versions(data: dict) -> dict[str, str]:
    versions: dict[str, str] = {}
    for section in ("dependencies", "devDependencies"):
        for name, version in package_section(data, section).items():
            if isinstance(name, str) and isinstance(version, str):
                versions[name] = version
    return versions


def join_major_versions(packages: dict[str, str], names: tuple[str, ...]) -> str | None:
    majors = [semver_major(packages.get(name)) for name in names]
    if any(major is None for major in majors):
        return None
    if len(set(majors)) == 1:
        return majors[0]
    return " / ".join(major for major in majors if major is not None)


def check_frontend_ci_scripts(scripts: dict) -> None:
    workflow = ROOT / ".github/workflows/ci.yml"
    if not workflow.exists():
        fail(".github/workflows/ci.yml missing")
        return
    commands = set()
    for match in re.finditer(r"\bpnpm\s+([A-Za-z0-9:_-]+)\b", read_text(workflow)):
        command = match.group(1)
        if command not in PNPM_BUILTINS:
            commands.add(command)
    for command in sorted(commands):
        if command not in scripts:
            fail(f"CI references missing frontend package script: pnpm {command}")


def check_frontend_script_contract(scripts: dict, packages: dict[str, str]) -> None:
    for name in sorted(FRONTEND_REQUIRED_SCRIPTS):
        if name not in scripts:
            fail(f"required frontend package script missing: {name}")
    installed = set(packages)
    for script, deps in sorted(FRONTEND_SCRIPT_TOOL_DEPS.items()):
        if script not in scripts:
            continue
        for dep in deps:
            if dep not in installed:
                fail(f"frontend script {script} requires package {dep}")


def parse_architecture_frontend_stack() -> dict[str, tuple[str, str]]:
    path = DOCS / "ARCHITECTURE.md"
    if not path.exists():
        fail("docs/ARCHITECTURE.md missing")
        return {}
    rows: dict[str, tuple[str, str]] = {}
    for line_no, line in enumerate(read_text(path).splitlines(), start=1):
        if not line.startswith("|"):
            continue
        parts = [part.strip() for part in line.strip().strip("|").split("|")]
        if len(parts) != 3 or parts[0] in {"Компонент", "---"}:
            continue
        rows[parts[0]] = (parts[1], parts[2])
        if parts[2] == "":
            fail(f"empty frontend stack version at docs/ARCHITECTURE.md:{line_no}")
    return rows


def check_frontend_stack_doc(packages: dict[str, str]) -> None:
    rows = parse_architecture_frontend_stack()
    expected = {
        "Framework": ("react + react-dom", join_major_versions(packages, ("react", "react-dom"))),
        "Build": ("vite", semver_major(packages.get("vite"))),
        "Styling": ("tailwindcss + @tailwindcss/vite", join_major_versions(packages, ("tailwindcss", "@tailwindcss/vite"))),
        "Router": ("react-router", semver_major(packages.get("react-router"))),
        "Server state": ("@tanstack/react-query", semver_major(packages.get("@tanstack/react-query"))),
        "Client state": ("zustand", semver_major(packages.get("zustand"))),
        "i18n": ("i18next + react-i18next", join_major_versions(packages, ("i18next", "react-i18next"))),
        "Unit tests": ("vitest + @testing-library/react", join_major_versions(packages, ("vitest", "@testing-library/react"))),
        "Types": ("typescript", semver_major_minor(packages.get("typescript"))),
    }
    for component, (library, version) in expected.items():
        if version is None:
            fail(f"frontend package version missing for architecture stack row: {component}")
            continue
        documented = rows.get(component)
        if documented is None:
            fail(f"docs/ARCHITECTURE.md frontend stack row missing: {component}")
            continue
        documented_library, documented_version = documented
        if documented_library != library:
            fail(f"docs/ARCHITECTURE.md frontend stack library drift for {component}: {documented_library!r} != {library!r}")
        if documented_version != version:
            fail(f"docs/ARCHITECTURE.md frontend stack version drift for {component}: {documented_version!r} != {version!r}")


def check_frontend_contracts() -> None:
    data = frontend_package()
    if not data:
        return
    scripts = package_section(data, "scripts")
    packages = frontend_dependency_versions(data)
    check_frontend_ci_scripts(scripts)
    check_frontend_script_contract(scripts, packages)
    check_frontend_stack_doc(packages)


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


def normalize_frontend_route(path: str) -> str:
    return re.sub(r":[A-Za-z_][A-Za-z0-9_]*", ":param", path.split("?", 1)[0])


def frontend_route_paths() -> set[str]:
    router = ROOT / "frontend/src/app/router.tsx"
    if not router.exists():
        fail("frontend/src/app/router.tsx missing")
        return set()
    routes = set()
    for path in re.findall(r"path:\s*'([^']+)'", read_text(router)):
        if path == "*":
            continue
        routes.add(normalize_frontend_route(path))
    return routes


def manifest_route_paths(text: str) -> set[str]:
    return {
        normalize_frontend_route(route)
        for route in re.findall(r"`(/[^`]*)`", text)
    }


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
    documented_routes = manifest_route_paths(text)
    for route in sorted(frontend_route_paths() - documented_routes):
        fail(f"frontend route missing from screenshot manifest: {route}")


def check_plans() -> None:
    if not PLANS.exists():
        return
    for p in sorted(PLANS.glob("*.md")):
        rel = p.relative_to(ROOT).as_posix()
        lines = read_text(p).splitlines()
        head = "\n".join(lines[:8])
        if not re.search(r"\*\*(Статус|Status)\s+20\d{2}-\d{2}-\d{2}:", head):
            fail(f"plan lacks dated status note near top: {rel}")
        if re.search(r"^>\s+\*\*For Hermes:\*\*", "\n".join(lines[:5]), re.MULTILINE):
            fail(f"plan starts with active Hermes instruction before status: {rel}")


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
        m = re.match(r"^  (/.+):\s*$", line)
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
        "readiness-examples",
        "frontend-contracts",
        "screenshots",
        "manifest",
        "plans",
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
        "readiness-examples": check_readiness_examples,
        "frontend-contracts": check_frontend_contracts,
        "screenshots": check_screenshots,
        "manifest": check_manifest,
        "plans": check_plans,
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
