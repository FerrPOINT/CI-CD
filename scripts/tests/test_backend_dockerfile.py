#!/usr/bin/env python3
"""Regression checks for runner pins and compile-time delivery witnesses."""

from __future__ import annotations

import unittest
import re
import shlex
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

DOCKERFILE = Path(__file__).resolve().parents[2] / "backend/Dockerfile.umbrella"


def staged_paths(dockerfile: str, required: set[str]) -> set[str]:
    staged = set()
    for line in dockerfile.replace("\\\n", " ").splitlines():
        if not line.startswith("COPY services-base/"):
            continue
        *sources, destination = shlex.split(line)[1:]
        for source in sources:
            for path in required:
                if path == source:
                    target = destination + Path(source).name if destination.endswith("/") else destination
                elif path.startswith(source.rstrip("/") + "/"):
                    target = destination.rstrip("/") + path[len(source.rstrip("/")):]
                else:
                    continue
                if target == "/" + path:
                    staged.add(path)
    return staged


class BackendDockerfileTests(unittest.TestCase):
    def test_all_embedded_base_helpers_are_staged_at_the_include_path(self) -> None:
        root = DOCKERFILE.parents[1]
        source = (root / "backend/src/task_delivery/postgres.rs").read_text(encoding="utf-8")
        required = set(re.findall(r'include_bytes!\("(?:\.\./)+(services-base/[^\"]+)"\)', source))
        required.update("services-base/" + path for path in re.findall(r'pinned_base_helper!\("([^\"]+)"\)', source))
        self.assertTrue(required, "The privileged coordinator must pin its Base helpers")
        for name in ("Dockerfile", "Dockerfile.umbrella"):
            dockerfile = (root / "backend" / name).read_text(encoding="utf-8")
            with self.subTest(dockerfile=name):
                self.assertFalse(
                    required - staged_paths(dockerfile, required),
                    "Docker build must stage every embedded Base helper at its include path",
                )

    def test_missing_transitive_copy_or_wrong_destination_is_detected(self) -> None:
        required = {"services-base/scripts/platform_namespace.py", "services-base/base-operator/src/pdlc_operator/backup/postgres.py"}
        copies = "COPY services-base/scripts /services-base/scripts\nCOPY services-base/base-operator/src /services-base/base-operator/src\n"
        self.assertEqual(staged_paths(copies, required), required)
        self.assertEqual(staged_paths(copies.splitlines()[0], required), {"services-base/scripts/platform_namespace.py"})
        self.assertEqual(staged_paths(copies.replace(" /services-base/", " /wrong/"), required), set())

    def test_explicit_files_are_supported_without_accepting_a_sibling(self) -> None:
        required = {"services-base/scripts/platform_namespace.py", "services-base/scripts/_platform_namespace_contract.py"}
        copies = "COPY services-base/scripts/platform_namespace.py /services-base/scripts/\n"
        self.assertEqual(staged_paths(copies, required), {"services-base/scripts/platform_namespace.py"})

    def test_dependency_and_production_graphs_build_only_the_runtime_binaries(self) -> None:
        for name in ('Dockerfile', 'Dockerfile.umbrella'):
            with self.subTest(recipe=name):
                dockerfile = (DOCKERFILE.parent / name).read_text(encoding="utf-8")
                shipped = set(re.findall(r"COPY --from=build /app/target/release/([A-Za-z0-9_-]+)", dockerfile))
                builds = re.findall(r"/usr/local/cargo/bin/cargo build ([^\n\\]+)", dockerfile)
                self.assertEqual(len(builds), 2)
                self.assertTrue(shipped)
                for build in builds:
                    arguments = shlex.split(build)
                    self.assertEqual({arguments[i + 1] for i, value in enumerate(arguments) if value == "--bin"}, shipped)
                    self.assertIn("--locked", arguments)
                    self.assertIn("--release", arguments)

    def test_sdk_dependency_copy_excludes_unused_sources_and_keeps_cli_manifest(self) -> None:
        expected = {'services-base/base-modules/rust/pdlc-' + name
                    for name in ('auth-core', 'shared', 'telemetry')}
        expected.add('services-base/base-modules/rust/pdlc-cli-core/Cargo.toml')
        for name in ('Dockerfile', 'Dockerfile.umbrella'):
            with self.subTest(recipe=name):
                source = (DOCKERFILE.parent / name).read_text(encoding='utf-8')
                dependency_stage = source.split('FROM dependencies AS unit-dependencies', 1)[0]
                actual = set(re.findall(r'^COPY (services-base/base-modules/rust\S*)\s', dependency_stage, re.M))
                self.assertEqual(actual, expected)

    def test_dependency_shell_creates_cli_metadata_target_before_selected_cargo_build(self) -> None:
        bash = Path('C:/Program Files/Git/bin/bash.exe') if os.name == 'nt' else Path(shutil.which('bash') or '/missing')
        if not bash.is_file():
            self.skipTest('Bash required for the isolated dependency command fixture')
        for name in ('Dockerfile', 'Dockerfile.umbrella'):
            with self.subTest(recipe=name), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                source = (DOCKERFILE.parent / name).read_text(encoding='utf-8')
                stage = source.split('FROM dependencies AS unit-dependencies', 1)[0]
                command = next(line for line in stage.replace('\\\n', ' ').splitlines() if line.startswith('RUN '))
                command = re.sub(r'^RUN(?: --mount=\S+)* ', '', command)
                command = command.replace('/services-base/', (root / 'services-base').as_posix() + '/')
                command = command.replace('/usr/local/cargo/bin/cargo', 'cargo')
                cli = root / 'services-base/base-modules/rust/pdlc-cli-core'
                cli.mkdir(parents=True)
                manifest = cli / 'Cargo.toml'
                manifest.write_text('[package]\nname="pdlc-cli-core"\nversion="0.1.0"\n')
                original = manifest.read_bytes()
                environment = {**os.environ, 'CLI_METADATA_TARGET': (cli / 'src/lib.rs').as_posix()}
                stub = ('cargo() { if ! grep -q "compile_error!" "$CLI_METADATA_TARGET"; then return 61; fi; '
                        'printf "%s\\n" "$*" >> cargo.log; }\n')
                result = subprocess.run([str(bash), '--noprofile', '--norc', '-euo', 'pipefail', '-c', stub + command],
                                        cwd=root, env=environment, capture_output=True, text=True, timeout=10)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertTrue((cli / 'src/lib.rs').is_file())
                self.assertIn('compile_error!("metadata-only pdlc-cli-core must not be compiled")',
                              (cli / 'src/lib.rs').read_text())
                self.assertEqual(manifest.read_bytes(), original)
                builds = [shlex.split(line) for line in (root / 'cargo.log').read_text().splitlines()
                          if line.startswith('build ')]
                self.assertEqual(len(builds), 1)
                self.assertEqual({builds[0][i + 1] for i, value in enumerate(builds[0][:-1]) if value == '--bin'},
                                 {'cicd-server', 'forge-runner'})

    def test_runner_cli_pin_covers_the_critical_tls_fix(self) -> None:
        dockerfile = DOCKERFILE.read_text(encoding="utf-8")
        self.assertIn("ARG DOCKER_CLI_VERSION=29.5.0", dockerfile)

    def test_buildx_pin_and_checksums_match_in_both_backend_recipes(self) -> None:
        blocks = []
        for name in ("Dockerfile", "Dockerfile.umbrella"):
            dockerfile = (DOCKERFILE.parent / name).read_text(encoding="utf-8")
            with self.subTest(dockerfile=name):
                self.assertEqual(dockerfile.count("ARG BUILDX_VERSION="), 1)
                block = re.search(
                    r"^# Official Buildx.*?^ && docker buildx version$",
                    dockerfile,
                    re.MULTILINE | re.DOTALL,
                )
                self.assertIsNotNone(block)
                blocks.append(block.group(0))
                self.assertIn("ARG BUILDX_VERSION=0.37.2", block.group(0))
                self.assertIn(
                    "amd64) buildx_arch=amd64; buildx_sha="
                    "982ca20490b45ed1ec8d99795974d3d874a358f75938c9c237305010e6b7e548 ;;",
                    block.group(0),
                )
                self.assertIn(
                    "arm64) buildx_arch=arm64; buildx_sha="
                    "efa38cb7aa7db2dbb9ad049b00b0a9737f66f033626177b5a4e845184ad7ab29 ;;",
                    block.group(0),
                )
                self.assertIn("*) exit 1 ;; esac", block.group(0))
        if len(blocks) == 2:
            self.assertEqual(blocks[0], blocks[1])

    def test_buildx_is_verified_and_installed_in_final_backend_stage(self) -> None:
        for name in ("Dockerfile", "Dockerfile.umbrella"):
            dockerfile = (DOCKERFILE.parent / name).read_text(encoding="utf-8")
            build, runtime = dockerfile.split("FROM debian:bookworm-slim", 1)
            with self.subTest(dockerfile=name):
                self.assertNotIn("BUILDX_VERSION", build)
                steps = [
                    'curl -fsSL "https://github.com/docker/buildx/releases/download/'
                    'v${BUILDX_VERSION}/buildx-v${BUILDX_VERSION}.linux-${buildx_arch}"',
                    'echo "${buildx_sha}  /tmp/docker-buildx" | sha256sum -c -',
                    "install -m 0755 /tmp/docker-buildx /usr/local/lib/docker/cli-plugins/docker-buildx",
                    "rm /tmp/docker-buildx",
                    "docker buildx version",
                ]
                for step in steps:
                    self.assertIn(step, runtime)
                positions = [runtime.index(step) for step in steps]
                self.assertEqual(positions, sorted(positions))


if __name__ == "__main__":
    unittest.main()
