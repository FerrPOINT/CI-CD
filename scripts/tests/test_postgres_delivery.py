"""Control-flow regressions; SQL execution remains a separate PostgreSQL gate."""

import importlib.util
import copy
import hashlib
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import types
import unittest
from contextlib import redirect_stderr, redirect_stdout
from unittest.mock import Mock, patch

PATH = Path(__file__).resolve().parents[1] / "postgres-delivery.py"
SPEC = importlib.util.spec_from_file_location("forge_postgres_delivery_test", PATH)
DELIVERY = importlib.util.module_from_spec(SPEC)
if os.name == "nt":
    # Only pure control flow is exercised here, never the Linux lock implementation.
    with patch.dict(sys.modules, {"fcntl": types.ModuleType("fcntl")}):
        SPEC.loader.exec_module(DELIVERY)
else:
    SPEC.loader.exec_module(DELIVERY)


def owner():
    value = DELIVERY.Owner.__new__(DELIVERY.Owner)
    value.pg = types.SimpleNamespace(ident=lambda text: '"' + text + '"',
                                    literal=lambda text: "'" + text + "'")
    return value


class CommandFailureDiagnosticTests(unittest.TestCase):
    def setUp(self):
        self.value = owner()
        self.value.env = lambda: {"PRIVATE_ENV": "env-secret"}
        self.command = ["tool", "postgres://private-user:argv-secret@private.invalid/db"]
        self.child = Mock(returncode=3)
        self.child.communicate.return_value = (b"stdout-secret", b"stderr-secret")

    def failure(self, stderr=b"stderr-secret", **options):
        self.child.communicate.return_value = (b"stdout-secret", stderr)
        with patch.object(DELIVERY.subprocess, "Popen", return_value=self.child) as spawn, \
                patch.object(DELIVERY.time, "monotonic", side_effect=[10.0, 12.5]):
            with self.assertRaises(DELIVERY.Blocked) as caught:
                self.value.run(self.command, b"SELECT 'sql-secret';", **options)
        spawn.assert_called_once()
        self.child.communicate.assert_called_once_with(b"SELECT 'sql-secret';", timeout=20)
        self.child.kill.assert_not_called()
        self.child.wait.assert_not_called()
        return caught.exception

    def test_normal_failure_keeps_blocked_reason_and_safe_metadata(self):
        error = self.failure()
        self.assertEqual(str(error), "owner_command_failed_or_unknown")
        self.assertEqual(error.command_failure, {"returncode": 3, "elapsedSeconds": 2.5, "sqlstate": None})

    def test_code_only_sqlstate_failure_does_not_export_secret_args_or_input(self):
        error = self.failure(b"ERROR:  42P04\n", sqlstate_only=True)
        self.assertEqual(str(error), "owner_command_failed_or_unknown")
        self.assertEqual(error.command_failure, {"returncode": 3, "elapsedSeconds": 2.5, "sqlstate": "42P04"})
        self.assertNotIn("secret", json.dumps(error.command_failure))
        self.assertNotIn("private", json.dumps(error.command_failure))

    def test_sqlstate_validation_rejects_messages_unknown_codes_and_ambiguity(self):
        for stderr in (b"ERROR:  ABCDE\n", b"ERROR:  42XYZ\n", b"ERROR:  00000\n",
                       b"ERROR:  42p04\n", b"ERROR:  5701\n", b"ERROR:  570140\n",
                       b"ERROR:  57014 stderr-secret\n", b"stderr-secret\nERROR:  57014\n",
                       b"ERROR:  57014\nERROR:  42P04\n", b"SQLSTATE=57014",
                       b"ERROR:  \xff\n", b"NOTICE:  57014\n"):
            with self.subTest(stderr_kind=stderr[:8]):
                self.assertIsNone(DELIVERY.safe_sqlstate(stderr))
        for severity, code in ((b"ERROR", b"57014"), (b"FATAL", b"53300"), (b"PANIC", b"53100")):
            self.assertEqual(DELIVERY.safe_sqlstate(severity + b":  " + code + b"\r\n"), code.decode())

    def test_secret_bearing_stderr_cannot_enter_failure_metadata(self):
        error = self.failure(b"ERROR:  57014 postgres://user:stderr-secret@private.invalid/db\n",
                             sqlstate_only=True)
        self.assertEqual(error.command_failure, {"returncode": 3, "elapsedSeconds": 2.5, "sqlstate": None})
        self.assertNotIn("secret", json.dumps(error.command_failure))

    def test_non_sql_command_never_classifies_stderr_as_sqlstate(self):
        self.assertIsNone(self.failure(b"ERROR:  57014\n").command_failure["sqlstate"])

    def test_success_returns_same_bytes_without_diagnostic(self):
        self.child.returncode = 0
        output = io.StringIO()
        with patch.object(DELIVERY.subprocess, "Popen", return_value=self.child) as spawn, \
                patch.object(DELIVERY.time, "monotonic", return_value=10.0) as clock, \
                redirect_stdout(output), redirect_stderr(output):
            self.assertEqual(self.value.run(self.command), b"stdout-secret")
        spawn.assert_called_once()
        clock.assert_called_once()
        self.assertEqual(output.getvalue(), "")
        self.assertNotIn("command_failure", vars(self.child))

    def test_output_bounds_still_block_before_nonzero_classification(self):
        for out, stderr, limit in ((b"too-long", b"ERROR:  57014\n", 1),
                                   (b"", b"stderr-secret" * 100000, 32 * 1024 * 1024)):
            with self.subTest(limit=limit):
                self.child.communicate.return_value = (out, stderr)
                with patch.object(DELIVERY.subprocess, "Popen", return_value=self.child):
                    with self.assertRaisesRegex(DELIVERY.Blocked, "command_output_exceeds_bound") as caught:
                        self.value.run(self.command, limit=limit, sqlstate_only=True)
                self.assertEqual(caught.exception.command_failure["returncode"], 3)
                if len(stderr) > 1024 * 1024:
                    self.assertIsNone(caught.exception.command_failure["sqlstate"])

    def test_timeout_kills_once_waits_and_reraises_same_exception(self):
        error = DELIVERY.subprocess.TimeoutExpired(self.command, 20, stderr=b"stderr-secret")
        self.child.communicate.side_effect = error
        self.child.returncode = -9
        with patch.object(DELIVERY.subprocess, "Popen", return_value=self.child) as spawn, \
                patch.object(DELIVERY.time, "monotonic", side_effect=[10.0, 30.0]):
            with self.assertRaises(DELIVERY.subprocess.TimeoutExpired) as caught:
                self.value.run(self.command, sqlstate_only=True)
        self.assertIs(caught.exception, error)
        self.assertEqual(error.command_failure, {"returncode": -9, "elapsedSeconds": 20.0, "sqlstate": None})
        spawn.assert_called_once()
        self.child.communicate.assert_called_once_with(None, timeout=20)
        self.child.kill.assert_called_once_with()
        self.child.wait.assert_called_once_with()

    def test_spawn_failure_reraises_same_exception_with_unknown_returncode(self):
        error = OSError("private-path-secret")
        with patch.object(DELIVERY.subprocess, "Popen", side_effect=error) as spawn, \
                patch.object(DELIVERY.time, "monotonic", side_effect=[10.0, 10.5]):
            with self.assertRaises(OSError) as caught:
                self.value.run(self.command)
        self.assertIs(caught.exception, error)
        self.assertEqual(error.command_failure, {"returncode": None, "elapsedSeconds": 0.5, "sqlstate": None})
        spawn.assert_called_once()

    def test_sql_dispatch_changes_only_error_format_not_limits_or_statement(self):
        self.value.base = types.SimpleNamespace(compose_command=Mock(return_value=self.command))
        self.value.args = object()
        self.value.run = Mock(return_value=b"value\n")
        self.assertEqual(self.value.sql("postgres", "SELECT 1;"), "value")
        self.value.base.compose_command.assert_called_once_with(
            self.value.args, "exec", "-T", "database", "psql", "-X", "-qAt",
            "-v", "ON_ERROR_STOP=1", "-v", "VERBOSITY=sqlstate", "-U", "postgres", "-d", "postgres")
        self.value.run.assert_called_once_with(
            self.command, b"SET statement_timeout=8000;\nSET lock_timeout=3000;\nSELECT 1;",
            limit=2 * 1024 * 1024, sqlstate_only=True)

    def test_execute_persists_only_safe_metadata_and_leaves_original_unknown(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        value = self.value
        value.root = Path(temporary.name)
        (value.root / "operations").mkdir()
        value.dir = value.root / "operations" / "original"
        descriptor = {"sourceSchema": {"version": 1}}
        value.packet = {"readOnly": False, "reconcile": False, "guardPid": 17, "policySha256": "a" * 64,
                        "sealedCandidate": {"descriptor": descriptor, "candidate": {}}}
        value.policy = {"initialDatabase": "source", "leaseSeconds": 30, "systemIdentifier": "system"}
        value.command = {"action": "deploy", "expectedManifestSha256": None}
        value.original = {"operationId": "original"}
        value.point = lambda name: None
        for name in ("engine", "roles", "history", "image", "guard"):
            setattr(value, name, Mock())
        value.fingerprint = lambda source: {"schema": descriptor["sourceSchema"]}
        value.drain = lambda source: value.run(self.command, b"sql-secret", sqlstate_only=True)
        output = io.StringIO()
        with patch.object(DELIVERY.subprocess, "Popen", return_value=self.child) as spawn, \
                patch.object(DELIVERY.time, "monotonic", side_effect=[10.0, 12.5]), \
                patch.object(DELIVERY, "sync_directory"), redirect_stdout(output), redirect_stderr(output):
            result = value.execute()
        spawn.assert_called_once()
        diagnostic = json.loads((value.dir / "diagnostic.json").read_bytes())
        self.assertEqual(diagnostic["class"], "Blocked")
        self.assertEqual(diagnostic["reason"], "owner_command_failed_or_unknown")
        self.assertEqual(diagnostic["commandFailure"], {"returncode": 3, "elapsedSeconds": 2.5, "sqlstate": None})
        self.assertNotIn("secret", json.dumps(diagnostic))
        self.assertNotIn("private", json.dumps(diagnostic))
        self.assertEqual(output.getvalue(), "")
        self.assertEqual(result["receipt"]["status"], "unknown")
        self.assertFalse(result["receipt"]["dispatchAllowed"])
        self.assertFalse(result["receipt"]["sdlcAcceptanceVerified"])
        self.assertFalse((value.dir / "result.json").exists())
        self.assertFalse((value.dir / "release-intent.json").exists())
        self.assertEqual(DELIVERY.read(value.dir / "intent.json")["receipt"], result["receipt"])


class SessionDiagnosticTests(unittest.TestCase):
    def setUp(self):
        self.value = owner()
        self.value.packet = {"guardPid": 17}
        self.value.intent = {"sourceDatabase": "source"}

    def observation(self, count=1, role="postgres", database="target", state="idle"):
        return {"unknownCount": count,
                "roleCounts": {name: count if name == role else 0
                               for name in ("postgres", "forge_writer", "reader", "other")},
                "sessions": [{"pid": 100 + i, "roleCategory": role, "state": state,
                              "databaseKind": database} for i in range(min(count, 16))]}

    def blocked(self, observation, released=None):
        self.value.sql = Mock(return_value=json.dumps(observation))
        with self.assertRaisesRegex(DELIVERY.Blocked, "^unconfirmed_previous_or_unknown_writer$") as caught:
            self.value.sessions(released)
        self.value.sql.assert_called_once()
        return caught.exception

    def test_foreign_reader_writer_and_other_categories_still_block(self):
        for role in ("postgres", "forge_writer", "reader", "other"):
            with self.subTest(role=role):
                observation = self.observation(role=role, database="foreign", state="active")
                error = self.blocked(observation, "candidate")
                self.assertEqual(error.session_observation["sessions"], observation["sessions"])
                self.assertEqual(error.session_observation["roleCounts"], observation["roleCounts"])
                query = self.value.sql.call_args.args[1]
                self.assertIn("AND NOT (usename IN ('forge_reader','forge_writer') AND datname='candidate')", query)
                self.assertIn("CASE WHEN datname='candidate' THEN 'target' ELSE 'foreign' END", query)

    def test_full_count_survives_sample_truncation(self):
        error = self.blocked(self.observation(count=37))
        self.assertEqual(error.session_observation["unknownCount"], 37)
        self.assertEqual(error.session_observation["roleCounts"]["postgres"], 37)
        self.assertEqual(len(error.session_observation["sessions"]), 16)
        self.assertTrue(error.session_observation["truncated"])
        query = self.value.sql.call_args.args[1]
        self.assertIn("WITH unknown AS MATERIALIZED", query)
        self.assertIn("ORDER BY pid LIMIT 16) sample)) FROM unknown;", query)
        self.assertIn("'unknownCount',count(*)", query)
        self.assertIn("backend_type='client backend' AND pid NOT IN (pg_backend_pid(),17) AND NOT (usename='forge_reader')", query)
        self.assertIn("CASE WHEN datname='source' THEN 'target' ELSE 'foreign' END", query)

    def test_invalid_json_and_unconfirmed_count_always_block_without_payload(self):
        for document in ("sql-secret", "{", "[]", "null", "1", '"0"',
                         '{"unknownCount":true}', '{"unknownCount":"0"}',
                         '{"unknownCount":-1}', '{"unknownCount":9223372036854775808}',
                         " " * (16 * 1024) + '{"unknownCount":0}'):
            with self.subTest(size=len(document)):
                self.value.sql = Mock(return_value=document)
                with self.assertRaisesRegex(DELIVERY.Blocked, "^unconfirmed_previous_or_unknown_writer$") as caught:
                    self.value.sessions()
                self.assertFalse(hasattr(caught.exception, "session_observation"))

    def test_malformed_optional_details_cannot_override_nonzero_count(self):
        for key, changed in (("sessions", []), ("sessions", [None]),
                             ("roleCounts", {"postgres": 0}), ("roleCounts", None)):
            error = self.blocked({**self.observation(), key: changed})
            self.assertEqual(error.session_observation, {"unknownCount": 1, "detailsAvailable": False})
        for key, changed in (("pid", True), ("pid", "100"), ("pid", 0), ("pid", 2147483648),
                             ("roleCategory", "raw-role-secret"), ("state", "SELECT 'sql-secret'"),
                             ("databaseKind", "postgres://user:password-secret@private/db")):
            observation = self.observation()
            observation["sessions"][0][key] = changed
            error = self.blocked(observation)
            self.assertEqual(error.session_observation, {"unknownCount": 1, "detailsAvailable": False})

    def test_duplicate_pids_inconsistent_counts_and_oversized_rows_are_unavailable(self):
        observations = [self.observation(2), self.observation(), self.observation(17)]
        observations[0]["sessions"][1]["pid"] = observations[0]["sessions"][0]["pid"]
        observations[1]["roleCounts"]["postgres"] = True
        observations[2]["sessions"].append(dict(observations[2]["sessions"][0], pid=200))
        for observation in observations:
            self.assertFalse(self.blocked(observation).session_observation["detailsAvailable"])

    def test_extra_raw_fields_are_dropped_and_sql_does_not_select_sensitive_columns(self):
        self.value.intent['sourceDatabase'] = 'raw-db-secret'
        observation = self.observation(role="other", state="other")
        secrets = {"query": "SELECT 'sql-secret'", "usename": "raw-role-secret", "datname": "raw-db-secret",
                   "application_name": "app-secret", "client_addr": "addr-secret",
                   "url": "postgres://user:password-secret@private/db"}
        observation.update(secrets)
        observation["sessions"][0].update(secrets)
        diagnostic = self.blocked(observation).session_observation
        self.assertNotIn("secret", json.dumps(diagnostic))
        self.assertEqual(set(diagnostic["sessions"][0]), {"pid", "roleCategory", "state", "databaseKind"})
        query = self.value.sql.call_args.args[1]
        for column in ("query", "application_name", "client_addr", "backend_start", "query_start"):
            self.assertNotIn(column, query)

    def test_missing_target_is_explicitly_unscoped_and_sample_boundary_is_not_truncated(self):
        self.value.intent = None
        error = self.blocked(self.observation(count=16, database="unscoped"))
        self.assertFalse(error.session_observation["truncated"])
        self.assertIn("'unscoped' AS database_kind", self.value.sql.call_args.args[1])

    def test_zero_count_preserves_normal_restore_even_with_unusable_optional_details(self):
        value = self.value
        value.sql = Mock(return_value='{"unknownCount":0,"sessions":"sql-secret"}')
        value.guard = Mock()
        value.args = object()
        value.database = lambda name: name
        value.base = types.SimpleNamespace(_restore_database=Mock())
        dump = Path(tempfile.gettempdir()) / "offline-dump"
        value.restore("target", dump)
        value.base._restore_database.assert_called_once_with(value.args, "target", dump)
        self.assertEqual(value.sql.call_count, 2)

    def test_unknown_blocks_before_restore_without_followup_sql_or_effect(self):
        value = self.value
        value.sql = Mock(return_value=json.dumps(self.observation()))
        value.guard = Mock()
        value.base = types.SimpleNamespace(_restore_database=Mock())
        with self.assertRaisesRegex(DELIVERY.Blocked, "unconfirmed_previous_or_unknown_writer"):
            value.restore("target", Path(tempfile.gettempdir()) / "offline-dump")
        value.sql.assert_called_once()
        value.base._restore_database.assert_not_called()

    def test_execute_stores_private_original_decision_and_keeps_unknown_receipt(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        value = self.value
        value.root = Path(temporary.name)
        (value.root / "operations").mkdir()
        value.dir = value.root / "operations" / "original"
        descriptor = {"sourceSchema": {"version": 1}}
        value.packet.update(readOnly=False, reconcile=False, policySha256="a" * 64,
                            sealedCandidate={"descriptor": descriptor, "candidate": {}})
        value.policy = {"initialDatabase": "source", "leaseSeconds": 30, "systemIdentifier": "system"}
        value.command = {"action": "deploy", "expectedManifestSha256": None}
        value.original = {"operationId": "original"}
        value.point = lambda name: None
        for name in ("engine", "roles", "history", "image", "guard", "drain", "release", "create_database"):
            setattr(value, name, Mock())
        value.fingerprint = lambda source: {"schema": descriptor["sourceSchema"]}
        value.base = types.SimpleNamespace(_restore_database=Mock())
        value.backup = lambda *_: value.restore("drill", value.root / "dump")
        value.sql = Mock(return_value=json.dumps(self.observation()))
        output = io.StringIO()
        with patch.object(DELIVERY, "sync_directory"), patch.object(DELIVERY.os, "chmod") as chmod, \
                redirect_stdout(output), redirect_stderr(output):
            result = value.execute()
        diagnostic = DELIVERY.read(value.dir / "diagnostic.json")
        self.assertEqual(diagnostic["reason"], "unconfirmed_previous_or_unknown_writer")
        self.assertEqual(diagnostic["sessionObservation"]["sessions"], self.observation()["sessions"])
        chmod.assert_any_call(value.dir / "diagnostic.json", 0o600)
        self.assertEqual(output.getvalue(), "")
        self.assertNotIn("sessionObservation", json.dumps(result))
        self.assertEqual(result["receipt"]["status"], "unknown")
        self.assertFalse(result["receipt"]["dispatchAllowed"])
        self.assertFalse(result["receipt"]["sdlcAcceptanceVerified"])
        self.assertEqual(DELIVERY.read(value.dir / "intent.json")["receipt"], result["receipt"])
        value.sql.assert_called_once()
        value.base._restore_database.assert_not_called()
        value.release.assert_not_called()
        value.create_database.assert_not_called()
        for name in ("result.json", "verified-checks.json", "release-intent.json", "reconciled.json"):
            self.assertFalse((value.dir / name).exists())
        # Even a forged diagnostic is never used as verification evidence by readback.
        (value.dir / "diagnostic.json").write_bytes(DELIVERY.canonical({"status": "verified", "unknownCount": 0}))
        self.assertEqual(value.readback()["receipt"]["status"], "unknown")


class ReaderPermissionsTests(unittest.TestCase):
    def test_all_mutation_surfaces_are_checked_and_select_is_not_denied(self):
        value = owner()
        queries = []

        def sql(database, query):
            self.assertEqual(database, "source")
            queries.append(query)
            return "f" if "has_database_privilege" in query else "0"

        value.sql = sql
        value.reader_permissions("source")
        self.assertEqual(len(queries), 5)
        self.assertIn("('r','p','v','m','f')", queries[0])
        self.assertIn("has_any_column_privilege", queries[0])
        self.assertIn("MAINTAIN", queries[0])
        self.assertIn("has_sequence_privilege", queries[1])
        self.assertIn("CASE WHEN c.relkind='S' THEN has_sequence_privilege", queries[1])
        self.assertIn("ELSE false END", queries[1])
        self.assertIn("'USAGE,UPDATE'", queries[1])
        self.assertNotIn("'SELECT", queries[1])
        self.assertIn("has_schema_privilege", queries[2])
        self.assertIn("'CREATE,TEMPORARY'", queries[3])
        self.assertIn("has_function_privilege", queries[4])
        self.assertIn("lo_from_bytea", queries[4])
        self.assertTrue(all("NOT LIKE 'pg_%'" not in query for query in queries))

    def test_each_write_permission_independently_blocks(self):
        for denied in range(4):
            with self.subTest(surface=denied):
                value = owner()
                calls = []

                def sql(database, query):
                    index = len(calls)
                    calls.append(query)
                    if index == denied:
                        return "t" if "has_database_privilege" in query else "1"
                    return "f" if "has_database_privilege" in query else "0"

                value.sql = sql
                with self.assertRaisesRegex(DELIVERY.Blocked, "application_reader_can_write_or_create"):
                    value.reader_permissions("source")

    def test_large_object_execute_permission_blocks(self):
        value = owner()
        value.sql = lambda database, query: ("1" if "has_function_privilege" in query else
                                              "f" if "has_database_privilege" in query else "0")
        with self.assertRaisesRegex(DELIVERY.Blocked, "application_reader_can_mutate_large_objects"):
            value.reader_permissions("source")

    def test_role_prefix_is_literal_and_memberships_are_not_skipped(self):
        value = owner()
        queries = []

        def sql(database, query):
            queries.append(query)
            if "pg_auth_members" in query:
                return "0"
            return json.dumps([dict(name=name, login=False, super=False, createdb=False,
                                    createrole=False, replication=False, bypass=False)
                               for name in ("postgres", "forge_reader", "forge_writer")])

        value.sql = sql
        self.assertEqual(set(value.roles()), {"postgres", "forge_reader", "forge_writer"})
        self.assertIn("left(rolname,3) <> 'pg_'", queries[0])
        self.assertIn("left(r.rolname,3) <> 'pg_'", queries[1])
        self.assertIn("left(u.rolname,3) <> 'pg_'", queries[1])


class SnapshotBoundaryTests(unittest.TestCase):
    def test_history_mutating_fk_actions_reject_before_base_evidence(self):
        value = owner()
        value.reader_permissions = lambda database: None
        value.args = None
        value.database = lambda database: database
        value.pg.database_evidence = lambda *_: self.fail("history FK reached Base")

        def sql(database, query):
            if "pg_database_size" in query:
                return "100"
            if "FROM pg_constraint" in query:
                self.assertIn("contype='f' AND conrelid='public._sqlx_migrations'::regclass", query)
                self.assertIn("confdeltype IN ('c','n','d') OR confupdtype IN ('c','n','d')", query)
                return "1"
            return "0"

        value.sql = sql
        with self.assertRaisesRegex(DELIVERY.Blocked, "unsupported_history_referential_actions"):
            value.fingerprint("source")

    def test_history_inheritance_rejects_in_either_direction_before_base_evidence(self):
        for relationship in ("child", "grandchild", "parent"):
            with self.subTest(relationship=relationship):
                value = owner()
                value.reader_permissions = lambda database: None
                value.args = None
                value.database = lambda database: database
                value.pg.database_evidence = lambda *_: self.fail("history inheritance reached Base")

                def sql(database, query):
                    if "pg_database_size" in query:
                        return "100"
                    if "FROM pg_inherits" in query:
                        self.assertIn("inhparent='public._sqlx_migrations'::regclass", query)
                        self.assertIn("OR inhrelid='public._sqlx_migrations'::regclass", query)
                        return "1"
                    return "0"

                value.sql = sql
                with self.assertRaisesRegex(DELIVERY.Blocked, "unsupported_history_inheritance"):
                    value.fingerprint("source")

    def test_application_stop_has_explicit_grace_inside_command_deadline(self):
        value = owner()
        value.project = "qa"
        value.application_compose = "compose.json"
        commands = []
        value.run = lambda command: commands.append(command)
        value.checkpoint = lambda *_: None
        value.guard = lambda: None
        value.sessions = lambda: None
        value.source_quiescent = lambda *_: None
        value.sql = lambda database, query: '["source","postgres","template0"]' if "json_agg(datname" in query else "0"
        value.drain("source")
        self.assertEqual(commands[0][-4:], ["stop", "--timeout", "5", "application"])

    def test_released_writer_sessions_are_allowed_only_on_the_selected_database(self):
        value = owner()
        value.packet = {"guardPid": 17}
        queries = []
        value.sql = lambda database, query: queries.append(query) or '{"unknownCount":0}'
        value.sessions("candidate")
        self.assertIn("usename IN ('forge_reader','forge_writer') AND datname='candidate'", queries[0])
        value.sessions()
        self.assertIn("NOT (usename='forge_reader')", queries[1])
        value.sql = lambda database, query: '{"unknownCount":1}'
        with self.assertRaisesRegex(DELIVERY.Blocked, "unconfirmed_previous_or_unknown_writer"):
            value.sessions("candidate")

    def test_application_dml_grants_exclude_views_and_history(self):
        value = owner()
        value.guard = lambda: None
        value.sessions = lambda: None
        value.reader_permissions = lambda database: None
        value.connection_scope = lambda *_: (_ for _ in ()).throw(DELIVERY.Blocked("stop_after_grants"))
        queries = []
        def sql(database, query):
            queries.append(query)
            return '["records"]' if "json_agg(c.relname" in query else "0"
        value.sql = sql
        with patch.dict(os.environ, {"CICD_LOCAL_PG_RUNTIME_PASSWORD": "disposable-test-password"}):
            with self.assertRaisesRegex(DELIVERY.Blocked, "stop_after_grants"):
                value.application({"database": "candidate"}, "hash")
        self.assertNotIn("GRANT SELECT,INSERT,UPDATE,DELETE ON ALL TABLES", queries[0])
        self.assertIn("c.relkind IN ('r','p')", queries[1])
        self.assertIn("c.relname<>'_sqlx_migrations'", queries[1])
        self.assertEqual(queries[2], 'GRANT INSERT,UPDATE,DELETE ON public."records" TO forge_writer;')

    def test_writer_view_and_rewrite_routes_are_rejected(self):
        for mode, reason in (("view", "application_writer_can_mutate_view"),
                             ("rule", "unsupported_writer_rules")):
            value = owner()
            value.reader_permissions = lambda database: None
            value.args = None
            value.database = lambda database: database
            value.pg.database_evidence = lambda *_: {"routines": [], "triggers": []}
            def sql(database, query):
                if "pg_database_size" in query:
                    return "100"
                if "pg_rewrite" in query:
                    return "1" if mode == "rule" else "0"
                if "has_schema_privilege('forge_writer'" in query:
                    return "f"
                if "c.relkind='v'" in query:
                    self.assertIn("has_any_column_privilege('forge_writer'", query)
                    return "1"
                return "0"
            value.sql = sql
            with self.assertRaisesRegex(DELIVERY.Blocked, reason):
                value.fingerprint("source")

    def test_reader_login_connect_or_session_prevents_drain_ack(self):
        for mode in ("reader_login", "writer_login", "connect", "session"):
            with self.subTest(mode=mode):
                value = owner()
                value.roles = lambda: {"forge_reader": {"login": mode == "reader_login"},
                                       "forge_writer": {"login": mode == "writer_login"}}
                value.sql = lambda database, query: (
                    "1" if mode == "connect" else "0"
                ) if "has_database_privilege" in query else ("1" if mode == "session" else "0")
                with self.assertRaisesRegex(DELIVERY.Blocked, "writer_drain_not_enforced"):
                    value.source_quiescent("source")

    def test_drain_closes_both_roles_before_ack(self):
        value = owner()
        events = []
        value.checkpoint = lambda name, payload=None: events.append(name)
        def sql(database, query):
            events.append(query)
            return '["source","failed","postgres"]' if "json_agg(datname" in query else None
        value.sql = sql
        value.guard = lambda: None
        value.sessions = lambda: None
        value.source_quiescent = lambda source: events.append("quiescent:" + source)
        value.drain("source")
        self.assertIn("ALTER ROLE forge_reader NOLOGIN", events[1])
        self.assertIn("FROM forge_writer,forge_reader", events[1])
        self.assertTrue(any("('forge_writer','forge_reader')" in event for event in events))
        self.assertTrue(any('DATABASE "failed"' in event for event in events))
        self.assertEqual(events[-2:], ["quiescent:source", "drained"])

    def test_connection_scope_rejects_old_database_or_old_sessions(self):
        for denied in (0, 1):
            value = owner()
            queries = []
            def sql(database, query):
                queries.append(query)
                return "1" if len(queries) - 1 == denied else "0"
            value.sql = sql
            with self.assertRaisesRegex(DELIVERY.Blocked, "application_database_connection_scope_changed"):
                value.connection_scope("target", True)
        value = owner()
        queries = []
        value.sql = lambda database, query: queries.append(query) or "0"
        value.connection_scope("target", False)
        self.assertIn("forge_writer',oid,'CONNECT') <> false", queries[0])
        self.assertIn("datname IS DISTINCT FROM 'target'", queries[1])

    def test_new_database_seals_large_object_functions_before_returning(self):
        value = owner()
        value.project = "qa"
        value.command = {"operationKey": "create"}
        value.guard = lambda: None
        queries = []
        def sql(database, query):
            queries.append((database, query))
            if "pg_proc" in query:
                return '["lo_create(oid)","lo_from_bytea(oid,bytea)"]'
            return "0"
        value.sql = sql
        target = value.create_database("candidate")
        self.assertEqual(queries[-1], (target, "REVOKE EXECUTE ON FUNCTION lo_from_bytea(oid,bytea) FROM PUBLIC,forge_reader,forge_writer;"))

    def test_writer_history_column_privileges_are_checked(self):
        value = owner()
        value.reader_permissions = lambda database: None
        value.args = None
        value.database = lambda database: database
        value.pg.database_evidence = lambda *_: {"routines": [], "triggers": []}
        def sql(database, query):
            if "pg_database_size" in query:
                return "100"
            if "has_schema_privilege('forge_writer'" in query:
                self.assertIn("has_any_column_privilege('forge_writer','public._sqlx_migrations'", query)
                self.assertIn("MAINTAIN", query)
                return "t"
            return "0"
        value.sql = sql
        with self.assertRaisesRegex(DELIVERY.Blocked, "application_writer_can_create_or_mutate_history"):
            value.fingerprint("source")

    def test_unmodeled_schema_or_large_objects_reject_before_base_evidence(self):
        for mode, reason in (("schema", "unsupported_database_catalog"),
                             ("large_objects", "unsupported_large_object_inventory"),
                             ("external_storage", "unsupported_database_catalog")):
            with self.subTest(mode=mode):
                value = owner()
                value.reader_permissions = lambda database: None
                value.args = None
                value.database = lambda database: database
                value.pg.database_evidence = lambda *_: self.fail("unsupported inventory reached Base")

                def sql(database, query):
                    if "pg_database_size" in query:
                        return "100"
                    if "FROM pg_namespace WHERE" in query:
                        self.assertIn("left(nspname,3) <> 'pg_'", query)
                        return "1" if mode == "schema" else "0"
                    if "pg_largeobject_metadata" in query:
                        return "1" if mode == "large_objects" else "0"
                    if "('m','f')" in query:
                        return "1" if mode == "external_storage" else "0"
                    return "0"

                value.sql = sql
                with self.assertRaisesRegex(DELIVERY.Blocked, reason):
                    value.fingerprint("source")


class LosslessRowFingerprintTests(unittest.TestCase):
    def fingerprint(self, legacy_json, records):
        value = owner()
        value.args = None
        value.database = lambda name: name
        value.reader_permissions = lambda name: None
        value.pg.database_evidence = lambda *_: {
            "schema_hash": "a" * 64, "routines": [], "triggers": [], "sequences": []
        }
        queries = []

        def sql(database, query):
            queries.append(query)
            if "pg_database_size" in query:
                return "1024"
            if "'schema',n.nspname,'table',c.relname" in query:
                return json.dumps([{"schema": "public", "table": "precise_values"}])
            if "jsonb_agg" in query:
                return json.dumps(records) if "ROW(t.*)::text" in query else legacy_json
            if "'version',version" in query:
                return json.dumps([{"version": 1, "checksum": "b" * 96, "success": True}])
            if "has_schema_privilege" in query:
                return "f"
            return "0"

        value.sql = sql
        return value.fingerprint("source"), queries

    def test_existing_precise_numeric_and_raw_json_changes_affect_digest(self):
        pairs = [
            ('[{"value":1.0000000000000000000000000001}]', '(1.0000000000000000000000000001)',
             '[{"value":1.0000000000000000000000000002}]', '(1.0000000000000000000000000002)'),
            ('[{"value":{"a":2}}]', '({"a":1,"a":2})',
             '[{"value":{"a":2}}]', '({"a":2})'),
        ]
        for first_json, first_record, second_json, second_record in pairs:
            with self.subTest(first_record=first_record):
                first, _ = self.fingerprint(first_json, [first_record])
                second, _ = self.fingerprint(second_json, [second_record])
                self.assertEqual(first["schema"], second["schema"])
                self.assertNotEqual(first["dataSha256"], second["dataSha256"])

    def test_query_has_fixed_lossless_text_format_and_order(self):
        _, queries = self.fingerprint('[]', ['(1)'])
        query = next(query for query in queries if "jsonb_agg" in query)
        for setting in ("timezone='UTC'", "extra_float_digits=3", "datestyle='ISO,YMD'",
                        "intervalstyle='postgres'", "bytea_output='hex'", "lc_monetary='C'"):
            self.assertIn(setting, query)
        self.assertIn('jsonb_agg(ROW(t.*)::text ORDER BY ROW(t.*)::text COLLATE "C")', query)
        self.assertNotIn("to_jsonb(t)", query)

    def test_row_encoding_is_domain_separated_and_preserves_null_empty_and_duplicates(self):
        encoded = json.dumps(['(,)', '("",)', '(1)', '(1)'])
        expected = DELIVERY.digest(DELIVERY.canonical({
            "encoding": "postgres-record-text/v1", "rows": json.loads(encoded)
        }))
        self.assertEqual(DELIVERY.row_fingerprint(encoded), expected)
        self.assertNotEqual(expected, DELIVERY.digest(DELIVERY.canonical(json.loads(encoded))))
        self.assertNotEqual(DELIVERY.row_fingerprint('["(,)"]'), DELIVERY.row_fingerprint('["(\\\"\\\",)"]'))
        self.assertNotEqual(DELIVERY.row_fingerprint('["(1)"]'), DELIVERY.row_fingerprint('["(1)","(1)"]'))

    def test_non_string_or_unbounded_record_inventory_is_rejected(self):
        for rows in (None, {}, [1], [None], [{}], [True], ['(1)'] * 513):
            with self.subTest(rows_type=type(rows).__name__):
                with self.assertRaisesRegex(DELIVERY.Blocked, "unsupported_record_text_inventory"):
                    DELIVERY.row_fingerprint(json.dumps(rows))
        self.assertIsInstance(DELIVERY.row_fingerprint(json.dumps(['(1)'] * 512)), str)


class HistoricalEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.value = value = owner()
        value.root = Path(self.temporary.name)
        value.dir = value.root / "operation"
        value.dir.mkdir()
        (value.root / "manifests").mkdir()
        value.packet = {"projectId": "project", "policySha256": "a" * 64}
        value.policy = {"systemIdentifier": "system", "checks": {"healthBodySha256": "b" * 64,
                                                                 "acceptanceBodySha256": "c" * 64}}
        value.command = {"operationKey": "deploy"}
        value.original = {"operationId": "original"}
        sql = "CREATE TABLE records(id integer);"
        descriptor = {"imageId": "sha256:" + "d" * 64, "targetSchema": {"version": 1, "catalogSha256": "e" * 64},
                      "migrations": [{"version": 1, "sql": sql}]}
        unknown = {"schema": "forge/local-postgres-operation/v1", "scope": "owner_local_isolated_verification",
                   "commandSha256": DELIVERY.digest(DELIVERY.canonical(value.command)),
                   "originalOperation": value.original, "generation": 1, "previousManifestSha256": None,
                   "manifestSha256": None, "status": "unknown", "dispatchAllowed": False,
                   "sdlcAcceptanceVerified": False}
        self.intent = {"command": value.command, "originalOperation": value.original, "policySha256": "a" * 64,
                       "lease": {"generation": 1}, "receipt": unknown, "candidate": {"artifactId": "artifact"},
                       "descriptor": descriptor}
        fingerprint = {"schema": descriptor["targetSchema"], "dataSha256": "1" * 64, "sequencesSha256": "2" * 64,
                       "history": [{"version": 1, "checksum": hashlib.sha384(sql.encode()).hexdigest(), "success": True}]}
        manifest = {"projectId": "project", "policySha256": "a" * 64, "operationKey": "deploy", "generation": 1,
                    "candidate": self.intent["candidate"], "descriptor": descriptor,
                    "systemIdentifier": "system", "database": "candidate", "databaseFingerprint": fingerprint}
        self.manifest_sha = DELIVERY.digest(DELIVERY.canonical(manifest))
        self.write(value.root / "manifests" / (self.manifest_sha + ".json"), manifest)
        self.write(value.dir / "intent.json", self.intent)
        expected = {"version": self.manifest_sha, "endVersion": self.manifest_sha,
                    "database": DELIVERY.digest(DELIVERY.canonical({"database": "candidate", "schemaVersion": 1})),
                    "health": "b" * 64, "acceptance": "c" * 64}
        self.proof = {**unknown, "status": "verified", "manifestSha256": self.manifest_sha,
                      "reason": "isolated_image_database_application_checks_verified", "database": "candidate",
                      "imageId": descriptor["imageId"], "containerId": "f" * 64,
                      "databaseFingerprint": fingerprint,
                      "probes": {name: {"status": "verified", "httpStatus": 200, "bodySha256": body,
                                        "observedAt": 1.0} for name, body in expected.items()}}

    def write(self, path, value):
        path.write_bytes(DELIVERY.canonical(value))

    def test_unknown_readback_validates_existing_historical_proof(self):
        path = self.value.dir / "verified-checks.json"
        self.write(path, self.proof)
        saved = path.read_bytes()
        self.assertTrue(self.value.readback()["reconciliationNeeded"])
        self.assertEqual(path.read_bytes(), saved)

    def test_foreign_or_tampered_proof_is_rejected_before_reconciliation(self):
        mutations = [("schema", "foreign"), ("scope", "foreign"), ("commandSha256", "0" * 64),
                     ("originalOperation", {"operationId": "foreign"}), ("generation", 2), ("generation", True),
                     ("previousManifestSha256", "0" * 64), ("status", "unknown"),
                     ("dispatchAllowed", True), ("sdlcAcceptanceVerified", True),
                     ("database", "foreign"), ("imageId", "sha256:" + "0" * 64),
                     ("containerId", "invalid"), ("reason", "foreign_success"), ("manifestSha256", "0" * 64)]
        path = self.value.dir / "verified-checks.json"
        for key, value in mutations:
            with self.subTest(field=key):
                proof = {**self.proof, key: value}
                self.write(path, proof)
                saved = path.read_bytes()
                with self.assertRaises((DELIVERY.Blocked, FileNotFoundError)):
                    self.value.readback()
                self.assertEqual(path.read_bytes(), saved)
                self.assertFalse((self.value.dir / "reconciled.json").exists())

    def test_probe_and_fingerprint_tampering_rejects(self):
        path = self.value.dir / "verified-checks.json"
        for key, changed in (("httpStatus", 503), ("bodySha256", "0" * 64),
                             ("status", "unavailable"), ("observedAt", float("nan")), ("observedAt", True)):
            proof = copy.deepcopy(self.proof)
            proof["probes"]["health"][key] = changed
            self.write(path, proof)
            with self.assertRaises(DELIVERY.Blocked):
                self.value.readback()
        proof = copy.deepcopy(self.proof)
        proof["databaseFingerprint"]["history"][0]["checksum"] = "0" * 96
        self.write(path, proof)
        with self.assertRaisesRegex(DELIVERY.Blocked, "applied_migration_bytes_changed_or_unknown"):
            self.value.readback()

    def test_fresh_reconciliation_preserves_old_proof_and_allows_new_rows(self):
        path = self.value.dir / "verified-checks.json"
        self.write(path, self.proof)
        saved = path.read_bytes()
        fresh = copy.deepcopy(self.proof)
        fresh["databaseFingerprint"]["dataSha256"] = "3" * 64
        fresh["probes"]["health"]["observedAt"] = 2.0
        self.write(self.value.dir / "reconciled.json", fresh)
        self.assertEqual(self.value.readback()["reconciledReceipt"], fresh)
        self.assertEqual(path.read_bytes(), saved)

    def test_historical_data_and_sequence_hashes_are_bound_to_manifest(self):
        path = self.value.dir / "verified-checks.json"
        for key in ("dataSha256", "sequencesSha256"):
            with self.subTest(field=key):
                proof = copy.deepcopy(self.proof)
                proof["databaseFingerprint"][key] = "0" * 64
                self.write(path, proof)
                with self.assertRaisesRegex(DELIVERY.Blocked, "historical_fingerprint_binding_mismatch"):
                    self.value.readback()
                self.assertFalse((self.value.dir / "reconciled.json").exists())

    def test_verified_receipt_requires_original_proof(self):
        self.write(self.value.dir / "result.json", self.proof)
        with self.assertRaisesRegex(DELIVERY.Blocked, "verified_evidence_incomplete"):
            self.value.readback()


if __name__ == "__main__":
    unittest.main()
