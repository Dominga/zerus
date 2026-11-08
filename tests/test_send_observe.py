#!/usr/bin/env python3
"""The session resume actor's effect-free send observation (zerus-5wmh.66.4.10).

`hgs send <session> --observe --json` must say whether `hgs send` would take a
message for the exact run and conversation now, agreeing with what the send
itself then does, and must leave the whole fixture tree byte-identical: no
record, receipt, attachment, diagnostic capture, read mark, attention state,
lock file or directory change, nothing typed, and no terminal resize. Every
case runs on a private tmux socket with a fake agent; the pending Codex resume
is the input suite's native stand-in with an open rollout. An OpenCode or
DeepSeek session, or another computer, takes no observation and refuses
before any effect (zerus-5wmh.66.4.10.1).
"""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest
import uuid

import test_answer_observe
import test_dsh_session_actions
import test_input as fixtures

HGS = fixtures.HGS
tree = test_answer_observe.tree
DRAFT = 'terminal has a draft, dialog, or unrecognized prompt; open Terminal and return to an empty agent prompt'
# `cli::OBSERVE_ONLY`: the refusal of every send --observe but the supported one.
OBSERVE_ONLY = 'takes exactly `hgs send <codex session> --observe --json` on this computer'
# `zerus_client_core::run::rendezvous::ARG`.
RENDEZVOUS = '--desktop-helper-rendezvous-v1'


class SendObservation(unittest.TestCase):
    script = fixtures.InputTransport.script
    tmux = fixtures.InputTransport.tmux
    wait_for = fixtures.InputTransport.wait_for
    write_record = fixtures.InputTransport.write_record
    payload = fixtures.InputTransport.payload
    send = fixtures.InputTransport.send
    received = fixtures.InputTransport.received
    screen = fixtures.InputTransport.screen
    resumed_unconfirmed = fixtures.InputTransport.resumed_unconfirmed
    setUp = fixtures.InputTransport.setUp
    tearDown = fixtures.InputTransport.tearDown

    def observe(self, raw=None, **update):
        payload = dict(expected_run_id=self.run_id, expected_conversation_id=self.conversation_id)
        payload.update(update)
        return subprocess.run([str(HGS), 'send', self.name, '--observe', '--json'],
                              input=raw if raw is not None else json.dumps(payload),
                              text=True, capture_output=True, env=self.env, timeout=30)

    def observed(self, **update):
        """One observation, which must change nothing in the fixture root or
        the terminal: its reply."""
        before, height = tree(self.root), self.height()
        result = self.observe(**update)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(tree(self.root), before, 'the observation changed the fixture tree')
        self.assertEqual(self.height(), height, 'the observation resized the terminal')
        self.assertEqual(self.received(), b'', 'the observation typed into the agent')
        reply = json.loads(result.stdout)
        self.assertEqual(set(reply), {'ready', 'reason'})
        self.assertEqual(reply['ready'], reply['reason'] == '', reply)
        return reply

    def height(self):
        return self.tmux('display-message', '-p', '-t', self.pane, '#{window_height}').strip()

    def test_an_idle_agent_at_its_empty_prompt_is_ready_and_the_send_agrees(self):
        self.screen('\x1b[2J\x1b[H› \x1b[?2004h', '2:0')
        self.assertEqual(self.observed(), dict(ready=True, reason=''))
        # The send the observation predicted goes through.
        self.send()
        self.wait_for(lambda: self.received().endswith(b'\r'))

    def test_every_refusal_is_the_sends_own_and_changes_nothing(self):
        self.screen('\x1b[2J\x1b[H› existing draft', '16:0')
        self.assertEqual(self.observed(), dict(ready=False, reason=DRAFT))
        self.assertIn('draft', self.send(success=False))
        self.screen('\x1b[2J\x1b[H› \x1b[?2004h', '2:0')
        for update in [dict(expected_run_id=str(uuid.uuid4())),
                       dict(expected_conversation_id=str(uuid.uuid4()))]:
            with self.subTest(update=update):
                self.assertFalse(self.observed(**update)['ready'])
                self.send(self.payload(**update), success=False)
        # Busy with an unconfirmed input, or failed: never ready, as the send.
        initial = dict(self.record)
        for patch in [dict(input_pending_at=time.time() + 60), dict(error='provider failed')]:
            with self.subTest(patch=patch):
                self.record = dict(initial, **patch)
                self.write_record()
                self.assertFalse(self.observed()['ready'])
                self.send(success=False)
        self.record = initial
        self.write_record()
        self.assertEqual(self.received(), b'')

    def test_a_pending_codex_resume_is_ready_by_its_own_verification(self):
        self.resumed_unconfirmed()
        self.screen('\x1b[2J\x1b[H› ', '2:0')
        initial = dict(self.record)
        # A dialog instead of the restored conversation's empty composer, a
        # different history or a message already sent: not ready, as the send.
        other = self.root / 'other.jsonl'
        other.write_bytes(self.rollout.read_bytes())
        for patch, screen in [({}, ('\x1b[2J\x1b[HTrust this folder? ', '19:0')),
                              (dict(transcript=str(other)), None),
                              (dict(input_pending_at=time.time()), None)]:
            with self.subTest(patch=patch, screen=screen):
                self.record = dict(initial, **patch)
                self.write_record()
                self.screen(*(screen or ('\x1b[2J\x1b[H› ', '2:0')))
                self.assertFalse(self.observed()['ready'])
                self.send(success=False)
        self.record = initial
        self.write_record()
        self.screen('\x1b[2J\x1b[H› ', '2:0')
        self.assertEqual(self.observed(), dict(ready=True, reason=''))
        receipt = self.send()
        self.assertEqual(receipt['conversation_id'], self.conversation_id)
        self.wait_for(lambda: self.received().endswith(b'\r'))

    def test_a_remote_observation_is_refused_before_any_forwarding(self):
        # An older remote hgs would not know the flag (zerus-5wmh.66.4.10.1).
        before = tree(self.root)
        result = subprocess.run([str(HGS), '@remote', 'send', self.name, '--observe', '--json'],
                                input=json.dumps(dict(expected_run_id=self.run_id)), text=True,
                                capture_output=True, env=self.env, timeout=30)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(OBSERVE_ONLY, result.stderr)
        self.assertEqual(tree(self.root), before)

    def test_invalid_stdin_is_refused_and_changes_nothing(self):
        before = tree(self.root)
        for raw in ['', 'not json', json.dumps(dict(expected_run_id='')),
                    json.dumps(dict(expected_run_id=self.run_id, text='hello')),
                    json.dumps(dict(expected_run_id=self.run_id, pad='x' * (9 << 10)))]:
            with self.subTest(raw=raw[:40]):
                result = self.observe(raw=raw)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, '')
        self.assertEqual(tree(self.root), before)



def provider_send(case, *args, body):
    return subprocess.run([str(HGS), 'send', *args], input=json.dumps(body), text=True,
                          capture_output=True, env=case.env, timeout=30)


def deepseek_observation_never_contacts_its_host(self):
    """zerus-5wmh.66.4.10.1: a DeepSeek session's own send ignores the flag, so
    any send carrying --observe refuses before provider routing: no RPC call,
    receipt or state change, whatever the body. The same fixture takes an
    ordinary send, so the refusal is the observation's own."""
    send = dict(request_id=str(uuid.uuid4()), text='an observation must not send', attachments=[],
                expected_run_id='run', expected_conversation_id='session-synthetic')
    observe = dict(expected_run_id='run', expected_conversation_id='session-synthetic')
    before = tree(self.root)
    for args in [(self.name, '--observe', '--json'), (self.name, '--json', '--observe'),
                 (self.name, '--observe')]:
        for body in (send, observe):
            with self.subTest(args=args, body=sorted(body)):
                result = provider_send(self, *args, body=body)
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertEqual(result.stdout, '')
    self.assertEqual(self.calls, [], 'the observation contacted the DeepSeek host')
    self.assertEqual(tree(self.root), before)
    sent = provider_send(self, self.name, '--json', body=send)
    self.assertEqual(sent.returncode, 0, sent.stderr)
    self.assertEqual(json.loads(sent.stdout)['status'], 'submitted')
    self.assertIn('send', [call['method'] for call in self.calls])


def opencode_observation_refuses_by_its_name(self):
    """An OpenCode session refuses the same way, by its name alone."""
    before = tree(self.root)
    for name in ('opencode/example/session', 'opencode/other'):
        result = provider_send(self, name, '--observe', '--json',
                               body=dict(expected_run_id='run', expected_conversation_id='session'))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(OBSERVE_ONLY, result.stderr)
    self.assertEqual(tree(self.root), before)
    self.assertEqual(self.calls, [])


def gate_setup(self):
    """The DeepSeek fixture, plus a fake `ssh` and a stdin file outside the
    fixture tree: what would be forwarded, and whether stdin was read."""
    test_dsh_session_actions.DeepSeekScopedActions.setUp(self)
    outside = tempfile.TemporaryDirectory(prefix='hgs-observe-gate-')
    self.addCleanup(outside.cleanup)
    self.outside = Path(outside.name)
    (self.outside / 'bin').mkdir()
    self.forwarded = self.outside / 'forwarded'
    ssh = self.outside / 'bin/ssh'
    ssh.write_text(f'#!/bin/sh\necho "$@" >> {self.forwarded}\n')
    ssh.chmod(0o755)
    self.env['PATH'] = str(self.outside / 'bin') + ':' + self.env['PATH']
    self.env['HGS_PEERS'] = 'remote'


def gate_run(self, argv, body):
    """`hgs ARGV` with `body` on a file stdin: its result, and how far it
    read that stdin (the shared file offset)."""
    path = self.outside / 'stdin.json'
    path.write_text(json.dumps(body))
    with path.open('rb') as stdin:
        result = subprocess.run([str(HGS), *argv], stdin=stdin, text=True, capture_output=True,
                                env=self.env, timeout=30)
        return result, os.lseek(stdin.fileno(), 0, os.SEEK_CUR)


def every_other_entry_refuses_with_no_effect(self):
    """design-tima2 / review-win-e R2 on #1777 (zerus-5wmh.66.4.10.1): one
    first gate takes only `hgs send <codex session> --observe --json`. Every
    other way a send carrying --observe can enter hgs refuses before any
    effect: nothing forwarded, no RPC call, no stdin read, and no lock, ID
    repair or receipt anywhere in the fixture tree."""
    session_id = self.record['session_id']
    machine = str(uuid.uuid4())
    remote_reference = ('zerus://session?machine=' + machine +
                        '&host=remote&name=codex%2Fexample&conversation=c&run=r')
    codex = 'codex/example/session'
    cases = {
        'a Claude session': ['send', 'claude/example/session', '--observe', '--json'],
        'a Kimi session': ['send', 'kimi/example/session', '--observe', '--json'],
        'a DeepSeek session': ['send', self.name, '--observe', '--json'],
        'an OpenCode session': ['send', 'opencode/example/session', '--observe', '--json'],
        'another computer': ['@remote', 'send', codex, '--observe', '--json'],
        'a session reference': ['send', remote_reference, '--observe', '--json'],
        'a remote reference through @host': ['@remote', 'send', remote_reference, '--observe', '--json'],
        'a session ID': ['send', '--session-id', session_id, '--observe', '--json'],
        'a name and a session ID': ['send', codex, '--session-id', session_id, '--observe', '--json'],
        'a dry run': ['--dry-run', 'send', codex, '--observe', '--json'],
        'a forwarded client': ['--client', 'remote', 'send', codex, '--observe', '--json'],
        'a guarded mobile call': ['--expect-machine', machine, 'send', codex, '--observe', '--json'],
        'the private state ABI': ['__state', 'send', codex, '--observe', '--json'],
        'the helper rendezvous': [RENDEZVOUS, 'send', codex, '--observe', '--json'],
        'another order': ['send', codex, '--json', '--observe'],
        'no --json': ['send', codex, '--observe'],
        'an extra word': ['send', codex, '--observe', '--json', 'more'],
        'a name with a host': ['send', codex + '@remote', '--observe', '--json'],
        # Task send and handoff deliver in process (`input::submit_task`): no
        # flag reaches a send that way, and the task command never observes.
        'a task send': ['task', 'send', 'zerus-0000', '--observe'],
    }
    bodies = {
        'a send body': dict(request_id=str(uuid.uuid4()), text='an observation must not send', attachments=[],
                            expected_run_id='run', expected_conversation_id='session-synthetic'),
        'an observation body': dict(expected_run_id='run', expected_conversation_id='session-synthetic'),
    }
    before = tree(self.root)
    for case, argv in cases.items():
        for kind, body in bodies.items():
            with self.subTest(case=case, body=kind):
                result, read = gate_run(self, argv, body)
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertEqual(result.stdout, '')
                self.assertIn(OBSERVE_ONLY, result.stderr)
                self.assertEqual(read, 0, 'stdin was read')
    self.assertFalse(self.forwarded.exists(), 'forwarded to another computer')
    self.assertEqual(self.calls, [], 'contacted the DeepSeek host')
    self.assertEqual(tree(self.root), before, 'wrote into the fixture tree')
    # Controls: the same harness sees a forward and a stdin read.
    gate_run(self, ['@remote', 'send', codex, '--json'], bodies['a send body'])
    self.assertTrue(self.forwarded.exists(), 'an ordinary remote send forwards')
    result, read = gate_run(self, ['send', codex, '--observe', '--json'], bodies['an observation body'])
    self.assertEqual(result.returncode, 0, result.stderr)
    self.assertFalse(json.loads(result.stdout)['ready'])
    self.assertGreater(read, 0, 'the supported observation reads its stdin')
    self.assertEqual(tree(self.root), before)


ObservationGate = type('ObservationGate', (test_dsh_session_actions.DeepSeekScopedActions,), {
    **{name: None for name in dir(test_dsh_session_actions.DeepSeekScopedActions) if name.startswith('test')},
    'setUp': gate_setup,
    'test_every_other_entry_refuses_with_no_effect': every_other_entry_refuses_with_no_effect,
})


# The DeepSeek suite's private synthetic Unix RPC host; its own tests are not run again.
ProviderSessionsRefuse = type('ProviderSessionsRefuse', (test_dsh_session_actions.DeepSeekScopedActions,), {
    **{name: None for name in dir(test_dsh_session_actions.DeepSeekScopedActions) if name.startswith('test')},
    'test_a_deepseek_observation_never_contacts_its_host': deepseek_observation_never_contacts_its_host,
    'test_an_opencode_observation_refuses_by_its_name': opencode_observation_refuses_by_its_name,
})


if __name__ == '__main__':
    unittest.main()
