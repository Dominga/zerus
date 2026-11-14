"""Dispatch isolation through two scripted hosts and private binding state only."""
import hashlib
from itertools import product
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import uuid

import receipt_locator

ROOT = Path(__file__).resolve().parents[1]
HGS = Path(os.environ.get('HGS_TEST_BIN', ROOT / 'target/debug/hgs')).resolve()
GENERATION = 'private-scripted-generation'
QUESTION = {'id': 'question', 'approval': False, 'questions': [
    {'id': 'choice', 'header': 'Fixture', 'question': 'Choose',
     'multiSelect': False, 'options': [{'label': 'One'}]}]}


class Host:
    def __init__(self, scenario, label):
        self.record = {'name': f'dsh/example/{label}', 'backend': 'dsh', 'agent': 'dsh',
                       'session_id': str(uuid.uuid4()), 'run_id': str(uuid.uuid4()),
                       'conversation_id': f'session-{label}',
                       'agent_home': str(scenario.root / f'native-{label}'),
                       'cwd': str(scenario.root), 'paused': False}
        self.binding = scenario.binding(self.record['name'])
        self.binding.parent.mkdir(parents=True, exist_ok=True)
        self.binding.write_text(json.dumps(self.record))
        directory = scenario.state / 'dsh/hosts' / hashlib.sha256(self.record['agent_home'].encode()).hexdigest()[:16]
        directory.mkdir(parents=True)
        self.socket = socket.socket(socket.AF_UNIX)
        self.socket.bind(str(directory / 'host.sock'))
        self.socket.listen(16)
        self.socket.settimeout(.05)
        self.calls, self.errors, self.workers = [], [], []
        self.mode = ''
        self.hold = None
        self.unknown = set()
        self.replies = {}
        self.entered, self.release, self.writer_entered = threading.Event(), threading.Event(), threading.Event()
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self.serve)
        self.thread.start()

    def serve(self):
        while not self.stop.is_set():
            try:
                client, _ = self.socket.accept()
            except socket.timeout:
                continue
            worker = threading.Thread(target=self.answer, args=(client,))
            self.workers.append(worker)
            worker.start()

    def answer(self, client):
        try:
            with client:
                client.settimeout(5)
                request = json.loads(client.makefile('rb').readline(1 << 20))
                self.calls.append(request)
                method, params = request['method'], request['params']
                if method in ('rename', 'pause', 'create', 'session-action'):
                    self.writer_entered.set()
                if method == self.hold:
                    self.entered.set()
                    if not self.release.wait(10):
                        raise AssertionError('own scripted RPC was not released')
                selection = {'provider': 'hgs-test', 'model': 'test-a', 'reasoningEffort': 'high'}
                values = {'modelSelection': {'lastUsed': selection}, 'turnOutline': []}
                if method == 'ping':
                    value = {'generation': GENERATION, 'version': '0.2.1-rc.2', 'protocol': 1,
                             'accountPermissions': True, 'sessionActions': True}
                elif method == 'inspect':
                    events = [{'event': {'type': 'turn/start', 'time': 1_000_000}}] if self.mode == 'interrupt' else []
                    value = {'generation': GENERATION, 'live': True, 'authRequired': False,
                             'baseline': {'values': values}, 'page': {'records': events, 'hasMore': False},
                             'selection': selection, 'pendingQuestions': [QUESTION] if self.mode == 'answer' else [],
                             'compactSupported': True, 'interruptSupported': True,
                             'lifecycleActions': ['pause', 'resume', 'rename', 'forget']}
                elif method == 'list':
                    value = {'generation': GENERATION, 'items': [
                        {'sessionId': self.record['conversation_id'], 'agentAvailable': True,
                         'running': self.mode == 'interrupt', 'updatedAt': 1_000_000,
                         'projections': {'values': values}, 'selection': selection}]}
                elif method == 'settings':
                    value = {'selected': selection}
                else:
                    value = {}
                if method in self.replies:
                    value = self.replies[method](params, value)
                if method in self.unknown:
                    client.sendall(b'{lost acknowledgement\n')
                else:
                    client.sendall((json.dumps({'ok': True, 'value': value}) + '\n').encode())
        except BaseException as error:
            self.errors.append(error)

    def close(self):
        self.release.set()
        self.stop.set()
        self.thread.join(timeout=2)
        self.socket.close()
        for worker in self.workers:
            worker.join(timeout=2)
        assert not self.thread.is_alive() and not any(worker.is_alive() for worker in self.workers)
        assert not self.errors, self.errors


class Scenario:
    def __enter__(self):
        self.temp = tempfile.TemporaryDirectory(prefix='zdl-', dir='/tmp')
        self.root = Path(self.temp.name)
        self.state = self.root / 'state'
        self.children = []
        for name in ('home', 'native-default', 'tmux', 'config', 'bin', 'xdg-state', 'xdg-cache', 'xdg-data'):
            (self.root / name).mkdir()
        # Even a fixture error must not fall back to a real installed harness.
        shim = self.root / 'bin/dsh'
        shim.write_text('#!/bin/sh\nprintf "unexpected native harness invocation\\n" >&2\nexit 77\n')
        shim.chmod(0o700)
        self.env = {k: v for k, v in os.environ.items()
                    if not k.startswith(('HGS_', 'DSH_', 'TMUX', 'XDG_', 'ZERUS_', 'OPENCODE_'))
                    and k not in ('DISPLAY', 'WAYLAND_DISPLAY', 'DBUS_SESSION_BUS_ADDRESS', 'SSH_AUTH_SOCK', 'LD_PRELOAD')}
        self.env.update(HOME=str(self.root / 'home'), DSH_HOME=str(self.root / 'native-default'),
                        HGS_STATE_DIR=str(self.state), HGS_CONFIG_DIR=str(self.root / 'config'),
                        HGS_SELF='fixture', HGS_PEERS='', TMUX_TMPDIR=str(self.root / 'tmux'),
                        XDG_CONFIG_HOME=str(self.root / 'config'), XDG_STATE_HOME=str(self.root / 'xdg-state'),
                        XDG_CACHE_HOME=str(self.root / 'xdg-cache'), XDG_DATA_HOME=str(self.root / 'xdg-data'),
                        PATH=str(self.root / 'bin') + os.pathsep + self.env['PATH'])
        self.a, self.b = Host(self, 'a'), Host(self, 'b')
        return self

    def __exit__(self, kind, error, traceback):
        self.a.release.set()
        self.b.release.set()
        problems = []
        try:
            for child in self.children:
                if child.poll() is None:
                    try:
                        child.communicate(timeout=5)
                    except subprocess.TimeoutExpired:
                        child.kill()
                        child.communicate(timeout=5)
                        problems.append('own CLI needed fallback cleanup')
            for host in (self.a, self.b):
                try:
                    host.close()
                except BaseException as problem:
                    problems.append(str(problem))
            try:
                receipt_locator.idle(self.state)
            except BaseException as problem:
                problems.append(str(problem))
        finally:
            self.temp.cleanup()
        if problems:
            detail = 'Private fixture cleanup: ' + repr(problems)
            if error is not None:
                error.add_note(detail)
            else:
                raise AssertionError(detail)

    def binding(self, name):
        return self.state / 'dsh/bindings' / (hashlib.sha256(name.encode()).hexdigest() + '.json')

    def payload(self, host, command):
        value = {'request_id': str(uuid.uuid4()), 'expected_run_id': host.record['run_id'],
                 'expected_conversation_id': host.record['conversation_id']}
        if command == 'send':
            value['text'] = 'Private scripted message'
        elif command == 'settings':
            value.update(model='hgs-test/test-a', effort='high')
        elif command == 'answer':
            digest = hashlib.sha256(json.dumps([GENERATION, QUESTION], sort_keys=True,
                                               separators=(',', ':')).encode()).hexdigest()
            value.update(question_id='question', expected_question_hash=digest,
                         answers=[{'question_id': 'choice', 'selected_option_ids': ['0'], 'text': ''}])
        elif command == 'interrupt':
            value['expected_turn_started'] = 1000.0
        return value

    def start(self, host, command, *, request=None, extra=(), public=False):
        args = [str(HGS)] + ([] if public else ['__state']) + [command, host.record['name'], *extra]
        if request is not None:
            args.append('--json')
        child = subprocess.Popen(args, cwd=ROOT, env=self.env, text=True,
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.children.append(child)
        child.stdin.write(json.dumps(request) if request is not None else '')
        child.stdin.close()
        child.stdin = None
        return child


@unittest.skipUnless(os.name == 'posix', 'Unix scripted native hosts')
class DeepSeekDispatchLocks(unittest.TestCase):
    def done(self, child, timeout=5):
        out, err = child.communicate(timeout=timeout)
        self.assertEqual(child.returncode, 0, out + err)
        return json.loads(out) if out.strip().startswith('{') else out

    def test_fast_send_is_independent_of_each_slow_rpc(self):
        for command, method in [('send', 'inspect'), ('send', 'send'), ('settings', 'settings'),
                                ('answer', 'answer'), ('interrupt', 'interrupt'), ('compact-context', 'compact')]:
            with self.subTest(command=command, method=method), Scenario() as scenario:
                started = time.monotonic()
                self.done(scenario.start(scenario.b, 'send', request=scenario.payload(scenario.b, 'send'), public=True))
                baseline = time.monotonic() - started
                scenario.a.mode, scenario.a.hold = command, method
                slow = scenario.start(scenario.a, command, request=scenario.payload(scenario.a, command))
                self.assertTrue(scenario.a.entered.wait(5), 'slow A never entered its original RPC')
                started = time.monotonic()
                fast = scenario.start(scenario.b, 'send', request=scenario.payload(scenario.b, 'send'), public=True)
                completed = True
                try:
                    receipt = self.done(fast, timeout=2)
                    self.assertEqual(receipt['status'], 'submitted')
                except subprocess.TimeoutExpired:
                    completed = False
                elapsed = time.monotonic() - started
                self.assertIsNone(slow.poll(), 'slow RPC was no longer held')
                scenario.a.release.set()
                self.done(slow)
                if not completed:
                    self.done(fast)
                print('DSH_DISPATCH_TIMING ' + json.dumps(dict(command=command, method=method,
                      baseline=baseline, elapsed=elapsed, completed_before_release=completed)), file=sys.stderr)
                self.assertTrue(completed, 'fast B waited behind slow A until its RPC was released')

    def test_binding_writers_wait_for_their_session_without_blocking_another(self):
        for (slow_command, method), command in product(
                [('send', 'send'), ('interrupt', 'interrupt')], ('pause', 'resume', 'rename', 'forget')):
            with self.subTest(command=command, slow_command=slow_command), Scenario() as scenario:
                scenario.a.mode, scenario.a.hold = slow_command, method
                slow = scenario.start(scenario.a, slow_command, request=scenario.payload(scenario.a, slow_command))
                self.assertTrue(scenario.a.entered.wait(5))
                before = scenario.a.binding.read_bytes()
                writer = scenario.start(scenario.a, command, extra=('dsh/example/renamed',) if command == 'rename' else ())
                started = time.monotonic()
                fast = scenario.start(scenario.b, 'send', request=scenario.payload(scenario.b, 'send'), public=True)
                self.done(fast, timeout=2)
                self.assertFalse(scenario.a.writer_entered.wait(1), 'writer bypassed the in-flight session RPC')
                self.assertEqual(scenario.a.binding.read_bytes(), before)
                self.assertIsNone(writer.poll())
                scenario.a.release.set()
                receipt = self.done(slow)
                self.done(writer)
                self.assertEqual(receipt['conversation_id'], scenario.a.record['conversation_id'])
                self.assertTrue(scenario.a.writer_entered.is_set())
                if command == 'rename':
                    self.assertFalse(scenario.a.binding.exists())
                    renamed = json.loads(scenario.binding('dsh/example/renamed').read_text())
                    self.assertEqual(renamed['conversation_id'], scenario.a.record['conversation_id'])
                elif command == 'forget':
                    self.assertFalse(scenario.a.binding.exists())
                else:
                    changed = json.loads(scenario.a.binding.read_text())
                    self.assertEqual(changed['paused'], command == 'pause')
                    if command == 'resume':
                        self.assertNotEqual(changed['run_id'], scenario.a.record['run_id'])
                print('DSH_DISPATCH_WRITER ' + json.dumps(dict(command=command, elapsed=time.monotonic()-started)), file=sys.stderr)

    def test_unknown_send_is_not_replayed(self):
        with Scenario() as scenario:
            scenario.a.unknown.add('send')
            request = scenario.payload(scenario.a, 'send')
            for _ in range(2):
                child = scenario.start(scenario.a, 'send', request=request)
                out, err = child.communicate(timeout=5)
                self.assertNotEqual(child.returncode, 0, out + err)
                self.assertIn('uncertain', err.lower())
            sends = [call for call in scenario.a.calls if call['method'] == 'send']
            self.assertEqual(len(sends), 1)
            self.assertEqual(sends[0]['params']['requestId'], request['request_id'])
            receipt = json.loads((scenario.state / 'dsh/receipts' / (request['request_id'] + '.json')).read_text())
            self.assertEqual(receipt['status'], 'pending')

    def test_binding_write_refuses_a_changed_record_after_rpc(self):
        import fcntl
        with Scenario() as scenario:
            scenario.a.hold = 'pause'
            writer = scenario.start(scenario.a, 'pause')
            self.assertTrue(scenario.a.entered.wait(5))
            with (scenario.state / 'dsh/bindings.lock').open('r+b') as bindings:
                fcntl.flock(bindings, fcntl.LOCK_EX)
                changed = json.loads(scenario.a.binding.read_text())
                changed['metadata_fixture'] = 'written while the RPC was in flight'
                scenario.a.binding.write_text(json.dumps(changed))
            scenario.a.release.set()
            out, err = writer.communicate(timeout=5)
            self.assertNotEqual(writer.returncode, 0, out + err)
            self.assertIn('binding changed', err.lower())
            self.assertEqual(json.loads(scenario.a.binding.read_text()), changed)

    def test_concurrent_renames_preserve_the_first_destination_binding(self):
        with Scenario() as scenario:
            target = 'dsh/example/destination'
            scenario.a.hold = scenario.b.hold = 'rename'
            first = scenario.start(scenario.a, 'rename', extra=(target,))
            self.assertTrue(scenario.a.entered.wait(5))
            second = scenario.start(scenario.b, 'rename', extra=(target,))
            self.assertTrue(scenario.b.entered.wait(5), 'a different native session waited behind A')
            scenario.a.release.set()
            self.done(first)
            published = scenario.binding(target).read_bytes()
            scenario.b.release.set()
            out, err = second.communicate(timeout=5)
            self.assertNotEqual(second.returncode, 0, out + err)
            self.assertIn('already used', err)
            self.assertEqual(scenario.binding(target).read_bytes(), published)
            self.assertEqual(json.loads(published)['conversation_id'], scenario.a.record['conversation_id'])
            self.assertEqual(json.loads(scenario.b.binding.read_text()), scenario.b.record)

    def test_id_binding_rename_keeps_the_native_target_and_route(self):
        with Scenario() as scenario:
            original = scenario.a.record.copy()
            scenario.a.record.update(layout='id', journal_name=original['session_id'])
            scenario.a.binding.unlink()
            scenario.a.binding = scenario.state / 'dsh/sessions' / (original['session_id'] + '.json')
            scenario.a.binding.parent.mkdir(parents=True)
            scenario.a.binding.write_text(json.dumps(scenario.a.record))
            self.done(scenario.start(scenario.a, 'rename', extra=('dsh/example/renamed',)))
            renamed = json.loads(scenario.a.binding.read_text())
            self.assertEqual(renamed['name'], 'dsh/example/renamed')
            for field in ('session_id', 'conversation_id', 'run_id', 'agent_home'):
                self.assertEqual(renamed[field], original[field])
            scenario.a.record = renamed
            request = scenario.payload(scenario.a, 'send')
            receipt = self.done(scenario.start(scenario.a, 'send', request=request))
            self.assertEqual(receipt['request_id'], request['request_id'])
            sent = next(call for call in scenario.a.calls if call['method'] == 'send')
            self.assertEqual(sent['params']['sessionId'], original['conversation_id'])

    def test_interrupt_request_id_is_reserved_across_native_sessions(self):
        with Scenario() as scenario:
            scenario.a.mode = scenario.b.mode = 'interrupt'
            scenario.a.hold = 'interrupt'
            request = scenario.payload(scenario.a, 'interrupt')
            first = scenario.start(scenario.a, 'interrupt', request=request)
            self.assertTrue(scenario.a.entered.wait(5))
            duplicate = scenario.payload(scenario.b, 'interrupt')
            duplicate['request_id'] = request['request_id']
            second = scenario.start(scenario.b, 'interrupt', request=duplicate)
            out, err = second.communicate(timeout=2)
            self.assertNotEqual(second.returncode, 0, out + err)
            self.assertIn('already attempted', err)
            self.assertFalse(any(call['method'] == 'interrupt' for call in scenario.b.calls))
            scenario.a.release.set()
            receipt = self.done(first)
            self.assertEqual(receipt['request_id'], request['request_id'])
            self.assertEqual(receipt['conversation_id'], scenario.a.record['conversation_id'])

    def test_unknown_interrupt_is_not_replayed(self):
        with Scenario() as scenario:
            scenario.a.mode = 'interrupt'
            scenario.a.unknown.add('interrupt')
            request = scenario.payload(scenario.a, 'interrupt')
            for expected in ('uncertain', 'already attempted'):
                child = scenario.start(scenario.a, 'interrupt', request=request)
                out, err = child.communicate(timeout=5)
                self.assertNotEqual(child.returncode, 0, out + err)
                self.assertIn(expected, err.lower())
            self.assertEqual(sum(call['method'] == 'interrupt' for call in scenario.a.calls), 1)
