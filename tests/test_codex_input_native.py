#!/usr/bin/env python3
"""Literal Codex message delivery against a keyless localhost Responses provider."""
import http.server
import io
import json
import os
from pathlib import Path
import subprocess
import threading
import time
import tomllib
import uuid
import unittest
from unittest import mock

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
        class Provider(questions.Provider):
            def do_POST(self):
                raw = self.rfile.read(int(self.headers.get('Content-Length', '0')))
                self.server.raw_requests.append(json.loads(raw))
                self.rfile = io.BytesIO(raw)
                super().do_POST()
        self.server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Provider)
        self.server.raw_requests = []
        self.server.requests, self.server.release, self.server.questions = [], threading.Event(), []
        self.server.collaboration = False
        self.addCleanup(self.server.server_close)
        self.addCleanup(self.server.shutdown)
        self.addCleanup(self.server.release.set)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.provider_url = f'http://127.0.0.1:{self.server.server_port}/v1'
        native.NativeCodexClear.setUp(self)
        (self.root / 'literalreviewer.log').write_text('Synthetic file-completion target.\n')

    def screen(self):
        return self.tmux('capture-pane', '-p', '-t', self.pane)

    def prepare_native_hooks(self):
        if self._testMethodName == 'test_wrapped_native_title_keeps_guarded_input_and_drafts':
            self.geometry = (80, 24)  # This added case owns the reported narrow frame.
            isolated_home = self.root / 'native-profile-home'
            isolated_home.mkdir()
            self.env['HOME'] = str(isolated_home)  # Keep cwd absolute, not the home alias.
            config = self.native_home / 'config.toml'
            config.write_text('model_reasoning_effort="xhigh"\n' + config.read_text()
                              + '\n[tui]\nstatus_line=["model-with-reasoning","current-dir","thread-name"]\n')
            return
        if self._testMethodName != 'test_native_five_second_hook_timeout_keeps_acceptance_under_contention':
            return
        import shlex
        # Model two seconds of scheduling/startup delay before the real hook.
        # A held SQLite writer adds its real three-second busy wait. The native
        # five-second hook budget then expires; acceptance must already survive.
        wrapper = self.root / 'delayed-hook.py'
        wrapper.write_text('import subprocess,time\nfrom pathlib import Path\ntime.sleep(2)\n'
                           + 'child=subprocess.Popen(' + repr([str(test_input.HGS),'__state','hook']) + ')\n'
                           + 'Path(' + repr(str(self.root/'hook-child-pid')) + ').write_text(str(child.pid))\n'
                           + 'raise SystemExit(child.wait())\n')
        env = dict(HGS_SESSION=self.name,HGS_RUN_ID=self.run_id,HGS_STATE_DIR=str(self.state),
                   HGS_CONFIG_DIR=str(self.root/'config'),PATH=self.env['PATH'])
        command = 'env ' + ' '.join(shlex.quote(k+'='+v) for k,v in env.items()) + ' python3 ' + shlex.quote(str(wrapper))
        (self.native_home/'hooks.json').write_text(json.dumps(dict(hooks=dict(UserPromptSubmit=[dict(hooks=[
            dict(type='command',command=command,timeout=5)])]))))

    def test_native_five_second_hook_timeout_keeps_acceptance_under_contention(self):
        import sqlite3
        inspected = subprocess.run([str(test_input.HGS),'inspect',self.name],env=self.env,capture_output=True,text=True,timeout=15)
        self.assertEqual(inspected.returncode,0,inspected.stderr)
        db=sqlite3.connect(self.state/'events.sqlite3'); db.execute('BEGIN IMMEDIATE')
        started=time.monotonic()
        try:
            receipt=self.send(self.payload(text='Native timed hook fixture'))
            deadline=started+4
            while time.monotonic()<deadline:
                record=json.loads(self.record_path.read_text())
                if record.get('input_seen_at',0)>=receipt['submitted_at']: break
                time.sleep(.02)
            self.assertGreaterEqual(record.get('input_seen_at',0),receipt['submitted_at'])
            accepted=time.monotonic()-started
            # Keep contention until the provider has moved past its timeout.
            self.wait_for(lambda:'Fixture turn complete.' in self.screen())
            elapsed=time.monotonic()-started
            self.assertGreater(elapsed,4.5)
            print('ZERUS_NATIVE_HOOK_TIMEOUT='+json.dumps(dict(version=self.native_version,
                accepted_ms=round(accepted*1000),model_completed_ms=round(elapsed*1000))),flush=True)
        finally:
            db.rollback(); db.close()
            pid_path=self.root/'hook-child-pid'
            if pid_path.exists():
                pid=pid_path.read_text().strip()
                def ended():
                    state=subprocess.run(['ps','-p',pid,'-o','stat='],capture_output=True,text=True,timeout=5).stdout.strip()
                    return not state or state.startswith(('Z','X'))
                self.wait_for(ended)

    def test_consumed_input_without_hook_is_reconciled_from_exact_native_user_row(self):
        text = 'Exact hookless Codex fixture input ' + str(uuid.uuid4())
        payload = self.payload(text=text)
        receipt = self.send(payload)
        self.wait_for(lambda: 'Fixture turn complete.' in self.screen())
        before = json.loads(self.record_path.read_text())
        self.assertGreater(before['input_pending_at'], before['input_seen_at'])
        original = (self.state / 'input_receipts' / (payload['request_id'] + '.json')).read_bytes()
        inspected = subprocess.run([str(test_input.HGS), 'inspect', self.name],
            env=self.env, capture_output=True, text=True, timeout=15)
        self.assertEqual(inspected.returncode, 0, inspected.stderr)
        after = json.loads(self.record_path.read_text())
        self.assertGreaterEqual(after['input_seen_at'], after['input_pending_at'])
        self.assertEqual((self.state / 'input_receipts' / (payload['request_id'] + '.json')).read_bytes(), original)
        self.assertEqual(self.send(payload), receipt, 'the original request is never replayed')
        self.assertEqual(sum(request['texts'][-1:] == [text] for request in self.server.requests), 1)

    def test_prompt_acceptance_is_durable_before_a_busy_activity_journal(self):
        import sqlite3
        # Initialize the real private journal, then hold its writer. Native hook
        # acceptance must survive a slow/failed journal or a five-second timeout.
        inspected = subprocess.run([str(test_input.HGS), 'inspect', self.name],
            env=self.env, capture_output=True, text=True, timeout=15)
        self.assertEqual(inspected.returncode, 0, inspected.stderr)
        self.record.update(input_pending_request=str(uuid.uuid4()), input_pending_at=time.time(), input_seen_at=0)
        self.write_record()
        db = sqlite3.connect(self.state / 'events.sqlite3')
        db.execute('BEGIN IMMEDIATE')
        env = dict(self.env, HGS_SESSION=self.name, HGS_RUN_ID=self.run_id)
        event = dict(hook_event_name='UserPromptSubmit', session_id=self.conversation_id,
                     transcript_path=self.record['transcript'], prompt='Private durable acceptance fixture')
        started = time.monotonic()
        child = subprocess.Popen([str(test_input.HGS), '__state', 'hook'], env=env,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            child.stdin.write(json.dumps(event))
            child.stdin.close()
            deadline = started + 1.5
            while time.monotonic() < deadline:
                record = json.loads(self.record_path.read_text())
                if record.get('input_seen_at', 0) >= self.record['input_pending_at']:
                    break
                time.sleep(.02)
            self.assertGreaterEqual(record.get('input_seen_at', 0), self.record['input_pending_at'],
                'journal contention must not postpone durable native acceptance')
            print('ZERUS_HOOK_ACCEPTANCE_MS=' + str(round((time.monotonic()-started)*1000)), flush=True)
        finally:
            db.rollback()
            db.close()
            child.stdin = None
            child.communicate(timeout=10)

    def test_scrolled_working_composer_page_accepts_send_and_preserves_a_real_draft(self):
        self.scrolled_working_composer('page')

    def test_scrolled_working_composer_wheel_accepts_send_and_preserves_a_real_draft(self):
        self.scrolled_working_composer('wheel')

    def scrolled_working_composer(self, method):
        # Keep an actual native Responses turn open with enough rendered history
        # for both PageUp and terminal mouse-wheel input to pause the viewport.
        def busy(handler):
            body = json.loads(handler.rfile.read(int(handler.headers.get('Content-Length', '0'))))
            texts = [part.get('text', '') for item in body.get('input', []) if item.get('role') == 'user'
                     for part in item.get('content', []) if isinstance(part, dict)]
            handler.server.requests.append(dict(texts=texts))
            text = '\n'.join(f'Private history row {n:03d}: scroll control.' for n in range(180))
            item = dict(type='message', id='msg_scroll', role='assistant', status='completed',
                        content=[dict(type='output_text', text=text, annotations=[])])
            response = dict(id='resp_scroll', object='response', status='completed', output=[item],
                            usage=dict(input_tokens=1, output_tokens=5, total_tokens=6))
            try:
                handler.send_response(200)
                handler.send_header('Content-Type', 'text/event-stream')
                handler.end_headers()
                handler.emit('response.created', response=dict(response, status='in_progress', output=[]))
                handler.emit('response.output_item.added', output_index=0, item=dict(item, status='in_progress'))
                handler.emit('response.output_item.done', output_index=0, item=item)
                handler.server.release.wait(60)
                handler.emit('response.completed', response=response)
            except OSError:
                pass
        self.server.RequestHandlerClass.do_POST = busy
        self.send(self.payload(text='Start the private scroll control'))
        self.wait_for(lambda: 'Private history row 179' in self.screen())
        self.record.update(activity='busy', phase='working')
        self.write_record()
        def scroll():
            if method == 'page':
                self.tmux('send-keys', '-t', self.pane, 'PPage')
            else:
                self.tmux('send-keys', '-l', '-t', self.pane, '\x1b[<64;40;10M' * 8)
            self.wait_for(lambda: 'Back to bottom · esc' in self.screen())
            self.assertEqual(self.tmux('display-message', '-p', '-t', self.pane, '#{pane_in_mode}').strip(), '0')
        scroll()
        draft = 'Retain this private draft'
        self.tmux('send-keys', '-l', '-t', self.pane, draft)
        self.wait_for(lambda: draft in self.screen())
        scroll()
        refused = 'Must never enter the composer'
        self.assertIn('draft', self.send(self.payload(text=refused), success=False))
        self.assertIn(draft, self.screen())
        self.assertNotIn(refused, self.screen())
        # A tmux mode is a different input owner. It remains closed, with no
        # automatic exit, draft edit or message bytes sent through that mode.
        self.tmux('copy-mode', '-t', self.pane)
        before = self.screen()
        self.assertIn('expected agent input mode', self.send(self.payload(text=refused), success=False))
        self.assertEqual(self.tmux('display-message', '-p', '-t', self.pane, '#{pane_in_mode}').strip(), '1')
        self.assertEqual(self.screen(), before)
        self.tmux('send-keys', '-X', '-t', self.pane, 'cancel')
        # Clear only this fixture's own draft after proving both refusals kept it.
        self.tmux('send-keys', '-t', self.pane, 'C-a', 'C-k')
        self.wait_for(lambda: draft not in self.screen())
        scroll()
        self.wait_for(lambda: 'enter/esc latest' in self.screen())
        text = f'Private scrolled {method} message'
        receipt = self.send(self.payload(text=text))
        self.assertEqual(receipt['status'], 'submitted')
        self.assertEqual(receipt['submitted_text'], text)
        self.server.release.set()
        self.wait_for(lambda: any(text in part for request in self.server.requests for part in request['texts']))
        rows = map(json.loads, Path(self.record['transcript']).read_text().splitlines())
        messages = [row['payload']['message'] for row in rows if row.get('type') == 'event_msg'
                    and row.get('payload', {}).get('type') == 'user_message']
        self.assertEqual(messages.count(text), 1)
        self.assertNotIn(draft, messages)
        self.assertNotIn(refused, messages)

    def test_wrapped_native_title_keeps_guarded_input_and_drafts(self):
        import codex_wrapped_title_native
        codex_wrapped_title_native.exercise(self)

    def test_native_refusal_has_an_opted_in_terminal_outcome(self):
        import native_send_outcome
        native_send_outcome.refused_draft(self, self.name, self.record, self.screen,
                                         lambda: len(self.server.raw_requests))

    def test_id_pending_settings_then_message_makes_one_model_call_and_keeps_defaults(self):
        self.pending_message(named=True)

    def test_id_unnamed_pending_settings_then_message_counts_the_auxiliary_title_request(self):
        self.pending_message(named=False)

    def pending_message(self, *, named):
        import native_effort_id as identity
        if named:
            # An existing explicitly named conversation has no automatic title
            # request after the message. Use the native action, not a DB edit
            # or product config change. /rename can generate its own suggestion
            # before this operation; that request is measured separately.
            self.tmux('send-keys', '-t', self.pane, '-l', '/rename')
            self.wait_for(lambda: '/rename' in self.screen())
            self.tmux('send-keys', '-t', self.pane, 'Enter')
            self.wait_for(lambda: 'Type a name and press Enter' in self.screen())
            self.wait_for(lambda: self.server.raw_requests)
            self.tmux('send-keys', '-t', self.pane, '-l', 'Pending fixture')
            self.wait_for(lambda: 'Pending fixture' in self.screen())
            time.sleep(.2)  # Let Codex finish its native paste-burst handling.
            self.tmux('send-keys', '-t', self.pane, 'Enter')
            self.wait_for(lambda: 'Pending fixture' in self.screen()
                          and 'Name thread' not in self.screen() and 'Rename thread' not in self.screen())
            print('ZERUS_NATIVE_CODEX_RENAMED=' + json.dumps(dict(
                screen=self.screen(), cursor=self.tmux('display-message', '-p', '-t', self.pane,
                                                       '#{cursor_x}:#{cursor_y}').strip())), flush=True)
        models = [dict(slug='gpt-6.1-sol', display_name='GPT-6.1-Sol', default_reasoning_level='medium',
                       supported_reasoning_levels=[dict(effort=value) for value in
                                                  ['low', 'medium', 'high', 'xhigh', 'max', 'ultra']])]
        (self.native_home / 'models_cache.json').write_text(json.dumps(dict(models=models)))
        self.record.update(model='gpt-6.1-sol', phase='idle', activity='idle', active_tools={})
        self.write_record()
        defaults = self.native_home / 'config.toml'
        before, requests = defaults.read_bytes(), len(self.server.requests)
        record, path, _ = identity.moved_record(self, self.name, self.record, existing_machine=True)
        self.record, self.record_path = record, path
        attempts = identity.key_log(self)
        text = 'Continue after pending session reasoning @literalreviewer'
        identity.pending_then_message(self, self.name, record, path,
                                      'gpt-6.1-sol', 'high', text, attempts)
        expected = 1 if named else 2
        self.wait_for(lambda: len(self.server.requests) >= requests + expected)
        self.assertEqual(len(self.server.requests), requests + expected, self.server.raw_requests)
        offered = self.server.raw_requests[requests:]
        def title_request(body):
            schema = body.get('text', {}).get('format', {}).get('schema', {})
            return schema.get('required') == ['title'] and 'title' in schema.get('properties', {})
        titles = sum(title_request(body) for body in offered)
        self.assertEqual(titles, 0 if named else 1, offered)
        self.assertEqual(len(offered) - titles, 1, offered)
        print('ZERUS_NATIVE_CODEX_CALLS=' + json.dumps(dict(
            case=self.id(), named=named, before=requests, message_calls=len(offered)-titles,
            title_calls=titles, total_calls=len(offered))), flush=True)
        rows = map(json.loads, Path(record['transcript']).read_text().splitlines())
        messages = [row['payload']['message'] for row in rows if row.get('type') == 'event_msg'
                    and row.get('payload', {}).get('type') == 'user_message']
        self.assertEqual(messages.count(text), 1)
        self.assertEqual(defaults.read_bytes(), before)
        self.assertNotIn('Select Model', self.screen())

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
        self.server.collaboration = False
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

    def turn_completed(self, text):
        # SessionStart or a previous Stop can still describe idle after Enter.
        # Require this exact provider request too; never resend to make it true.
        record = self.binding(self.name)
        return (any(row['texts'][-1:] == [text] for row in self.server.requests)
                and record.get('activity') == 'idle' and not record.get('expected_id'))

    def pause_settled_original(self):
        # SessionStart can publish idle before this turn's UserPromptSubmit.
        # Only these preflight refusals prove that pause sent no signal.
        name = self.name
        original = self.binding(name)
        fields = ('run_id', 'conversation_id', 'pid', 'process_start')
        identity = tuple(original.get(key) for key in fields)
        self.assertTrue(all(identity), 'The fixture must retain its original native binding')
        refusals = {
            f'hgs: {name}: activity changed while checking; retry pause',
            f'hgs: {name}: agent is busy or unconfirmed; wait for its answer before pausing',
        }
        deadline = time.monotonic() + 20  # Existing fixture command budget, not per retry.
        last = ''
        while True:
            current = self.binding(name)
            self.assertEqual(tuple(current.get(key) for key in fields), identity,
                             'The original changed while waiting to pause; never adopt another run')
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                self.fail('The original did not settle for pause within 20 s: ' + last)
            request = dict(request_id=str(uuid.uuid4()), action='pause',
                           expected_run_id=identity[0], expected_conversation_id=identity[1])
            result = subprocess.run([str(test_pause.HGS), 'session-action', name, '--json'], env=self.env,
                                    input=json.dumps(request), capture_output=True, text=True, timeout=remaining)
            if result.returncode != 0:
                self.fail(result.stdout + result.stderr)
            try:
                receipt = json.loads(result.stdout)
            except ValueError:
                self.fail('Pause returned no scoped receipt: ' + result.stdout + result.stderr)
            self.assertIsInstance(receipt, dict, 'Invalid pause receipt')
            self.assertEqual(tuple(receipt.get(key) for key in ('request_id', 'name', 'run_id', 'conversation_id')),
                             (request['request_id'], name, identity[0], identity[1]),
                             'Pause receipt does not match the captured original')
            if receipt.get('status') == 'completed':
                # Pause retains the original PID/birth in its saved record;
                # exit/paused metadata may change, but no replacement is accepted.
                current = self.binding(name)
                self.assertEqual(tuple(current.get(key) for key in fields), identity,
                                 'The original changed during a successful pause; never adopt another run')
                return result.stdout
            last = 'hgs: ' + str(receipt.get('error', ''))
            if receipt.get('status') != 'failed' or result.stderr or last not in refusals:
                self.fail(result.stdout + result.stderr)  # An uncertain effect is never retried.
            time.sleep(min(.1, max(0, deadline - time.monotonic())))

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
                   and self.turn_completed(text))
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

    def test_resumed_and_restored_hook_review_blocks_input_until_the_person_answers(self):
        def card():
            return next((q for q in self.inspect().get('pending_questions', [])
                         if q.get('source') == 'codex_hooks_trust'), None)

        def answer(question):
            record = self.binding(self.name)
            payload = dict(request_id=str(uuid.uuid4()), expected_run_id=record['run_id'],
                           expected_conversation_id=record.get('conversation_id') or '',
                           question_id=question['question_id'], expected_question_hash=question['question_hash'],
                           answers=[dict(question_id='hooks_trust', selected_option_ids=['trust'], text='')])
            result = subprocess.run([str(test_pause.HGS), 'answer', self.name, '--json'],
                                    input=json.dumps(payload), env=self.env, text=True,
                                    capture_output=True, timeout=20)
            self.assertEqual(result.returncode, 0, result.stderr + '\n' + self.screen())

        def message(text):
            record = self.binding(self.name)
            payload = dict(request_id=str(uuid.uuid4()), expected_run_id=record['run_id'],
                           expected_conversation_id=record['conversation_id'], text=text)
            return subprocess.run([str(test_pause.HGS), 'send', self.name, '--json'],
                                  input=json.dumps(payload), env=self.env, text=True,
                                  capture_output=True, timeout=20)

        self.launch(trust=True)
        initial = self.until('The initial native hook review must be explicit', card)
        answer(initial)
        self.deliver_first()
        conversation = self.binding(self.name)['conversation_id']
        hooks = self.native_home / 'hooks.json'
        original = hooks.read_bytes()
        stamp = hooks.stat().st_mtime_ns
        previous = self.binding(self.name)['run_id']
        self.pause_settled_original()
        self.hgs('resume', self.name, '-d')
        self.until('The unchanged resume must reach its native composer', lambda:
                   self.binding(self.name).get('run_id') != previous and '›' in self.screen()
                   and 'Hooks need review' not in self.screen()
                   and self.inspect().get('resume_message_can_send'))
        self.assertEqual((hooks.read_bytes(), hooks.stat().st_mtime_ns), (original, stamp))
        self.assertIsNone(card(), 'An unchanged registration must not request trust again')
        unchanged = 'Synthetic unchanged-hook resume.'
        result = message(unchanged)
        self.assertEqual(result.returncode, 0, result.stderr + '\n' + self.screen())
        self.until('The resumed turn must finish', lambda: self.turn_completed(unchanged))

        for mode in ('resume', 'restore'):
            with self.subTest(mode=mode):
                previous = self.binding(self.name)['run_id']
                self.pause_settled_original()
                # Change one of our fixture's existing definitions, not its
                # executable or trust state. Codex must request new consent.
                document = json.loads(hooks.read_text())
                document['hooks']['SessionStart'][0]['hooks'][0]['timeout'] += 1
                hooks.write_text(json.dumps(document, indent=2))
                options = []
                if mode == 'restore':
                    self.hgs('archive', self.name)
                    archived = next(json.loads(path.read_text())
                                    for path in (self.root / 'state/archives').glob('*.json')
                                    if json.loads(path.read_text()).get('name') == self.name)
                    options = ['--archive', archived['archive_id']]
                self.hgs('resume', self.name, *options, '-d')
                self.until('The new original run must be bound', lambda:
                           self.binding(self.name).get('run_id') not in (None, previous))
                review = self.until('The saved conversation must expose its real hook review', card)
                self.assertEqual(review['conversation_id'], conversation)
                self.assertEqual(review['run_id'], self.binding(self.name)['run_id'])
                self.assertNotEqual(review['question_hash'], initial['question_hash'])
                result = message('MUST NOT ENTER THE HOOK TRUST DIALOG')
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('hook trust review', result.stderr)
                self.assertIn('› 1. Review hooks', self.screen())
                self.assertFalse(any('MUST NOT ENTER' in text
                                     for request in self.server.requests for text in request['texts']))
                answer(review)  # This fixture's explicit decision; never automatic trust.
                text = 'Synthetic after ' + mode + ' hook review.'
                result = message(text)
                self.assertEqual(result.returncode, 0, result.stderr + '\n' + self.screen())
                self.until('The original resumed conversation must complete its turn',
                           lambda: self.turn_completed(text))
                self.assertEqual(self.binding(self.name)['conversation_id'], conversation)
                self.assertEqual(sum(row['texts'][-1:] == [text] for row in self.server.requests), 1)

    def test_hook_review_pause_rechecks_each_original_after_preflight_refusals(self):
        real_run = subprocess.run
        refused, paused = {}, []
        errors = ('agent is busy or unconfirmed; wait for its answer before pausing',
                  'activity changed while checking; retry pause')

        def run(argv, *args, **kwargs):
            if argv == [str(test_pause.HGS), 'session-action', getattr(self, 'name', None), '--json']:
                request = json.loads(kwargs['input'])
                if request.get('action') != 'pause':
                    return real_run(argv, *args, **kwargs)
                original = self.binding(self.name)
                run_id = original['run_id']
                self.assertEqual(request['expected_run_id'], run_id)
                self.assertEqual(request['expected_conversation_id'], original['conversation_id'])
                count = refused.get(run_id, 0)
                if count < len(errors):
                    refused[run_id] = count + 1
                    # A scoped fixture preflight refusal: no native pause occurred.
                    receipt = dict(request_id=request['request_id'], name=self.name, run_id=run_id,
                                   conversation_id=original['conversation_id'], status='failed',
                                   error=f'{self.name}: {errors[count]}')
                    return subprocess.CompletedProcess(argv, 0, json.dumps(receipt), '')
                result = real_run(argv, *args, **kwargs)
                if result.returncode == 0 and json.loads(result.stdout).get('status') == 'completed':
                    paused.append(run_id)
                return result
            return real_run(argv, *args, **kwargs)

        with mock.patch.object(subprocess, 'run', side_effect=run):
            # Delegate every trust, send, resume, archive and scope assertion to
            # the original native contract. A missing retry at either call site fails.
            self.test_resumed_and_restored_hook_review_blocks_input_until_the_person_answers()
        self.assertEqual(len(refused), 3)
        self.assertEqual(list(refused.values()), [2, 2, 2])
        self.assertEqual(paused, list(refused))

    def untrusted(self, git=False):
        self.server.collaboration = False
        # A project config makes the ordinary folder a project in 0.167 too:
        # genuinely projectless directories no longer require folder consent.
        config = self.native_home / 'config.toml'
        config.write_text(config.read_text().split('[projects.', 1)[0])
        target = self.project
        (target / '.codex').mkdir()
        (target / '.codex/config.toml').write_text('# Synthetic project, no hooks or commands.\n')
        if git:
            subprocess.run(['git', 'init', '-q', str(target)], check=True, env=self.env)
            self.project = target / 'subdir'
            self.project.mkdir()
            (self.root / 'cfg/projects.local').write_text('p=' + str(self.project) + '\n')
        self.launch()
        card = self.until('The real folder trust choice must be answerable', lambda: next(
            (q for q in self.inspect().get('pending_questions', [])
             if q.get('source') == 'codex_folder_trust' and q.get('can_answer')), None))
        self.assertFalse(self.inspect()['first_message_can_send'])
        self.assertEqual(self.server.requests, [])
        self.assertNotIn('projects', tomllib.loads(config.read_text()))
        return card, target

    def folder_answer(self, card, choice):
        request = dict(request_id=str(uuid.uuid4()), expected_run_id=self.run_id,
                       expected_conversation_id='', question_id=card['question_id'],
                       expected_question_hash=card['question_hash'],
                       answers=[dict(question_id='trust', selected_option_ids=[f'trust_{choice}'], text='')])
        args = ['answer', self.name, '--json']
        # Harness.hgs intentionally accepts only argv; keep the real JSON
        # submission and its replay on subprocess stdin.
        def submit():
            result = subprocess.run([str(test_pause.HGS), *args], input=json.dumps(request), env=self.env,
                                    text=True, capture_output=True, timeout=20)
            # Quit can already have removed the pane; do not capture it while
            # evaluating the assertion's diagnostic on a successful answer.
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            return json.loads(result.stdout)
        receipt = submit()
        self.assertEqual(receipt['status'], 'answered')
        self.assertEqual(submit(), receipt)
        return receipt

    def test_explicit_native_folder_trust_then_first_turn(self):
        card, target = self.untrusted()
        self.assertEqual(card['questions'][0]['question'], 'Trust this folder?')
        self.folder_answer(card, 0)
        self.assertEqual(tomllib.loads((self.native_home / 'config.toml').read_text())['projects'][str(target)]['trust_level'], 'trusted')
        self.deliver_first()

    def test_explicit_native_git_subdirectory_trust_names_the_root(self):
        card, target = self.untrusted(git=True)
        self.assertEqual(card['questions'][0]['question'], 'Trust this Git repository?')
        self.assertIn(str(target), card['questions'][0]['options'][0]['description'])
        self.folder_answer(card, 0)
        projects = tomllib.loads((self.native_home / 'config.toml').read_text())['projects']
        self.assertEqual(projects[str(target)]['trust_level'], 'trusted')
        self.assertNotIn(str(self.project), projects)
        self.deliver_first()

    def test_explicit_native_folder_cancel_does_not_trust(self):
        card, _ = self.untrusted()
        cancel = card['questions'][0]['options'][1]['label']
        self.assertIn(cancel, ('Quit', 'Back to Agent Command Center'))
        self.folder_answer(card, 1)
        if cancel == 'Back to Agent Command Center':
            self.until('Back must show the live command center', lambda: 'Agent command center' in self.screen())
            record = self.binding(self.name)
            self.assertFalse(record.get('conversation_id'))
            os.kill(record['pid'], 0)
        self.assertNotIn('projects', tomllib.loads((self.native_home / 'config.toml').read_text()))
        self.assertEqual(self.server.requests, [])
