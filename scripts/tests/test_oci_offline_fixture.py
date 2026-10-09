"""Pure fixture guard regressions; no Docker commands or native acceptance."""

import copy
import hashlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile
import unittest
from contextlib import redirect_stderr
from unittest.mock import patch


SOURCE = (Path(__file__).resolve().parents[2] /
          "backend/tests/support/oci_delivery.rs").read_text(encoding="utf-8")
SCRIPT = SOURCE.split('const OCI_OFFLINE_BASE_GUARD: &str = r#"', 1)[1].split('\n"#;', 1)[0]
DIAGNOSTIC = SOURCE.split('const OCI_CLI_FAILURE_DIAGNOSTIC: &str = r#"', 1)[1].split('\n"#;', 1)[0]


class CliFailureDiagnosticTests(unittest.TestCase):
    def setUp(self):
        self.guard = {"__name__": "oci_diagnostic_test"}
        exec(compile(DIAGNOSTIC, "oci-cli-diagnostic.py", "exec"), self.guard)
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name) / "diagnostics"

    def payload(self, **changes):
        value = {"schema": "forge/local-oci-rejection/v1", "status": "unknown_or_rejected",
                 "reason": "oci_command_rejected_or_unavailable", "dispatchAllowed": False,
                 "sdlcAcceptanceVerified": False, "private": "token-sql-role-db-secret"}
        value.update(changes)
        return json.dumps(value).encode()

    def test_only_closed_rejection_categories_are_retained(self):
        for status, reason in self.guard["REJECTIONS"]:
            with self.subTest(reason=reason):
                value = self.guard["project"](self.payload(status=status, reason=reason), 1)
                self.assertEqual(value["payloadKind"], "rejection")
                self.assertEqual(value["status"], status)
                self.assertEqual(value["reason"], reason)
                self.assertFalse(value["acceptanceVerified"])
                self.assertEqual(set(value), {"schema", "scope", "acceptanceVerified", "exitCode",
                                             "payloadKind", "status", "reason"})
                self.assertNotIn("secret", json.dumps(value))

    def test_unknown_categories_and_acceptance_flags_fail_closed(self):
        for changes in ({"reason": "private-secret"}, {"status": "private-secret"},
                        {"reason": []}, {"dispatchAllowed": True}, {"dispatchAllowed": 0},
                        {"sdlcAcceptanceVerified": True}, {"sdlcAcceptanceVerified": None}):
            with self.subTest(changes=changes):
                value = self.guard["project"](self.payload(**changes), 1)
                self.assertEqual(value["payloadKind"], "invalid_rejection")
                self.assertNotIn("reason", value)
                self.assertNotIn("status", value)

    def test_invalid_oversized_empty_and_other_documents_are_bounded(self):
        for payload, kind in ((b"", "empty"), (b"secret" * 3000, "oversized"),
                              (b"{private-secret", "invalid_json"), (b"\xff", "invalid_json"),
                              (b'[{"secret":"private"}]', "other_document"),
                              (b'{"schema":"private-secret"}', "other_document"),
                              (b'{"schema":1,"schema":2}', "invalid_json")):
            with self.subTest(kind=kind):
                value = self.guard["project"](payload, 1)
                self.assertEqual(value["payloadKind"], kind)
                self.assertNotIn("secret", json.dumps(value))
                self.assertLess(len(json.dumps(value)), 512)

    def test_exit_code_cannot_carry_arbitrary_data(self):
        for code in (None, -1, 256, True, "secret"):
            self.assertIsNone(self.guard["project"](b"", code)["exitCode"])
        self.assertEqual(self.guard["project"](b"", 1)["exitCode"], 1)

    def test_private_retention_is_unique_and_survives_fixture_temp_removal(self):
        value = self.guard["project"](self.payload(), 1)
        with tempfile.TemporaryDirectory(dir=self.temp.name) as fixture:
            Path(fixture, "private-state").write_bytes(b"private-secret")
            self.guard["retain"](self.directory, value)
        self.assertFalse(Path(fixture).exists())
        self.guard["retain"](self.directory, value)
        self.assertEqual(self.directory.stat().st_mode & 0o777, 0o700)
        files = list(self.directory.iterdir())
        self.assertEqual(len(files), 2)
        for path in files:
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)
            self.assertEqual(json.loads(path.read_bytes()), value)
        with patch.object(self.guard["uuid"], "uuid4") as identifier:
            identifier.return_value.hex = files[0].stem
            with self.assertRaises(FileExistsError):
                self.guard["retain"](self.directory, {})
        self.assertEqual(json.loads(files[0].read_bytes()), value)

    def test_existing_unsafe_directory_and_symlink_are_not_modified(self):
        target = Path(self.temp.name) / "foreign"
        target.mkdir(mode=0o755)
        self.directory.symlink_to(target, target_is_directory=True)
        with self.assertRaises(OSError):
            self.guard["retain"](self.directory, {})
        self.assertEqual(list(target.iterdir()), [])
        self.directory.unlink()
        self.directory.mkdir(mode=0o755)
        with self.assertRaises(ValueError):
            self.guard["retain"](self.directory, {})
        self.assertEqual(self.directory.stat().st_mode & 0o777, 0o755)

    def test_retention_failure_logs_only_closed_code(self):
        output = io.StringIO()
        main = DIAGNOSTIC[DIAGNOSTIC.index("if __name__ == '__main__':"):]
        with patch("sys.argv", ["diagnostic", "1"]), \
                patch("sys.stdin", io.TextIOWrapper(io.BytesIO(self.payload()))), \
                patch.dict(self.guard, __name__="__main__",
                           retain=lambda *args: (_ for _ in ()).throw(OSError("private-path-secret"))), \
                redirect_stderr(output):
            exec(compile(main, "oci-cli-diagnostic.py", "exec"), self.guard)
        lines = output.getvalue().splitlines()
        self.assertEqual(len(lines), 2)
        self.assertEqual(lines[1], "OCI_CLI_FAILURE_DIAGNOSTIC_UNAVAILABLE")
        value = json.loads(lines[0].split(":", 1)[1])
        self.assertEqual(value["payloadKind"], "rejection")
        self.assertNotIn("secret", output.getvalue())

    def test_hook_keeps_original_deadline_exit_and_receipt_assertions(self):
        execute = SOURCE.split("    async fn execute(", 1)[1].split("#[tokio::test]", 1)[0]
        self.assertIn("Duration::from_secs(50)", execute)
        self.assertIn("if !output.status.success()", execute)
        self.assertIn("oci_cli_failure_diagnostic(&output);", execute)
        self.assertIn("output.status.code().unwrap()", execute)
        self.assertIn("serde_json::from_slice(&output.stdout).ok()", execute)
        self.assertIn("assert_eq!(code, 0);", SOURCE)
        self.assertIn('latest(&first)["compatibility"]["status"], "verified"', SOURCE)
        hook = SOURCE.split("fn oci_cli_failure_diagnostic", 1)[1].split(
            "const OCI_OFFLINE_BASE_GUARD", 1)[0]
        self.assertIn("output.stdout.len().min(16385)", hook)
        self.assertNotIn("output.stderr", hook)
        self.assertIn("sys.stdin.buffer.read(LIMIT + 1)", DIAGNOSTIC)
        self.assertNotIn("subprocess", DIAGNOSTIC)


class OfflineBaseTests(unittest.TestCase):
    def setUp(self):
        self.guard = {"__name__": "oci_fixture_test"}
        exec(compile(SCRIPT, "oci-offline-base.py", "exec"), self.guard)
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.layout = Path(self.temp.name) / "layout"
        self.progress_dir = Path(self.temp.name) / "oci-build-progress"
        self.progress_dir.mkdir(mode=0o700)
        self.progress_file = self.progress_dir / "state.json"
        self.entries = {}

        def descriptor(value, media_type):
            data = json.dumps(value).encode() if isinstance(value, dict) else value
            digest = "sha256:" + hashlib.sha256(data).hexdigest()
            self.entries["blobs/sha256/" + digest[7:]] = data
            return {"digest": digest, "size": len(data), "mediaType": media_type}

        self.config = descriptor({"os": "linux", "architecture": "amd64"},
                                 "application/vnd.oci.image.config.v1+json")
        self.layer = descriptor(b"local-layer", "application/vnd.oci.image.layer.v1.tar+gzip")
        manifest = descriptor({"schemaVersion": 2, "config": self.config,
                               "layers": [self.layer]}, "application/vnd.oci.image.manifest.v1+json")
        manifest["platform"] = {"os": "linux", "architecture": "amd64"}
        # A saved multi-platform index can retain unavailable OTHER-platform descriptors.
        other = {"digest": "sha256:" + "a" * 64, "size": 1,
                 "platform": {"os": "linux", "architecture": "arm64"}}
        self.root = descriptor({"schemaVersion": 2, "manifests": [manifest, other]},
                               "application/vnd.oci.image.index.v1+json")
        self.guard["PIN"] = self.root["digest"]
        self.guard["SOURCE"] = "python:3.12-bookworm@" + self.root["digest"]
        self.entries["oci-layout"] = b'{"imageLayoutVersion":"1.0.0"}'
        self.entries["index.json"] = json.dumps({"schemaVersion": 2,
                                                 "manifests": [self.root]}).encode()
        self.image = {"Id": self.root["digest"], "Os": "linux", "Architecture": "amd64",
                      "RepoTags": ["python@" + self.root["digest"]],
                      "RepoDigests": ["python@" + self.root["digest"]]}

    def materialize(self):
        for name, data in self.entries.items():
            path = self.layout / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)

    def archive(self, extra=()):
        stream = io.BytesIO()
        with tarfile.open(fileobj=stream, mode="w") as writer:
            for name, data in self.entries.items():
                header = tarfile.TarInfo(name)
                header.size = len(data)
                writer.addfile(header, io.BytesIO(data))
            for header in extra:
                writer.addfile(header, io.BytesIO(b""))
        return stream.getvalue()

    def fake_docker(self, command, **options):
        self.assertEqual(command[0], "/usr/local/bin/docker")
        self.assertEqual(options["env"]["DOCKER_HOST"], "unix:///var/run/docker.sock")
        args = command[1:]
        if args[0] == "info":
            data = b'"owned-daemon"'
        elif args[:2] == ["image", "inspect"]:
            self.assertEqual(args[2], self.guard["SOURCE"])
            data = json.dumps([self.image]).encode()
        else:
            self.assertEqual(args, ["image", "save", self.guard["SOURCE"]])
            options["stdout"].write(self.archive())
            data = None
        return subprocess.CompletedProcess(command, 0, data, b"stderr-secret")

    def test_prepare_exact_local_index_without_docker_mutations(self):
        with patch.object(subprocess, "run", side_effect=self.fake_docker) as run:
            self.guard["prepare"](self.layout, "owned-daemon")
        commands = [call.args[0][1:] for call in run.call_args_list]
        self.assertEqual(sum(cmd[:2] == ["image", "save"] for cmd in commands), 1)
        self.assertTrue(all(cmd[0] == "info" or cmd[:2] in
                            (["image", "inspect"], ["image", "save"]) for cmd in commands))
        self.assertFalse((self.layout.parent / "offline-base.tar").exists())
        self.guard["validate"](self.layout)

    def test_tampered_or_missing_selected_blobs_denied(self):
        self.materialize()
        path = self.layout / "blobs/sha256" / self.layer["digest"][7:]
        path.write_bytes(b"wrong-layer")
        with self.assertRaises(ValueError):
            self.guard["validate"](self.layout)
        path.unlink()
        with self.assertRaises(ValueError):
            self.guard["validate"](self.layout)

    def test_wrong_root_digest_denied(self):
        self.materialize()
        self.guard["PIN"] = "sha256:" + "b" * 64
        with self.assertRaises(ValueError):
            self.guard["validate"](self.layout)

    def test_unsafe_archive_members_denied(self):
        for name, kind in (("../escaped", tarfile.REGTYPE),
                           ("/absolute", tarfile.REGTYPE),
                           ("oci-layout", tarfile.REGTYPE),
                           ("blobs/sha256/" + "c" * 64, tarfile.SYMTYPE),
                           ("blobs/sha256/" + "d" * 64, tarfile.LNKTYPE)):
            with self.subTest(name=name, kind=kind), tempfile.TemporaryDirectory() as root:
                header = tarfile.TarInfo(name)
                header.type = kind
                header.linkname = "../escaped"
                archive = Path(root) / "base.tar"
                archive.write_bytes(self.archive([header]))
                with self.assertRaises(ValueError):
                    self.guard["unpack"](archive, Path(root) / "layout")
                self.assertFalse((Path(root) / "escaped").exists())

    def test_foreign_platform_and_external_layer_denied(self):
        self.materialize()
        self.guard["PIN"] = self.root["digest"]
        external = dict(self.layer, urls=["https://private.invalid/secret"])
        with self.assertRaises(ValueError):
            self.guard["blob"](self.layout, external)
        self.image["Architecture"] = "arm64"
        with patch.object(subprocess, "run", side_effect=self.fake_docker), self.assertRaises(ValueError):
            self.guard["identity"](self.layout, "owned-daemon")

    def test_daemon_pin_image_id_and_original_references_drift_denied(self):
        with patch.object(subprocess, "run", side_effect=self.fake_docker):
            with self.assertRaises(ValueError):
                self.guard["identity"](self.layout, "foreign-daemon")
            original = self.guard["identity"](self.layout, "owned-daemon")
            for key, changed in (("id", "sha256:" + "e" * 64), ("tags", []), ("digests", [])):
                current = copy.deepcopy(original)
                current[key] = changed
                with self.subTest(key=key), self.assertRaises(ValueError):
                    self.guard["preserve"](original, current)
            self.image["RepoDigests"] = ["python@sha256:" + "f" * 64]
            with self.assertRaises(ValueError):
                self.guard["identity"](self.layout, "owned-daemon")

    def test_no_existing_export_directory_overwrite(self):
        self.layout.mkdir()
        marker = self.layout / "foreign"
        marker.write_bytes(b"preserve")
        archive = self.layout.parent / "base.tar"
        archive.write_bytes(self.archive())
        with self.assertRaises(FileExistsError):
            self.guard["unpack"](archive, self.layout)
        self.assertEqual(marker.read_bytes(), b"preserve")

    def test_build_uses_oci_session_and_rechecks_on_success_failure_and_spawn_error(self):
        for outcome in (subprocess.CompletedProcess([], 0), subprocess.CompletedProcess([], 2),
                        OSError("private-path-secret")):
            with self.subTest(outcome=type(outcome).__name__):
                events = []

                def check(layout, daemon):
                    self.assertEqual((layout, daemon), (self.layout, "owned-daemon"))
                    events.append("check")

                def run(command, **options):
                    events.append("build")
                    self.assertIn("--builder", command)
                    self.assertEqual(command[command.index("--builder") + 1], "default")
                    self.assertIn("--pull=false", command)
                    self.assertIn("--network=none", command)
                    self.assertIn("--platform=linux/amd64", command)
                    self.assertIn("offline-python-base=oci-layout://" + str(self.layout) +
                                  "@" + self.guard["PIN"], command)
                    self.assertNotIn("timeout", options)  # Existing runner deadline still owns build.
                    if isinstance(outcome, Exception):
                        raise outcome
                    return outcome

                with patch.dict(self.guard, check=check), patch.object(subprocess, "run", side_effect=run):
                    if isinstance(outcome, Exception) or outcome.returncode:
                        with self.assertRaises((ValueError, OSError)):
                            self.guard["build"](self.layout, "owned-daemon", "original-commit")
                    else:
                        self.guard["build"](self.layout, "owned-daemon", "original-commit")
                self.assertEqual(events, ["check", "build", "check"])

    def test_prebuild_guard_failure_has_zero_build_effects(self):
        with patch.dict(self.guard, check=lambda *args: (_ for _ in ()).throw(ValueError())), \
                patch.object(subprocess, "run") as run, self.assertRaises(ValueError):
            self.guard["build"](self.layout, "owned-daemon", "commit")
        run.assert_not_called()

    def test_foreign_builder_and_credentials_environment_not_used(self):
        overrides = {key: "private-env-secret" for key in
                     ("DOCKER_CONTEXT", "DOCKER_TLS_VERIFY", "DOCKER_CERT_PATH",
                      "BUILDX_BUILDER", "BUILDKIT_HOST", "BUILDKIT_SYNTAX", "DOCKER_CONFIG")}
        with patch.dict(os.environ, overrides):
            env = self.guard["docker_env"](self.layout)
        for key in overrides:
            if key != "DOCKER_CONFIG":
                self.assertNotIn(key, env)
        self.assertEqual(env["DOCKER_CONFIG"], str(self.layout.parent / "offline-docker-config"))

    def test_failure_main_never_prints_secret_stderr_args_or_exception(self):
        output = io.StringIO()
        command = subprocess.CompletedProcess([], 2, b"stdout-secret", b"stderr-secret")
        main = {"__name__": "__main__"}
        with patch("sys.argv", ["guard", "prepare", str(self.layout), "argv-secret"]), \
                patch.object(subprocess, "run", return_value=command) as run, redirect_stderr(output):
            with self.assertRaises(SystemExit) as error:
                exec(compile(SCRIPT, "oci-offline-base.py", "exec"), main)
        self.assertEqual(str(error.exception), "OCI_OFFLINE_BASE_GUARD_FAILED:prepare_identity:command_failed")
        self.assertEqual(output.getvalue(), "")
        run.assert_called_once()

    def test_timeout_is_code_only_and_does_not_export_args_or_stderr(self):
        error = subprocess.TimeoutExpired(['docker', 'private-argv-secret'], 25,
                                          output=b'private-output-secret', stderr=b'private-stderr-secret')
        main = {"__name__": "__main__"}
        with patch("sys.argv", ["guard", "prepare", str(self.layout), "private-daemon-secret"]), \
                patch.object(subprocess, "run", side_effect=error) as run:
            with self.assertRaises(SystemExit) as exit_error:
                exec(compile(SCRIPT, "oci-offline-base.py", "exec"), main)
        self.assertEqual(str(exit_error.exception),
                         'OCI_OFFLINE_BASE_GUARD_FAILED:prepare_identity:command_timeout')
        run.assert_called_once()

    def progress_phase(self):
        data = self.progress_file.read_bytes()
        self.assertLessEqual(len(data), 128)
        document = json.loads(data)
        self.assertEqual(set(document), {"schema", "phase"})
        self.assertEqual(document["schema"], "forge/oci-build-progress/v1")
        return document["phase"]

    def test_progress_atomic_private_bounded_and_matches_rust_closed_enum(self):
        accepted = dict((json.loads('"' + wire + '"').encode(), name) for wire, name in
                        re.findall(r'Ok\(b"([^"\n]*(?:\\.[^"\n]*)*)"\)\s*=>\s*(?:\{\s*)?"([a-z_]+)"', SOURCE))
        self.assertEqual(set(accepted.values()),
                         {"not_started", "prebuild", "build", "postbuild", "completed"})
        for name in ("prebuild", "build", "postbuild", "completed"):
            self.guard["progress"](self.layout, name)
            before = self.progress_file.read_bytes()
            self.assertEqual(accepted[before], name)
            if os.name != "nt":
                self.assertEqual(self.progress_file.stat().st_mode & 0o777, 0o600)
            replace = os.replace

            def atomic(source, target):
                self.assertEqual(self.progress_file.read_bytes(), before)
                self.assertEqual(Path(source).parent, self.progress_dir)
                self.assertEqual(Path(target), self.progress_file)
                self.assertEqual(json.loads(Path(source).read_bytes())["phase"], name)
                if os.name != "nt":
                    self.assertEqual(Path(source).stat().st_mode & 0o777, 0o600)
                replace(source, target)

            with patch.object(os, "replace", side_effect=atomic) as publish:
                self.guard["progress"](self.layout, name)
            publish.assert_called_once()
            self.assertFalse((self.progress_dir / "next").exists())
        for invalid in ("private-secret", "prepare_identity", "", "build\nsecret"):
            before = self.progress_file.read_bytes()
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                self.guard["progress"](self.layout, invalid)
            self.assertEqual(self.progress_file.read_bytes(), before)

    def test_progress_distinguishes_prebuild_build_postbuild_and_completed(self):
        phases = []

        def check(*args):
            phases.append(self.progress_phase())

        def run(*args, **kwargs):
            phases.append(self.progress_phase())
            return subprocess.CompletedProcess([], 0)

        with patch.dict(self.guard, check=check), patch.object(subprocess, "run", side_effect=run):
            self.guard["build"](self.layout, "owned-daemon", "private-commit-secret")
        self.assertEqual(phases, ["prebuild", "build", "postbuild"])
        self.assertEqual(self.progress_phase(), "completed")
        self.assertNotIn(b"private-commit-secret", self.progress_file.read_bytes())

    def test_no_completed_progress_after_build_or_postbuild_failure(self):
        for postbuild_failure in (False, True):
            checks = []
            original = ValueError("private-validation-secret")

            def check(*args):
                checks.append(self.progress_phase())
                if postbuild_failure and len(checks) == 2:
                    raise original

            result = subprocess.CompletedProcess([], 0 if postbuild_failure else 2)
            with self.subTest(postbuild_failure=postbuild_failure), patch.dict(self.guard, check=check), \
                    patch.object(subprocess, "run", return_value=result) as run:
                with self.assertRaises(ValueError) as error:
                    self.guard["build"](self.layout, "owned-daemon", "commit")
            if postbuild_failure:
                self.assertIs(error.exception, original)
            run.assert_called_once()
            self.assertEqual(checks, ["prebuild", "postbuild"])
            self.assertEqual(self.progress_phase(), "postbuild")

    def test_prebuild_failure_keeps_phase_and_original_exception_without_build(self):
        original = ValueError("private-validation-secret")
        with patch.dict(self.guard, check=lambda *args: (_ for _ in ()).throw(original)), \
                patch.object(subprocess, "run") as run, self.assertRaises(ValueError) as error:
            self.guard["build"](self.layout, "owned-daemon", "commit")
        self.assertIs(error.exception, original)
        self.assertEqual(self.progress_phase(), "prebuild")
        run.assert_not_called()

    def test_progress_write_failure_does_not_mask_original_validation_error(self):
        original = ValueError("private-validation-secret")
        output = io.StringIO()
        with patch.dict(self.guard, check=lambda *args: (_ for _ in ()).throw(original)), \
                patch.object(os, "replace", side_effect=OSError("private-path-secret")), \
                patch.object(subprocess, "run") as run, redirect_stderr(output):
            with self.assertRaises(ValueError) as error:
                self.guard["build"](self.layout, "owned-daemon", "private-label-secret")
        self.assertIs(error.exception, original)
        self.assertEqual(output.getvalue(), "OCI_BUILD_PROGRESS_WRITE_FAILED\n")
        self.assertFalse((self.progress_dir / "next").exists())
        run.assert_not_called()

    def test_existing_pending_progress_is_not_overwritten_or_removed(self):
        pending = self.progress_dir / "next"
        pending.write_bytes(b"private-foreign-secret")
        output = io.StringIO()
        with redirect_stderr(output):
            self.guard["progress"](self.layout, "build")
        self.assertEqual(pending.read_bytes(), b"private-foreign-secret")
        self.assertFalse(self.progress_file.exists())
        self.assertEqual(output.getvalue(), "OCI_BUILD_PROGRESS_WRITE_FAILED\n")

    def test_secret_bearing_failed_main_progress_and_diagnostic_remain_closed(self):
        self.materialize()
        (self.layout.parent / "offline-base-identity.json").write_text(json.dumps({
            "id": self.image["Id"], "tags": self.image["RepoTags"],
            "digests": self.image["RepoDigests"]}))
        output = io.StringIO()

        def run(command, **options):
            if command[1] != "buildx":
                return self.fake_docker(command, **options)
            return subprocess.CompletedProcess(command, 2, b"stdout-secret", b"stderr-secret")

        with patch("sys.argv", ["guard", "build", str(self.layout), "owned-daemon", "argv-secret"]), \
                patch.object(subprocess, "run", side_effect=run), redirect_stderr(output), \
                patch.dict(self.guard, __name__="__main__"):
            with self.assertRaises(SystemExit) as error:
                # Exercise the unchanged main boundary against the synthetic local base.
                main = SCRIPT[SCRIPT.index("if __name__ == '__main__':"):]
                exec(compile(main, "oci-offline-base.py", "exec"), self.guard)
        self.assertEqual(str(error.exception), "OCI_OFFLINE_BASE_GUARD_FAILED:postbuild:validation_rejected")
        self.assertEqual(self.progress_phase(), "postbuild")
        self.assertEqual(output.getvalue(), "")

    def test_timeout_hook_preserves_original_deadline_and_error_and_is_optional(self):
        helper = (Path(__file__).resolve().parents[2] /
                  "backend/tests/support/task_delivery.rs").read_text(encoding="utf-8")
        self.assertIn("runner_timeout_diagnostic: None,", helper)
        self.assertIn("Duration::from_secs(25)", helper)
        wait = helper.split("let output = tokio::time::timeout(completion_timeout, child.wait_with_output())", 1)[1]
        wait = wait.split("assert!(output.status.success()", 1)[0]
        self.assertRegex(wait, r'\.await\s*\.inspect_err\(\|_\|')
        self.assertEqual(wait.count(".unwrap()"), 2)
        self.assertNotIn("unwrap_or", wait)
        self.assertIn("diagnostic(&self.temp);", wait)
        self.assertIn("reset_oci_build_progress(&self.t.temp);", SOURCE)
        reader = SOURCE.split("fn oci_build_timeout_diagnostic", 1)[1].split(
            "const OCI_OFFLINE_BASE_GUARD", 1)[0]
        self.assertIn("file.take(129)", reader)
        self.assertIn("libc::O_NOFOLLOW | libc::O_NONBLOCK", reader)
        self.assertIn('Ok(_) => "invalid"', reader)
        self.assertIn('Err(_) => "unavailable"', reader)
        self.assertIn('OCI_RUNNER_COMPLETION_TIMEOUT:last_progress={phase}', reader)
        self.assertIn("Duration::from_secs(50)", SOURCE)


if __name__ == "__main__":
    unittest.main()
