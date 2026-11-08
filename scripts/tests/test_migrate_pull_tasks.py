"""Confirmed historical mappings must survive unknown acceptance without guessing."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "migrate_pull_tasks", Path(__file__).parents[1] / "migrate_pull_tasks.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ConfirmedMappingTests(unittest.TestCase):
    def setUp(self):
        self.task = {"tracker_instance_id": "tracker", "task_id": "task"}
        self.entry = {"repository_id": "repo", "pull_request_id": "pull", "number": 1,
                      "expected_updated_at": "original", "confirmed_task": self.task, "reason": "Owner confirmed"}
        self.current = {"id": "pull", "updated_at": "original", "primary_task_id": None}
        self.posts = 0

    def call(self, path, body=None):
        if body is not None:
            self.posts += 1
            self.current.update(primary_task_id=body["task_id"],
                                primary_tracker_instance_id=body["tracker_instance_id"])
            raise TimeoutError("Accepted but response lost")
        return dict(self.current)

    def test_unknown_acceptance_reads_back_and_replay_does_not_write(self):
        self.assertEqual(module.apply_mapping(self.call, [self.entry]), 1)
        self.assertEqual(module.apply_mapping(self.call, [self.entry]), 1)
        self.assertEqual(self.posts, 1)

    def test_changed_pr_is_not_reassigned(self):
        self.current["updated_at"] = "changed"
        with self.assertRaisesRegex(ValueError, "PR changed"):
            module.apply_mapping(self.call, [self.entry])
        self.assertEqual(self.posts, 0)

    def test_mapping_requires_explicit_task_and_reason(self):
        for field, value in (("reason", ""), ("confirmed_task", None)):
            with self.subTest(field=field), self.assertRaises(ValueError):
                module.apply_mapping(self.call, [{**self.entry, field: value}])
        self.assertEqual(self.posts, 0)

    def test_unresolved_acceptance_is_not_blindly_retried(self):
        def unavailable(path, body=None):
            if body is not None:
                self.posts += 1
                raise TimeoutError()
            return self.current
        with self.assertRaisesRegex(RuntimeError, "unresolved"):
            module.apply_mapping(unavailable, [self.entry])
        self.assertEqual(self.posts, 1)

    def test_inventory_walks_repository_and_link_pages_and_never_selects_a_task(self):
        calls = []
        def read(path):
            calls.append(path)
            if path == "/catalog/repositories?limit=100&offset=0":
                return {"items": [{"id": "repo"}], "total": 2}
            if path == "/catalog/repositories?limit=100&offset=1":
                return {"items": [{"id": "second"}], "total": 2}
            if path == "/catalog/repositories/repo/pulls":
                return [{"id": "pull", "number": 1, "updated_at": "original"}]
            if path == "/catalog/repositories/second/pulls":
                return []
            if path.endswith("offset=0"):
                return [{"task": self.task}] * 100
            if path.endswith("offset=100"):
                return [{"task": {**self.task, "task_id": "different"}}]
            self.fail(path)
        entries = module.inventory(read)
        self.assertEqual(len(entries), 1)
        self.assertEqual(len(entries[0]["verified_links"]), 101)
        self.assertIsNone(entries[0]["confirmed_task"])
        self.assertEqual(entries[0]["reason"], "")
        self.assertIn("/catalog/repositories?limit=100&offset=1", calls)


if __name__ == "__main__":
    unittest.main()
