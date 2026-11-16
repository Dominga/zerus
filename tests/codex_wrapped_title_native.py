"""One real, keyless Codex title-wrap contract; all keys target its private pane."""
import json
import os
from pathlib import Path
import re
import subprocess
import shlex
import sys
import time
import uuid
import native_effort_id
import test_input
import unittest
import test_codex_input_native as input_fixture


def exercise(case):
    case.assertEqual(case.geometry, (80, 24), 'This contract needs its captured wrap geometry')
    case.record.update(model='gpt-6.1-sol')
    case.write_record()
    # Native resumed history can retain its effort. Use the displayed label,
    # and fill that field so the final word must wrap at the reported width.
    plain_before = case.screen().splitlines()
    status = next(row for row in plain_before if 'gpt-6.1-sol' in row.lower() and ' · ' in row)
    label = status.strip().split(' · ')[0]
    available = 80 - len('  '+label+' · '+str(case.root)+' · ')
    case.assertGreaterEqual(available, 1, 'Use a short private TMPDIR for this frame')
    prefix = 'R'*available
    title = prefix+' instructions'
    attempts = native_effort_id.key_log(case)
    # Observe every input mutation, forwarding unchanged argv/stdin to this
    # fixture's original private tmux. Reads keep their original fast path.
    observer = case.root / 'native-effort-tmux.py'
    observer.write_text(observer.read_text().replace(
        "if args[:1]==['send-keys']:",
        "if args[:1] in (['send-keys'],['load-buffer'],['paste-buffer'],['delete-buffer']):"))
    original = case.bin / 'tmux-native-effort-original'
    case.script('tmux', '#!/bin/sh\ncase "$1" in\n'
                'send-keys|load-buffer|paste-buffer|delete-buffer) exec '
                + shlex.quote(sys.executable) + ' ' + shlex.quote(str(observer))
                + ' "$@" ;;\n*) exec ' + shlex.quote(str(original)) + ' "$@" ;;\nesac\n')
    case.tmux('send-keys', '-t', case.pane, '-l', '/rename')
    case.wait_for(lambda: '/rename' in case.screen())
    case.tmux('send-keys', '-t', case.pane, 'Enter')
    case.wait_for(lambda: 'Type a name and press Enter' in case.screen())
    case.wait_for(lambda: case.server.raw_requests)  # Separate native title generation.
    # The native dialog must stay a dialog; inspection/refusal never answers it.
    before, calls = attempts(), len(case.server.raw_requests)
    case.send(case.payload(text='Must not answer a private rename dialog'), success=False)
    case.assertEqual(attempts(), before)
    case.assertEqual(len(case.server.raw_requests), calls)
    case.assertIn('Type a name and press Enter', case.screen())
    case.tmux('send-keys', '-t', case.pane, '-l', title)
    case.wait_for(lambda: 'instructions' in case.screen())
    time.sleep(.2)  # Native paste-burst handling, as in the maintained rename case.
    case.tmux('send-keys', '-t', case.pane, 'Enter')
    case.wait_for(lambda: 'Name thread' not in case.screen() and 'Rename thread' not in case.screen())
    ansi = re.compile(r'\x1b\[[0-?]*[ -/]*[@-~]')
    def named_footer():
        rows = case.screen().splitlines()
        return (len(rows) == 24 and rows[-2].strip() == 'instructions'
                and ' · '+str(case.root)+' · ' in rows[-3])
    test_input.InputTransport.wait_until(case, named_footer, 15,
        lambda: 'The private named status did not reach its two-row frame: '+case.screen())
    styled = case.tmux('capture-pane', '-ep', '-t', case.pane)
    if artifact := os.environ.get('ZERUS_NATIVE_WRAPPED_CAPTURE'):
        Path(artifact).write_text(styled)  # This fixture owns all visible data.
    plain = [ansi.sub('', row) for row in styled.splitlines()]
    first = next((i for i, row in enumerate(plain) if 'gpt-6.1-sol' in row.lower() and ' · ' in row), None)
    case.assertIsNotNone(first, styled)
    case.assertIn(' · '+str(case.root)+' · ', plain[first])
    case.assertEqual(plain[first + 1].strip(), 'instructions')
    case.assertIn('← for agents · ? for shortcuts', plain[first + 2])
    x, y = map(int, case.tmux('display-message', '-p', '-t', case.pane,
                            '#{cursor_x}:#{cursor_y}').strip().split(':'))
    case.assertEqual((x, y), (2, 19))
    read = dict(request_id=str(uuid.uuid4()), action='composer',
                expected_run_id=case.run_id, expected_conversation_id=case.conversation_id)
    before, calls = attempts(), len(case.server.raw_requests)
    result = subprocess.run([str(test_input.HGS), 'terminal', case.name, '--json'],
                            input=json.dumps(read), env=case.env, capture_output=True, text=True, timeout=15)
    case.assertEqual(result.returncode, 0, result.stderr)
    case.assertEqual(json.loads(result.stdout)['composer']['state'], 'empty', styled)
    case.assertEqual(attempts(), before)
    case.assertEqual(len(case.server.raw_requests), calls)
    for changed in [dict(expected_run_id=str(uuid.uuid4())),
                    dict(expected_conversation_id=str(uuid.uuid4()))]:
        case.send(case.payload(text='Must not cross a private identity', **changed), success=False)
        case.assertEqual(attempts(), before)
    # Preserve a real native draft under the same title footer.
    draft = 'Keep this private wrapped-title draft'
    case.tmux('send-keys', '-t', case.pane, '-l', draft)
    case.wait_for(lambda: draft in case.screen())
    before = attempts()
    case.send(case.payload(text='Must not overwrite the private draft'), success=False)
    case.assertEqual(attempts(), before)
    case.assertIn(draft, case.screen())
    case.tmux('send-keys', '-t', case.pane, 'C-a', 'C-k')
    case.wait_for(lambda: draft not in case.screen())
    payload = case.payload(text='Guarded wrapped-title fixture '+str(uuid.uuid4()))
    receipt = case.send(payload)
    case.assertEqual(receipt['status'], 'submitted')
    after = attempts()
    case.assertEqual(case.send(payload), receipt)
    case.assertEqual(attempts(), after)
    case.wait_for(lambda: 'Fixture turn complete.' in case.screen())
    rows = [json.loads(row) for row in Path(case.record['transcript']).read_text().splitlines()]
    messages = [row['payload']['message'] for row in rows if row.get('type') == 'event_msg'
                and row.get('payload', {}).get('type') == 'user_message']
    case.assertEqual(messages.count(payload['text']), 1)
    case.assertNotIn(draft, messages)
    case.assertFalse(any(text.startswith('Must not ') for text in messages))
    print('ZERUS_NATIVE_WRAPPED_TITLE='+json.dumps(dict(version=case.native_version,
        geometry=case.geometry, cursor=[x,y], inspected_without_keys=True,
        dialog_preserved=True, draft_preserved=True, wrong_identity_no_keys=True,
        exactly_one_user_row=True, duplicate_no_keys=True)))


@unittest.skipUnless(os.environ.get('HGS_CODEX_TEST_BIN'), 'Set HGS_CODEX_TEST_BIN for isolated native Codex input')
class NativeWrappedTitle(unittest.TestCase):
    # Reuse the maintained provider/setup without inheriting its native methods
    # into the busy-input classes (and their distinct geometry contracts).
    script = input_fixture.NativeCodexInput.script
    write_record = input_fixture.NativeCodexInput.write_record
    tmux = input_fixture.NativeCodexInput.tmux
    tearDown = input_fixture.NativeCodexInput.tearDown
    payload = input_fixture.NativeCodexInput.payload
    send = input_fixture.NativeCodexInput.send
    fail = input_fixture.NativeCodexInput.fail
    wait_for = input_fixture.NativeCodexInput.wait_for
    native_features = input_fixture.NativeCodexInput.native_features
    setUp = input_fixture.NativeCodexInput.setUp
    screen = input_fixture.NativeCodexInput.screen

    def prepare_native_hooks(self):
        self.geometry = (80, 24)
        isolated_home = self.root / 'native-profile-home'
        isolated_home.mkdir()
        self.env['HOME'] = str(isolated_home)  # Display the exact cwd, not '~'.
        config = self.native_home / 'config.toml'
        config.write_text(config.read_text() + '\n[tui]\n'
                          'status_line=["model-with-reasoning","current-dir","thread-name"]\n')

    def test_wrapped_native_title_keeps_guarded_input_and_drafts(self):
        exercise(self)
