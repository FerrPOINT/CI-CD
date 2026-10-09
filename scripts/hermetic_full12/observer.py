"""Private SQL smoke observer; requires an existing actual ComposeHelper v2 session."""
import ast
from collections import Counter
import contextlib
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import time


TEMPLATE_SHA256 = 'd6c565d0c088583c7b34cacff4d74b44653e1e4f430afa275dcd45f12f0afce0'
EXECUTOR_SHA256 = '3cb62f6ed3dbfd641eb540eb2cdd603ee6ff7ba9a6ce81702c6f47444673c56f'
SDK_SHA256 = '2569882c3bb6a8b86ebb42367254720c5f802d5423bc98ef02c6fe3283e0874f'
DAEMON_ID = '38138e49-da1a-4f64-ba35-15cc6ef9400e'
AGGREGATE_SECONDS = 300
CALL_SECONDS = 30
FINAL_MARKER = 'ACTUAL_PG17_SQL_SMOKE:PASS'
MARKERS = (
    *(f'PG17_EMPTY_HISTORY_DESCENDANT_DEPTH_{depth}_GUARD_AND_INHERITED_DML:PASS' for depth in (1, 2)),
    'PG17_HISTORY_AS_CHILD_GUARD_AND_INHERITED_PARENT_DML:PASS',
    *(f'PG17_HISTORY_FK_{operation}_{action}_GUARD_AND_EFFECT:PASS'
      for operation in ('DELETE', 'UPDATE') for action in ('CASCADE', 'SET_NULL', 'SET_DEFAULT')),
    'PG17_HISTORY_FK_PASSIVE_ACTION_CONTRAST:PASS',
    'PG17_RESTORE_PRESERVES_SEALED_BUILTIN_FUNCTION_ACL:PASS',
    'PG17_REAL_BOTH_ROLE_DRAIN_AND_CANDIDATE_ONLY_READER:PASS',
    FINAL_MARKER,
)
REASONS = frozenset(('aggregate_deadline', 'call_timeout', 'command_unavailable', 'cleanup_unknown',
                     'assertion_failed', 'execution_failed', 'interrupted', 'incomplete_markers',
                     'input_rejected'))


class ClosedFailure(Exception):
    def __init__(self, reason):
        self.reason = reason if reason in REASONS else 'execution_failed'
        super().__init__(self.reason)


class Discard:
    def write(self, text):
        return len(text)

    def flush(self):
        pass


class Instrument(ast.NodeTransformer):
    """Instrument caller statements, not SQL function bodies or assertion expressions."""
    def visit_FunctionDef(self, node):
        return node

    def visit_ClassDef(self, node):
        return node

    def generic_visit(self, node):
        node = super().generic_visit(node)
        for field in ('body', 'orelse', 'finalbody'):
            values = getattr(node, field, None)
            if not isinstance(values, list):
                continue
            out = []
            for item in values:
                if isinstance(item, ast.stmt):
                    phase = ast.Expr(ast.Call(ast.Name('_phase', ast.Load()), [ast.Constant(item.lineno)], []))
                    out.append(ast.copy_location(phase, item))
                out.append(item)
            setattr(node, field, out)
        return node


def instrument_tree(raw):
    tree = ast.parse(raw)
    # The original import must not replace the injected bounded subprocess facade.
    tree.body = [ast.copy_location(ast.Pass(), node) if isinstance(node, ast.Import)
                 and [alias.name for alias in node.names] == ['subprocess'] else node for node in tree.body]
    return ast.fix_missing_locations(Instrument().visit(tree))


class Child:
    def __init__(self, owner, process, ordinal, phase, kind, started):
        self.owner, self.process = owner, process
        self.ordinal, self.phase, self.kind, self.started = ordinal, phase, kind, started
        self.closed = False

    @property
    def returncode(self):
        return self.process.returncode

    def poll(self):
        return self.process.poll()

    def completed(self):
        if not self.closed:
            self.closed = True
            self.owner.event('call_end', self, 'zero' if self.returncode == 0 else 'nonzero')

    def communicate(self, input=None, timeout=None):
        wait, limited_by_aggregate = self.owner.wait_budget(timeout)
        try:
            result = self.process.communicate(input=input, timeout=wait)
        except subprocess.TimeoutExpired:
            reason = 'aggregate_deadline' if limited_by_aggregate else 'call_timeout'
            self.owner.event('call_timeout', self, reason)
            self.owner.fail(reason)
        self.completed()
        self.owner.check()
        return result

    def terminate(self):
        self.owner.signal_group(self.process.pid, signal.SIGTERM)

    def wait(self, timeout=None):
        wait, limited_by_aggregate = self.owner.wait_budget(timeout)
        try:
            result = self.process.wait(timeout=wait)
        except subprocess.TimeoutExpired:
            reason = 'aggregate_deadline' if limited_by_aggregate else 'call_timeout'
            self.owner.event('call_timeout', self, reason)
            self.owner.fail(reason)
        self.completed()
        self.owner.check()
        return result


class Observer:
    PIPE = subprocess.PIPE

    def __init__(self, sink, *, clock=time.monotonic, popen=subprocess.Popen, signal_group=os.killpg if os.name == 'posix' else None):
        self.sink, self.clock, self.popen, self.signal_group = sink, clock, popen, signal_group
        self.started = clock()
        self.phase_id, self.ordinal, self.failure = 0, 0, None
        self.cleaning = False
        self.children, self.markers = [], Counter()

    def emit(self, record):
        self.sink.write('SMOKE_DIAGNOSTIC ' + json.dumps(record, sort_keys=True, allow_nan=False) + '\n')
        self.sink.flush()

    def event(self, event, child=None, outcome=None):
        now = self.clock()
        record = {'event': event, 'phase': f'L{self.phase_id:04d}', 'call': self.ordinal,
                  'elapsedSeconds': round(max(0, now - self.started), 6)}
        if child is not None:
            record.update(phase=f'L{child.phase:04d}', call=child.ordinal, kind=child.kind,
                          durationSeconds=round(max(0, now - child.started), 6))
        if outcome is not None:
            record['outcome'] = outcome
        self.emit(record)

    def fail(self, reason):
        self.failure = self.failure or reason
        raise ClosedFailure(self.failure)

    def interrupt(self, *_):
        self.failure = self.failure or 'interrupted'
        if not self.cleaning:
            raise ClosedFailure(self.failure)

    def check(self):
        if self.failure:
            raise ClosedFailure(self.failure)
        if self.clock() - self.started >= AGGREGATE_SECONDS:
            self.fail('aggregate_deadline')

    def phase(self, line):
        self.check()
        if type(line) is not int or not 1 <= line <= 248:
            self.fail('input_rejected')
        self.phase_id = line
        self.event('phase')

    def wait_budget(self, requested=None):
        self.check()
        remaining = AGGREGATE_SECONDS - (self.clock() - self.started)
        limit = CALL_SECONDS if requested is None else min(CALL_SECONDS, requested)
        return min(limit, remaining), remaining <= limit

    def Popen(self, args, **kwargs):
        self.check()
        if self.signal_group is None:
            self.fail('input_rejected')
        # Every CLI owns a new process group; parent/native/daemon processes are never signalled.
        kwargs['start_new_session'] = True
        kind = 'held' if not kwargs.pop('_synchronous', False) else 'command'
        self.ordinal += 1
        started = self.clock()
        self.event('call_start', outcome=kind)
        try:
            process = self.popen(args, **kwargs)
        except OSError:
            self.fail('command_unavailable')
        child = Child(self, process, self.ordinal, self.phase_id, kind, started)
        self.children.append(child)
        return child

    def run(self, args, *, capture_output=False, input=None, text=False):
        if not capture_output:
            self.fail('input_rejected')
        child = self.Popen(args, stdin=subprocess.PIPE if input is not None else None,
                           stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=text, _synchronous=True)
        out, error = child.communicate(input)
        return subprocess.CompletedProcess(args, child.returncode, out, error)

    def print(self, *values, **kwargs):
        self.check()
        if len(values) == 1 and isinstance(values[0], str) and values[0] in MARKERS:
            marker = values[0]
            self.markers[marker] += 1
            if marker != FINAL_MARKER:
                self.sink.write(marker + '\n')
                self.sink.flush()
        # Version/catalog payloads are deliberately not exported.

    def cleanup(self):
        self.cleaning = True
        try:
            self._cleanup()
        finally:
            self.cleaning = False

    def _cleanup(self):
        unknown = False
        # Do not signal historical/reaped PIDs: those IDs may already have been reused.
        active = [child for child in self.children if child.process.poll() is None]
        # Send both signals before reaping leaders, so each group ID remains owned.
        for sig in (signal.SIGTERM, signal.SIGKILL):
            for child in active:
                try:
                    self.signal_group(child.process.pid, sig)
                except ProcessLookupError:
                    pass
                except OSError:
                    unknown = True
        until = self.clock() + 2
        for child in active:
            remaining = until - self.clock()
            if remaining <= 0:
                unknown = True
                continue
            try:
                child.process.wait(timeout=remaining)
            except (subprocess.TimeoutExpired, OSError):
                unknown = True
        self.event('cleanup', outcome='unknown' if unknown else 'reaped')
        if unknown:
            self.failure = 'cleanup_unknown'
            raise ClosedFailure(self.failure)

    def finish(self):
        self.check()
        if self.markers != Counter(MARKERS):
            self.fail('incomplete_markers')
        self.sink.write(FINAL_MARKER + '\n')
        self.sink.flush()
        self.event('terminal', outcome='passed')


def validate_inputs(project, manifest, executor, template):
    if os.name != 'posix' or not re.fullmatch('sdlc-qa-forge-delivery-[a-f0-9]{12,32}', project):
        raise ClosedFailure('input_rejected')
    manifest, executor, template = Path(manifest), Path(executor), Path(template)
    if any(not path.is_absolute() or path.is_symlink() for path in (manifest, executor, template)):
        raise ClosedFailure('input_rejected')
    raw = template.read_bytes()
    if hashlib.sha256(raw).hexdigest() != TEMPLATE_SHA256 or hashlib.sha256(executor.read_bytes()).hexdigest() != EXECUTOR_SHA256:
        raise ClosedFailure('input_rejected')
    journal = json.loads((manifest.parent / 'journal.json').read_bytes())
    # SDK.run records execution-started; the reviewed parent's Compose up path
    # leaves manifest-validated until finally. Neither state is an acceptance receipt.
    if journal.get('phase') not in ('manifest-validated', 'execution-started'):
        raise ClosedFailure('input_rejected')
    expected = {'version': 2, 'project': project,
                'task': 'forge-task-delivery', 'purpose': 'native-pg17-oci-gates',
                'daemon_id': DAEMON_ID, 'sdk_sha256': SDK_SHA256,
                'manifest': str(manifest), 'manifest_sha256': hashlib.sha256(manifest.read_bytes()).hexdigest(),
                'docker': ['docker', '--context', 'rootless']}
    if any(journal.get(key) != value for key, value in expected.items()):
        raise ClosedFailure('input_rejected')
    pid = journal.get('owner_pid')
    if type(pid) is not int or pid <= 1:
        raise ClosedFailure('input_rejected')
    os.kill(pid, 0)
    return raw


def execute(raw, observer, argv):
    namespace = {'__name__': '__main__', 'subprocess': observer,
                 '_phase': observer.phase, 'print': observer.print}
    before = sys.argv
    sys.argv = ['sql_smoke.py', *argv]
    failure = None
    try:
        with contextlib.redirect_stdout(Discard()), contextlib.redirect_stderr(Discard()):
            exec(compile(instrument_tree(raw), 'pinned_sql_smoke', 'exec', optimize=0), namespace)
    except BaseException as error:
        failure = error.reason if isinstance(error, ClosedFailure) else (
            'assertion_failed' if isinstance(error, AssertionError) else
            'interrupted' if isinstance(error, KeyboardInterrupt) else 'execution_failed')
    finally:
        sys.argv = before
        try:
            observer.cleanup()
        except BaseException:
            failure = 'cleanup_unknown'
    try:
        if failure:
            observer.fail(failure)
        observer.finish()
        return 0
    except ClosedFailure as error:
        observer.event('terminal', outcome=error.reason)
        return 1


def main():
    observer = Observer(sys.stdout)
    handlers = {}
    try:
        if len(sys.argv) != 4:
            raise ClosedFailure('input_rejected')
        project, manifest, executor = sys.argv[1:]
        raw = validate_inputs(project, manifest, executor, Path(__file__).resolve().parent.parent / 'sql_smoke.py')
        for sig in (signal.SIGTERM, signal.SIGINT):
            handlers[sig] = signal.signal(sig, observer.interrupt)
        return execute(raw, observer, [project, manifest, executor])
    except BaseException as error:
        observer.event('terminal', outcome=error.reason if isinstance(error, ClosedFailure) else 'input_rejected')
        return 1
    finally:
        for sig, handler in handlers.items():
            signal.signal(sig, handler)


if __name__ == '__main__':
    sys.exit(main())
