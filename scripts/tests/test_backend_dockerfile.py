#!/usr/bin/env python3
"""Regression checks for the runner image's Docker CLI supply-chain pin."""

from __future__ import annotations

import unittest
import re
import shlex
from pathlib import Path

DOCKERFILE = Path(__file__).resolve().parents[2] / "backend/Dockerfile.umbrella"


class BackendDockerfileTests(unittest.TestCase):
    def test_unit_ancestry_excludes_server_preparation_and_keeps_both_suites(self):
        import json
        from fnmatch import fnmatchcase
        root = DOCKERFILE.parents[1]
        descriptor = json.loads((root / '.quality-gate.json').read_text(encoding='utf-8'))
        owners = [gate for gate in descriptor['gates']
                  if 'scripts.tests.test_backend_dockerfile' in gate['argv']]
        self.assertEqual(len(owners), 1)
        self.assertEqual(owners[0]['component'], 'tooling')
        for name in ('Dockerfile', 'Dockerfile.umbrella'):
            path = 'backend/' + name
            self.assertTrue(any(fnmatchcase(path, item) for item in descriptor['components']['tooling']))
            self.assertTrue(any(fnmatchcase(path, item) for item in owners[0]['inputs']))
            with self.subTest(recipe=name):
                source = (DOCKERFILE.parent / name).read_text(encoding='utf-8')
                stages = re.split(r'(?m)^FROM ', source)[1:]
                self.assertTrue(stages[0].splitlines()[0].endswith(' AS manifests'))
                self.assertTrue(stages[0].startswith('rust:1.88.0-'))
                prepared = next(stage for stage in stages if stage.startswith('manifests AS unit-dependencies\n'))
                unit = next(stage for stage in stages if stage.startswith('unit-dependencies AS unit\n'))
                unit_path = '\n'.join((stages[0], prepared, unit))
                self.assertNotIn('cargo build', unit_path)
                self.assertEqual(set(re.findall(r'(?:--from=|\bfrom=)([A-Za-z0-9_-]+)', unit_path)), set())
                for command in ('cargo test --locked --release -p cicd-domain --lib',
                                'cargo test --locked --release --manifest-path tests/cache-protocol/Cargo.toml --lib'):
                    self.assertIn(command + ' --no-run', prepared)
                    self.assertIn(command, unit)
                self.assertIn('forge-cache-protocol-tests', prepared)
                self.assertIn("printf 'passed\\n' > /tmp/rust-unit-check", unit)
                full = next(stage for stage in stages if stage.startswith('unit-dependencies AS dependencies\n'))
                self.assertIn('cargo build --locked --release -p cicd-server --bin cicd-server --bin forge-runner', full)
                self.assertIn('cargo clean -p cicd-server', full)
                command = next(line for line in full.replace('\\\n', ' ').splitlines() if line.startswith('RUN '))
                command = re.sub(r'^RUN(?: --mount=\S+)* ', '', command)
                self.assertEqual(shlex.split(command)[0], '/usr/local/cargo/bin/cargo')
                build = next(stage for stage in stages if stage.startswith('dependencies AS build\n'))
                self.assertIn('from=unit,source=/tmp/rust-unit-check', build)

    def test_dependency_and_production_graphs_build_only_the_runtime_binaries(self) -> None:
        for name in ("Dockerfile", "Dockerfile.umbrella"):
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

    def test_runner_cli_pin_covers_the_critical_tls_fix(self) -> None:
        dockerfile = DOCKERFILE.read_text(encoding="utf-8")
        self.assertIn("ARG DOCKER_CLI_VERSION=29.5.0", dockerfile)


if __name__ == "__main__":
    unittest.main()
