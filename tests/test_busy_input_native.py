"""Busy native queue contracts: private homes, own tools and localhost providers."""
import base64
import datetime
import http.server
import json
import struct
import subprocess
import zlib
import os
import re
from pathlib import Path
import shlex
import threading
import time
import unittest

import test_codex_input_native as codex
import test_codex_async_questions_native as responses
import test_clear_context_native as clear
import test_claude_input_native as claude

PROMPT = 'Start the private busy queue fixture.'
FIRST = 'First queued fixture message.'
SECOND = 'Second queued fixture message.'
LAST = 'Exact hgs queued follow-up.'


def command(root):
    # Only this fixture's tool runs here. A finite escape also bounds failure
    # cleanup; normal completion is released explicitly by the original test.
    return (f'touch {shlex.quote(str(root / "tool-started"))}; '
            f'for i in $(seq 1 600); do [ -f {shlex.quote(str(root / "tool-release"))} ] '
            '&& break; sleep .1; done; printf "private tool finished\\n"')


def terminal_send(case, text):
    case.tmux('send-keys', '-t', case.pane, '-l', text)
    case.wait_for(lambda: text in case.screen())
    # Native paste detection must settle before the deliberate Enter.
    time.sleep(.2)
    case.tmux('send-keys', '-t', case.pane, 'Enter')


def screenshot_png(seed, width, height):
    """A flat UI-like screenshot at a size Claude must downscale after the paste."""
    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data) & 0xffffffff)
    rows = []
    for y in range(height):
        shade = 30 + seed * 20 + ((y // 90) % 2) * 40
        rows.append(b'\x00' + bytes([shade, shade, shade + 10]) * width)
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(b''.join(rows), 6)) + chunk(b'IEND', b''))


IMAGES = {}


def image_message(root):
    """zerus-iitw8: the owner's shape. The text names screenshots on lines of their own,
    which Claude attaches asynchronously, plus two hgs attachments."""
    if not IMAGES:
        IMAGES.update(big=[screenshot_png(n, 6000, 4000) for n in range(2)], small=[screenshot_png(n, 64, 48) for n in range(2)])
    target = root / 'peer-target'
    target.mkdir(exist_ok=True)
    paths = [target / 'confirmation-dark.png', target / 'confirmation-light.png']
    for path, data in zip(paths, IMAGES['big']):
        path.write_bytes(data)
    body = '\n'.join(f'- Item {n}: the peer report explains one change of the fixture in a full sentence.' for n in range(1, 31))
    text = (f'<zerus-message from="codex/fixture/peer" host="arch">\nSettings preview delta is ready.\n{body}\n'
            f'Screenshots:\n{paths[0]}\n{paths[1]}\n</zerus-message>')
    attachments = [dict(name=f'row-{n}.png', mime='image/png', data_base64=base64.b64encode(data).decode())
                   for n, data in enumerate(IMAGES['small'])]
    return text, attachments


class BusyProvider(responses.Provider):
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get('Content-Length', 0))) or b'{}')
        self.server.raw_requests.append(body)
        self.server.requests.append(body)
        main = any(i.get('role') == 'user' and any(p.get('text') == PROMPT for p in i.get('content', []) if isinstance(p, dict)) for i in body.get('input', []))
        if main and not self.server.called:
            self.server.called = True
            items = body.get('input') or []
            tools = responses.offered((body.get('tools') or []) + [t for i in items
                                      if i.get('type') == 'additional_tools' for t in i.get('tools', [])])
            args = dict(cmd=self.server.command, yield_time_ms=10000, max_output_tokens=1000)
            # Invoke Codex's native shell subtool directly. The compatibility
            # runner intentionally extracts no separate code-mode host; the
            # actual file marker proves this native tool ran, not a simulated
            # Working screen or a fabricated hook.
            assert 'exec_command' in tools or 'exec' in tools, tools
            item = dict(type='function_call', id='fc_busy', call_id='call_busy', name='exec_command',
                        arguments=json.dumps(args), status='completed')
        else:
            item = dict(type='message', id='msg_busy', role='assistant', status='completed',
                        content=[dict(type='output_text', text='Busy fixture completed.', annotations=[])])
        response = dict(id='resp_busy', object='response', status='completed', output=[item],
                        usage=dict(input_tokens=1, output_tokens=5, total_tokens=6))
        try:
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.end_headers()
            self.emit('response.created', response=dict(response, status='in_progress', output=[]))
            self.emit('response.output_item.added', output_index=0, item=dict(item, status='in_progress'))
            self.emit('response.output_item.done', output_index=0, item=item)
            self.emit('response.completed', response=response)
        except OSError:
            pass  # Only this private native child can abandon the response.


@unittest.skipUnless(os.environ.get('HGS_CODEX_TEST_BIN'), 'Set HGS_CODEX_TEST_BIN for the private busy Codex contract')
class NativeBusyCodex(codex.NativeCodexInput):
    # Compatibility CI extracts the CLI, not the separate code-mode host.
    native_features = 'mentions_v2=false\ncode_mode_host=false\n'
    # Do not inherit the ordinary input test methods into this separate suite.
    test_leading_absolute_path_is_literal_message_text = None
    test_literal_at_mentions_reach_the_model_once_without_file_completion = None

    def setUp(self):
        self.server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), BusyProvider)
        self.server.raw_requests = []
        self.server.requests, self.server.called = [], False
        self.addCleanup(self.server.server_close)
        self.addCleanup(self.server.shutdown)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.provider_url = f'http://127.0.0.1:{self.server.server_port}/v1'
        clear.NativeCodexClear.setUp(self)
        self.server.command = command(self.root)

    def test_busy_tool_queue_preserves_drafts_and_accepts_exactly_once(self):
        release = self.root / 'tool-release'
        try:
            terminal_send(self, PROMPT)
            self.wait_for((self.root / 'tool-started').exists)
            # This fixture binds the observed native tool to its own tracked
            # record; it does not synthesize any acknowledgement for hgs input.
            self.record.update(activity='busy', phase='tool', active_tools={'call_busy': {'tool_name': 'Bash'}})
            self.write_record()
            terminal_send(self, FIRST)
            terminal_send(self, SECOND)
            self.wait_for(lambda: SECOND in self.screen() and 'Messages to be submitted' in self.screen())
            draft = 'keep this user draft'
            self.tmux('send-keys', '-t', self.pane, '-l', draft)
            self.wait_for(lambda: '› ' + draft in self.screen())
            refused = self.payload(text='must not be typed')
            self.assertIn('agent busy:', self.send(refused, success=False))
            self.assertIn('› ' + draft, self.screen())
            self.tmux('send-keys', '-t', self.pane, 'C-a', 'C-k')
            self.wait_for(lambda: '› ' + draft not in self.screen())
            payload = self.payload(text=LAST)
            receipt = self.send(payload)
            self.assertEqual(receipt['status'], 'submitted')
            self.assertEqual(self.send(payload), receipt)
            release.touch()
            def messages():
                return [r['payload']['message'] for r in map(json.loads, Path(self.record['transcript']).read_text().splitlines())
                        if r.get('type') == 'event_msg' and r.get('payload', {}).get('type') == 'user_message']
            self.wait_for(lambda: LAST in messages())
            self.assertEqual(messages(), ['Previous fixture request', PROMPT, FIRST, SECOND, LAST])
            self.wait_for(lambda: 'Busy fixture completed.' in self.screen())
        finally:
            release.touch()


    def test_busy_attachment_send_is_queued_and_delivered_once(self):
        # zerus-iitw8: Codex shows a pasted image path or a long paste at once
        # (no asynchronous attachment as in Claude), so its single Enter and
        # submitted receipt stay; the message waits in its native queue.
        release = self.root / 'tool-release'
        try:
            terminal_send(self, PROMPT)
            self.wait_for((self.root / 'tool-started').exists)
            self.record.update(activity='busy', phase='tool', active_tools={'call_busy': {'tool_name': 'Bash'}})
            self.write_record()
            text, attachments = image_message(self.root)
            payload = self.payload(text=text, attachments=attachments)
            receipt = self.send(payload)
            self.assertEqual(receipt['status'], 'submitted')
            self.assertEqual(self.send(payload), receipt)
            self.wait_for(lambda: 'Messages to be submitted' in self.screen())
            release.touch()
            def messages():
                return [r['payload']['message'] for r in map(json.loads, Path(self.record['transcript']).read_text().splitlines())
                        if r.get('type') == 'event_msg' and r.get('payload', {}).get('type') == 'user_message']
            self.wait_for(lambda: receipt['submitted_text'] in messages())
            self.wait_for(lambda: 'Busy fixture completed.' in self.screen())
            self.assertEqual(messages().count(receipt['submitted_text']), 1)
        finally:
            release.touch()


@unittest.skipUnless(claude.native.CLAUDE, 'Set HGS_CLAUDE_TEST_BIN for the private busy Claude contract')
class NativeBusyClaude(claude.NativeClaudeInput):
    native_permission_mode = 'bypassPermissions'
    assert_refusal = claude.fixtures.InputTransport.assert_refusal
    input_events = claude.native.NativeClaudeQuestions.events
    test_trailing_backslash_starts_one_turn_with_the_exact_message = None
    test_multiline_message_ending_in_backslashes_is_preserved = None
    test_leading_absolute_path_is_literal_message_text = None

    def setUp(self):
        if self._testMethodName.startswith('test_no_color_'):
            self.native_env = (*self.native_env, 'NO_COLOR=1')
        super().setUp()
        if self._testMethodName.startswith(('test_busy_long_hgs_reports', 'test_no_color_')):
            claude.native.NativeClaudeQuestions.enable_gutter_capture(self, False)

    def queued_messages(self):
        # Claude steers queued inputs into the original Bash tool result, not
        # standalone native user turns. Inspect that exact model-bound envelope.
        for request in reversed(self.native_requests):
            results = [block.get('content', '') for message in request.get('messages', [])
                       if isinstance(message.get('content'), list) for block in message['content']
                       if block.get('type') == 'tool_result' and block.get('tool_use_id') == 'toolu_busy']
            if results:
                return re.findall(r'The user sent a new message while you were working:\n(.*?)\n\nThis is how Claude Code',
                                  '\n'.join(results), flags=re.S)
        return []

    def native_response_blocks(self, body):
        if getattr(self, 'busy_ready', False) and not getattr(self, 'tool_sent', False):
            self.tool_sent = True
            return [dict(type='tool_use', id='toolu_busy', name='Bash',
                         input=dict(command=command(self.root), description='Wait for the private queue fixture'))]
        return [dict(type='text', text='Fixture complete.')]

    def test_busy_tool_queue_preserves_drafts_and_accepts_exactly_once(self):
        release = self.root / 'tool-release'
        self.busy_ready = True
        try:
            terminal_send(self, PROMPT)
            self.wait_for((self.root / 'tool-started').exists)
            self.record.update(activity='busy', phase='tool', active_tools={'toolu_busy': {'tool_name': 'Bash'}})
            self.write_record()
            terminal_send(self, FIRST)
            terminal_send(self, SECOND)
            self.wait_for(lambda: 'Press up to edit queued messages' in self.screen())
            draft = 'keep this user draft'
            self.tmux('send-keys', '-t', self.pane, '-l', draft)
            self.wait_for(lambda: draft in self.screen())
            self.assertIn('agent busy:', self.send(self.payload(text='must not be typed'), success=False))
            self.assertIn(draft, self.screen())
            self.tmux('send-keys', '-t', self.pane, 'C-u')
            self.wait_for(lambda: draft not in self.screen())
            payload = self.payload(text=LAST)
            receipt = self.send(payload)
            self.assertEqual(receipt['status'], 'submitted')
            self.assertEqual(self.send(payload), receipt)
            release.touch()
            self.wait_for(lambda: LAST in self.queued_messages())
            self.assertEqual(self.queued_messages(), [FIRST, SECOND, LAST])
            self.assertEqual(self.accepted(PROMPT), 1)
            self.assertEqual(self.accepted('must not be typed'), 0)
            self.assertEqual(self.accepted(draft), 0)
            # A later ordinary turn exposes any duplicate left in the queue.
            control = 'After the private busy tool.'
            self.wait_for(lambda: 'Press up to edit queued messages' not in self.screen())
            terminal_send(self, control)
            self.wait_for(lambda: self.accepted(control))
            self.assertEqual(self.queued_messages(), [FIRST, SECOND, LAST])
        finally:
            release.touch()

    def test_busy_tool_queue_80x24(self):
        self.test_busy_tool_queue_preserves_drafts_and_accepts_exactly_once()

    def test_busy_long_hgs_reports_preserve_a_real_draft_then_accept_the_persons_send(self):
        release = self.root / 'tool-release'
        self.busy_ready = True
        reports = [f'<zerus-message from="codex/fixture/child-{n}">\n' +
                   '\n'.join(f'Report {n}, line {line}: ' + 'long private fixture report ' * 5 for line in range(12)) +
                   '\n</zerus-message>' for n in range(3)]
        try:
            terminal_send(self, PROMPT)
            self.wait_for((self.root / 'tool-started').exists)
            self.record.update(activity='busy', phase='tool', active_tools={'toolu_busy': {'tool_name': 'Bash'}})
            self.write_record()
            for report in reports:
                payload = self.payload(text=report)
                receipt = self.send(payload)
                self.assertEqual(receipt['status'], 'submitted')
                self.assertEqual(self.send(payload), receipt)
            self.wait_for(lambda: 'Press up to edit queued messages' in self.screen())
            draft = 'keep the persons real draft'
            self.tmux('send-keys', '-t', self.pane, '-l', draft)
            self.wait_for(lambda: draft in self.screen())
            refused = self.payload(text='must not be typed')
            offered = self.input_events()
            self.assertIn('agent busy:', self.send(refused, success=False))
            self.assertEqual(self.input_events(), offered, "a pre-input refusal must offer no terminal bytes")
            self.assert_refusal(refused)
            self.assertIn(draft, self.screen())
            self.tmux('send-keys', '-t', self.pane, 'C-u')
            self.wait_for(lambda: draft not in self.screen())
            payload = self.payload(text=LAST)
            receipt = self.send(payload)
            self.assertEqual(receipt['status'], 'submitted')
            self.assertEqual(self.send(payload), receipt)
            release.touch()
            self.wait_for(lambda: LAST in self.queued_messages())
            self.assertEqual(self.queued_messages(), [*reports, LAST])
            self.assertEqual(self.accepted('must not be typed'), 0)
        finally:
            release.touch()

    def test_busy_long_hgs_reports_80x24(self):
        self.test_busy_long_hgs_reports_preserve_a_real_draft_then_accept_the_persons_send()

    def test_no_color_short_terminal_queue_explains_the_refusal_and_never_types_into_a_draft(self):
        release = self.root / 'tool-release'
        self.busy_ready = True
        reason = 'This agent was started with NO_COLOR, so Zerus cannot tell a queued message from a draft. Restart the session to fix sending.'
        try:
            terminal_send(self, PROMPT)
            self.wait_for((self.root / 'tool-started').exists)
            self.record.update(activity='busy', phase='tool', active_tools={'toolu_busy': {'tool_name': 'Bash'}})
            self.write_record()
            terminal_send(self, 'Short terminal queue')
            hint = 'Press up to edit queued messages'
            self.wait_for(lambda: hint in self.screen())
            payload = self.payload(text='must not be typed')
            offered = self.input_events()
            self.assertIn(reason, self.send(payload, success=False))
            self.assertEqual(self.input_events(), offered, "a pre-input refusal must offer no terminal bytes")
            self.assert_refusal(payload)
            # The same hint as a real draft with its caret at Home must refuse too.
            self.tmux('send-keys', '-t', self.pane, '-l', hint)
            self.wait_for(lambda: self.tmux('display-message', '-p', '-t', self.pane, '#{cursor_x}').strip() == str(2 + len(hint)))
            self.tmux('send-keys', '-t', self.pane, 'C-a')
            self.wait_for(lambda: self.tmux('display-message', '-p', '-t', self.pane, '#{cursor_x}').strip() == '2')
            payload = self.payload(text='also must not be typed')
            offered = self.input_events()
            self.assertIn(reason, self.send(payload, success=False))
            self.assertEqual(self.input_events(), offered, "a pre-input refusal must offer no terminal bytes")
            self.assert_refusal(payload)
            self.assertIn(hint, self.screen())
            release.touch()
            self.wait_for(lambda: 'Short terminal queue' in self.queued_messages())
            self.assertEqual(self.queued_messages(), ['Short terminal queue'])
            self.assertEqual(self.accepted('must not be typed'), 0)
            self.assertEqual(self.accepted('also must not be typed'), 0)
        finally:
            release.touch()

    def test_no_color_short_terminal_queue_80x24(self):
        self.test_no_color_short_terminal_queue_explains_the_refusal_and_never_types_into_a_draft()

    def enqueued(self, content):
        return [r for r in self.rows() if r.get('type') == 'queue-operation'
                and r.get('operation') == 'enqueue' and r.get('content') == content]

    def test_an_identical_earlier_queued_prompt_never_proves_a_new_send(self):
        # zerus-iitw8.4: an identical prompt already waits in Claude's queue.
        # Only this send's own new enqueue may prove it, and both arrive.
        release = self.root / 'tool-release'
        self.busy_ready = True
        try:
            terminal_send(self, PROMPT)
            self.wait_for((self.root / 'tool-started').exists)
            self.record.update(activity='busy', phase='tool', active_tools={'toolu_busy': {'tool_name': 'Bash'}})
            self.write_record()
            terminal_send(self, 'continue')
            self.wait_for(lambda: 'Press up to edit queued messages' in self.screen())
            # Claude writes rows late: the earlier enqueue may land after the
            # banner, even after this send's start. The stamps tell them apart.
            sent_at = time.time()
            receipt = self.send(self.payload(text='continue'))
            self.assertEqual((receipt['status'], receipt['native']), ('submitted', 'queue'), self.screen())
            def stamped(row):
                return datetime.datetime.fromisoformat(row['timestamp'].replace('Z', '+00:00')).timestamp()
            self.wait_for(lambda: len(self.enqueued('continue')) == 2)
            earlier, own = self.enqueued('continue')
            self.assertLess(stamped(earlier), sent_at)
            self.assertGreaterEqual(stamped(own), sent_at, 'the proof is this send\'s own enqueue')
            release.touch()
            self.wait_for(lambda: self.queued_messages().count('continue') == 2)
            self.assertEqual(self.queued_messages(), ['continue', 'continue'])
        finally:
            release.touch()

    def rows(self):
        return [json.loads(line) for line in Path(self.record['transcript']).read_text().splitlines()]

    def carrying(self, rows, marker='Settings preview delta is ready.'):
        def text(row):
            content = row.get('message', {}).get('content')
            if isinstance(content, list):
                return '\n'.join(b.get('text', '') for b in content if b.get('type') == 'text')
            return content if isinstance(content, str) else ''
        return [r for r in rows if r.get('type') == 'user' and marker in text(r)]

    def composer(self):
        lines = self.screen().splitlines()
        prompt = max(i for i, line in enumerate(lines) if line.startswith('❯'))
        return lines[prompt].strip()

    def test_pasted_images_settle_before_the_one_enter_when_idle(self):
        # zerus-iitw8: Claude converts the bare image paths after the paste and
        # swallows an Enter while it shows Pasting…; hgs must wait, then prove.
        text, attachments = image_message(self.root)
        payload = self.payload(text=text, attachments=attachments)
        receipt = self.send(payload)
        self.assertEqual((receipt['status'], receipt['native']), ('submitted', 'transcript'), self.screen())
        rows = self.carrying(self.rows())
        self.assertEqual(len(rows), 1)
        self.assertEqual(sum(b.get('type') == 'image' for b in rows[0]['message']['content']), 2)
        self.wait_for(lambda: self.composer() in ('❯', '❯\xa0'))
        self.assertEqual(self.send(payload), receipt)
        self.assertEqual(len(self.carrying(self.rows())), 1)

    def test_busy_pasted_images_are_queued_with_native_proof_and_delivered_once(self):
        release = self.root / 'tool-release'
        self.busy_ready = True
        try:
            terminal_send(self, PROMPT)
            self.wait_for((self.root / 'tool-started').exists)
            self.record.update(activity='busy', phase='tool', active_tools={'toolu_busy': {'tool_name': 'Bash'}})
            self.write_record()
            text, attachments = image_message(self.root)
            payload = self.payload(text=text, attachments=attachments)
            receipt = self.send(payload)
            self.assertEqual((receipt['status'], receipt['native']), ('submitted', 'queue'), self.screen())
            self.assertEqual(self.send(payload), receipt)
            release.touch()
            def delivered():
                rows = self.rows()
                queued = [r for r in rows if r.get('type') == 'attachment' and r.get('attachment', {}).get('type') == 'queued_command'
                          and 'Settings preview delta is ready.' in str(r['attachment'].get('prompt'))]
                return len(self.carrying(rows)) + len(queued)
            self.wait_for(delivered)
            enqueued = [r for r in self.rows() if r.get('type') == 'queue-operation' and r.get('operation') == 'enqueue'
                        and 'Settings preview delta is ready.' in r.get('content', '')]
            self.assertEqual(len(enqueued), 1)
            self.wait_for(lambda: 'Press up to edit queued messages' not in self.screen())
            self.assertEqual(delivered(), 1)
        finally:
            release.touch()


@unittest.skipUnless(claude.native.CLAUDE, 'Set HGS_CLAUDE_TEST_BIN for the private hooked Claude contract')
class NativeHookedImagesClaude(NativeBusyClaude):
    """The idle image send with hgs's own UserPromptSubmit hook, which takes the state lock:
    Claude runs it before recording the message, so the proof wait must not hold that lock."""
    test_busy_tool_queue_preserves_drafts_and_accepts_exactly_once = None
    test_busy_tool_queue_80x24 = None
    test_busy_pasted_images_are_queued_with_native_proof_and_delivered_once = None
    test_an_identical_earlier_queued_prompt_never_proves_a_new_send = None

    @property
    def native_env(self):
        # The parent prepares its catalog before the native run IDs exist.
        environment = getattr(self, '_native_settings_env', super().native_env)
        if not hasattr(self, 'name'):
            return environment
        return (*environment, 'HGS_SESSION=' + self.name, 'HGS_RUN_ID=' + self.run_id,
                'HGS_STATE_DIR=' + str(self.state), 'HGS_CONFIG_DIR=' + str(self.root / 'config'))

    @native_env.setter
    def native_env(self, environment):
        self._native_settings_env = environment

    def native_settings(self, work):
        settings = super().native_settings(work)
        hook = shlex.quote(str(claude.fixtures.HGS)) + ' __state hook'
        settings.setdefault('hooks', {})['UserPromptSubmit'] = [
            {'hooks': [{'type': 'command', 'command': hook, 'timeout': 60}]}]
        if '_after_turn' in self._testMethodName:
            settings['hooks']['Stop'] = [{'hooks': [{'type': 'command', 'command': hook, 'timeout': 60}]}]
        return settings

    def pending_images(self, *, after_turn=False, attached=False):
        import native_effort_id as identity
        if after_turn:
            terminal_send(self, 'Complete the private preceding turn.')
            self.wait_for(lambda: self.accepted('Complete the private preceding turn.'))
            self.wait_for(lambda: self.composer() in ('❯', '❯\xa0'))
            self.wait_for(lambda: json.loads(self.record_path.read_bytes()).get('activity') == 'idle')
            current = json.loads(self.record_path.read_bytes())
            self.assertEqual((current['run_id'], current['pid']), (self.run_id, self.record['pid']))
            self.record = current
        defaults = self.root / '.claude/settings.json'
        before_defaults = defaults.read_bytes()
        text, attachments = image_message(self.root)
        text = text.replace('Screenshots:', '\n'.join('Extra fixture detail %d: preserve this entire line in order.' % n for n in range(10)) + '\nScreenshots:')
        self.assertGreaterEqual(len(text), 3072)
        self.assertLessEqual(len(text), 4096)
        record, path, _ = identity.moved_record(self, self.name, self.record, existing_machine=True)
        self.record, self.record_path = record, path
        attempts = identity.key_log(self)
        calls = len(self.native_requests)
        receipt = identity.pending_then_message(self, self.name, record, path,
            'claude-fable-5-2', 'xhigh', text, attempts, attachments[:1] if attached else ())
        self.assertEqual((receipt['status'], receipt['native']), ('submitted', 'transcript'))
        rows = self.carrying(self.rows())
        self.assertEqual(len(rows), 1)
        self.assertEqual(sum(b.get('type') == 'image' for b in rows[0]['message']['content']), 2)
        self.assertEqual(len(self.native_requests) - calls, 1, self.native_requests[calls:])
        current = json.loads(path.read_bytes())
        self.assertEqual(current['input_pending_request'], receipt['request_id'])
        self.assertGreater(current['input_seen_at'], current['input_pending_at'])
        self.assertNotIn('input_typing', current)
        self.assertEqual(defaults.read_bytes(), before_defaults)

    def test_pending_settings_then_message_settles_two_image_paths_when_idle(self):
        self.pending_images()

    def test_pending_settings_then_message_settles_two_image_paths_after_turn(self):
        self.pending_images(after_turn=True)

    def test_pending_settings_then_message_settles_two_image_paths_with_attachments(self):
        self.pending_images(attached=True)

    def test_pending_settings_then_message_swallowed_enter_keeps_input_and_manual_clear(self):
        import fcntl
        import hashlib
        import hook_caller_fixtures
        import native_effort_id as identity
        import test_clear_input_native as clearing
        import uuid
        record, path, _ = identity.moved_record(self, self.name, self.record, existing_machine=True)
        self.record, self.record_path = record, path
        attempts = identity.key_log(self)
        pending = str(uuid.uuid4())
        record['pending_settings'] = dict(request_id=pending, model='claude-fable-5-2', effort='xhigh',
            run_id=self.run_id, conversation_id=self.conversation_id,
            receipt_name=self.name, receipt_run_id=self.run_id)
        path.write_text(json.dumps(record))
        scheduled = self.state / 'settings_receipts' / (pending + '.json')
        scheduled.parent.mkdir(mode=0o700, exist_ok=True)
        scheduled.write_text(json.dumps(dict(request_id=pending, status='scheduled', name=self.name,
            run_id=self.run_id, conversation_id=self.conversation_id,
            model='claude-fable-5-2', effort='xhigh', scope='session')))
        scheduled.chmod(0o600)
        text, _ = image_message(self.root)
        payload = self.payload(text=text)
        raw = json.dumps(payload)
        receipt_path = self.state / 'input_receipts' / (payload['request_id'] + '.json')
        self.env['ZERUS_PENDING_SEND_RECEIPT'] = str(receipt_path)
        swallowed = self.root / 'swallowed-message-enter'
        observer = self.root / 'native-effort-tmux.py'
        source = observer.read_text()
        needle = 'raise SystemExit(subprocess.call('
        assert source.count(needle) == 1
        fault = ("if args[:1]==['send-keys'] and args[-1:]==['Enter'] and "
                 "json.loads(pathlib.Path(" + repr(str(path)) + ").read_bytes()).get('input_pending_request') == "
                 + repr(payload['request_id']) + ":\n pathlib.Path(" + repr(str(swallowed)) + ").touch()\n raise SystemExit(0)\n")
        observer.write_text(source.replace(needle, fault + needle))
        command = [str(claude.fixtures.HGS), 'send', self.name, '--session-id', record['session_id'], '--json']
        calls = len(self.native_requests)
        child = subprocess.Popen(command, env=self.env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                 stderr=subprocess.PIPE, text=True)
        child.stdin.write(raw); child.stdin.close(); child.stdin = None
        try:
            self.wait_for(swallowed.exists)
            # Root is free during proof, but the original SID and typing marker
            # still exclude another input to the same session.
            with (self.state / '.lock').open('rb') as writer:
                fcntl.flock(writer, fcntl.LOCK_EX | fcntl.LOCK_NB)
                fcntl.flock(writer, fcntl.LOCK_UN)
            competing = dict(payload, request_id=str(uuid.uuid4()), text='Do not enter this second message')
            refused = subprocess.run(command, env=self.env, input=json.dumps(competing),
                                     text=True, capture_output=True, timeout=5)
            self.assertNotEqual(refused.returncode, 0)
            self.assertIn('another message is being entered', refused.stderr)
            _, shown = clearing.ClearInput.terminal_request(self, 'composer')
            self.assertFalse(shown['composer']['clearable'])
            _, denied = clearing.ClearInput.terminal_request(self, 'input',
                terminal_binding_id=shown['terminal_binding_id'], expires_at=time.time() + 5,
                text='do not type', enter=False)
            self.assertEqual(denied['status'], 'failed')
            self.assertIn('message is being entered', denied['error'])
            # A second scripted agent, its own process and genuine hook child,
            # shares only this private state Root. No borrowed ancestry markers.
            other = claude.fixtures.InputTransport(methodName='runTest')
            other.setUp()
            self.addCleanup(other.doCleanups)
            self.addCleanup(other.tearDown)
            other.state = self.state
            other.env['HGS_STATE_DIR'] = str(self.state)
            other.record_path = self.state / other.record_path.name
            other.claude_ready()
            other.record_path.chmod(0o600)
            event = dict(hook_event_name='PreToolUse', session_id=other.conversation_id,
                         tool_name='Bash', tool_use_id='other-private-tool')
            hook = hook_caller_fixtures.run(other, event, dict(other.env,
                HGS_TEST_BIN=str(claude.fixtures.HGS), HGS_SESSION=other.name, HGS_RUN_ID=other.run_id))
            self.assertEqual(hook.returncode, 0, hook.stderr)
            self.assertIn('other-private-tool', json.loads(other.record_path.read_bytes())['active_tools'])
            self.assertIsNone(child.poll(), 'the unrelated hook ran while proof was still waiting')
            out, err = child.communicate(timeout=30)
        finally:
            if child.poll() is None:
                child.terminate(); child.communicate(timeout=30)
        self.assertNotEqual(child.returncode, 0, out)
        self.assertIn('delivery uncertain', err)
        receipt = json.loads(receipt_path.read_bytes())
        self.assertEqual((receipt['status'], receipt['delivery']), ('in_progress', 'uncertain'))
        self.assertEqual(receipt['digest'], hashlib.sha256(raw.encode()).hexdigest())
        self.assertEqual(receipt['text'], text)
        self.assertTrue(receipt['composer'])
        current = json.loads(path.read_bytes())
        self.assertEqual(current['input_pending_request'], payload['request_id'])
        self.assertNotIn('input_typing', current)
        self.assertEqual(len(self.native_requests), calls)
        self.assertEqual(self.carrying(self.rows()), [])
        before = attempts()
        replay = subprocess.run(command, env=self.env, input=raw, text=True, capture_output=True, timeout=10)
        self.assertNotEqual(replay.returncode, 0)
        self.assertEqual(attempts(), before, 'an uncertain UUID cannot replay input')
        _, shown = clearing.ClearInput.terminal_request(self, 'composer')
        self.assertTrue(shown['composer']['clearable'], shown)
        _, cleared = clearing.ClearInput.terminal_request(self, 'clear-input',
            terminal_binding_id=shown['terminal_binding_id'], expires_at=time.time() + 5,
            text=shown['composer']['text'])
        self.assertEqual(cleared['status'], 'cleared')
        # The person explicitly sends the retained payload from Terminal after
        # clearing. This is not a retry or another delivery by the original UUID.
        observer.write_text(source)
        self.tmux('set-buffer', '-b', 'person-kept-message', receipt['submitted_text'])
        self.tmux('paste-buffer', '-p', '-r', '-d', '-b', 'person-kept-message', '-t', self.pane)
        # Native attachment IDs advance across Clear; they do not restart at 1.
        prior = max(map(int, re.findall(r'\[(?:Image|Pasted text) #(\d+)', receipt['composer'])))
        expected = f'[Image #{prior + 1}] [Image #{prior + 2}][Pasted text #{prior + 3} '
        self.wait_for(lambda: expected in self.screen() and 'Pasting…' not in self.screen())
        time.sleep(.3)
        self.tmux('send-keys', '-t', self.pane, 'Enter')
        self.wait_for(lambda: len(self.carrying(self.rows())) == 1)
        rows = self.carrying(self.rows())
        self.assertEqual(sum(b.get('type') == 'image' for b in rows[0]['message']['content']), 2)
        self.assertEqual(len(self.native_requests), calls + 1)
        self.assertEqual(json.loads(receipt_path.read_bytes()), receipt)

    def test_pasted_images_settle_before_the_one_enter_when_idle(self):
        started = time.monotonic()
        super().test_pasted_images_settle_before_the_one_enter_when_idle()
        record = json.loads(self.record_path.read_text())
        self.assertGreater(record['input_seen_at'], record['input_pending_at'], 'the hook recorded the acceptance')
        self.assertLess(time.monotonic() - started, 20)
