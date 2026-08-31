#!/usr/bin/env python3
"""Backup, verify, and restore helper for the local Forge CI/CD compose stack.

The helper intentionally covers the current Docker Compose MVP only. It creates
PostgreSQL logical dumps plus file copies of the Git and artifact volumes, then
writes a checksum manifest that can be verified without reading secrets.
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import shlex
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_GIT_PATH = "/var/lib/forge/git"
DEFAULT_ARTIFACTS_PATH = "/var/lib/forge/artifacts"
TOOL_VERSION = "1"


@dataclass(frozen=True)
class ComposeConfig:
    project_dir: Path
    compose_file: Path
    env_file: Path | None
    db_user: str
    db_name: str
    dry_run: bool


def utc_now() -> dt.datetime:
    return dt.datetime.now(dt.timezone.utc).replace(microsecond=0)


def timestamp() -> str:
    return utc_now().strftime("%Y%m%dT%H%M%SZ")


def parse_env_file(path: Path | None) -> dict[str, str]:
    if path is None or not path.exists():
        return {}
    values: dict[str, str] = {}
    for raw_line in path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        key = key.strip()
        value = value.strip().strip("\"'")
        if key:
            values[key] = value
    return values


def env_value(values: dict[str, str], key: str, default: str) -> str:
    value = os.environ.get(key, values.get(key, default))
    return value if value else default


def build_config(args: argparse.Namespace) -> ComposeConfig:
    project_dir = Path(args.project_dir).resolve()
    compose_file = Path(args.compose_file)
    if not compose_file.is_absolute():
        compose_file = project_dir / compose_file
    env_file = Path(args.env_file) if args.env_file else project_dir / ".env"
    if not env_file.is_absolute():
        env_file = project_dir / env_file
    env_file = env_file if env_file.exists() else None
    env_values = parse_env_file(env_file)
    return ComposeConfig(
        project_dir=project_dir,
        compose_file=compose_file.resolve(),
        env_file=env_file.resolve() if env_file else None,
        db_user=env_value(env_values, "CICD_DATABASE_USER", "cicd"),
        db_name=env_value(env_values, "CICD_DATABASE_NAME", "cicd"),
        dry_run=args.dry_run,
    )


def quote_command(cmd: Iterable[str]) -> str:
    return " ".join(shlex.quote(str(part)) for part in cmd)


def compose(config: ComposeConfig, *args: str) -> list[str]:
    cmd = ["docker", "compose", "--project-directory", str(config.project_dir)]
    if config.env_file:
        cmd.extend(["--env-file", str(config.env_file)])
    cmd.extend(["-f", str(config.compose_file), *args])
    return cmd


def run(config: ComposeConfig, cmd: list[str], *, stdin_path: Path | None = None, stdout_path: Path | None = None) -> None:
    print(f"+ {quote_command(cmd)}")
    if config.dry_run:
        return
    stdin = stdin_path.open("rb") if stdin_path else None
    stdout = stdout_path.open("wb") if stdout_path else None
    try:
        subprocess.run(cmd, cwd=config.project_dir, stdin=stdin, stdout=stdout, check=True)
    finally:
        if stdin:
            stdin.close()
        if stdout:
            stdout.close()


def capture(config: ComposeConfig, cmd: list[str], placeholder: str) -> str:
    print(f"+ {quote_command(cmd)}")
    if config.dry_run:
        return placeholder
    result = subprocess.run(cmd, cwd=config.project_dir, check=True, text=True, capture_output=True)
    return result.stdout.strip()


def ensure_empty_backup_dir(path: Path) -> None:
    if path.exists() and any(path.iterdir()):
        raise SystemExit(f"backup directory is not empty: {path}")
    path.mkdir(parents=True, exist_ok=True)


def backup_dir_from_args(args: argparse.Namespace) -> Path:
    if args.backup_dir:
        return Path(args.backup_dir).resolve()
    return (ROOT / "backups" / timestamp()).resolve()


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as fh:
        for chunk in iter(lambda: fh.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def relative_posix(path: Path, root: Path) -> str:
    return path.relative_to(root).as_posix()


def data_files(backup_dir: Path) -> list[Path]:
    files: list[Path] = []
    for top in ("git", "artifacts"):
        base = backup_dir / top
        if base.exists():
            files.extend(sorted(p for p in base.rglob("*") if p.is_file()))
    return files


def write_inventory(backup_dir: Path) -> dict[str, int]:
    git_artifact_files = data_files(backup_dir)
    (backup_dir / "files.txt").write_text(
        "".join(f"{relative_posix(path, backup_dir)}\n" for path in git_artifact_files),
        encoding="utf-8",
    )
    checksum_paths = [backup_dir / "postgres.dump", *git_artifact_files]
    with (backup_dir / "SHA256SUMS").open("w", encoding="utf-8", newline="\n") as fh:
        for path in checksum_paths:
            rel = relative_posix(path, backup_dir)
            fh.write(f"{sha256_file(path)}  {rel}\n")
    git_files = sum(1 for path in git_artifact_files if path.relative_to(backup_dir).parts[:1] == ("git",))
    artifact_files = sum(1 for path in git_artifact_files if path.relative_to(backup_dir).parts[:1] == ("artifacts",))
    return {
        "git_files": git_files,
        "artifact_files": artifact_files,
        "data_bytes": sum(p.stat().st_size for p in checksum_paths if p.exists()),
    }


def git_head(project_dir: Path) -> str | None:
    try:
        result = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=project_dir,
            check=True,
            text=True,
            capture_output=True,
        )
        return result.stdout.strip()
    except (FileNotFoundError, subprocess.CalledProcessError):
        return None


def write_manifest(config: ComposeConfig, backup_dir: Path, counts: dict[str, int]) -> None:
    manifest = {
        "tool": "forge_backup.py",
        "tool_version": TOOL_VERSION,
        "created_at": utc_now().isoformat().replace("+00:00", "Z"),
        "source": {
            "git_head": git_head(config.project_dir),
            "compose_file": str(config.compose_file.relative_to(config.project_dir))
            if config.compose_file.is_relative_to(config.project_dir)
            else str(config.compose_file),
        },
        "services": {"postgres": "postgres", "backend": "backend", "frontend": "frontend"},
        "database": {"name": config.db_name, "user": config.db_user},
        "container_paths": {"git": DEFAULT_GIT_PATH, "artifacts": DEFAULT_ARTIFACTS_PATH},
        "contents": {
            "postgres_dump": "postgres.dump",
            "git_dir": "git",
            "artifacts_dir": "artifacts",
            "files": "files.txt",
            "checksums": "SHA256SUMS",
            **counts,
        },
        "security": {
            "env_values_included": False,
            "sensitive_values_included": False,
            "requires_external_encryption_for_offsite_copy": True,
        },
    }
    (backup_dir / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def run_git_fsck(config: ComposeConfig) -> None:
    script = (
        "set -eu; "
        f"find {DEFAULT_GIT_PATH} -type d -name '*.git' "
        "-exec git --git-dir={} fsck --no-dangling ';'"
    )
    run(config, compose(config, "run", "--rm", "--no-deps", "--user", "root", "backend", "sh", "-ceu", script))


def command_backup(args: argparse.Namespace) -> int:
    config = build_config(args)
    backup_dir = backup_dir_from_args(args)
    if config.dry_run:
        print(f"backup directory: {backup_dir}")
    else:
        ensure_empty_backup_dir(backup_dir)
        (backup_dir / "git").mkdir()
        (backup_dir / "artifacts").mkdir()

    backend_cid = capture(config, compose(config, "ps", "-q", "backend"), "DRY_RUN_BACKEND_CONTAINER")
    if not backend_cid:
        raise SystemExit("backend container not found; run `docker compose up` before backup")

    stopped = False
    try:
        if not args.no_stop:
            run(config, compose(config, "stop", "frontend", "backend"))
            stopped = True
        if not args.skip_git_fsck:
            run_git_fsck(config)
        run(
            config,
            compose(config, "exec", "-T", "postgres", "pg_dump", "-U", config.db_user, "-d", config.db_name, "--format=custom", "--no-owner"),
            stdout_path=backup_dir / "postgres.dump",
        )
        run(config, ["docker", "cp", f"{backend_cid}:{DEFAULT_GIT_PATH}/.", str(backup_dir / "git")])
        run(config, ["docker", "cp", f"{backend_cid}:{DEFAULT_ARTIFACTS_PATH}/.", str(backup_dir / "artifacts")])
    finally:
        if stopped and not args.leave_stopped:
            run(config, compose(config, "up", "-d", "backend", "frontend"))

    if not config.dry_run:
        counts = write_inventory(backup_dir)
        write_manifest(config, backup_dir, counts)
        print(f"backup created: {backup_dir}")
    return 0


def parse_checksum_line(line: str) -> tuple[str, str]:
    digest, sep, rel = line.partition("  ")
    if not sep or len(digest) != 64 or any(c not in "0123456789abcdef" for c in digest.lower()):
        raise ValueError(f"invalid checksum line: {line!r}")
    return digest.lower(), rel.strip()


def checked_relative_path(backup_dir: Path, rel: str) -> Path:
    if not rel or rel.startswith("/") or "\\" in rel or ":" in rel or rel.startswith("../") or "/../" in rel:
        raise ValueError(f"unsafe relative path in manifest: {rel!r}")
    path = (backup_dir / rel).resolve()
    if backup_dir.resolve() not in path.parents and path != backup_dir.resolve():
        raise ValueError(f"path escapes backup directory: {rel!r}")
    return path


def verify_backup_dir(backup_dir: Path) -> None:
    required = ["postgres.dump", "SHA256SUMS", "files.txt", "manifest.json", "git", "artifacts"]
    missing = [name for name in required if not (backup_dir / name).exists()]
    if missing:
        raise SystemExit(f"backup is incomplete; missing: {', '.join(missing)}")

    checksum_entries: list[tuple[str, str]] = []
    for raw_line in (backup_dir / "SHA256SUMS").read_text(encoding="utf-8").splitlines():
        if raw_line.strip():
            checksum_entries.append(parse_checksum_line(raw_line))
    if not checksum_entries:
        raise SystemExit("SHA256SUMS is empty")
    checksum_rels = [rel for _, rel in checksum_entries]
    if len(checksum_rels) != len(set(checksum_rels)):
        raise SystemExit("SHA256SUMS contains duplicate file entries")

    for expected, rel in checksum_entries:
        path = checked_relative_path(backup_dir, rel)
        if not path.is_file():
            raise SystemExit(f"missing checksummed file: {rel}")
        actual = sha256_file(path)
        if actual != expected:
            raise SystemExit(f"checksum mismatch for {rel}: expected {expected}, got {actual}")

    listed = {
        line.strip()
        for line in (backup_dir / "files.txt").read_text(encoding="utf-8").splitlines()
        if line.strip()
    }
    actual_files = {relative_posix(path, backup_dir) for path in data_files(backup_dir)}
    if listed != actual_files:
        missing = sorted(actual_files - listed)
        stale = sorted(listed - actual_files)
        raise SystemExit(f"files.txt drift; missing={missing[:5]} stale={stale[:5]}")
    expected_checksum_rels = {"postgres.dump", *actual_files}
    checksum_rel_set = set(checksum_rels)
    if checksum_rel_set != expected_checksum_rels:
        missing = sorted(expected_checksum_rels - checksum_rel_set)
        stale = sorted(checksum_rel_set - expected_checksum_rels)
        raise SystemExit(f"SHA256SUMS drift; missing={missing[:5]} stale={stale[:5]}")

    manifest = json.loads((backup_dir / "manifest.json").read_text(encoding="utf-8"))
    manifest_text = json.dumps(manifest, sort_keys=True).lower()
    forbidden = ("password", "token", "secret", "cicd_secrets_key", "database_url")
    found = [word for word in forbidden if word in manifest_text]
    if found:
        raise SystemExit(f"manifest contains forbidden secret-bearing keys/words: {', '.join(found)}")


def command_verify(args: argparse.Namespace) -> int:
    backup_dir = Path(args.backup_dir).resolve()
    verify_backup_dir(backup_dir)
    if args.pg_restore_list:
        cmd = ["pg_restore", "--list", str(backup_dir / "postgres.dump")]
        print(f"+ {quote_command(cmd)}")
        subprocess.run(cmd, check=True)
    print(f"backup verified: {backup_dir}")
    return 0


def command_self_test(_args: argparse.Namespace) -> int:
    with tempfile.TemporaryDirectory(prefix="forge-backup-selftest-") as raw_dir:
        backup_dir = Path(raw_dir)
        (backup_dir / "git").mkdir()
        (backup_dir / "artifacts").mkdir()
        (backup_dir / "postgres.dump").write_bytes(b"fake custom dump for checksum smoke")
        (backup_dir / "git" / "repo.txt").write_text("git-data\n", encoding="utf-8")
        (backup_dir / "artifacts" / "artifact.txt").write_text("artifact-data\n", encoding="utf-8")
        write_inventory(backup_dir)
        (backup_dir / "manifest.json").write_text(
            json.dumps({"tool": "self-test", "contents": {"checksums": "SHA256SUMS"}}),
            encoding="utf-8",
        )
        verify_backup_dir(backup_dir)
        checksums = (backup_dir / "SHA256SUMS").read_text(encoding="utf-8").splitlines()
        (backup_dir / "SHA256SUMS").write_text(
            "\n".join(line for line in checksums if "artifacts/artifact.txt" not in line) + "\n",
            encoding="utf-8",
        )
        try:
            verify_backup_dir(backup_dir)
        except SystemExit as exc:
            if "SHA256SUMS drift" not in str(exc):
                raise
        else:
            raise AssertionError("tampered backup without artifact checksum unexpectedly verified")
    print("backup helper self-test passed")
    return 0


def command_restore(args: argparse.Namespace) -> int:
    config = build_config(args)
    backup_dir = Path(args.backup_dir).resolve()
    if not args.confirm_restore:
        raise SystemExit("restore is destructive; pass --confirm-restore after choosing an isolated/maintenance target")
    if not config.dry_run:
        verify_backup_dir(backup_dir)
    else:
        print(f"restore source: {backup_dir}")

    run(config, compose(config, "stop", "frontend", "backend"))
    run(
        config,
        compose(config, "exec", "-T", "postgres", "pg_restore", "-U", config.db_user, "-d", config.db_name, "--clean", "--if-exists", "--no-owner"),
        stdin_path=backup_dir / "postgres.dump",
    )
    volume_script = (
        "set -eu; "
        f"mkdir -p {DEFAULT_GIT_PATH} {DEFAULT_ARTIFACTS_PATH}; "
        f"find {DEFAULT_GIT_PATH} -mindepth 1 -maxdepth 1 -exec rm -rf -- {{}} +; "
        f"find {DEFAULT_ARTIFACTS_PATH} -mindepth 1 -maxdepth 1 -exec rm -rf -- {{}} +; "
        f"tar -C /backup/git -cf - . | tar -C {DEFAULT_GIT_PATH} -xf -; "
        f"tar -C /backup/artifacts -cf - . | tar -C {DEFAULT_ARTIFACTS_PATH} -xf -"
    )
    run(
        config,
        compose(
            config,
            "run",
            "--rm",
            "--no-deps",
            "--user",
            "root",
            "-v",
            f"{backup_dir}:/backup:ro",
            "backend",
            "sh",
            "-ceu",
            volume_script,
        ),
    )
    if not args.skip_git_fsck:
        run_git_fsck(config)
    run(config, compose(config, "up", "-d", "backend", "frontend"))
    run(config, compose(config, "ps"))
    print("restore finished; run read-only API/Git/artifact smoke before accepting traffic")
    return 0


def add_common_args(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--project-dir", default=str(ROOT), help="repository root with docker-compose.yml")
    parser.add_argument("--compose-file", default="docker-compose.yml", help="compose file path")
    parser.add_argument("--env-file", default=".env", help="optional compose env file")
    parser.add_argument("--dry-run", action="store_true", help="print commands without executing them")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Forge CI/CD local backup/restore helper")
    sub = parser.add_subparsers(dest="command", required=True)

    backup = sub.add_parser("backup", help="create a PostgreSQL + Git + artifacts backup")
    add_common_args(backup)
    backup.add_argument("--backup-dir", help="destination directory; default backups/<UTC timestamp>")
    backup.add_argument("--no-stop", action="store_true", help="do not stop backend/frontend; unsafe for production snapshots")
    backup.add_argument("--leave-stopped", action="store_true", help="do not restart services after backup")
    backup.add_argument("--skip-git-fsck", action="store_true", help="skip Git repository fsck before copying")
    backup.set_defaults(func=command_backup)

    verify = sub.add_parser("verify", help="verify checksum manifest for an existing backup")
    verify.add_argument("backup_dir", help="backup directory")
    verify.add_argument("--pg-restore-list", action="store_true", help="also run local pg_restore --list on postgres.dump")
    verify.set_defaults(func=command_verify)

    self_test = sub.add_parser("self-test", help="run checksum/manifest smoke without Docker")
    self_test.set_defaults(func=command_self_test)

    restore = sub.add_parser("restore", help="restore a backup into the local compose stack")
    add_common_args(restore)
    restore.add_argument("backup_dir", help="backup directory")
    restore.add_argument("--confirm-restore", action="store_true", help="required; restore replaces DB/Git/artifact contents")
    restore.add_argument("--skip-git-fsck", action="store_true", help="skip Git fsck after restoring file data")
    restore.set_defaults(func=command_restore)

    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        return args.func(args)
    except FileNotFoundError as exc:
        raise SystemExit(f"required executable not found: {exc.filename}") from exc


if __name__ == "__main__":
    sys.exit(main())
