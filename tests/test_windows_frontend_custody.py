"""Portable controls for original-child custody in the Windows native fixture."""
import subprocess
from types import SimpleNamespace
import unittest

import windows_frontend_auth as native_fixture


class WindowsFrontendCustody(unittest.TestCase):
    def child(self, *, exited=False, timeout=False):
        events = []
        process = SimpleNamespace(returncode=78 if exited else None)
        handle = SimpleNamespace(closed=False)

        def poll():
            return process.returncode

        def terminate():
            events.append('original_terminate')

        def communicate(*, timeout):
            self.assertEqual(timeout, 8)
            events.append('original_pipe_join')
            if must_timeout:
                raise subprocess.TimeoutExpired('original fixture child', timeout)
            process.returncode = 78 if exited else 0
            return '', ''

        def close():
            self.assertIsNotNone(process.returncode)
            events.append('original_handle_close')
            handle.closed = True

        must_timeout = timeout
        process.poll, process.terminate, process.communicate = poll, terminate, communicate
        handle.Close = close
        process._handle = handle
        return process, events

    def test_already_exited_original_still_drains_pipes_and_closes_its_handle(self):
        process, events = self.child(exited=True)
        native_fixture.WindowsFrontend.stop(self, process)
        self.assertEqual(events, ['original_pipe_join', 'original_handle_close'])
        self.assertTrue(process._handle.closed)

    def test_live_original_is_joined_before_its_handle_closes(self):
        process, events = self.child()
        native_fixture.WindowsFrontend.stop(self, process)
        self.assertEqual(events, ['original_terminate', 'original_pipe_join', 'original_handle_close'])

    def test_uncertain_original_pipe_join_retains_the_original_handle(self):
        process, events = self.child(timeout=True)
        with self.assertRaises(subprocess.TimeoutExpired):
            native_fixture.WindowsFrontend.stop(self, process)
        self.assertEqual(events, ['original_terminate', 'original_pipe_join'])
        self.assertFalse(process._handle.closed)
        self.assertFalse(getattr(process, '_fixture_settled', False))

    def test_repeated_cleanup_does_not_signal_or_join_a_settled_original_again(self):
        process, events = self.child()
        native_fixture.WindowsFrontend.stop(self, process)
        before = events.copy()
        native_fixture.WindowsFrontend.stop(self, process)
        self.assertEqual(events, before)


if __name__ == '__main__':
    unittest.main()
