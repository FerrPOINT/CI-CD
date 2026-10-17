"""Pure recipe admission regressions: no Docker, secrets, filesystem or process IO."""

import copy
from dataclasses import replace
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import unittest


SPEC = importlib.util.spec_from_file_location(
    "workspace_deploy_admission", Path(__file__).resolve().parents[1] / "workspace_deploy_admission.py")
admission = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = admission
SPEC.loader.exec_module(admission)
SOURCE = "a" * 40
APP = "sha256:" + "b" * 64
DATABASE = "sha256:" + "c" * 64


def encoded(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode()


def service(image):
    return {"image": image, "mem_limit": 256 * 1024**2, "cpus": 0.5, "pids_limit": 64}


class WorkspaceDeployAdmissionTests(unittest.TestCase):
    def setUp(self):
        self.compose = {"services": {"application": service(APP), "database": service(DATABASE)},
                        "volumes": {"data": {}}}
        self.compose["services"]["application"].update(
            environment={"DATABASE_URL": "${APP_DATABASE_URL}", "BIND": "0.0.0.0:8000"},
            depends_on=["database"], read_only=True)
        self.compose["services"]["database"]["volumes"] = [
            {"type": "volume", "source": "data", "target": "/var/lib/postgresql/data"}]
        self.manifest = {"schema": "forge/workspace-images/v1", "sourceCommit": SOURCE,
                         "images": {"application": APP, "database": DATABASE}}

    def seal(self, compose=None, manifest=None):
        return admission.OwnerSeal(
            SOURCE, hashlib.sha256(compose or encoded(self.compose)).hexdigest(),
            hashlib.sha256(manifest or encoded(self.manifest)).hexdigest(),
            {"application": APP, "database": DATABASE},
            {"application": frozenset(("DATABASE_URL", "BIND", "DOCKER_HOST"))},
            frozenset(("APP_DATABASE_URL",)))

    def check(self):
        return admission.validate_recipe(encoded(self.compose), encoded(self.manifest), self.seal())

    def rejects(self):
        with self.assertRaises(admission.Rejected):
            self.check()

    def test_exact_registry_owned_volumes_and_declared_secret_reference(self):
        before = copy.deepcopy((self.compose, self.manifest))
        result = self.check()
        self.assertEqual(result.service_names, ("application", "database"))
        self.assertEqual(result.volume_names, ("data",))
        self.assertEqual(result.secret_refs, (("application", "DATABASE_URL", "APP_DATABASE_URL"),))
        self.assertEqual((self.compose, self.manifest), before)
        self.assertFalse(hasattr(result, "command"))

    def test_compose_bytes_must_match_owner_hash(self):
        seal = self.seal()
        self.compose["services"]["application"]["cpus"] = 0.75
        with self.assertRaisesRegex(admission.Rejected, "sealed bytes mismatch"):
            admission.validate_recipe(encoded(self.compose), encoded(self.manifest), seal)

    def test_manifest_bytes_must_match_owner_hash(self):
        seal = self.seal()
        changed = dict(self.manifest, sourceCommit="d" * 40)
        with self.assertRaisesRegex(admission.Rejected, "sealed bytes mismatch"):
            admission.validate_recipe(encoded(self.compose), encoded(changed), seal)

    def test_foreign_commit_rejected_even_when_manifest_hash_matches(self):
        self.manifest["sourceCommit"] = "d" * 40
        self.rejects()

    def test_forged_registry_image_rejected(self):
        forged = "sha256:" + "d" * 64
        self.manifest["images"]["application"] = forged
        self.compose["services"]["application"]["image"] = forged
        self.rejects()

    def test_missing_and_extra_registry_services_rejected(self):
        for images in ({"application": APP}, dict(self.manifest["images"], foreign=APP)):
            with self.subTest(images=images):
                self.manifest["images"] = images
                self.rejects()

    def test_tags_digests_and_interpolated_image_not_accepted_as_sealed_engine_ids(self):
        for image in ("postgres:17", "repo/app@sha256:" + "b" * 64, "${IMAGE}", APP.upper()):
            with self.subTest(image=image):
                self.compose["services"]["application"]["image"] = image
                self.rejects()

    def test_privileged_and_host_control_fields_rejected_even_false(self):
        for field, value in (("privileged", False), ("network_mode", "host"), ("pid", "host"),
                             ("ipc", "host"), ("devices", ["/dev/sda"]),
                             ("cap_add", ["SYS_ADMIN"]), ("security_opt", ["seccomp:unconfined"]),
                             ("build", "."), ("userns_mode", "host"), ("cgroup", "host")):
            with self.subTest(field=field):
                self.compose["services"]["application"][field] = value
                self.rejects()
                del self.compose["services"]["application"][field]

    def test_entrypoint_command_and_extra_hosts_overrides_rejected(self):
        for field, value in (("entrypoint", ["/bin/sh"]), ("command", "docker ps"),
                             ("extra_hosts", ["host.docker.internal:host-gateway"]),
                             ("env_file", ["/host/secrets"]), ("volumes_from", ["foreign"])):
            with self.subTest(field=field):
                self.compose["services"]["application"][field] = value
                self.rejects()
                del self.compose["services"]["application"][field]

    def test_all_host_port_publication_rejected(self):
        self.compose["services"]["application"]["ports"] = ["127.0.0.1:8000:8000"]
        self.rejects()

    def test_repo_cannot_choose_network_or_external_secret(self):
        for field in ("networks", "secrets", "configs", "name", "include", "version"):
            with self.subTest(field=field):
                self.compose[field] = {"foreign": {"external": True}}
                self.rejects()
                del self.compose[field]
        self.compose["services"]["application"]["networks"] = ["foreign"]
        self.rejects()

    def test_external_driver_and_name_volume_options_rejected(self):
        for definition in ({"external": True}, {"name": "permanent"},
                           {"driver_opts": {"device": "/"}}, {"driver": "local"}):
            with self.subTest(definition=definition):
                self.compose["volumes"]["data"] = definition
                self.rejects()

    def test_host_and_docker_socket_binds_rejected(self):
        for source in ("/", "C:/", "/var/run/docker.sock"):
            with self.subTest(source=source):
                self.compose["services"]["application"]["volumes"] = [
                    {"type": "bind", "source": source, "target": "/host"}]
                self.rejects()

    def test_anonymous_short_and_foreign_volume_mounts_rejected(self):
        for mount in ("data:/data", {"type": "volume", "target": "/data"},
                      {"type": "volume", "source": "foreign", "target": "/data"}):
            with self.subTest(mount=mount):
                self.compose["services"]["application"]["volumes"] = [mount]
                self.rejects()

    def test_mount_targets_are_closed_and_unique(self):
        for target in ("/", "/proc/x", "/sys", "/dev/x", "/run/x", "/var/run/docker.sock",
                       "/data/../host", "/data//x", "/data/${HOST}", "C:/data", "/data\x00"):
            with self.subTest(target=target):
                self.compose["services"]["application"]["volumes"] = [
                    {"type": "volume", "source": "data", "target": target}]
                self.rejects()
        mount = {"type": "volume", "source": "data", "target": "/data"}
        self.compose["services"]["application"]["volumes"] = [mount, mount]
        self.rejects()

    def test_unowned_environment_and_host_docker_env_rejected(self):
        for key in ("UNDECLARED", "DOCKER_HOST", "CICD_RUNNER_TOKEN"):
            with self.subTest(key=key):
                self.compose["services"]["application"]["environment"] = {key: "x"}
                self.rejects()

    def test_missing_secret_and_shell_default_expansion_rejected(self):
        for value in ("${UNDECLARED}", "${APP_DATABASE_URL:-fallback}", "$APP_DATABASE_URL",
                      "prefix-${APP_DATABASE_URL}", "$(docker ps)"):
            with self.subTest(value=value):
                self.compose["services"]["application"]["environment"] = {"DATABASE_URL": value}
                self.rejects()

    def test_environment_list_and_nonstring_values_rejected(self):
        for value in (["DATABASE_URL=x"], {"DATABASE_URL": None}, {"DATABASE_URL": {"file": "/key"}},
                      {"DATABASE_URL": "x\ny"}, {"DATABASE_URL": "x" * 4097}):
            with self.subTest(value_type=type(value).__name__):
                self.compose["services"]["application"]["environment"] = value
                self.rejects()

    def test_positive_bounded_resources_required(self):
        for field, values in (("cpus", (0, -1, 3, True, "0.5")),
                              ("mem_limit", (0, -1, 3 * 1024**3, True, "512m")),
                              ("pids_limit", (0, -1, 513, True, "64"))):
            for value in values:
                with self.subTest(field=field, value=value):
                    before = self.compose["services"]["application"][field]
                    self.compose["services"]["application"][field] = value
                    self.rejects()
                    self.compose["services"]["application"][field] = before
        del self.compose["services"]["application"]["cpus"]
        self.rejects()

    def test_nonfinite_numbers_rejected(self):
        for value in (float("nan"), float("inf"), float("-inf")):
            with self.subTest(value=value):
                self.compose["services"]["application"]["cpus"] = value
                self.rejects()

    def test_large_cpu_integer_and_overflowing_float_rejected(self):
        self.compose["services"]["application"]["cpus"] = 10**400
        self.rejects()
        raw = encoded(self.compose).replace(str(10**400).encode(), b"1e10000")
        with self.assertRaises(admission.Rejected):
            admission.validate_recipe(raw, encoded(self.manifest), self.seal(compose=raw))

    def test_unpaired_unicode_surrogate_rejected(self):
        self.compose["services"]["application"]["environment"] = {"BIND": "\ud800"}
        self.rejects()

    def test_aggregate_resources_bounded(self):
        for field, value in (("mem_limit", 2 * 1024**3), ("cpus", 2), ("pids_limit", 512)):
            with self.subTest(field=field):
                self.setUp()
                for index in range(3):
                    name = "other" + str(index)
                    self.compose["services"][name] = service(APP)
                    self.manifest["images"][name] = APP
                for item in self.compose["services"].values():
                    item[field] = value
                raw = encoded(self.compose)
                manifest = encoded(self.manifest)
                seal = replace(self.seal(), images=dict(self.manifest["images"]))
                with self.assertRaisesRegex(admission.Rejected, "aggregate resource bound"):
                    admission.validate_recipe(raw, manifest, seal)

    def test_service_and_volume_count_bounded(self):
        self.compose["volumes"] = {"v" + str(i): {} for i in range(9)}
        self.rejects()
        self.setUp()
        self.compose["services"] = {"s" + str(i): service(APP) for i in range(9)}
        self.rejects()

    def test_empty_or_bad_service_definitions_rejected(self):
        for services in ({}, [], {"application": None}, {"../host": service(APP)}):
            with self.subTest(services=services):
                self.compose["services"] = services
                self.rejects()

    def test_cyclic_duplicate_and_foreign_dependencies_rejected(self):
        self.compose["services"]["database"]["depends_on"] = ["application"]
        self.rejects()
        del self.compose["services"]["database"]["depends_on"]
        for depends in (["application"], ["database", "database"], ["foreign"], {"database": {}}):
            with self.subTest(depends=depends):
                self.compose["services"]["application"]["depends_on"] = depends
                self.rejects()

    def test_duplicate_json_keys_and_yaml_are_not_accepted(self):
        for raw in (b'{"services":{},"services":{}}', b'services:\n  application: {}',
                    b'{"services": {"application": {"image":"a","image":"b"}}}'):
            with self.subTest(raw=raw):
                with self.assertRaises(admission.Rejected):
                    admission.validate_recipe(raw, encoded(self.manifest), self.seal(compose=raw))

    def test_truncated_invalid_utf8_oversize_and_nested_input_rejected(self):
        for raw in (b'{', b'\xff', b' ' * (admission.MAX_BYTES + 1),
                    b'[' * 30 + b'0' + b']' * 30):
            with self.subTest(size=len(raw)):
                with self.assertRaises(admission.Rejected):
                    admission.validate_recipe(raw, encoded(self.manifest), self.seal(compose=raw))

    def test_unknown_manifest_fields_and_schemas_rejected(self):
        self.manifest["verified"] = True
        self.rejects()
        del self.manifest["verified"]
        self.manifest["schema"] = "forge/oci-candidate/v1"
        self.rejects()

    def test_raw_mapping_cannot_substitute_for_owner_seal(self):
        with self.assertRaises(admission.Rejected):
            admission.validate_recipe(encoded(self.compose), encoded(self.manifest), {"verified": True})

    def test_errors_never_echo_values(self):
        secret_marker = "protected-value-must-not-appear"
        self.compose["services"]["application"]["environment"] = {"UNDECLARED": secret_marker}
        with self.assertRaises(admission.Rejected) as caught:
            self.check()
        self.assertNotIn(secret_marker, str(caught.exception))


if __name__ == "__main__":
    unittest.main()
