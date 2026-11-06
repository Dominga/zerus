#!/usr/bin/env python3
"""Literal Codex message delivery against a keyless localhost Responses provider."""
import http.server
import json
import os
from pathlib import Path
import subprocess
import threading
import time
import uuid
import unittest

import test_clear_context_native as native
import test_codex_async_questions_native as questions
import test_codex_update
import test_input
import test_pause


@unittest.skipUnless(os.environ.get('HGS_CODEX_TEST_BIN'), 'Set HGS_CODEX_TEST_BIN for isolated native Codex input')
class NativeCodexInput(unittest.TestCase):
    script = test_input.InputTransport.script
    write_record = test_input.InputTransport.write_record
    tmux = test_input.InputTransport.tmux
    tearDown = test_input.InputTransport.tearDown
    payload = test_input.InputTransport.payload
    send = test_input.InputTransport.send
    fail = native.NativeCodexClear.fail
    wait_for = test_codex_update.NativeCodexUpdate.wait_for
    native_features = 'mentions_v2=false\n'

    def setUp(self):
        self.server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), questions.Provider)
        self.server.requests, self.server.release, self.server.questions = [], threading.Event(), []
        self.addCleanup(self.server.server_close)
        self.addCleanup(self.server.shutdown)
        self.addCleanup(self.server.release.set)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.provider_url = f'http://127.0.0.1:{self.server.server_port}/v1'
        native.NativeCodexClear.setUp(self)
        (self.root / 'literalreviewer.log').write_text('Synthetic file-completion target.\n')

    def screen(self):
        return self.tmux('capture-pane', '-p', '-t', self.pane)

    def test_leading_absolute_path_is_literal_message_text(self):
        text = '/help/fixture/capture.png\nWhat is shown in this screenshot?'
        payload = self.payload(text=text)
        receipt = self.send(payload)
        self.assertEqual(receipt['submitted_text'], text)
        self.wait_for(lambda: any(request['texts'][-1:] == [text] for request in self.server.requests))
        self.wait_for(lambda: 'Fixture turn complete.' in self.screen())
        self.assertEqual(self.send(payload), receipt)
        rows = map(json.loads, Path(self.record['transcript']).read_text().splitlines())
        messages = [row['payload']['message'] for row in rows if row.get('type') == 'event_msg'
                    and row.get('payload', {}).get('type') == 'user_message']
        self.assertEqual(messages, ['Previous fixture request', text])

    def test_literal_at_mentions_reach_the_model_once_without_file_completion(self):
        # Positive control: the native completion menu really has a matching file.
        # Warm the exact query before hgs sends, so its fixed paste delay cannot
        # accidentally avoid the regression through a slow first file search.
        text = 'Send a fixture test to @literalreviewer'
        self.tmux('send-keys', '-t', self.pane, '-l', text)
        self.wait_for(lambda: 'literalreviewer.log' in self.screen())
        self.tmux('send-keys', '-t', self.pane, 'Enter')
        self.wait_for(lambda: 'Send a fixture test to literalreviewer.log' in self.screen())
        self.assertEqual(self.server.requests, [], 'Completion must not submit a native turn')
        self.tmux('send-keys', '-t', self.pane, 'C-a', 'C-k')
        self.wait_for(lambda: 'Send a fixture test to' not in self.screen())

        payload = self.payload(text=text)
        receipt = self.send(payload)
        self.assertEqual(receipt['submitted_text'], text)
        self.wait_for(lambda: self.server.requests)
        self.assertEqual(self.server.requests[0]['texts'][-1], text)
        self.wait_for(lambda: 'Fixture turn complete.' in self.screen())
        self.assertEqual(self.send(payload), receipt)
        # A later native request exposes any replayed/queued duplicate too. The
        # reused fixture has no progress hooks, so send this control in Terminal
        # rather than fabricate an acknowledgement to unlock another hgs send.
        control = 'After the literal mention.'
        self.tmux('set-buffer', '-b', 'literal-control', control)
        self.tmux('paste-buffer', '-p', '-r', '-d', '-b', 'literal-control', '-t', self.pane)
        self.wait_for(lambda: control in self.screen())
        self.tmux('send-keys', '-t', self.pane, 'Enter')
        self.wait_for(lambda: any(request['texts'][-1:] == [control] for request in self.server.requests))
        # Codex may also ask this provider for an automatic task title. Check
        # typed user-message events in this exact native conversation, so
        # auxiliary model requests and injected environment context cannot
        # impersonate an accepted or duplicate turn.
        rows = map(json.loads, Path(self.record['transcript']).read_text().splitlines())
        messages = [row['payload']['message'] for row in rows if row.get('type') == 'event_msg'
                    and row.get('payload', {}).get('type') == 'user_message']
        self.assertEqual(messages, ['Previous fixture request', text, control])


class NativeCodexMentionsV2Input(NativeCodexInput):
    native_features = 'mentions_v2=true\n'


@unittest.skipUnless(os.environ.get('HGS_CODEX_TEST_BIN'), 'Set HGS_CODEX_TEST_BIN for isolated native Codex startup')
class NativeCodexStartup(test_pause.Harness):
    """Production launch/hooks/first send, with a keyless localhost model only."""
    def setUp(self):
        super().setUp()
        (self.bin / 'codex').unlink()
        (self.bin / 'codex').symlink_to(Path(os.environ['HGS_CODEX_TEST_BIN']).resolve())
        for key in list(self.env):
            if key.startswith(('OPENAI_', 'CODEX_', 'ANTHROPIC_', 'CLAUDE_')):
                self.env.pop(key, None)
        self.server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), questions.Provider)
        self.server.requests, self.server.release, self.server.questions = [], threading.Event(), []
        self.addCleanup(self.server.server_close)
        self.addCleanup(self.server.shutdown)
        self.addCleanup(self.server.release.set)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.native_home = self.root / '.codex'
        self.native_home.mkdir()
        (self.native_home / 'config.toml').write_text(
            'model="gpt-6.1-sol"\nmodel_provider="fixture"\ncheck_for_update_on_startup=false\n'
            '[features]\nplugins=false\nhooks=true\n'
            '[model_providers.fixture]\nname="Fixture"\nwire_api="responses"\n'
            f'base_url="http://127.0.0.1:{self.server.server_port}/v1"\n'
            '[projects.' + json.dumps(str(self.project)) + ']\ntrust_level="trusted"\n')
        self.env['CODEX_HOME'] = str(self.native_home)
        for key in ('XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_CACHE_HOME', 'XDG_STATE_HOME'):
            directory = self.root / key.lower()
            directory.mkdir()
            self.env[key] = str(directory)

    def screen(self):
        return self.t('capture-pane', '-p', '-t', '=' + self.name + ':')

    def until(self, label, predicate):
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            result = predicate()
            if result:
                return result
            time.sleep(.1)
        self.fail(label + '\n' + self.screen())

    def launch(self, trust=False):
        self.name = 'codex/p/startup'
        args = [] if trust else ['--dangerously-bypass-hook-trust']
        self.hgs('codex', 'p', '--new', '-n', 'startup', '-d', '--',
                 '--dangerously-bypass-approvals-and-sandbox', *args)
        self.run_id = self.until('The original supervisor must publish its binding',
                                 lambda: self.binding(self.name).get('run_id'))

    def inspect(self):
        return json.loads(self.hgs('inspect', self.name))

    def deliver_first(self):
        self.until('Fresh native composer must accept its first message',
                   lambda: self.inspect().get('first_message_can_send'))
        text = 'Synthetic first native message.'
        request = dict(request_id=str(uuid.uuid4()), expected_run_id=self.run_id,
                       expected_conversation_id='', text=text)
        argv = [str(test_pause.HGS), 'send', self.name, '--json']
        first = subprocess.run(argv, input=json.dumps(request), env=self.env,
                               text=True, capture_output=True, timeout=20)
        self.assertEqual(first.returncode, 0, first.stderr + '\n' + self.screen())
        self.until('The real hooks must confirm the first conversation and completed turn',
                   lambda: self.binding(self.name).get('conversation_id')
                   and self.binding(self.name).get('activity') == 'idle')
        record = self.binding(self.name)
        self.assertEqual(record['run_id'], self.run_id)
        self.assertTrue(record.get('transcript'))
        events = self.inspect()['events']
        self.assertTrue({'SessionStart', 'UserPromptSubmit', 'Stop'} <= {event['type'] for event in events})
        replay = subprocess.run(argv, input=json.dumps(request), env=self.env,
                                text=True, capture_output=True, timeout=20)
        self.assertEqual(replay.returncode, 0, replay.stderr)
        self.assertEqual(json.loads(first.stdout), json.loads(replay.stdout))
        self.assertEqual(sum(row['texts'][-1:] == [text] for row in self.server.requests), 1)

    def test_fresh_native_launch_reaches_its_first_turn_through_the_real_hooks(self):
        self.launch()
        self.deliver_first()

    def test_explicit_native_hook_trust_then_first_turn(self):
        self.launch(trust=True)
        card = self.until('The native hook trust choice must stay explicit', lambda: next(
            (q for q in self.inspect().get('pending_questions', []) if q.get('source') == 'codex_hooks_trust'), None))
        self.assertFalse(self.inspect()['first_message_can_send'])
        self.assertEqual(self.server.requests, [])
        request = dict(request_id=str(uuid.uuid4()), expected_run_id=self.run_id,
                       expected_conversation_id='', question_id=card['question_id'],
                       expected_question_hash=card['question_hash'],
                       answers=[dict(question_id='hooks_trust', selected_option_ids=['trust'], text='')])
        result = subprocess.run([str(test_pause.HGS), 'answer', self.name, '--json'],
                                input=json.dumps(request), env=self.env, text=True,
                                capture_output=True, timeout=20)
        self.assertEqual(result.returncode, 0, result.stderr + '\n' + self.screen())
        self.assertEqual(json.loads(result.stdout)['status'], 'answered')
        self.deliver_first()
