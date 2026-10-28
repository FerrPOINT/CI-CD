"""Source contracts: bin test edits do not select an application producer."""
import json
from pathlib import Path
import shlex
import unittest

from scripts.vendor import base_quality_selection as selection

ROOT = Path(__file__).resolve().parents[2]
PAIRS = (("forge-runner", "forge_runner"), ("forge-delivery", "forge_delivery"))
OWNER = "tooling:bin-test-inputs"


class BinTestInputContracts(unittest.TestCase):
    def pairs(self):
        self.assertTrue((ROOT / "backend/src/bin/forge-runner.rs").is_file())
        return [(ROOT / f"backend/src/bin/{name}.rs", f"tests/{module}.rs")
                for name, module in PAIRS if (ROOT / f"backend/src/bin/{name}.rs").is_file()]

    def artifacts(self):
        descriptor = json.loads((ROOT / ".quality-gate.json").read_text(encoding="utf-8"))
        return [gate for gate in descriptor["gates"] if gate.get("kind") == "artifact"]

    def owners(self, source):
        path = source.relative_to(ROOT).as_posix()
        return [gate for gate in self.artifacts()
                if selection.matches(path, gate.get("inputs", []))
                and not selection.matches(path, gate.get("exclude_inputs", []))]

    def test_external_test_modules_keep_cfg_and_namespace(self):
        for source, child in self.pairs():
            with self.subTest(source=source.name):
                text = source.read_text(encoding="utf-8")
                self.assertTrue('#[cfg(test)]\n#[path = "' + child + '"]\nmod tests;' in text, source.name)
                self.assertNotIn("mod tests {", text)
                test = source.parent / child
                self.assertTrue(test.is_file())
                self.assertIn("#[", test.read_text(encoding="utf-8"))
        if not (ROOT / "backend/src/bin/forge-delivery.rs").exists():
            self.assertFalse((ROOT / "backend/src/bin/tests/forge_delivery.rs").exists())

    def test_production_copy_excludes_only_known_test_modules(self):
        for filename in ("Dockerfile", "Dockerfile.umbrella"):
            recipe = (ROOT / "backend" / filename).read_text(encoding="utf-8")
            copies = [shlex.split(line) for line in recipe.splitlines() if line.startswith("COPY ")]
            source = [row for row in copies if row[-2:] == ["CI-CD/backend/src", "./src"]]
            self.assertEqual(len(source), 1)
            expected = {"--exclude=**/src/bin/" + child for _parent, child in self.pairs()}
            self.assertEqual({token for token in source[0] if token.startswith("--exclude=")}, expected)
            self.assertTrue(recipe.startswith("# syntax=docker/dockerfile:1.27.1@sha256:"))

    def test_test_changes_keep_units_and_static_owner_without_app_build(self):
        for source, child in self.pairs():
            changed = (source.parent / child).relative_to(ROOT).as_posix()
            plan = selection.plan(ROOT, profile="build", changed=[changed])
            ids = {gate["id"] for gate in plan["gates"]}
            self.assertIn("backend:unit", ids)
            self.assertIn(OWNER, ids)
            self.assertFalse({gate["id"] for gate in self.artifacts()} & ids)
            owner = next(gate for gate in plan["gates"] if gate["id"] == OWNER)
            self.assertEqual(owner["execution_slot"], "none")

    def test_production_neighbors_still_select_the_application(self):
        for source, _child in self.pairs():
            changed = source.relative_to(ROOT).as_posix()
            ids = {gate["id"] for gate in selection.plan(ROOT, profile="build", changed=[changed])["gates"]}
            owners = {gate["id"] for gate in self.owners(source)}
            self.assertTrue(owners, source.name)
            self.assertTrue(owners <= ids, (source.name, owners, ids))
            self.assertIn(OWNER, ids)

    def test_existing_artifact_owner_excludes_the_new_test_files(self):
        for source, child in self.pairs():
            test = (source.parent / child).relative_to(ROOT).as_posix()
            owners = self.owners(source)
            self.assertTrue(owners, source.name)
            for gate in owners:
                if selection.matches(test, gate.get("inputs", [])):
                    self.assertTrue(selection.matches(test, gate.get("exclude_inputs", [])))


if __name__ == "__main__":
    unittest.main()
