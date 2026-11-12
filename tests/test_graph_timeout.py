"""Bounded failure observations through the maintained graph fixture call site."""
import contextlib
import io
import json
import os
from pathlib import Path
import select
import subprocess
import sys
import tempfile
import threading
import time
from types import SimpleNamespace
import unittest
from unittest import mock

import graph_timeout as diagnostic
import test_session_graph as graph


class GraphTimeout(unittest.TestCase):
    def child(self, error=None):
        child = mock.MagicMock()
        child.__enter__.return_value = child
        child.returncode = 0
        child.communicate.side_effect = error
        return child

    def test_success_preserves_original_arguments_capture_and_result(self):
        child = self.child()
        child.communicate.return_value = ('out', 'err')
        with mock.patch.object(diagnostic.subprocess, 'Popen', return_value=child) as launch, \
                mock.patch.object(diagnostic, 'snapshot') as snapshot:
            result = diagnostic.run(['hgs', 'graph'], env={'SYNTHETIC': '1'}, timeout=5)
        launch.assert_called_once_with(['hgs', 'graph'], env={'SYNTHETIC': '1'}, text=True,
                                       stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        child.communicate.assert_called_once_with(timeout=5)
        snapshot.assert_not_called()
        child.kill.assert_not_called()
        self.assertEqual((result.args, result.returncode, result.stdout, result.stderr),
                         (['hgs', 'graph'], 0, 'out', 'err'))

    def test_snapshot_failure_keeps_the_same_timeout_and_one_original_cleanup(self):
        error = subprocess.TimeoutExpired(['original'], 5, output=b'out', stderr=b'err')
        child = self.child(error)
        # subprocess.run has an extra communicate after kill on Windows.
        if os.name == 'nt':
            child.communicate.side_effect = [error, ('out', 'err')]
        log = io.StringIO()
        with mock.patch.object(diagnostic.subprocess, 'Popen', return_value=child) as launch, \
                mock.patch.object(diagnostic, 'snapshot', side_effect=OSError('fixture')), \
                contextlib.redirect_stderr(log), self.assertRaises(subprocess.TimeoutExpired) as caught:
            diagnostic.run(['original'], env={}, timeout=5)
        self.assertIs(caught.exception, error)
        self.assertEqual(launch.call_count, 1)
        child.kill.assert_called_once_with()
        self.assertIn('diagnostic failed', log.getvalue())

    def test_tails_and_emitted_frame_are_bounded(self):
        self.assertEqual(diagnostic._tail(b'first\nlast\n'), 'first\nlast')
        tail = diagnostic._tail(('x' * 100 + '\n') * 100)
        self.assertLessEqual(len(tail), diagnostic.TAIL_BYTES)
        self.assertLessEqual(len(tail.splitlines()), diagnostic.TAIL_LINES)
        self.assertLessEqual(len(diagnostic._tail('🦊' * 10000).encode()), diagnostic.TAIL_BYTES)
        child = self.child(subprocess.TimeoutExpired(['fixture'], 5))
        if os.name == 'nt':
            child.communicate.side_effect = [subprocess.TimeoutExpired(['fixture'], 5), ('', '')]
        report = {'native': {'oversized': 'x' * 100000}, 'tails': {'stdout': 'last'}}
        log = io.StringIO()
        with mock.patch.object(diagnostic.subprocess, 'Popen', return_value=child), \
                mock.patch.object(diagnostic, 'snapshot', return_value=report), \
                contextlib.redirect_stderr(log), self.assertRaises(subprocess.TimeoutExpired):
            diagnostic.run(['fixture'], env={}, timeout=5)
        self.assertLess(len(log.getvalue()), diagnostic.MAX_FRAME)
        self.assertEqual(json.loads(log.getvalue().split(' ', 1)[1])['tails']['stdout'], 'last')

    def test_unwritable_diagnostic_log_cannot_mask_timeout_or_skip_cleanup(self):
        error = subprocess.TimeoutExpired(['original'], 5)
        child = self.child(error)
        if os.name == 'nt':
            child.communicate.side_effect = [error, ('', '')]
        log = mock.Mock()
        log.write.side_effect = OSError('private log unavailable')
        with mock.patch.object(diagnostic.subprocess, 'Popen', return_value=child), \
                mock.patch.object(diagnostic, 'snapshot', return_value={}), \
                contextlib.redirect_stderr(log), self.assertRaises(subprocess.TimeoutExpired) as caught:
            diagnostic.run(['original'], env={}, timeout=5)
        self.assertIs(caught.exception, error)
        child.kill.assert_called_once_with()

    def test_late_exit_during_snapshot_never_turns_timeout_into_success(self):
        error = subprocess.TimeoutExpired(['original'], 5)
        child = self.child(error)
        if os.name == 'nt':
            child.communicate.side_effect = [error, ('', '')]

        def exited(*_args):
            child.returncode = 0
            return {'native': {'unavailable': 'original child exited'}}

        with mock.patch.object(diagnostic.subprocess, 'Popen', return_value=child) as launch, \
                mock.patch.object(diagnostic, 'snapshot', side_effect=exited), \
                contextlib.redirect_stderr(io.StringIO()), self.assertRaises(subprocess.TimeoutExpired) as caught:
            diagnostic.run(['original'], env={}, timeout=5)
        self.assertIs(caught.exception, error)
        self.assertEqual(launch.call_count, 1)

    def test_unsupported_native_observer_preserves_captured_tails_without_pid_lookup(self):
        with mock.patch.object(diagnostic.sys, 'platform', 'unsupported'):
            report = diagnostic.snapshot(SimpleNamespace(),
                                         subprocess.TimeoutExpired(['fixture'], 5, b'last-out', b'last-err'))
        self.assertEqual(report['tails'], {'stdout': 'last-out', 'stderr': 'last-err'})
        self.assertIn('unavailable', report['native'])

    def test_native_read_limit_refuses_without_leaking_the_open_descriptor(self):
        if os.name != 'posix':
            self.skipTest('Unix native observer read primitive')
        with tempfile.TemporaryDirectory(prefix='graph-read-limit-') as name:
            path = Path(name) / 'private'
            path.write_bytes(b'x' * (diagnostic.MAX_READ + 1))
            close = os.close
            with mock.patch.object(diagnostic.os, 'close', wraps=close) as closed:
                with self.assertRaisesRegex(ValueError, 'native read limit'):
                    diagnostic._read(path)
            closed.assert_called_once()
            with self.assertRaises(OSError):
                os.fstat(closed.call_args.args[0])


@unittest.skipUnless(sys.platform.startswith('linux') and hasattr(os, 'pidfd_open'),
                     'Linux native own-custody observer')
class NativeGraphTimeout(unittest.TestCase):
    def test_maintained_graph_reports_only_original_custody_before_timeout_cleanup(self):
        with tempfile.TemporaryDirectory(prefix='graph-timeout-') as name:
            root = Path(name)
            fake = root / 'fake'
            fake.write_text('#!' + sys.executable + '\n' + '''
import os, pathlib, subprocess, sys, time
root=pathlib.Path(__file__).parent
child=subprocess.Popen([sys.executable, '-c',
    "import os,pathlib,sys; print('child-out',flush=True); print('child-err',file=sys.stderr,flush=True); pid=pathlib.Path(sys.argv[1]); pending=pid.with_suffix('.tmp'); pending.write_text(str(os.getpid())); pending.replace(pid); sys.stdin.read()", str(root/'child')], stdin=subprocess.PIPE)
while not (root/'child').exists(): time.sleep(.001)
print('root-out',flush=True); print('root-err',file=sys.stderr,flush=True)
(root/'ready').touch()
time.sleep(60)
''')
            fake.chmod(0o700)
            fixture = graph.SessionGraph()
            fixture.env = {k: v for k, v in os.environ.items()
                           if not k.startswith(('HGS_', 'ZERUS_', 'TMUX'))}
            fixture.marker = root / 'read-marks'
            fixture.marker.write_bytes(b'{"fixture":"untouched"}\n')
            popen = subprocess.Popen
            originals = []
            descendant_pins = []

            def start(*args, **kwargs):
                child = popen(*args, **kwargs)
                originals.append(child)
                deadline = time.monotonic() + 3
                while not (root/'ready').exists():
                    if child.poll() is not None or time.monotonic() >= deadline:
                        raise AssertionError('private fake did not become ready')
                    time.sleep(.001)
                descendant_pins.append(os.pidfd_open(int((root/'child').read_text())))
                return child

            with popen([sys.executable, '-c', 'import sys; sys.stdin.read()'],
                       stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL) as outside:
                log = io.StringIO()
                try:
                    with mock.patch.object(graph, 'HGS', fake), \
                            mock.patch.object(diagnostic.subprocess, 'Popen', side_effect=start), \
                            contextlib.redirect_stderr(log), self.assertRaises(subprocess.TimeoutExpired):
                        fixture.graph('snapshot', timeout=.03)
                    self.assertEqual(len(originals), 1)
                    self.assertIsNotNone(originals[0].returncode)
                    self.assertTrue(log.getvalue().startswith('GRAPH_TIMEOUT '),
                                    'the maintained graph call site lost its pre-cleanup timeout snapshot')
                    report = json.loads(log.getvalue().split(' ', 1)[1])
                    rows = {p['pid']: p for p in report['native']['processes']}
                    child_pid = int((root/'child').read_text())
                    self.assertEqual(set(rows), {originals[0].pid, child_pid})
                    self.assertNotIn(outside.pid, rows)
                    self.assertIsNone(outside.poll())
                    for row in rows.values():
                        self.assertGreater(row['birth'], 0)
                        self.assertRegex(row['state'], r'^[A-Za-z]$')
                        self.assertGreaterEqual(row['cpu_ticks'], 0)
                        self.assertTrue(row['pipes'])
                        self.assertEqual(row['output_tails'], {'1': 'stdout', '2': 'stderr'})
                    for stream in ('stdout', 'stderr'):
                        self.assertIn('root-' + ('out' if stream == 'stdout' else 'err'), report['tails'][stream])
                        self.assertIn('child-' + ('out' if stream == 'stdout' else 'err'), report['tails'][stream])
                    poll = select.poll()
                    poll.register(descendant_pins[0], select.POLLIN)
                    self.assertTrue(poll.poll(3000), 'the original root pipe did not release its private child')
                finally:
                    # Only original Popen custody; never adopt a PID to clean up.
                    for child in originals:
                        if child.poll() is None:
                            child.kill()
                        child.wait()
                    for pin in descendant_pins:
                        os.close(pin)
                    outside.communicate('')

    def test_reaped_original_is_not_adopted_by_numeric_pid(self):
        child = SimpleNamespace(pid=os.getpid(), returncode=0)
        with mock.patch.object(os, 'pidfd_open') as pin:
            report = diagnostic.snapshot(child, subprocess.TimeoutExpired(['fixture'], 5))
        pin.assert_not_called()
        self.assertIn('already reaped', report['native']['unavailable'])

    def test_stalled_observer_is_bounded_and_keeps_its_pin_until_actual_release(self):
        release, done = threading.Event(), threading.Event()
        pins = []

        def stalled(_pid, pin, _deadline, _outputs):
            pins.append(pin)
            try:
                release.wait(3)
                return {'unavailable': 'fixture released'}
            finally:
                os.close(pin)
                done.set()

        with subprocess.Popen([sys.executable, '-c', 'import sys; sys.stdin.read()'],
                              stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE) as child:
            try:
                start = time.monotonic()
                with mock.patch.object(diagnostic, '_native', side_effect=stalled):
                    report = diagnostic.snapshot(child, subprocess.TimeoutExpired(['fixture'], 5))
                self.assertIn('deadline', report['native']['unavailable'])
                self.assertLess(time.monotonic() - start, 1)
                self.assertFalse(done.is_set())
                os.fstat(pins[0])
            finally:
                release.set()
                self.assertTrue(done.wait(3))
                child.communicate('')
            with self.assertRaises(OSError):
                os.fstat(pins[0])
