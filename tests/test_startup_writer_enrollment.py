"""Actual original supervisor and one held private appearance-helper pipe."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import unittest

import test_claude_theme as theme
import test_pause

@unittest.skipUnless(os.environ.get('HGS_TEST_STARTUP_ENROLLMENT_BIN'),
                     'requires a private const-true original-supervisor image')
class StartupWriterEnrollment(unittest.TestCase):
    def setUp(self):
        # theme.ClaudeTheme.setUp uses super(), so invoke the real hierarchy
        # through a contained fixture and use its private helpers explicitly.
        self.fixture = theme.ClaudeTheme('test_original_supervisor_confirms_dark_once_then_leaves_login_manual')
        self.fixture.setUp()
        self.root, self.env, self.bin, self.socket = (getattr(self.fixture, name)
                                                     for name in ('root', 'env', 'bin', 'socket'))
        self.assertEqual(Path(os.environ['HGS_TEST_STARTUP_ENROLLMENT_BIN']), test_pause.HGS)
        real = test_pause.TMUX
        holder = '''import os,pathlib,sys,time
release=pathlib.Path(sys.argv[1]);end=time.monotonic()+15
while not release.exists():
    assert time.monotonic()<end
    time.sleep(.005)
'''
        wrapper = '''#!PYTHON
import json,os,pathlib,subprocess,sys
home=pathlib.Path.home();args=sys.argv[1:]
command=[REAL,'-S',SOCKET,'-f','/dev/null',*args]
if args[:3]==['capture-pane','-p','-t'] and os.environ.get('HGS_RUN_ID'):
    try:fd=os.open(home/'appearance-helper-once',os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
    except FileExistsError:fd=None
    if fd is not None:
        os.close(fd)
        result=subprocess.run(command)
        child=subprocess.Popen([sys.executable,'-c',HOLDER,str(home/'release-helper-output')],
                               stdin=subprocess.DEVNULL,stdout=sys.stdout,stderr=subprocess.DEVNULL)
        payload=dict(helper_pid=os.getpid(),holder_pid=child.pid,supervisor=os.getppid(),
                     session=os.environ['HGS_SESSION_ID'],run=os.environ['HGS_RUN_ID'])
        staged=home/'held-helper.staged';staged.write_text(json.dumps(payload));staged.replace(home/'held-helper.json')
        sys.exit(result.returncode)
os.execv(REAL,command)
'''.replace('REAL', repr(real)).replace('SOCKET', repr(self.socket)).replace('HOLDER', repr(holder))
        self.fixture.script('tmux', wrapper.replace('#!PYTHON', '#!' + sys.executable))

    def tearDown(self):
        if hasattr(self, 'root'):
            (self.root / 'release-helper-output').touch()
        if hasattr(self, 'fixture'):
            self.fixture.tearDown()

    def held(self):
        try:
            return json.loads((self.root / 'held-helper.json').read_text())
        except (OSError, ValueError):
            return None

    def rows(self, held):
        path = self.root / 'state/terminating-writers' / held['session'] / held['run'] / 'pending.json'
        return json.loads(path.read_text())['pending']

    def test_original_helper_exit_with_held_output_keeps_its_exact_run_row(self):
        before = self.fixture.launch('enrollment')
        self.fixture.wait(lambda: self.held() is not None)
        held = self.held()
        self.assertEqual(held['supervisor'], before['supervisor']['pid'])
        self.assertEqual(held['session'], before['session_id'])
        self.assertEqual(held['run'], before['run_id'])
        # The helper has already offered all output and exited. Its descendant
        # holds the SAME original output pipe, so EOF/join cannot be invented.
        def helper_exited():
            result = subprocess.run(['ps', '-p', str(held['helper_pid']), '-o', 'stat='],
                                    text=True, capture_output=True, timeout=1)
            return result.returncode != 0 or result.stdout.strip().startswith('Z')
        self.fixture.wait(helper_exited)
        rows = self.rows(held)
        self.assertEqual(len(rows), 1, 'held original appearance IO has no durable run row')
        original = rows[0]
        self.assertEqual(original['kind'], 'launch')
        self.assertEqual(original['owner']['pid'], held['supervisor'])
        self.assertEqual(self.fixture.received(), b'')
        (self.root / 'release-helper-output').touch()
        self.fixture.wait(lambda: all(row['request'] != original['request'] for row in self.rows(held)))

    def test_expired_helper_keeps_unknown_row_and_never_offers_enter_after_eof(self):
        self.fixture.launch('expired-enrollment')
        self.fixture.wait(lambda: self.held() is not None)
        held = self.held()
        original, = self.rows(held)
        # Consume the existing 3 s observation window while the actual pipe is
        # held. Neither helper exit nor expiry can retire this exact row.
        time.sleep(3.2)
        self.assertEqual(self.rows(held), [original])
        import fcntl
        gate = self.root / 'state/terminating-writers' / held['session'] / held['run'] / 'gate'
        with gate.open('r+b') as probe:
            with self.assertRaises(BlockingIOError):
                fcntl.flock(probe, fcntl.LOCK_EX | fcntl.LOCK_NB)
        self.assertEqual(self.fixture.received(), b'')
        (self.root / 'release-helper-output').touch()
        def lease_released():
            try:
                with gate.open('r+b') as probe:
                    fcntl.flock(probe, fcntl.LOCK_EX | fcntl.LOCK_NB)
                return True
            except BlockingIOError:
                return False
        self.fixture.wait(lease_released)
        # Drop/timeout is not a retained physical report at this adapter. Keep
        # the unresolved row; a free lease or later inspect cannot replay it.
        self.assertEqual(self.rows(held), [original])
        for _ in range(3):
            self.fixture.hgs('inspect', self.fixture.name)
        self.assertEqual(self.fixture.received(), b'')
        self.assertEqual(self.fixture.claims(), [])


if __name__ == '__main__':
    unittest.main()
