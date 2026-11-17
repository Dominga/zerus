"""Raw Terminal's Unix termination fence; private backend and synthetic marker data only.

No marker producer or graceful signal is enabled. These controls complement
the real-provider Clear input suites, which use their own private profiles.
"""
from contextlib import contextmanager
import json
from pathlib import Path
import sys
import time
import unittest

import test_backend_contract as backend


@contextmanager
def terminating(root, name, value):
    """Publish/remove only this fixture's marker, preserving completion writes."""
    import fcntl

    def write(remove=False):
        state = Path(root) / 'state'
        with (state / '.lock').open('r+b') as lock:
            until = time.monotonic() + 2
            while True:
                try:
                    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    break
                except BlockingIOError:
                    if time.monotonic() >= until:
                        raise AssertionError('Private fixture writer stayed busy')
                    time.sleep(.01)
            paths = [*state.glob('*.json'), *(state / 'bindings').glob('*.json'),
                     *(state / 'sessions').glob('*.json')]
            matches = [(path, row) for path in paths
                       if isinstance(row := json.loads(path.read_text()), dict)
                       and row.get('name') == name and not row.get('rename_shadow')]
            if len(matches) != 1:
                raise AssertionError('Expected one original private fixture binding')
            path, row = matches[0]
            if remove:
                row.pop('terminating', None)
            else:
                row['terminating'] = value
            temporary = path.with_suffix('.terminal-fence-fixture')
            temporary.write_text(json.dumps(row))
            temporary.chmod(0o600)
            temporary.replace(path)
            return path

    path = write()
    try:
        yield path
    finally:
        write(remove=True)


MARKERS = (None, False, 'damaged', {'request_id': '11111111-1111-4111-8111-111111111111'})
WAIT = 'Session termination is unfinished'


@unittest.skipUnless(sys.platform in ('linux', 'darwin') and backend.BACKEND == 'tmux',
                     'Unix termination markers; Windows behavior is unchanged')
class TerminalTerminationFence(backend.Contract):
    composer = backend.Input.composer
    clear = backend.Input.clear

    def test_terminal_text_and_keys_refuse_every_marker_before_delivery(self):
        name, row, agent = self.launch()
        frame = self.snapshot(row)
        pid = agent['pid']
        for marker in MARKERS:
            with self.subTest(marker=marker), terminating(self.root, name, marker) as path:
                before, received = path.read_bytes(), self.received(pid)
                # Read-only inspection remains usable during the fence.
                self.assertEqual(self.snapshot(row)['terminal_binding_id'], frame['terminal_binding_id'])
                for fields in [dict(text='Never reach the agent', enter=False),
                               dict(text='Never submit this', enter=True), dict(key='End')]:
                    request, receipt = self.terminal(frame, **fields)
                    self.assertEqual(receipt['status'], 'failed', receipt)
                    self.assertIn(WAIT, receipt['error'])
                    self.assertEqual(self.json('terminal', name, '--json', input=json.dumps(request)), receipt)
                    self.assertEqual(path.read_bytes(), before)
                    self.assertEqual(self.received(pid), received)
        # The same terminal is still usable; this is not a broken input fixture.
        _, receipt = self.terminal(frame, text='Unmarked positive', enter=False)
        self.assertEqual(receipt['status'], 'submitted', receipt)
        self.wait_for(lambda: b'Unmarked positive' in self.received(pid), 'unmarked input')

    def test_clear_input_refuses_without_clearing_and_failed_uuids_never_replay(self):
        self.configure(ECHO='1', CLAUDE_FRAME='1')
        name, row, agent = self.launch()
        pid = agent['pid']
        frame = self.snapshot(row)
        self.assertEqual(self.terminal(frame, text='Keep this residue', enter=False)[1]['status'], 'submitted')
        self.screen(row, lambda frame, rows: rows[1] == backend.PROMPT + 'Keep this residue', 'the residue')
        state = self.composer(row)
        for marker in MARKERS:
            with self.subTest(marker=marker):
                with terminating(self.root, name, marker) as path:
                    before, received = path.read_bytes(), self.received(pid)
                    self.assertEqual(self.composer(row)['composer']['text'], 'Keep this residue')
                    request, receipt = self.clear(state, 'Keep this residue')
                    self.assertEqual(receipt['status'], 'failed', receipt)
                    self.assertIn(WAIT, receipt['error'])
                    self.assertEqual(path.read_bytes(), before)
                    self.assertEqual(self.received(pid), received)
                # Removing fixture marker data never replays an old UUID.
                self.assertEqual(self.json('terminal', name, '--json', input=json.dumps(request)), receipt)
                self.assertEqual(self.composer(row)['composer']['text'], 'Keep this residue')
        request, receipt = self.clear(state, 'Keep this residue')
        self.assertEqual(receipt['status'], 'cleared', receipt)
        with terminating(self.root, name, None):
            received = self.received(pid)
            self.assertEqual(self.json('terminal', name, '--json', input=json.dumps(request)), receipt)
            self.assertEqual(self.received(pid), received)

    def test_submitted_and_unknown_original_terminal_receipts_are_lookup_only(self):
        name, row, agent = self.launch()
        pid = agent['pid']
        request, receipt = self.terminal(self.snapshot(row), text='One original input', enter=False)
        self.assertEqual(receipt['status'], 'submitted', receipt)
        self.wait_for(lambda: b'One original input' in self.received(pid), 'the original input')
        path = self.root / 'state/terminal_receipts' / (request['request_id'] + '.json')
        saved = json.loads(path.read_text())
        with terminating(self.root, name, None):
            received = self.received(pid)
            for status in ['submitted', 'uncertain', 'claimed']:
                with self.subTest(status=status):
                    original = dict(saved, status=status)
                    path.write_text(json.dumps(original))
                    path.chmod(0o600)
                    found = self.json('terminal', name, '--json', input=json.dumps(request))
                    self.assertEqual(found['status'], 'uncertain' if status == 'claimed' else status)
                    before = path.read_bytes()
                    self.assertEqual(self.json('terminal', name, '--json', input=json.dumps(request)), found)
                    self.assertEqual(path.read_bytes(), before)
                    self.assertEqual(self.received(pid), received)


if __name__ == '__main__':
    unittest.main()
