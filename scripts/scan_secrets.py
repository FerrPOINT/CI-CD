#!/usr/bin/env python3
"""Fail when repository text appears to contain committed secrets.

This is a first-party baseline scanner for CI. It intentionally avoids
network calls and marketplace actions, so every push can run the same checks
locally and in GitHub Actions. It is not a DLP system; deeper entropy,
history, and container-layer scanners remain release hardening.
"""
from __future__ import annotations

import argparse
import math
import re
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MAX_FILE_BYTES = 1_000_000

SKIP_DIRS = {
    ".git",
    ".pytest_cache",
    "__pycache__",
    "coverage",
    "dist",
    "node_modules",
    "target",
}

SKIP_FILES = {
    "backend/Cargo.lock",
    "frontend/pnpm-lock.yaml",
    "docs/assets/sbom.json",
}

BINARY_SUFFIXES = {
    ".7z",
    ".avif",
    ".bmp",
    ".br",
    ".class",
    ".dll",
    ".exe",
    ".gif",
    ".gz",
    ".ico",
    ".jar",
    ".jpeg",
    ".jpg",
    ".mov",
    ".mp4",
    ".pdf",
    ".png",
    ".pyc",
    ".so",
    ".tar",
    ".tgz",
    ".ttf",
    ".webp",
    ".woff",
    ".woff2",
    ".zip",
}

ALLOW_VALUE_MARKERS = (
    "changeme",
    "change-me",
    "dev-token",
    "dummy",
    "example",
    "fake",
    "fixture",
    "forge-internal-dev-token",
    "forge-seed-",
    "forge_test_",
    "local_only",
    "placeholder",
    "sample",
    "synthetic",
    "test",
)

PATTERNS: tuple[tuple[str, re.Pattern[str]], ...] = (
    ("private-key", re.compile(r"-----BEGIN (?:RSA |DSA |EC |OPENSSH |PGP )?PRIVATE KEY-----")),
    ("aws-access-key", re.compile(r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b")),
    ("github-token", re.compile(r"\bgh[pousr]_[A-Za-z0-9_]{20,}\b")),
    ("slack-token", re.compile(r"\bxox[baprs]-[A-Za-z0-9-]{20,}\b")),
    (
        "generic-secret",
        re.compile(
            r"""(?ix)
            \b(?:api[_-]?key|secret|token|password|passwd|pwd|private[_-]?key|client[_-]?secret)\b
            [\w\s./-]{0,32}
            (?:=|:)\s*
            (?P<quote>['"]?)
            (?P<secret>[A-Za-z0-9][A-Za-z0-9_./+=:@-]{19,})
            """,
        ),
    ),
)


@dataclass(frozen=True)
class Finding:
    path: Path
    line: int
    rule: str


def display_path(path: Path) -> str:
    try:
        return path.relative_to(ROOT).as_posix()
    except ValueError:
        return str(path)


def relative_path(path: Path, root: Path) -> Path:
    try:
        return path.relative_to(root)
    except ValueError:
        return path


def should_skip(path: Path, root: Path) -> bool:
    rel_to_repo = relative_path(path, ROOT).as_posix()
    rel_to_scan_root = relative_path(path, root)
    if rel_to_repo in SKIP_FILES:
        return True
    if path.suffix.lower() in BINARY_SUFFIXES:
        return True
    return any(part in SKIP_DIRS for part in rel_to_scan_root.parts)


def is_probably_binary(data: bytes) -> bool:
    if b"\x00" in data[:8192]:
        return True
    if not data:
        return False
    control = sum(1 for byte in data[:8192] if byte < 9 or 13 < byte < 32)
    return control / min(len(data), 8192) > 0.10


def shannon_entropy(value: str) -> float:
    if not value:
        return 0.0
    counts = {char: value.count(char) for char in set(value)}
    return -sum((count / len(value)) * math.log2(count / len(value)) for count in counts.values())


def allowed_generic(value: str, line: str, tail: str) -> bool:
    lower_value = value.lower().strip("\"'")
    lower_line = line.lower()
    if tail.lstrip().startswith("(") or "::" in value:
        return True
    if lower_value.startswith(("$", "{", "<")):
        return True
    if any(marker in lower_value for marker in ALLOW_VALUE_MARKERS):
        return True
    if "example" in lower_line or "placeholder" in lower_line:
        return True
    if shannon_entropy(lower_value) < 3.25:
        return True
    return False


def scan_file(path: Path) -> list[Finding]:
    try:
        data = path.read_bytes()
    except OSError as error:
        return [Finding(path, 0, f"read-error:{error.__class__.__name__}")]
    if len(data) > MAX_FILE_BYTES or is_probably_binary(data):
        return []
    text = data.decode("utf-8", errors="replace")
    findings: list[Finding] = []
    for line_number, line in enumerate(text.splitlines(), start=1):
        for rule, pattern in PATTERNS:
            for match in pattern.finditer(line):
                if rule == "generic-secret" and allowed_generic(
                    match.group("secret"),
                    line,
                    line[match.end("secret") :],
                ):
                    continue
                findings.append(Finding(path, line_number, rule))
    return findings


def iter_files(root: Path) -> list[Path]:
    return sorted(path for path in root.rglob("*") if path.is_file() and not should_skip(path, root))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=ROOT)
    args = parser.parse_args()

    root = args.root.resolve()
    findings: list[Finding] = []
    scanned = 0
    for path in iter_files(root):
        scanned += 1
        findings.extend(scan_file(path))

    if findings:
        print("Secret scan failed; review these locations before committing:")
        for finding in findings:
            location = display_path(finding.path)
            suffix = f":{finding.line}" if finding.line else ""
            print(f"- {location}{suffix} [{finding.rule}]")
        return 1

    print(f"Secret scan passed: {scanned} text files scanned")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
