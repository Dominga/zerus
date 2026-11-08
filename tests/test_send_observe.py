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
import subprocess
import time
import unittest
import uuid

import test_answer_observe
import test_dsh_session_actions
import test_input as fixtures

HGS = fixtures.HGS
tree = test_answer_observe.tree
DRAFT = 'terminal has a draft, dialog, or unrecognized prompt; open Terminal and return to an empty agent prompt'


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
        self.assertIn("this computer's sessions only", result.stderr)
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
        self.assertIn('terminal agent sessions only', result.stderr)
    self.assertEqual(tree(self.root), before)
    self.assertEqual(self.calls, [])


# The DeepSeek suite's private synthetic Unix RPC host; its own tests are not run again.
ProviderSessionsRefuse = type('ProviderSessionsRefuse', (test_dsh_session_actions.DeepSeekScopedActions,), {
    **{name: None for name in dir(test_dsh_session_actions.DeepSeekScopedActions) if name.startswith('test')},
    'test_a_deepseek_observation_never_contacts_its_host': deepseek_observation_never_contacts_its_host,
    'test_an_opencode_observation_refuses_by_its_name': opencode_observation_refuses_by_its_name,
})


if __name__ == '__main__':
    unittest.main()
