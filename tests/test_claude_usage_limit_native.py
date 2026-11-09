"""Real Claude subscription-limit choices against a private, keyless TLS fixture.

The proxy accepts only api.anthropic.com and routes it to localhost. Temporary
credentials, certificates, subscription metadata and quota headers are synthetic.
No live account, session, paid usage or external model endpoint is used.
"""
import hashlib
import json
import os
import select
import signal
import shlex
import socket
import ssl
import subprocess
import tempfile
import threading
import time
import unittest
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import test_claude_questions_native as native
import test_input as fixtures
import test_questions as questions


@unittest.skipUnless(native.CLAUDE, 'Set HGS_CLAUDE_TEST_BIN for private native quota qualification')
class NativeClaudeUsageLimit(unittest.TestCase):
    script = fixtures.InputTransport.script
    tmux = fixtures.InputTransport.tmux
    write_record = fixtures.InputTransport.write_record
    wait_for = native.NativeClaudeQuestions.wait_for
    screen = native.NativeClaudeQuestions.screen
    inspect = questions.Questions.inspect
    answer = questions.Questions.answer
    hook = questions.Questions.hook

    team_account = False

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='hgs-claude-limit-native-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.bin = self.root / '.local/bin'
        self.bin.mkdir(parents=True)
        self.state = self.root / 'state'
        self.state.mkdir()
        work = self.root / 'work'
        work.mkdir()
        config = self.root / '.claude'
        config.mkdir()
        subscription = 'team' if self.team_account else 'max'
        tier = 'default_claude_team' if self.team_account else 'default_claude_max_20x'
        self.account = dict(accountUuid='00000000-0000-4000-8000-000000000001',
                            organizationUuid='00000000-0000-4000-8000-000000000002',
                            billingType='stripe_subscription', hasExtraUsageEnabled=False)
        (self.root / '.claude.json').write_text(json.dumps(dict(
            hasCompletedOnboarding=True, hasSeenAutoDefaultNudge=True, theme='dark', oauthAccount=self.account,
            projects={str(work): dict(hasTrustDialogAccepted=True)},
            cachedGrowthBookFeatures={'tengu_jade_anvil_4': True,
                'tengu_harbor_kite_limits': True, 'tengu_drifting_nova': True})))
        (config / '.credentials.json').write_text(json.dumps({'claudeAiOauth': dict(
            accessToken='fixture-token', refreshToken='fixture-refresh',
            expiresAt=int((time.time() + 86400) * 1000),
            scopes=['user:inference', 'user:profile'], subscriptionType=subscription,
            rateLimitTier=tier)}))
        # An explicit config directory also isolates macOS Keychain service
        # identity. The native master config lives beside credentials there.
        (config / '.claude.json').write_bytes((self.root / '.claude.json').read_bytes())
        (config / 'settings.json').write_text(json.dumps({'autoContinueAtUsageLimit': False}))
        self.reset = int(time.time()) + 5 * 86400
        fixture = self
        self.requests = []
        # The shared timeout reporter expects tool payloads, not our (method, path) pairs.
        self.native_requests = []

        class Server(BaseHTTPRequestHandler):
            def log_message(self, *args): pass

            def do_GET(self):
                fixture.requests.append(('GET', self.path))
                self.send_response(200)
                self.send_header('Content-Type', 'application/json')
                self.end_headers()
                self.wfile.write(json.dumps({'account': {'uuid': fixture.account['accountUuid'],
                    'has_claude_max': not fixture.team_account}, 'organization': {
                    'uuid': fixture.account['organizationUuid'], 'organization_type': 'claude_team' if fixture.team_account else 'claude_max',
                    'billing_type': 'stripe_subscription', 'rate_limit_tier': tier,
                    'has_extra_usage_enabled': False}}).encode())

            def do_POST(self):
                self.rfile.read(int(self.headers.get('Content-Length', 0)))
                fixture.requests.append(('POST', self.path))
                self.send_response(429)
                for key, value in {'Content-Type': 'application/json',
                    'anthropic-ratelimit-unified-status': 'rejected',
                    'anthropic-ratelimit-unified-reset': str(fixture.reset),
                    'anthropic-ratelimit-unified-7d-reset': str(fixture.reset),
                    'anthropic-ratelimit-unified-7d-utilization': '1',
                    'anthropic-ratelimit-unified-representative-claim': 'seven_day',
                    'anthropic-ratelimit-unified-overage-status': 'rejected',
                    'anthropic-ratelimit-unified-overage-disabled-reason': 'extra_usage_disabled'}.items():
                    self.send_header(key, value)
                self.end_headers()
                self.wfile.write(json.dumps(dict(type='error', error=dict(
                    type='rate_limit_error', message='Weekly usage limit reached'))).encode())

        # Keep the official origin: a custom ANTHROPIC_BASE_URL is a gateway,
        # which intentionally has different subscription/auto-continue behavior.
        cert, key = self.root / 'cert.pem', self.root / 'key.pem'
        cert_config = self.root / 'cert.cnf'
        cert_config.write_text('[req]\ndistinguished_name=dn\nx509_extensions=ext\nprompt=no\n'
                               '[dn]\nCN=api.anthropic.com\n[ext]\nsubjectAltName=DNS:api.anthropic.com\n')
        subprocess.run(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes',
                        '-keyout', str(key), '-out', str(cert), '-days', '1', '-config', str(cert_config)],
                       check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=15)
        server = ThreadingHTTPServer(('127.0.0.1', 0), Server)
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(cert, key)
        server.socket = context.wrap_socket(server.socket, server_side=True)
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        threading.Thread(target=server.serve_forever, daemon=True).start()

        class Proxy(BaseHTTPRequestHandler):
            def log_message(self, *args): pass

            def do_CONNECT(self):
                if self.path != 'api.anthropic.com:443':
                    self.send_error(403)
                    return
                self.send_response(200)
                self.end_headers()
                with socket.create_connection(('127.0.0.1', server.server_port), timeout=5) as upstream:
                    while True:
                        ready, _, _ = select.select([self.connection, upstream], [], [], 10)
                        if not ready: return
                        for source in ready:
                            data = source.recv(65536)
                            if not data: return
                            (upstream if source is self.connection else self.connection).sendall(data)

        proxy = ThreadingHTTPServer(('127.0.0.1', 0), Proxy)
        self.addCleanup(proxy.server_close)
        self.addCleanup(proxy.shutdown)
        threading.Thread(target=proxy.serve_forever, daemon=True).start()
        self.socket = self.root / 'socket'
        self.script('tmux', '#!/bin/sh\nexec ' + shlex.quote(fixtures.TMUX) + ' -S ' +
                    shlex.quote(str(self.socket)) + ' -f /dev/null "$@"\n')
        # Native billing may hand off to a browser; never open the real desktop.
        for opener in ('open', 'xdg-open', 'fixture-browser'):
            self.script(opener, '#!/bin/sh\nprintf "%s\\n" "$@" >> ' + shlex.quote(str(self.root / 'browser-requests')) + '\n')
        self.env = {k: v for k, v in os.environ.items() if not k.startswith('HGS_')}
        self.env.update(HOME=str(self.root), HGS_STATE_DIR=str(self.state), HGS_SELF='test',
                        HGS_CONFIG_DIR=str(self.root / 'config'), PATH=str(self.bin) + ':' + os.environ['PATH'])
        self.env.pop('TMUX', None)
        self.env.pop('TMUX_PANE', None)
        self.name = 'claude/limit-test'
        self.run_id = str(uuid.uuid4())
        self.addCleanup(lambda: fixtures.InputTransport.tearDown(self))
        self.tmux('new-session', '-d', '-s', self.name, '-x', '100', '-y', '35', '-c', str(work),
                  'env', '-i', 'HOME=' + str(self.root), 'PATH=' + str(self.bin) + ':' + os.environ['PATH'],
                  'CLAUDE_CONFIG_DIR=' + str(config),
                  'SHELL=/bin/bash', 'TERM=xterm-256color', 'LANG=C.UTF-8',
                  'CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1', 'DISABLE_AUTOUPDATER=1',
                  'CLAUDE_CODE_GB_DISK_CACHE_WHEN_TELEMETRY_OFF=1',
                  'HTTPS_PROXY=http://127.0.0.1:' + str(proxy.server_port),
                  'BROWSER=' + str(self.bin / 'fixture-browser'),
                  'NODE_EXTRA_CA_CERTS=' + str(cert), native.CLAUDE,
                  '--model', 'claude-sonnet-4-6', 'Synthetic quota fixture request.')
        self.pane = self.tmux('display-message', '-p', '-t', '=' + self.name + ':', '#{pane_id}').strip()
        self.wait_for(lambda: "You've hit your weekly limit" in self.screen())
        # The error is drawn before the modal. Wait for the complete panel;
        # sending a slash command during that transition can hit the new modal.
        third = 'Ask your admin for more usage' if self.team_account else 'Switch to usage credits'
        self.wait_for(lambda: all(text in self.screen() for text in [
            'Enter to confirm · Esc to cancel', third, 'Wait here, then continue automatically']))
        history = next(config.glob('projects/*/*.jsonl'))
        self.conversation_id = history.stem
        pid = int(self.tmux('display-message', '-p', '-t', self.pane, '#{pane_pid}'))
        start = subprocess.check_output(['ps', '-p', str(pid), '-o', 'lstart='],
                                        env=dict(self.env, LC_ALL='C', TZ='UTC'), text=True).strip()
        self.tmux('set-option', '-t', '=' + self.name + ':', '@hgs_run', self.run_id)
        self.record_path = self.state / (hashlib.sha256(self.name.encode()).hexdigest() + '.json')
        self.record = dict(version=1, name=self.name, agent='claude', run_id=self.run_id,
            run_identity_version=1, supervisor={}, conversation_id=self.conversation_id,
            expected_id=self.conversation_id, launch_dir=str(work), pane=self.pane,
            pid=pid, process_start=start, activity='busy', phase='working',
            active_tools={}, subagents={}, transcript=str(history), last_event_at=time.time())
        self.write_record()
        self.hook(dict(hook_event_name='StopFailure', error="You've hit your weekly limit · resets Oct 20, 1pm (UTC)"))
        self.card = self.wait_for(lambda: next((q for q in self.inspect()['pending_questions']
            if q['source'] == 'claude_usage_limit' and q['can_answer']), None))
        self.assertEqual(self.card['source'], 'claude_usage_limit')
        self.assertTrue(self.card['can_answer'], self.screen())

    def payload(self, prefix):
        option = next(o for o in self.card['questions'][0]['options'] if o['label'].startswith(prefix))
        return dict(request_id=str(uuid.uuid4()), expected_run_id=self.run_id,
            expected_conversation_id=self.conversation_id, question_id=self.card['question_id'],
            expected_question_hash=self.card['question_hash'], answers=[dict(
                question_id='usage_limit', selected_option_ids=[option['id']], text='')])

    def test_stop_answers_native_once_and_preserves_failed_turn(self):
        before = list(self.requests)
        state = self.inspect()
        self.assertEqual(state['phase'], 'input')
        self.assertEqual(state['recovery_error_class'], 'manual')
        payload = self.payload('Stop and wait')
        receipt = self.answer(payload)
        self.assertEqual(receipt['status'], 'answered')
        self.assertEqual(self.inspect()['pending_questions'], [])
        self.assertEqual(self.inspect()['phase'], 'error')
        self.assertEqual(self.answer(payload), receipt)
        self.assertEqual(self.requests, before)

    def test_wait_is_an_explicit_native_choice_without_provider_retry(self):
        before = list(self.requests)
        receipt = self.answer(self.payload('Wait here'))
        self.assertEqual(receipt['status'], 'answered')
        self.assertIn('Claude Code will continue automatically', self.screen())
        self.assertEqual(self.requests, before)

    def test_credits_hands_off_to_native_billing_without_confirming_purchase(self):
        before = [r for r in self.requests if r[0] == 'POST']
        receipt = self.answer(self.payload('Switch to usage credits'))
        self.assertEqual(receipt['status'], 'answered')
        self.assertTrue(receipt['open_terminal'])
        self.assertEqual([r for r in self.requests if r[0] == 'POST'], before)

    def test_working_background_hook_does_not_hide_native_choice_or_allow_send(self):
        record = json.loads(self.record_path.read_text())
        record.update(phase='working', active_tools={'tool-fixture': {'name': 'Bash'}})
        self.record_path.write_text(json.dumps(record))
        state = self.inspect()
        self.assertEqual(state['phase'], 'input')
        payload = dict(request_id=str(uuid.uuid4()), expected_run_id=self.run_id,
                       expected_conversation_id=self.conversation_id, text='Keep this draft unsent')
        result = subprocess.run([str(fixtures.HGS), 'send', self.name, '--json'], env=self.env,
                                input=json.dumps(payload), text=True, capture_output=True, timeout=20)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('usage-limit choice', result.stderr)
        self.assertIn('What do you want to do?', self.screen())

    def test_changed_identity_rejects_without_touching_the_menu(self):
        payload = self.payload('Switch to usage credits')
        payload['expected_question_hash'] = '0' * 64
        self.answer(payload, success=False)
        self.assertIn('What do you want to do?', self.screen())

    def test_stale_credits_answer_cannot_be_sent_to_the_ready_composer(self):
        stale = self.payload('Switch to usage credits')
        self.answer(self.payload('Stop and wait'))
        before = list(self.requests)
        self.assertIn('no longer pending', self.answer(stale, success=False))
        self.assertEqual(self.inspect()['pending_questions'], [])
        self.assertEqual(self.requests, before)


@unittest.skipUnless(native.CLAUDE, 'Set HGS_CLAUDE_TEST_BIN for private native quota qualification')
class NativeClaudeOrganizationUsageLimit(unittest.TestCase):
    """The official executable must draw the admin variant itself; no screen injection."""
    team_account = True
    setUp = NativeClaudeUsageLimit.setUp
    script = NativeClaudeUsageLimit.script
    tmux = NativeClaudeUsageLimit.tmux
    write_record = NativeClaudeUsageLimit.write_record
    wait_for = NativeClaudeUsageLimit.wait_for
    screen = NativeClaudeUsageLimit.screen
    inspect = NativeClaudeUsageLimit.inspect
    answer = NativeClaudeUsageLimit.answer
    hook = NativeClaudeUsageLimit.hook
    payload = NativeClaudeUsageLimit.payload

    def test_admin_choice_opens_native_confirmation_without_confirming_or_replaying(self):
        options = self.card['questions'][0]['options']
        self.assertIn('Ask your admin for more usage', [o['label'] for o in options])
        self.assertTrue(self.card['can_answer'])
        self.assertNotIn('approval', self.card)
        self.assertTrue(all('selected' not in option for option in options))
        before = [r for r in self.requests if r[0] == 'POST']
        payload = self.payload('Ask your admin')
        # This is a second, unnumbered native confirmation, not the usage
        # menu we bound. Keep the original result uncertain rather than
        # treating a new footer as acknowledgement or confirming the request.
        self.assertIn('delivery uncertain', self.answer(payload, success=False))
        self.assertIn('Request usage credits from your admin', self.screen())
        self.assertIn('❯ Cancel', self.screen())
        self.assertEqual([r for r in self.requests if r[0] == 'POST'], before)
        self.assertIn('delivery uncertain', self.answer(payload, success=False))
        self.assertIn('❯ Cancel', self.screen())
        self.assertFalse(any(q['source'] == 'claude_usage_limit' for q in self.inspect()['pending_questions']))
        self.assertEqual([r for r in self.requests if r[0] == 'POST'], before)



class UsageLimitSnapshotPolicy(unittest.TestCase):
    """Scripted terminal policy/transport controls, not real-Claude coverage."""
    script = fixtures.InputTransport.script
    tmux = fixtures.InputTransport.tmux
    write_record = fixtures.InputTransport.write_record
    wait_for = fixtures.InputTransport.wait_for
    tearDown = fixtures.InputTransport.tearDown
    received = fixtures.InputTransport.received
    inspect = questions.Questions.inspect
    answer = questions.Questions.answer
    payload = NativeClaudeUsageLimit.payload

    def setUp(self):
        fixtures.InputTransport.setUp(self)
        self.record.update(agent='claude', run_identity_version=1, supervisor={},
                           launch_dir='/work/example', phase='error', activity='idle')
        self.write_record()
        self.menu = (Path(__file__).parent / 'fixtures/claude-usage-limit.txt').read_text()
        self.menu = self.menu.replace('Stop and wait for limit to reset', 'Keep this turn stopped')
        self.show(self.menu)
        self.card = self.inspect()['pending_questions'][0]

    def show(self, text):
        (self.root / 'screen').write_text('\x1b[2J\x1b[H' + text.replace('\n', '\r\n'))
        os.kill(self.record['pid'], signal.SIGUSR1)
        def displayed(value):
            return '\n'.join(line.rstrip() for line in value.splitlines()).strip()
        self.wait_for(lambda: displayed(self.tmux('capture-pane', '-p', '-t', self.pane)) == displayed(text))

    def test_unknown_label_is_manual_and_the_original_request_is_not_replayed(self):
        self.assertTrue(self.card['can_answer'])
        self.assertNotIn('approval', self.card)
        self.assertEqual(self.received(), b'')
        (self.root / 'replies').write_text(json.dumps([['0d', '\x1b[2J\x1b[HReady for a new message']]))
        payload = self.payload('Keep this turn stopped')
        receipt = self.answer(payload)
        self.assertEqual(receipt['status'], 'answered')
        self.assertEqual(self.received(), b'\r')
        self.assertEqual(self.answer(payload), receipt)
        self.assertEqual(self.received(), b'\r')

    def test_changed_menu_and_request_identity_refuse_before_input(self):
        payload = self.payload('Keep this turn stopped')
        changed = dict(payload, expected_conversation_id='another-conversation')
        self.answer(changed, success=False)
        self.assertEqual(self.received(), b'')
        self.show(self.menu.replace('Switch to usage credits', 'Ask your admin for more usage'))
        self.answer(payload, success=False)
        self.assertEqual(self.received(), b'')

    def test_partial_quoted_and_unbound_panels_have_no_input_capability(self):
        for invalid in [self.menu.replace('Enter to confirm · Esc to cancel', 'Enter to confirm'),
                        '\n'.join('> ' + line for line in self.menu.splitlines()),
                        self.menu.replace('  3.', '  4.')]:
            self.show(invalid)
            self.assertFalse(any(q['source'] == 'claude_usage_limit' for q in self.inspect()['pending_questions']))
        self.show(self.menu)
        for field in ['run_identity_version', 'supervisor']:
            original = self.record.pop(field)
            self.write_record()
            self.assertFalse(any(q['source'] == 'claude_usage_limit' for q in self.inspect()['pending_questions']))
            self.record[field] = original
        self.assertEqual(self.received(), b'')

    def test_a_changed_highlight_at_the_final_capture_refuses_before_input(self):
        # The real private pane and all identities stay fixed. Alternate only
        # its reported highlight so no adjacent pre-input snapshots can agree.
        wrapper = self.bin / 'tmux'
        wrapper.rename(self.bin / 'tmux-real')
        self.script('tmux', '#!/usr/bin/env python3\n' + '''
import pathlib,subprocess,sys
root=pathlib.Path(__file__).resolve().parent
r=subprocess.run([str(root/'tmux-real'),*sys.argv[1:]],capture_output=True)
out=r.stdout
if 'capture-pane' in sys.argv and '-e' not in sys.argv:
 p=root/'captures';n=int(p.read_text())+1 if p.exists() else 1;p.write_text(str(n))
 if n%2==0:out=out.replace('❯ 1.'.encode(),b'  1.').replace(b'  2.','❯ 2.'.encode())
sys.stdout.buffer.write(out);sys.stderr.buffer.write(r.stderr);sys.exit(r.returncode)
''')
        self.answer(self.payload('Keep this turn stopped'), success=False)
        self.assertEqual(self.received(), b'')


if __name__ == '__main__':
    unittest.main()
