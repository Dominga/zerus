"""Portable call-site controls: no shell, process, transport or native agent."""
import io
import os
from pathlib import Path
import tempfile
import unittest
from unittest import mock

import test_remote_windows as remote


class ReachedReadinessWait(Exception):
    pass


class DecoderStderr(unittest.TestCase):
    def exercise(self, *, exited=False):
        with tempfile.TemporaryDirectory(prefix='decoder-stderr-') as name:
            root = Path(name)
            fake = root/'fake-agent-data'
            fake.touch()  # Only copied; Popen below is always a model.
            child = mock.Mock()
            child.stdin, child.stdout, child.stderr = io.BytesIO(), io.BytesIO(), io.BytesIO()
            child.returncode = 7 if exited else None
            child.poll.side_effect = lambda: child.returncode
            child.wait.side_effect = lambda **_: setattr(child, 'returncode', 0)
            readers = []

            class Capture:
                def __init__(self, *, target, args):
                    self.pipe, self.path = args
                    readers.append(self)

                def start(self):
                    # Hold capture-file publication until after the first live
                    # poll. The exit case has a real diagnostic to preserve.
                    if exited:
                        self.path.write_bytes(b'original helper exited' if self.path.name == 'stderr' else b'')

                def join(self, **_):
                    self.pipe.close()

            def readiness_turn(_seconds):
                self.assertFalse(readers[1].path.exists())
                # End this control before any body/EOF is sent. The original
                # finally sees an exited model and must not use its fallback.
                child.returncode = 0
                raise ReachedReadinessWait

            fixture = remote.WindowsNativeTransport()
            with mock.patch.dict(os.environ, {'HGS_TEST_FAKE_AGENT': str(fake), 'SystemRoot': str(root)}), \
                    mock.patch.object(remote.subprocess, 'Popen', return_value=child) as launch, \
                    mock.patch.object(remote.subprocess, 'CREATE_NO_WINDOW', 0, create=True), \
                    mock.patch('threading.Thread', Capture), \
                    mock.patch.object(remote.time, 'sleep', side_effect=readiness_turn):
                try:
                    if exited:
                        with self.assertRaisesRegex(AssertionError, 'original helper exited'):
                            fixture.run_decoder('powershell')
                    else:
                        try:
                            fixture.run_decoder('powershell')
                        except ReachedReadinessWait:
                            pass
                        except (FileNotFoundError, AssertionError) as error:
                            self.fail('live-child observation eagerly read an unpublished stderr file: ' + str(error))
                        else:
                            self.fail('the maintained readiness wait was not reached')
                    self.assertEqual(launch.call_count, 1)
                    self.assertEqual(child.poll.call_count, 2)  # One observation and original finally.
                    child.wait.assert_not_called()
                finally:
                    for pipe in (child.stdin, child.stdout, child.stderr):
                        pipe.close()

    def test_live_original_does_not_read_stderr_before_capture_publishes_it(self):
        self.exercise()

    def test_original_exit_still_fails_with_its_captured_stderr(self):
        self.exercise(exited=True)
