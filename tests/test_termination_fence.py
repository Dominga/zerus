#!/usr/bin/env python3
"""Termination admission only: private fake agents and a private tmux socket.

The fixture writes marker data directly. Production has no marker producer or
graceful controller yet; these are not real-provider termination controls.
"""
import hashlib
import json
import os
import subprocess
import sys
import time
import unittest
import uuid

if os.name == 'posix':
    import fcntl
    import test_pause
else:
    test_pause = None


@unittest.skipUnless(test_pause and test_pause.TMUX and sys.platform in ('linux', 'darwin'),
                     'Private Linux/macOS tmux required')
class TerminationFence(test_pause.Harness if test_pause else unittest.TestCase):
    def setUp(self):
        super().setUp()
        self.fenced = []

    def tearDown(self):
        # Only synthetic marker data in this fixture, never a runtime claim.
        # Remove it before the existing harness ends its owned fake agents.
        for name in self.fenced:
            if self.binding(name):
                self.marker(name, remove=True)
        super().tearDown()

    def path(self, name):
        state = self.root / 'state'
        paths = [*state.glob('*.json'), *(state / 'bindings').glob('*.json'),
                 *(state / 'sessions').glob('*.json')]
        return next(path for path in paths if
                    (row := json.loads(path.read_text())).get('name') == name
                    and not row.get('rename_shadow'))

    def marker(self, name, value=None, remove=False):
        with (self.root / 'state/.lock').open('r+b') as lock:
            until = time.monotonic() + 2
            while True:
                try:
                    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    break
                except BlockingIOError:
                    self.assertLess(time.monotonic(), until, 'Fixture writer stayed busy')
                    time.sleep(.01)
            path = self.path(name)
            row = json.loads(path.read_text())
            if remove:
                row.pop('terminating', None)
            else:
                row['terminating'] = value
            temporary = path.with_suffix('.fixture')
            temporary.write_text(json.dumps(row))
            temporary.chmod(0o600)
            temporary.replace(path)
        if not remove and name not in self.fenced:
            self.fenced.append(name)
        return row

    def refused(self, args, payload=None):
        result = subprocess.run([str(test_pause.HGS), *args], env=self.env,
                                input=None if payload is None else json.dumps(payload),
                                text=True, capture_output=True, timeout=10)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn('Session termination is unfinished', result.stderr)

    def test_pending_or_damaged_marker_refuses_conflicting_cli_admissions(self):
        name = self.start('claude')
        for marker in [None, False, 'damaged', {'request_id': str(uuid.uuid4())}]:
            with self.subTest(marker=marker):
                record = self.marker(name, marker)
                before = self.path(name).read_bytes()
                original = (self.root / 'argv.jsonl').read_bytes()
                identity = dict(request_id=str(uuid.uuid4()),
                                expected_run_id=record['run_id'],
                                expected_conversation_id=record['conversation_id'])
                for args in [('terminate', name), ('pause', name), ('resume', name),
                             ('resume', name, '--fresh'), ('rename', name, 'claude/p/other'),
                             ('fork', name, '-n', 'other', '-d')]:
                    self.refused(args)
                for command, fields in [
                    ('send', dict(text='must not reach the fake agent')),
                    ('send-now', dict(queue_id='not-admitted')),
                    ('clear-context', {}), ('compact-context', {}),
                    ('settings', dict(model='claude-sonnet-4-6')),
                    ('answer', dict(question_id='fixture', expected_question_hash='a' * 64, answers=[])),
                ]:
                    self.refused((command, name, '--json'), dict(identity, **fields))
                self.assertEqual(self.path(name).read_bytes(), before)
                self.assertEqual((self.root / 'argv.jsonl').read_bytes(), original)
                view = json.loads(self.hgs('inspect', name))
                self.assertEqual(view['allowed_actions'], [])
                self.assertTrue(all('Session termination is unfinished' in reason
                                    for reason in view['action_reasons'].values()))

    def test_existing_input_receipts_are_lookup_only_while_fenced(self):
        name = self.start('claude')
        record = self.marker(name, None)
        before = self.path(name).read_bytes()
        directory = self.root / 'state/input_receipts'
        directory.mkdir(mode=0o700, exist_ok=True)
        for status in ('submitted', 'in_progress', 'uncertain'):
            payload = dict(request_id=str(uuid.uuid4()), expected_run_id=record['run_id'],
                           expected_conversation_id=record['conversation_id'], text='Synthetic earlier input')
            wire = json.dumps(payload)
            saved = dict(payload, name=name, digest=hashlib.sha256(wire.encode()).hexdigest(), status=status)
            path = directory / (payload['request_id'] + '.json')
            path.write_text(json.dumps(saved))
            path.chmod(0o600)
            receipt_before = path.read_bytes()
            result = subprocess.run([str(test_pause.HGS), 'send', name, '--json'], env=self.env,
                                    input=wire, text=True, capture_output=True, timeout=10)
            if status == 'submitted':
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(json.loads(result.stdout), saved)
            else:
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('delivery uncertain', result.stderr)
            self.assertEqual(path.read_bytes(), receipt_before)
            self.assertEqual(self.path(name).read_bytes(), before)

    def test_original_completion_hooks_progress_but_no_session_start_rebinds(self):
        for agent, reason in [('claude', 'prompt_input_exit'), ('codex', 'other'), ('kimi', 'exit')]:
            with self.subTest(agent=agent):
                name = self.start(agent)
                original = self.marker(name, {'request_id': str(uuid.uuid4())})
                env = dict(self.env, HGS_SESSION=name, HGS_RUN_ID=original['run_id'])
                event = dict(session_id=original['conversation_id'], hook_event_name='SessionEnd', reason=reason)
                result = subprocess.run([str(test_pause.HGS), '__state', 'hook'], env=env,
                                        input=json.dumps(event), text=True, capture_output=True, timeout=10)
                self.assertEqual(result.returncode, 0, result.stderr)
                current = self.binding(name)
                self.assertEqual(current['session_end']['reason'], reason)
                for key in ('terminating', 'name', 'session_id', 'run_id', 'conversation_id',
                            'pid', 'process_start', 'supervisor', 'pane', 'launch_dir'):
                    self.assertEqual(current.get(key), original.get(key), key)
                before = self.path(name).read_bytes()
                for kind in ('SessionStart', 'PreToolUse', 'UserPromptSubmit'):
                    result = subprocess.run([str(test_pause.HGS), '__state', 'hook'], env=env,
                                            input=json.dumps(dict(event, hook_event_name=kind)),
                                            text=True, capture_output=True, timeout=10)
                    self.assertEqual(result.returncode, 2, result.stderr)
                    self.assertEqual(self.path(name).read_bytes(), before)

    def test_voluntary_fake_agent_exit_keeps_binding_and_supervisor_evidence(self):
        name = self.start('claude')
        original = self.marker(name, None)
        # A command only our fake Python agent recognizes; no production
        # provider command or new graceful delivery mechanism is exercised.
        for keys in [('send-keys', '-t', original['pane'], '-l', 'exit'),
                     ('send-keys', '-t', original['pane'], 'Enter')]:
            subprocess.run([str(self.bin / 'tmux'), *keys], env=self.env,
                           capture_output=True, text=True, check=True, timeout=5)
        self.wait(lambda: self.binding(name).get('exited_at'))
        current = self.binding(name)
        self.assertEqual(current['exit_code'], 0)
        self.assertEqual(current['completion_reason'], 'unconfirmed_exit')
        self.assertIn('terminating', current)
        self.assertIsNone(current['terminating'])
        self.assertEqual(current['conversation_id'], original['conversation_id'])
        self.assertEqual(list((self.root / 'state/archives').glob('*.json')), [])


if __name__ == '__main__':
    unittest.main()
