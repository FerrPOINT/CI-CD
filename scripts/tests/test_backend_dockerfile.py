#!/usr/bin/env python3
"""Regression checks for runner pins and compile-time delivery witnesses."""

from __future__ import annotations

import unittest
import re
import shlex
from pathlib import Path

DOCKERFILE = Path(__file__).resolve().parents[2] / "backend/Dockerfile.umbrella"


def staged_paths(
    dockerfile: str, required: set[str], destinations: dict[str, str] | None = None
) -> set[str]:
    staged = set()
    for line in dockerfile.replace("\\\n", " ").splitlines():
        if not line.startswith("COPY "):
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
                if target == (destinations or {}).get(path, "/" + path):
                    staged.add(path)
    return staged


class BackendDockerfileTests(unittest.TestCase):
    def test_sdk_copy_matches_production_path_graph_and_preserves_cli_metadata(self):
        import tomllib

        product = DOCKERFILE.parents[1]
        backend, base = product / 'backend', product.parent / 'services-base'
        workspace = tomllib.loads((backend / 'Cargo.toml').read_text(encoding='utf-8'))
        pending, seen, required = [backend], set(), set()
        while pending:
            directory = pending.pop().resolve()
            if directory in seen:
                continue
            seen.add(directory)
            manifest = tomllib.loads((directory / 'Cargo.toml').read_text(encoding='utf-8'))
            for table in [manifest, *manifest.get('target', {}).values()]:
                for kind in ('dependencies', 'build-dependencies'):
                    for name, dependency in table.get(kind, {}).items():
                        if not isinstance(dependency, dict):
                            continue
                        owner = directory
                        if dependency.get('workspace'):
                            dependency = workspace['workspace']['dependencies'][name]
                            owner = backend
                        if not isinstance(dependency, dict) or 'path' not in dependency:
                            continue
                        path = (owner / dependency['path']).resolve()
                        if path.is_relative_to(base):
                            required.add(path.relative_to(base).as_posix())
                        else:
                            self.assertTrue(path.is_relative_to(backend), 'Unreviewed local source: ' + str(path))
                            pending.append(path)
        self.assertEqual(required, {'base-modules/rust/pdlc-auth-core',
                                    'base-modules/rust/pdlc-shared',
                                    'base-modules/rust/pdlc-telemetry'})
        self.assertNotIn((backend / 'cli').resolve(), seen)
        cli = tomllib.loads((backend / 'cli/Cargo.toml').read_text(encoding='utf-8'))
        self.assertEqual(cli['dependencies']['pdlc-cli-core']['path'],
                         '../../../services-base/base-modules/rust/pdlc-cli-core')
        for name in ('Dockerfile', 'Dockerfile.umbrella'):
            with self.subTest(recipe=name):
                text = (backend / name).read_text(encoding='utf-8')
                copied = set(re.findall(r'^COPY services-base/(base-modules/rust\S*)\s', text, re.M))
                self.assertEqual(copied, required | {'base-modules/rust/pdlc-cli-core/Cargo.toml'})
                self.assertIn('compile_error!("metadata-only pdlc-cli-core must not be compiled");', text)
                self.assertIn('> /services-base/base-modules/rust/pdlc-cli-core/src/lib.rs', text)

    def test_embedded_workspace_validators_are_staged_in_both_recipes(self) -> None:
        root = DOCKERFILE.parents[1]
        source = (root / "backend/src/runner_image_build/workspace_deploy.rs").read_text(encoding="utf-8")
        names = set(re.findall(r'include_str!\(\s*"(?:\.\./)+scripts/([^"/]+\.py)"\s*\)', source))
        self.assertTrue(names, "Workspace validation must embed its pinned scripts")
        required = {"CI-CD/scripts/" + name for name in names}
        destinations = {path: "/scripts/" + Path(path).name for path in required}
        for name in ("Dockerfile", "Dockerfile.umbrella"):
            dockerfile = (root / "backend" / name).read_text(encoding="utf-8")
            with self.subTest(dockerfile=name):
                self.assertFalse(required - staged_paths(dockerfile, required, destinations))

    def test_workspace_validator_interpreter_is_installed_in_both_runtime_stages(self) -> None:
        for name in ("Dockerfile", "Dockerfile.umbrella"):
            dockerfile = (DOCKERFILE.parent / name).read_text(encoding="utf-8")
            runtime = dockerfile.split("FROM debian:bookworm-slim", 1)[1].replace("\\\n", " ")
            packages = [shlex.split(command) for command in re.findall(r"apt-get install[^&;\n]+", runtime)]
            with self.subTest(dockerfile=name):
                self.assertTrue(any("python3" in command for command in packages))

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
