"""Remaining binding gates: private hosts, causal RPC holds, no live harness."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time
import test_dsh_dispatch_lock as dispatch_fixture
import unittest
import uuid

from test_dsh_dispatch_lock import GENERATION, HGS, Host, Scenario


@unittest.skipUnless(os.name == 'posix', 'Unix scripted native hosts')
class DeepSeekBindingLocks(unittest.TestCase):
    done = dispatch_fixture.DeepSeekDispatchLocks.done

    def cli(self, scenario, args, request=None):
        child = subprocess.Popen([str(HGS), *args], cwd=scenario.root, env=scenario.env,
                                 text=True, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                 stderr=subprocess.PIPE)
        scenario.children.append(child)
        child.stdin.write(json.dumps(request) if request is not None else '')
        child.stdin.close()
        child.stdin = None
        return child

    def action(self, scenario, action, host=None, **extra):
        host = host or scenario.a
        return dict(scenario.payload(host, ''), action=action, **extra)

    def scoped(self, scenario, request, host=None):
        return scenario.start(host or scenario.a, 'session-action', public=True, request=request)

    def setup_action(self, host):
        host.replies['session-action'] = lambda p, _: dict(
            generation=GENERATION, sessionId=host.record['conversation_id'],
            action=p['action'], status='completed')

    def independent(self, scenario, slow, *, expected_status=None, independent=True):
        self.assertTrue(scenario.a.entered.wait(5), 'A did not enter the held native RPC')
        started = time.monotonic()
        fast = scenario.start(scenario.b, 'send', public=True,
                              request=scenario.payload(scenario.b, 'send'))
        completed = True
        try:
            self.assertEqual(self.done(fast, timeout=1)['status'], 'submitted')
        except subprocess.TimeoutExpired:
            completed = False
        elapsed = time.monotonic() - started
        self.assertIsNone(slow.poll(), 'A must remain inside its held RPC')
        scenario.a.release.set()
        result = self.done(slow)
        if expected_status:
            self.assertEqual(result['status'], expected_status, result)
        if not completed:
            self.done(fast)
        print(f'B behind {scenario.a.hold}: {elapsed:.3f}s; before release={completed}', flush=True)
        self.assertEqual(completed, independent, 'unexpected cross-session lock scope')
        return result

    def test_scoped_lifecycle_does_not_hold_the_machine_lock(self):
        for action in ('pause', 'resume', 'rename', 'forget'):
            for method in ('ping', 'inspect', 'session-action'):
                with self.subTest(action=action, method=method), Scenario() as s:
                    self.setup_action(s.a)
                    s.a.hold = method
                    request = self.action(s, action, **({'new_name': 'dsh/example/renamed'}
                                                       if action == 'rename' else {}))
                    self.independent(s, self.scoped(s, request), expected_status='completed')

    def test_parent_claim_does_not_hold_the_child_machine_lock(self):
        with Scenario() as s:
            # Parent A's inspect/list is slow; the selected child is also Dsh.
            child = Host(s, 'child')
            try:
                self.done(self.cli(s, ['swarm', 'get', '--json']))
                s.a.hold = 'list'
                request = dict(s.payload(child, ''), action='adopt', parent={
                    key: s.a.record[key] for key in ('name', 'session_id', 'run_id', 'conversation_id')})
                request.pop('request_id')
                slow = s.start(child, 'parent', public=True, request=request)
                self.independent(s, slow)
            finally:
                child.close()

    @unittest.skipUnless(Path('/proc/self/fd').is_dir(), 'observes only this private CLI child on Linux')
    def test_parent_claim_is_observed_after_waiting_for_the_child_guard(self):
        with Scenario() as s:
            self.done(self.cli(s, ['swarm', 'get', '--json']))
            s.a.hold = 'send'
            slow = s.start(s.a, 'send', public=True, request=s.payload(s.a, 'send'))
            self.assertTrue(s.a.entered.wait(5))
            request = dict(s.payload(s.a, ''), action='adopt', parent={
                k: s.b.record[k] for k in ('name', 'session_id', 'run_id', 'conversation_id')})
            request.pop('request_id')
            parent = s.start(s.a, 'parent', public=True, request=request)
            identity = json.dumps([s.a.record['agent_home'], s.a.record['conversation_id']],
                                  separators=(',', ':'))
            guard = str(s.state / 'dsh/session-locks' / (hashlib.sha256(identity.encode()).hexdigest() + '.lock'))
            # Seeing the exact open guard fd proves the child reached its lock
            # wait. No fixed startup sleep and no owner's process inspection.
            deadline = time.monotonic() + 5
            reached = False
            while time.monotonic() < deadline and parent.poll() is None:
                for fd in (Path('/proc') / str(parent.pid) / 'fd').iterdir():
                    try:
                        if str(fd.readlink()) == guard:
                            reached = True
                    except FileNotFoundError:
                        pass
                if reached:
                    break
                time.sleep(.005)
            self.assertTrue(reached, 'private parent CLI never reached the selected session guard')
            # The parent becomes unavailable without a binding-file edit while
            # the child is queued. A pre-wait liveness snapshot is now stale.
            s.b.replies['list'] = lambda p, v: dict(v, items=[])
            s.a.release.set()
            self.done(slow)
            out, err = parent.communicate(timeout=5)
            self.assertNotEqual(parent.returncode, 0, out + err)
            self.assertIn('claimed parent must be a live session', err)
            self.assertNotIn('parent', json.loads(s.a.binding.read_text()))

    def test_launch_project_inspect_and_list_are_outside_machine_locks(self):
        for method in ('inspect', 'list'):
            with self.subTest(method=method), Scenario() as s:
                launch = str(uuid.uuid4())
                s.a.record['launch_id'] = launch
                s.a.binding.write_text(json.dumps(s.a.record))
                s.a.hold = method
                request = dict(request_id=launch, name=s.a.record['name'],
                               expected_run_id=s.a.record['run_id'],
                               expected_conversation_id=s.a.record['conversation_id'],
                               swarm_id=str(uuid.uuid4()), project_id='missing',
                               directory=str(s.root), add_folder=False)
                # A missing catalog project refuses after the binding gate; no mutation.
                slow = self.cli(s, ['swarm', 'assign-launch', '--json'], request)
                self.independent(s, slow, expected_status='failed')

    def launch(self, s, *, new=False):
        folder = s.root / 'example'
        folder.mkdir(exist_ok=True)
        s.env['DSH_HOME'] = s.a.record['agent_home']
        return self.cli(s, ['dsh', str(folder), '-n', 'created' if new else 'a', '-d'])

    def setup_fork(self, host):
        host.replies['ping'] = lambda p, v: dict(v, nativeOperations=['fork'])
        host.replies['inspect'] = lambda p, v: dict(v, forkSupported=True, forkReady=True)
        host.replies['fork'] = lambda p, v: dict(requestId=p['requestId'],
            sourceSessionId=p['sessionId'], sessionId='session-forked',
            cwd=p['cwd'], generation=GENERATION)

    def test_existing_launch_does_not_hold_the_machine_lock(self):
        for method in ('ping', 'create'):
            with self.subTest(method=method), Scenario() as s:
                s.a.hold = method
                self.independent(s, self.launch(s))

    def test_fork_preflight_does_not_hold_the_machine_lock(self):
        for method in ('ping', 'inspect'):
            with self.subTest(method=method), Scenario() as s:
                self.setup_fork(s.a)
                s.a.hold = method
                self.independent(s, self.cli(s, ['fork', s.a.record['name'], '-n', 'copy', '-d']))

    def test_creation_allocation_remains_serialized(self):
        for kind in ('launch', 'fork'):
            with self.subTest(kind=kind), Scenario() as s:
                self.setup_fork(s.a)
                s.a.hold = 'create' if kind == 'launch' else 'fork'
                slow = self.launch(s, new=True) if kind == 'launch' else self.cli(
                    s, ['fork', s.a.record['name'], '-n', 'copy', '-d'])
                self.independent(s, slow, independent=False)

    def test_unknown_launch_reuses_the_persisted_native_identity(self):
        with Scenario() as s:
            s.a.unknown.add('create')
            lost = self.launch(s, new=True)
            out, err = lost.communicate(timeout=5)
            self.assertNotEqual(lost.returncode, 0, out + err)
            pending = json.loads(s.binding('dsh/example/created').read_text())
            s.a.unknown.clear()
            folder = s.root / 'example'
            self.done(self.cli(s, ['dsh', str(folder), '-n', 'created', '-d']))
            creates = [c['params']['sessionId'] for c in s.a.calls if c['method'] == 'create']
            self.assertEqual(creates, [pending['conversation_id']] * 2)

    def test_scoped_writer_waits_for_ordinary_rpc_without_blocking_B(self):
        for action in ('pause', 'resume', 'rename', 'forget'):
            with self.subTest(action=action), Scenario() as s:
                self.setup_action(s.a)
                s.a.hold = 'send'
                slow = s.start(s.a, 'send', request=s.payload(s.a, 'send'), public=True)
                self.assertTrue(s.a.entered.wait(5))
                before = list(s.a.calls)
                request = self.action(s, action, **({'new_name': 'dsh/example/renamed'}
                                                   if action == 'rename' else {}))
                writer = self.scoped(s, request)
                self.done(s.start(s.b, 'send', request=s.payload(s.b, 'send'), public=True), timeout=1)
                self.assertFalse(s.a.writer_entered.wait(.3), 'writer bypassed the session guard')
                # The ordinary send has not returned; a scoped writer must not
                # even begin its host preflight while it is in flight.
                self.assertEqual(s.a.calls, before)
                self.assertIsNone(writer.poll())
                s.a.release.set()
                self.done(slow)
                self.assertEqual(self.done(writer)['status'], 'completed')

    def test_other_binding_paths_wait_for_the_inflight_session_without_blocking_B(self):
        for kind in ('parent', 'existing-launch', 'fork'):
            with self.subTest(kind=kind), Scenario() as s:
                self.setup_fork(s.a)
                s.a.record['parent'] = {'fixture': True}
                s.a.binding.write_text(json.dumps(s.a.record))
                before_bytes = s.a.binding.read_bytes()
                s.a.hold = 'send'
                slow = s.start(s.a, 'send', request=s.payload(s.a, 'send'), public=True)
                self.assertTrue(s.a.entered.wait(5))
                before = list(s.a.calls)
                if kind == 'parent':
                    request = dict(s.payload(s.a, ''), action='clear')
                    request.pop('request_id')
                    writer = s.start(s.a, 'parent', public=True, request=request)
                elif kind == 'existing-launch':
                    writer = self.launch(s)
                else:
                    writer = self.cli(s, ['fork', s.a.record['name'], '-n', 'copy', '-d'])
                self.done(s.start(s.b, 'send', request=s.payload(s.b, 'send'), public=True), timeout=1)
                try:
                    writer.wait(timeout=.3)
                except subprocess.TimeoutExpired:
                    pass
                self.assertIsNone(writer.poll(), 'binding writer bypassed the session guard')
                self.assertEqual(s.a.calls, before)
                self.assertEqual(s.a.binding.read_bytes(), before_bytes)
                s.a.release.set()
                self.done(slow)
                self.done(writer)

    def test_project_gate_is_metadata_only_even_with_an_unresponsive_host(self):
        with Scenario() as s:
            catalog = self.done(self.cli(s, ['swarm', 'get', '--json']))
            request = dict(s.payload(s.a, ''), name=s.a.record['name'],
                           expected_machine_id=catalog['node_id'])
            s.a.hold = 'inspect'
            started = time.monotonic()
            result = self.done(self.cli(s, ['swarm', 'session-project', '--json'], request), timeout=1)
            self.assertEqual(result['status'], 'ok', result)
            self.assertEqual(s.a.calls, [])
            print(f'project metadata gate: {time.monotonic() - started:.3f}s, zero host calls', flush=True)

    def test_recovery_gate_already_observes_outside_the_binding_lock(self):
        with Scenario() as s:
            s.a.hold = 'inspect'
            request = dict(s.payload(s.a, ''), name=s.a.record['name'],
                           job_id=str(uuid.uuid4()), action='cancel')
            slow = self.cli(s, ['recovery', 'action', '--scoped-json'], request)
            self.independent(s, slow, expected_status='failed')

    def test_launch_project_rechecks_the_full_binding_after_native_observation(self):
        with Scenario() as s:
            launch = str(uuid.uuid4())
            s.a.record['launch_id'] = launch
            s.a.binding.write_text(json.dumps(s.a.record))
            s.a.hold = 'list'
            request = dict(request_id=launch, name=s.a.record['name'],
                           expected_run_id=s.a.record['run_id'],
                           expected_conversation_id=s.a.record['conversation_id'],
                           swarm_id=str(uuid.uuid4()), project_id='missing',
                           directory=str(s.root), add_folder=False)
            slow = self.cli(s, ['swarm', 'assign-launch', '--json'], request)
            self.assertTrue(s.a.entered.wait(5))
            changed = dict(s.a.record, paused=True)
            s.a.binding.write_text(json.dumps(changed))
            s.a.release.set()
            result = self.done(slow)
            self.assertEqual(result['status'], 'failed', result)
            self.assertIn('binding changed during the native operation', result['error'])
            self.assertEqual(json.loads(s.a.binding.read_text()), changed)

    def test_scoped_unknown_is_not_replayed(self):
        with Scenario() as s:
            self.setup_action(s.a)
            s.a.unknown.add('session-action')
            request = self.action(s, 'pause')
            result = self.done(self.scoped(s, request))
            self.assertEqual(result['status'], 'uncertain', result)
            before = list(s.a.calls)
            self.assertEqual(self.done(self.scoped(s, request)), result)
            self.assertEqual(s.a.calls, before)
            self.assertEqual(json.loads(s.a.binding.read_text()), s.a.record)


if __name__ == '__main__':
    unittest.main()
