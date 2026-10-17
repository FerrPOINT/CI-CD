"""Compiled include_str inputs must invalidate the backend artifact without a frontend build."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("cache_checker", Path(__file__).resolve().parents[1] / "check_cache_tooling.py")
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


class CacheBuildScopeTests(unittest.TestCase):
    def check(self, manifest):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "backend/src").mkdir(parents=True)
            (root / "backend/src/runner_cache.rs").write_text('const BOOTSTRAP: &str = include_str!("../../scripts/vendor/base_forge_job_cache.py");')
            (root / ".quality-gate.json").write_text(json.dumps(manifest))
            checker.check_build_scope(root)

    def manifest(self):
        return {"components": {"backend": ["backend/**", "scripts/vendor/base_forge_job_cache.py"], "frontend": ["frontend/**"]},
                "gates": [{"id": "backend:build", "inputs": ["backend/**", "scripts/vendor/base_forge_job_cache.py"]}]}

    def test_external_compiled_input_selects_backend_and_invalidates_artifact(self):
        self.check(self.manifest())

    def test_unknown_or_frontend_owned_input_is_not_accepted(self):
        for field in ("backend", "frontend"):
            value = self.manifest()
            if field == "backend": value["components"]["backend"] = ["backend/**"]
            else: value["components"]["frontend"].append("scripts/vendor/**")
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, "backend only"):
                self.check(value)

    def test_missing_or_excluded_artifact_input_cannot_reuse_an_old_binary(self):
        for mode in ("missing", "excluded"):
            value = self.manifest()
            if mode == "missing": value["gates"][0]["inputs"] = ["backend/**"]
            else: value["gates"][0]["exclude_inputs"] = ["scripts/vendor/**"]
            with self.subTest(mode=mode), self.assertRaisesRegex(ValueError, "artifact identity"):
                self.check(value)

    def test_checked_in_manifest_covers_every_real_compiled_cache_input(self):
        checker.check_build_scope(Path(__file__).resolve().parents[2])


if __name__ == "__main__":
    unittest.main()
