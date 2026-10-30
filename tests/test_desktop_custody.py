"""Actual recover-only CLI input and inherited-flock boundary in private state.

No session, tmux server, service manager or native agent is contacted.
"""
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest

if os.name != 'nt':
    import fcntl

HGS = Path(os.environ.get('HGS_TEST_BIN', Path(__file__).resolve().parents[1] / 'target/debug/hgs')).resolve()


@unittest.skipIf(os.name == 'nt', 'Unix flock custody; Windows remains fenced')
class RecoverOnlyCustody(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='hgs-custody-')
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.config = self.root / 'config'
        self.config.mkdir(mode=0o700)
        self.state = self.root / 'state'
        self.tmux = self.root / 'tmux'
        self.tmux.mkdir(mode=0o700)
        self.env = {key: value for key, value in os.environ.items()
                    if not key.startswith('HGS_') and key not in
                    ('TMUX', 'TMUX_PANE', 'DBUS_SESSION_BUS_ADDRESS', 'DISPLAY', 'WAYLAND_DISPLAY')}
        self.env.update(HOME=str(self.root), HGS_CONFIG_DIR=str(self.config),
                        HGS_STATE_DIR=str(self.state), HGS_SELF='example', HGS_PEERS='',
                        TMPDIR=str(self.root), TMUX_TMPDIR=str(self.tmux),
                        XDG_CONFIG_HOME=str(self.root / 'xdg-config'),
                        XDG_STATE_HOME=str(self.root / 'xdg-state'),
                        XDG_DATA_HOME=str(self.root / 'xdg-data'),
                        XDG_CACHE_HOME=str(self.root / 'xdg-cache'))
        self.argv = [str(HGS), 'task', 'start-deliver',
                     '11111111-1111-4111-8111-111111111111', '--project',
                     '22222222-2222-4222-8222-222222222222', '--json', '--recover-only']

    def contents(self):
        return {str(path.relative_to(self.root)): hashlib.sha256(path.read_bytes()).hexdigest()
                for base in (self.state, self.config) for path in base.rglob('*') if path.is_file()}

    def refuse(self, body, existing=False):
        if existing:
            self.state.mkdir(mode=0o700)
            (self.state / 'unchanged-private-fixture').write_bytes(b'original')
        before = self.contents()
        result = subprocess.run(self.argv, input=body, env=self.env, capture_output=True, timeout=5)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b'line 1', result.stderr, 'must reach the bounded JSON input parser')
        self.assertEqual(self.contents(), before)
        self.assertEqual(self.state.exists(), existing)

    def test_empty_eof_never_opens_or_creates_a_task_store(self):
        self.refuse(b'')

    def test_partial_eof_leaves_existing_state_unchanged(self):
        self.refuse(b'{"schema":1,"request":', existing=True)

    def test_trailing_input_is_checked_before_any_task_effect(self):
        self.refuse(b'{} trailing')

    def test_real_hgs_retains_inherited_custody_through_partial_input_and_exit(self):
        lock_path = self.root / 'attempt.lock'
        original = lock_path.open('xb')
        fcntl.flock(original, fcntl.LOCK_EX | fcntl.LOCK_NB)
        helper = subprocess.Popen(self.argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.PIPE, env=self.env, pass_fds=(original.fileno(),))

        def cleanup():
            if helper.poll() is None:
                helper.kill()  # only this test's synthetic helper, never a group
            helper.communicate(timeout=5)

        self.addCleanup(cleanup)
        original.close()  # no explicit LOCK_UN: the inherited description stays held
        with lock_path.open('rb') as contender:
            with self.assertRaises(BlockingIOError):
                fcntl.flock(contender, fcntl.LOCK_EX | fcntl.LOCK_NB)
            helper.stdin.write(b'{"schema":1,')
            helper.stdin.flush()
            time.sleep(0.1)
            self.assertIsNone(helper.poll(), 'must await complete input/EOF')
            self.assertFalse(self.state.exists())
            with self.assertRaises(BlockingIOError):
                fcntl.flock(contender, fcntl.LOCK_EX | fcntl.LOCK_NB)
            output, error = helper.communicate(timeout=5)
            self.assertNotEqual(helper.returncode, 0, output)
            self.assertIn(b'EOF while parsing', error)
            self.assertFalse(self.state.exists())
            fcntl.flock(contender, fcntl.LOCK_EX | fcntl.LOCK_NB)


if __name__ == '__main__':
    unittest.main()
