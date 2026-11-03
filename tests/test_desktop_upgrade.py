"""Executable replacement through original private desktop CLI processes only."""
import json
import os
from pathlib import Path
import select
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
import unittest

HGS = Path(os.environ.get('HGS_TEST_BIN', Path(__file__).resolve().parents[1] / 'target/debug/hgs')).resolve()


@unittest.skipUnless(sys.platform.startswith(('linux', 'darwin')), 'Unix private replacement')
class Upgrade(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='desktop-upgrade-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        for name in ('config', 'state', 'tmp', 'tmux', 'bin'):
            (self.root / name).mkdir(mode=0o700)
        self.env = {k: v for k, v in os.environ.items()
                    if not k.startswith('HGS_') and k not in ('TMUX', 'TMUX_PANE')}
        self.env.update(HOME=str(self.root), USERPROFILE=str(self.root),
                        TMPDIR=str(self.root / 'tmp'), TMUX_TMPDIR=str(self.root / 'tmux'),
                        HGS_CONFIG_DIR=str(self.root / 'config'), HGS_STATE_DIR=str(self.root / 'state'),
                        HGS_SELF='example', HGS_PEERS='')
        self.selected = self.root / 'bin/hgs'
        shutil.copyfile(HGS, self.selected)
        self.selected.chmod(0o700)

    def status(self):
        result = subprocess.run([str(HGS), 'desktop', 'status', '--json'], env=self.env,
                                capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)

    def stop(self, child):
        if child.stdin is not None and not child.stdin.closed:
            child.stdin.close()
            child.stdin = None
        if child.poll() is None:
            child.terminate()  # Only this fixture's retained original Popen.
        try:
            child.communicate(timeout=8)
        except subprocess.TimeoutExpired:
            child.kill()
            child.communicate(timeout=5)
            self.fail('original private desktop child did not stop')

    def serve(self, selected):
        child = subprocess.Popen([str(selected), 'desktop', 'serve'], env=self.env,
                                 stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.addCleanup(self.stop, child)
        until = time.monotonic() + 10
        while time.monotonic() < until:
            self.assertIsNone(child.poll(), 'private service exited during startup')
            status = self.status()
            if status.get('running'):
                return child, status
            time.sleep(.02)
        self.fail('private desktop did not become ready')

    def replace(self, executable=True):
        replacement = self.root / 'bin/new-hgs'
        shutil.copyfile(HGS, replacement)
        replacement.chmod(0o700 if executable else 0o600)
        os.replace(replacement, self.selected)

    def no_operations(self):
        with sqlite3.connect(self.root / 'state/desktop/state.sqlite3') as db:
            for table in ('desktop_operations', 'desktop_operation_attempts',
                          'desktop_ownership_requests', 'desktop_commands'):
                self.assertEqual(db.execute(f'SELECT count(*) FROM {table}').fetchone()[0], 0)

    def test_service_keeps_missing_and_unready_images_then_exits_for_supervised_restart(self):
        child, original = self.serve(self.selected)
        self.selected.unlink()
        time.sleep(11)  # Two production five-second checks, not an injected fast detector.
        self.assertIsNone(child.poll())
        self.assertEqual(self.status()['epoch'], original['epoch'])
        self.replace(executable=False)
        time.sleep(11)
        self.assertIsNone(child.poll())
        self.assertEqual(self.status()['epoch'], original['epoch'])
        self.selected.chmod(0o700)
        output, error = child.communicate(timeout=13)
        self.assertEqual(output, b'')
        self.assertEqual(child.returncode, 1, error)
        self.assertIn(b'hgs_replaced', error)
        self.assertNotEqual(child.returncode, 78, 'replacement is not a permanent startup refusal')
        self.no_operations()
        replacement, current = self.serve(self.selected)
        self.assertNotEqual(current['epoch'], original['epoch'])
        self.assertFalse(current['writers_enabled'])
        self.no_operations()
        self.assertIsNone(replacement.poll())

    def test_replaced_watch_emits_a_resnapshot_without_restarting_or_granting_service(self):
        service, original = self.serve(HGS)
        watcher = subprocess.Popen([str(self.selected), 'desktop', 'watch', '--json'], env=self.env,
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   bufsize=0)
        self.addCleanup(self.stop, watcher)
        pending = bytearray()

        def frame(timeout, expected="original desktop"):

            until = time.monotonic() + timeout
            while time.monotonic() < until:
                if b'\n' in pending:
                    line, _, rest = pending.partition(b'\n')
                    pending[:] = rest
                    return json.loads(line)
                if select.select([watcher.stdout], [], [], max(0, until - time.monotonic()))[0]:
                    data = os.read(watcher.stdout.fileno(), 65536)
                    self.assertTrue(data, 'watch ended without its replacement frame')
                    pending.extend(data)
                    self.assertLessEqual(len(pending), 4 << 20)
            self.fail('no ' + expected + ' frame before deadline')

        hello = frame(5)
        self.assertEqual(hello['t'], 'hello')
        self.assertEqual(hello['epoch'], original['epoch'])
        self.replace()
        until = time.monotonic() + 13
        while True:
            observed = frame(max(0, until - time.monotonic()), 'hgs_replaced')
            if observed['t'] == 'error':
                break
        self.assertEqual(observed['value']['code'], 'hgs_replaced')
        self.assertEqual(observed['epoch'], original['epoch'])
        self.assertEqual(watcher.wait(timeout=5), 0)
        self.assertEqual(watcher.stderr.read(), b'')
        self.assertIsNone(service.poll())
        current = self.status()
        self.assertEqual(current['epoch'], original['epoch'])
        self.assertFalse(current['writers_enabled'])
        self.no_operations()
        pending.clear()
        watcher = subprocess.Popen([str(self.selected), 'desktop', 'watch', '--json'], env=self.env,
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   bufsize=0)
        self.addCleanup(self.stop, watcher)
        fresh = frame(5)
        self.assertEqual(fresh['t'], 'hello')
        self.assertEqual(fresh['epoch'], original['epoch'])
        self.assertIsNone(service.poll())
        self.no_operations()


if __name__ == '__main__':
    unittest.main()
