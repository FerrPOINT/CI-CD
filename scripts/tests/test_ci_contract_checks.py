"""Regression checks for isolated CLI integration gates."""

import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


class CliIntegrationGateTests(unittest.TestCase):
    def test_cli_database_is_scoped_to_cli_step(self) -> None:
        workflow = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        backend = workflow.split("\n  backend:\n", 1)[1].split("\n  frontend:\n", 1)[0]
        job_env = backend.split("\n    env:\n", 1)[1].split("\n    steps:\n", 1)[0]
        self.assertIn("/forge_test_cicd", job_env)
        self.assertNotIn("forge_test_cli", job_env)
        self.assertIn("createdb -h 127.0.0.1 -p 5432 -U forge_owner forge_test_cli", backend)
        cli_step = backend.split("- name: CLI tests (real PostgreSQL)", 1)[1].split("- name:", 1)[0]
        self.assertIn("/forge_test_cli", cli_step)
        self.assertIn(
            "cargo test --locked -p cicd-cli --features integration --test cli_real_api -- --test-threads=1",
            cli_step,
        )
        self.assertIn(
            "cargo clippy --locked -p cicd-cli --all-targets --features integration -- -D warnings",
            backend,
        )
        self.assertIn("cargo test --locked --features integration --test integration_db -- --test-threads=1", backend)

    def test_docs_job_runs_cli_gate_regressions(self) -> None:
        workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        docs = workflow.split("\n  docs:\n", 1)[1].split("\n  backend:\n", 1)[0]
        self.assertIn("scripts.tests.test_ci_contract_checks", docs)

class ServerCliMetadataTests(unittest.TestCase):
    def test_server_graph_keeps_unselected_cli_metadata_and_a_compilation_guard(self):
        import re
        import shlex
        import tomllib
        from pathlib import Path

        product = Path(__file__).resolve().parents[2]
        backend = product / 'backend'
        workspace = tomllib.loads((backend / 'Cargo.toml').read_text(encoding='utf-8'))
        cli = tomllib.loads((backend / 'cli/Cargo.toml').read_text(encoding='utf-8'))
        self.assertEqual(cli['bin'], [{'name': 'cicd-cli', 'path': 'src/main.rs'}])
        pending, seen = [backend], set()
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
                        if isinstance(dependency, dict) and 'path' in dependency:
                            path = (owner / dependency['path']).resolve()
                            if path.is_relative_to(backend):
                                pending.append(path)
        self.assertNotIn((backend / 'cli').resolve(), seen,
                         'A production CLI dependency requires a new source closure')
        self.assertTrue((backend / 'cli/src/main.rs').is_file(), 'Real CLI remains available separately')
        for name in ('Dockerfile', 'Dockerfile.umbrella'):
            with self.subTest(recipe=name):
                text = (backend / name).read_text(encoding='utf-8')
                copied = [item for line in text.splitlines() if line.startswith('COPY ')
                          for item in shlex.split(line)[1:-1] if item.startswith('CI-CD/backend/cli')]
                self.assertEqual(copied, ['CI-CD/backend/cli/Cargo.toml'])
                self.assertIn('compile_error!("metadata-only cicd-cli must not be compiled");', text)
                self.assertNotRegex(text, r'rm -rf --[^\n]*\bcli/src\b')
                self.assertNotIn('> cli/src/lib.rs', text, 'Do not introduce an empty auto-library')
                shipped = set(re.findall(r'COPY --from=build /app/target/release/([A-Za-z0-9_-]+)', text))
                self.assertEqual(shipped, {'cicd-server', 'forge-runner'})
                builds = re.findall(r'(?:/usr/local/cargo/bin/)?cargo build ([^\n\\]+)', text)
                self.assertEqual(len(builds), 2)
                for command in builds:
                    args = shlex.split(command)
                    self.assertEqual({args[i + 1] for i, value in enumerate(args) if value == '--bin'}, shipped)
                    self.assertIn('--locked', args)
                    self.assertIn('--release', args)


if __name__ == "__main__":
    unittest.main()
