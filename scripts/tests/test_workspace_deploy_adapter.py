"""Pure trusted-control assembly tests; no daemon or execution is simulated as success."""

from dataclasses import asdict, replace
import hashlib
import ipaddress
import os
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
from uuid import UUID

from scripts.tests.test_workspace_deploy_admission import APP, DATABASE, SOURCE, encoded, service
from scripts.workspace_deploy_adapter import (
    OwnerComposeSource, OwnerImageFacts, OwnerRuntime, read_only_inventory,
    resolve_services, verify_compose_source,
)
from scripts.workspace_deploy_admission import OwnerSeal, Rejected


class WorkspaceDeployAdapterTests(unittest.TestCase):
    def setUp(self):
        self.compose = {"services": {"application": service(APP), "database": service(DATABASE)},
                        "volumes": {"data": {}}}
        self.compose["services"]["application"].update(environment={"DATABASE_URL": "${APP_DB_URL}"},
                                                     depends_on=["database"], read_only=True)
        self.compose["services"]["database"]["volumes"] = [
            {"type": "volume", "source": "data", "target": "/var/lib/database"}]
        self.images = {"schema": "forge/workspace-images/v1", "sourceCommit": SOURCE,
                       "images": {"application": APP, "database": DATABASE}}
        self.producers = {}
        self.owner = OwnerRuntime(
            UUID("11111111-1111-4111-8111-111111111111"), UUID("22222222-2222-4222-8222-222222222222"),
            "owner-control", ipaddress.ip_network("10.246.200.0/24"), True,
            {"application": "65532:65532", "database": "999:999"},
            {"application": frozenset(("/tmp",)), "database": frozenset(("/tmp", "/run/database"))},
            {"application": OwnerImageFacts(APP, "65532:65532", frozenset(), "linux", "amd64"),
             "database": OwnerImageFacts(DATABASE, "999:999", frozenset(("/var/lib/database",)),
                                         "linux", "amd64")},
            {"APP_DB_URL": "synthetic-database-url-$literal", "DOCKER_HOST": "unused-host-credential"},
            1024**3, 2, 256)

    def source(self, raw):
        return OwnerComposeSource(
            self.owner.job_id, self.owner.attempt_id, SOURCE, "deploy/task.compose.json",
            hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw,
                         usedforsecurity=False).hexdigest(),
            hashlib.sha256(raw).hexdigest(), "f" * 64)

    def inventory_input(self):
        raw, images = encoded(self.compose), encoded(self.images)
        source = asdict(self.source(raw))
        source.update(job_id=str(self.owner.job_id), attempt_id=str(self.owner.attempt_id))
        return {"compose": raw.decode(), "manifest": images.decode(), "source": source,
                "job_id": str(self.owner.job_id), "attempt_id": str(self.owner.attempt_id),
                "image_facts": {name: {**asdict(facts), "volumes": sorted(facts.volumes)}
                                for name, facts in self.owner.image_facts.items()},
                "seal": {"source_commit": SOURCE, "compose_sha256": hashlib.sha256(raw).hexdigest(),
                         "images_manifest_sha256": hashlib.sha256(images).hexdigest(),
                         "images": dict(self.images["images"]),
                         "environment": {"application": ["DATABASE_URL"]},
                         "secret_names": ["APP_DB_URL"], "build_producers": dict(self.producers)}}

    def bridge(self, payload, cwd=None, operation="inspect"):
        root = Path(__file__).resolve().parents[2]
        bundle = {name: (root / "scripts" / (name + ".py")).read_text(encoding="utf-8")
                  for name in ("workspace_deploy_admission", "workspace_deploy_adapter")}
        bundle["input"] = payload
        bundle["operation"] = operation
        process = subprocess.run([sys.executable, "-I", "-c",
                                  (root / "scripts/workspace_deploy_bridge.py").read_text(encoding="utf-8")],
                                 input=json.dumps(bundle).encode(), stdout=subprocess.PIPE,
                                 stderr=subprocess.PIPE, timeout=10, check=True, cwd=cwd)
        self.assertEqual(process.stderr, b"")
        self.assertLess(len(process.stdout), 4096)
        return json.loads(process.stdout)

    def prepare(self):
        payload = self.inventory_input()
        payload.update(control_volume=self.owner.control_volume,
                       project_secret_values={"APP_DB_URL": self.owner.secret_values["APP_DB_URL"]})
        return self.bridge(payload, operation="prepare")

    def test_real_prepare_bridge_renders_owned_services_and_volume_users_without_claiming_started(self):
        result = self.prepare()
        self.assertEqual(result["status"], "prepared")
        self.assertEqual(result["volume_users"], {"data": "999:999"})
        fragment = json.loads(result["rendered"])
        self.assertNotIn("ipam", fragment["networks"]["job"])
        self.assertEqual(fragment["services"]["database"]["user"], "999:999")
        self.assertEqual(fragment["services"]["application"]["environment"],
                         {"DATABASE_URL": "synthetic-database-url-$literal"})
        self.assertNotIn("runtime_state", result)
        self.assertNotIn("verified", result)

    def test_upstream_extension_resolves_only_exact_owner_producer_and_is_stripped(self):
        service = self.compose["services"]["application"]
        service.pop("image")
        service["x-forge-image-build"] = "package.App"
        self.producers = {"application": "package.App"}
        original = encoded(self.compose)
        result = self.prepare()
        self.assertEqual(result["status"], "prepared")
        rendered = json.loads(result["rendered"])["services"]["application"]
        self.assertEqual(rendered["image"], APP)
        self.assertNotIn("x-forge-image-build", rendered)
        self.assertEqual(encoded(self.compose), original)
        self.producers = {"application": "foreign"}
        self.assertEqual(self.prepare()["status"], "held")
        self.producers = {}
        self.assertEqual(self.prepare()["status"], "held")

    def test_image_source_extension_is_exclusive_and_not_a_new_uri_or_interpolation(self):
        for value in (None, "${IMAGE}", "http://producer", "../job", {"verified": True}):
            with self.subTest(value=value):
                self.setUp()
                self.compose["services"]["application"].pop("image")
                self.compose["services"]["application"]["x-forge-image-build"] = value
                self.assertEqual(self.prepare()["status"], "held")
        self.setUp()
        self.compose["services"]["application"]["x-forge-image-build"] = "package"
        self.producers = {"application": "package"}
        self.assertEqual(self.prepare()["status"], "held")

    def test_explicit_repo_numeric_user_override_works_without_guessing_image_uid(self):
        self.owner = replace(self.owner, image_facts={**self.owner.image_facts,
            "database": replace(self.owner.image_facts["database"], default_user="")})
        self.assertEqual(self.prepare()["status"], "held")
        self.compose["services"]["database"]["user"] = "999:999"
        result = self.prepare()
        self.assertEqual(result["status"], "prepared")
        self.assertEqual(result["volume_users"], {"data": "999:999"})
        for user in ("root", "0:0", "999", "999:0", "0:999", "999:999999999999"):
            with self.subTest(user=user):
                self.compose["services"]["database"]["user"] = user
                self.assertEqual(self.prepare()["status"], "held")

    def test_prepare_rejects_conflicting_volume_owners_and_missing_secret_readback(self):
        self.compose["services"]["application"]["volumes"] = [
            {"type": "volume", "source": "data", "target": "/app-data"}]
        self.assertEqual(self.prepare()["status"], "held")
        self.setUp()
        payload = self.inventory_input()
        payload.update(control_volume=self.owner.control_volume, project_secret_values={})
        self.assertEqual(self.bridge(payload, operation="prepare")["status"], "held")

    def test_actual_inventory_bridge_holds_named_volume_preparation_without_fake_success(self):
        expected = {"status": "held", "reason": "owner_volume_user_preparation_unavailable"}
        self.assertEqual(read_only_inventory(self.inventory_input()), expected)
        self.assertEqual(self.bridge(self.inventory_input()), expected)

    def test_inventory_bridge_does_not_resolve_declared_secret_values(self):
        self.compose["services"]["database"].pop("volumes")
        self.compose.pop("volumes")
        self.owner = replace(self.owner, image_facts={**self.owner.image_facts,
            "database": replace(self.owner.image_facts["database"], volumes=frozenset())})
        with patch.dict(os.environ, {"APP_DB_URL": "host-secret-must-never-be-used"}):
            self.assertEqual(self.bridge(self.inventory_input()),
                             {"status": "held", "reason": "project_secret_readback_unavailable"})

    def test_inventory_bridge_cannot_admit_even_a_valid_volumeless_secretless_recipe(self):
        self.compose["services"]["database"].pop("volumes")
        self.compose["services"]["application"].pop("environment")
        self.compose.pop("volumes")
        self.owner = replace(self.owner, image_facts={**self.owner.image_facts,
            "database": replace(self.owner.image_facts["database"], volumes=frozenset())})
        self.assertEqual(self.bridge(self.inventory_input()),
                         {"status": "held", "reason": "owner_network_lifecycle_unavailable"})

    def test_inventory_bridge_rejects_forged_identity_or_missing_actual_config(self):
        for field in ("source", "image_facts"):
            payload = self.inventory_input()
            payload[field] = {"verified": True}
            with self.subTest(field=field):
                self.assertEqual(self.bridge(payload), {"status": "held", "reason": "recipe_rejected"})
        payload = self.inventory_input()
        payload["image_facts"]["database"]["image_id"] = APP
        self.assertEqual(self.bridge(payload), {"status": "held", "reason": "recipe_rejected"})

    def test_inventory_bridge_rejects_truncated_or_oversize_stdin_with_bounded_safe_output(self):
        root = Path(__file__).resolve().parents[2]
        code = (root / "scripts/workspace_deploy_bridge.py").read_text(encoding="utf-8")
        for raw in (b"{", b"x" * (1024 * 1024 + 1)):
            with self.subTest(size=len(raw)):
                result = subprocess.run([sys.executable, "-I", "-c", code], input=raw,
                                        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                        timeout=10, check=True)
                self.assertEqual(result.stderr, b"")
                self.assertEqual(json.loads(result.stdout), {"status": "held", "reason": "recipe_rejected"})

    def test_inventory_bridge_uses_compiled_sources_not_cwd_imports(self):
        with tempfile.TemporaryDirectory(prefix="forge-untrusted-import-") as temp:
            scripts = Path(temp) / "scripts"
            scripts.mkdir()
            (scripts / "__init__.py").write_text("raise RuntimeError('untrusted import')", encoding="utf-8")
            (scripts / "workspace_deploy_admission.py").write_text("raise RuntimeError('untrusted import')",
                                                                  encoding="utf-8")
            self.assertEqual(self.bridge(self.inventory_input(), cwd=temp),
                             {"status": "held", "reason": "owner_volume_user_preparation_unavailable"})

    def test_inventory_bridge_omitted_user_is_root_preparation_not_guessed_uid(self):
        payload = self.inventory_input()
        payload["image_facts"]["database"]["default_user"] = None
        self.assertEqual(self.bridge(payload),
                         {"status": "held", "reason": "owner_volume_user_preparation_unavailable"})

    def test_inventory_bridge_rejects_implicit_anonymous_volume_and_undeclared_secret(self):
        payload = self.inventory_input()
        payload["image_facts"]["application"]["volumes"] = ["/hidden"]
        self.assertEqual(self.bridge(payload), {"status": "held", "reason": "recipe_rejected"})
        payload = self.inventory_input()
        payload["seal"]["secret_names"] = []
        self.assertEqual(self.bridge(payload), {"status": "held", "reason": "recipe_rejected"})

    def resolve(self, owner=None, source=None):
        raw, images = encoded(self.compose), encoded(self.images)
        seal = OwnerSeal(SOURCE, hashlib.sha256(raw).hexdigest(), hashlib.sha256(images).hexdigest(),
                         dict(self.images["images"]), {"application": frozenset(("DATABASE_URL",))},
                         frozenset(("APP_DB_URL",)), dict(self.producers))
        return resolve_services(raw, images, seal, owner or self.owner, source or self.source(raw))

    def test_git_projection_required_before_private_secret_resolution(self):
        with self.assertRaisesRegex(Rejected, "source projection"):
            self.resolve(source={"verified": True})

    def test_git_projection_cannot_move_to_another_attempt_or_job(self):
        source = self.source(encoded(self.compose))
        for field in ("job_id", "attempt_id"):
            with self.subTest(field=field), self.assertRaisesRegex(Rejected, "attempt mismatch"):
                self.resolve(source=replace(source, **{field: UUID(int=9)}))

    def test_wrong_source_commit_or_plan_hash_is_held(self):
        source = self.source(encoded(self.compose))
        for changes in ({"source_commit": "b" * 40}, {"source_commit": "main"},
                        {"source_commit": None}, {"plan_sha256": None},
                        {"plan_sha256": ""}, {"plan_sha256": "G" * 64}):
            with self.subTest(changes=changes), self.assertRaisesRegex(Rejected, "commit/plan"):
                self.resolve(source=replace(source, **changes))

    def test_actual_sha256_and_git_blob_identity_are_both_required(self):
        source = self.source(encoded(self.compose))
        for changes in ({"sha256": "0" * 64}, {"blob_oid": "0" * 40}):
            with self.subTest(changes=changes), self.assertRaisesRegex(Rejected, "Git blob"):
                self.resolve(source=replace(source, **changes))
        self.compose["services"]["application"]["cpus"] = 0.7
        with self.assertRaisesRegex(Rejected, "Git blob"):
            self.resolve(source=source)

    def test_git_source_path_cannot_be_a_host_path_or_mutable_ref(self):
        source = self.source(encoded(self.compose))
        for path in ("/host", "../compose.json", "a//b", "C:/host", "a/./b", "a\\b", "",
                     "-option", "a/\x7fb", "a/\ud800b", "a" * 1025):
            with self.subTest(path=path), self.assertRaisesRegex(Rejected, "source path"):
                self.resolve(source=replace(source, compose_path=path))

    def test_source_path_validation_keeps_existing_portable_unicode_contract(self):
        self.resolve(source=replace(self.source(encoded(self.compose)),
                                    compose_path="deploy/\u0442\u0435\u0441\u0442.compose.json"))

    def test_git_source_byte_bound_is_checked_before_hashing_or_secret_resolution(self):
        raw = encoded(self.compose)
        seal = OwnerSeal(SOURCE, hashlib.sha256(raw).hexdigest(), "0" * 64, {}, {}, frozenset())
        for value in (b"", b"x" * (256 * 1024 + 1), raw.decode(), bytearray(raw)):
            with self.subTest(kind=type(value)), self.assertRaisesRegex(Rejected, "byte bound"):
                verify_compose_source(value, seal, self.owner, self.source(raw))

    def test_real_git_blob_projection_is_independent_of_mutable_checkout_bytes(self):
        # Exercises the pure bridge with actual Git objects, not the uncompiled Rust reader.
        raw = encoded(self.compose)
        with tempfile.TemporaryDirectory(prefix="forge-compose-source-") as temp:
            repo = Path(temp) / "source.git"
            environment = {key: value for key, value in os.environ.items()
                           if not key.startswith("GIT_")}
            environment.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
                               GIT_AUTHOR_NAME="Fixture", GIT_AUTHOR_EMAIL="fixture@example.invalid",
                               GIT_COMMITTER_NAME="Fixture", GIT_COMMITTER_EMAIL="fixture@example.invalid")

            def git(*args, data=None):
                return subprocess.run(["git", "--no-replace-objects", "--git-dir", str(repo),
                                       *args], input=data, stdout=subprocess.PIPE,
                                      stderr=subprocess.PIPE, env=environment,
                                      check=True, timeout=10).stdout

            git("init", "--bare", str(repo))
            oid = git("hash-object", "-w", "--stdin", data=raw).decode().strip()
            tree = git("mktree", data=f"100644 blob {oid}\ttask.compose.json\n".encode()).decode().strip()
            commit = git("commit-tree", tree, data=b"synthetic source\n").decode().strip()
            actual = git("cat-file", "blob", f"{commit}:task.compose.json")
            source = replace(self.source(raw), source_commit=commit,
                             compose_path="task.compose.json", blob_oid=oid)
            seal = OwnerSeal(commit, hashlib.sha256(raw).hexdigest(), "0" * 64, {}, {}, frozenset())
            verify_compose_source(actual, seal, self.owner, source)
            with self.assertRaisesRegex(Rejected, "Git blob"):
                verify_compose_source(actual + b" ", seal, self.owner, source)

    def test_owner_injects_complete_private_security_and_resource_identity(self):
        result = self.resolve()
        document = result.owner_document()
        project = "sdlc-build-job-" + self.owner.attempt_id.hex
        self.assertEqual(document["name"], project)
        self.assertEqual(document["networks"]["job"]["ipam"]["config"], [{"subnet": "10.246.200.0/24"}])
        self.assertEqual(document["volumes"]["data"]["name"], project + "_data")
        self.assertEqual(document["services"]["database"]["volumes"][0]["volume"], {"nocopy": True})
        for name, item in document["services"].items():
            self.assertEqual(item["image"], self.images["images"][name])
            self.assertEqual(item["user"], self.owner.users[name])
            self.assertEqual(item["cap_drop"], ["ALL"])
            self.assertEqual(item["security_opt"], ["no-new-privileges:true"])
            self.assertEqual(item["networks"], ["job"])
            self.assertEqual(item["pull_policy"], "never")
            self.assertEqual(item["restart"], "no")
            self.assertTrue(item["read_only"])
            self.assertEqual(item["labels"]["sdlc.attempt"], str(self.owner.attempt_id))
            self.assertEqual(item["container_name"], project + "-" + name)
            self.assertNotIn("ports", item)
            self.assertNotIn("entrypoint", item)

    def test_only_declared_project_values_are_injected_without_host_inheritance(self):
        result = self.resolve()
        document = result.owner_document()
        env = document["services"]["application"]["environment"]
        self.assertEqual(env, {"DATABASE_URL": "synthetic-database-url-$literal"})
        self.assertNotIn("unused-host-credential", str(document))
        self.assertNotIn("synthetic-database", repr(result))
        self.assertNotIn("synthetic-database", repr(self.owner))
        # Existing RemoteJob.literal/save_manifest, not the job, owns final escaping.
        self.assertIn("${APP_DB_URL}", encoded(self.compose).decode())

    def test_private_result_returns_copies_not_mutable_owner_state(self):
        result = self.resolve()
        document = result.owner_document()
        document["services"].clear()
        self.assertEqual(len(result.owner_document()["services"]), 2)

    def test_absent_project_secret_holds_instead_of_reading_process_environment(self):
        with patch.dict(os.environ, {"APP_DB_URL": "host-must-not-be-used"}), \
                self.assertRaisesRegex(Rejected, "secret value unavailable"):
            self.resolve(replace(self.owner, secret_values={}))

    def test_nonroot_complete_user_policy_required(self):
        for user in ("0:0", "0:999", "999:0", "root", "65532", "999:9999999999"):
            with self.subTest(user=user), self.assertRaises(Rejected):
                self.resolve(replace(self.owner, users={"application": user, "database": "999:999"}))
        with self.assertRaises(Rejected):
            self.resolve(replace(self.owner, users={"application": "65532:65532"}))

    def test_owner_cannot_publish_incomplete_image_volume_observation(self):
        facts = self.owner.image_facts
        for observation in ({"application": facts["application"]},
                            {**facts, "application": replace(facts["application"],
                                                             volumes=frozenset(("/hidden",)))}):
            with self.subTest(facts=observation), self.assertRaises(Rejected):
                self.resolve(replace(self.owner, image_facts=observation))

    def test_dependency_image_ids_do_not_substitute_for_actual_owner_config_facts(self):
        for observation in ({"application": APP, "database": DATABASE},
                            {name: {"verified": True} for name in self.owner.image_facts}):
            with self.subTest(facts=observation), self.assertRaisesRegex(Rejected, "Config facts"):
                self.resolve(replace(self.owner, image_facts=observation))

    def test_dependency_image_facts_must_match_the_sealed_id_and_supported_platform(self):
        facts = self.owner.image_facts["application"]
        for changes in ({"image_id": DATABASE}, {"os": "windows"}, {"architecture": "unknown"},
                        {"default_user": {"verified": True}},
                        {"volumes": []}):
            with self.subTest(changes=changes), self.assertRaisesRegex(Rejected, "Config facts"):
                self.resolve(replace(self.owner, image_facts={**self.owner.image_facts,
                    "application": replace(facts, **changes)}))

    def test_root_or_named_image_defaults_are_not_guessed_or_silently_overridden(self):
        facts = self.owner.image_facts["database"]
        for user in ("", "0", "root", "postgres", "999", "1000:1000"):
            with self.subTest(user=user), self.assertRaisesRegex(Rejected, "trusted preparation"):
                self.resolve(replace(self.owner, image_facts={**self.owner.image_facts,
                    "database": replace(facts, default_user=user)}))

    def test_image_implicit_volume_cannot_create_an_unowned_anonymous_volume(self):
        facts = self.owner.image_facts["database"]
        with self.assertRaisesRegex(Rejected, "implicit image volume"):
            self.resolve(replace(self.owner, image_facts={**self.owner.image_facts,
                "database": replace(facts, volumes=facts.volumes | {"/other"})}))

    def test_owner_resource_quotas_not_broadened_by_repository(self):
        for changes in ({"memory_bytes": 32 * 1024**2}, {"cpus": 0.5}, {"pids": 16},
                        {"memory_bytes": 8 * 1024**3}, {"cpus": 5}, {"pids": 4096}):
            with self.subTest(changes=changes), self.assertRaises(Rejected):
                self.resolve(replace(self.owner, **changes))

    def test_zero_or_untyped_execution_identity_rejected(self):
        for changes in ({"job_id": UUID(int=0)}, {"attempt_id": UUID(int=0)},
                        {"job_id": str(self.owner.job_id)}, {"control_volume": "/host"}):
            with self.subTest(changes=changes), self.assertRaises(Rejected):
                self.resolve(replace(self.owner, **changes))
        with self.assertRaises(Rejected):
            self.resolve({"verified": True})

    def test_network_requires_bounded_owner_ipv4_allocation(self):
        for subnet in (ipaddress.ip_network("0.0.0.0/0"), ipaddress.ip_network("8.8.8.0/24"),
                       ipaddress.ip_network("10.0.0.0/8"), ipaddress.ip_network("10.0.0.0/30"),
                       ipaddress.ip_network("fd00::/64"), "10.0.0.0/24"):
            with self.subTest(subnet=subnet), self.assertRaises(Rejected):
                self.resolve(replace(self.owner, subnet=subnet))

    def test_tmpfs_is_owner_only_and_not_a_host_device_mount(self):
        for path in ("/", "/dev", "/proc/x", "/sys/x", "/tmp/../host", "//tmp"):
            with self.subTest(path=path), self.assertRaises(Rejected):
                self.resolve(replace(self.owner, tmpfs={"application": frozenset((path,)),
                                                      "database": frozenset()}))

    def test_repository_cannot_shadow_control_service_or_volume_names(self):
        for reserved in ("job", "transfer"):
            with self.subTest(reserved=reserved):
                self.setUp()
                self.compose["services"][reserved] = self.compose["services"].pop("application")
                self.compose["services"][reserved].pop("environment")
                self.images["images"][reserved] = self.images["images"].pop("application")
                with self.assertRaisesRegex(Rejected, "reserved RemoteJob"):
                    self.resolve()
        for reserved in ("workspace", "cargo", "sources"):
            with self.subTest(reserved=reserved):
                self.setUp()
                self.compose["volumes"][reserved] = {}
                with self.assertRaisesRegex(Rejected, "reserved RemoteJob"):
                    self.resolve()

    def test_writable_root_and_repo_security_overrides_remain_held(self):
        self.compose["services"]["application"]["read_only"] = False
        with self.assertRaises(Rejected):
            self.resolve()
        self.setUp()
        self.compose["services"]["application"]["privileged"] = True
        with self.assertRaises(Rejected):
            self.resolve()

    def test_owner_tmpfs_cannot_shadow_a_data_volume(self):
        for path in ("/var/lib/database", "/var/lib", "/var/lib/database/tmp"):
            with self.subTest(path=path), self.assertRaises(Rejected):
                self.resolve(replace(self.owner, tmpfs={"application": frozenset(),
                                                      "database": frozenset((path,))}))


if __name__ == "__main__":
    unittest.main()
