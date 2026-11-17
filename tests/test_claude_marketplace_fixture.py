"""The common private Claude fixture must not leave a marketplace writer."""
from contextlib import contextmanager
import errno
import os
from pathlib import Path
import threading
import unittest
from unittest.mock import Mock, patch

import test_claude_questions_native as native
import test_input as fixtures

FLAG = 'CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL'


class PrivateMarketplaceFixture(unittest.TestCase):
    @contextmanager
    def launch(self):
        case = native.NativeClaudeQuestions()
        captured = {}
        class InterceptedLaunch(Exception):
            pass
        def tmux(*args, **kwargs):
            if args[0] == 'new-session':
                captured['args'] = args
                raise InterceptedLaunch
            return ''
        case.tmux = Mock(side_effect=tmux)
        try:
            with patch.object(fixtures, 'TMUX', 'fixture-tmux'), patch.object(native, 'CLAUDE', 'fixture-claude'):
                with self.assertRaises(InterceptedLaunch):
                    native.NativeClaudeQuestions.setUp(case)
            args = captured['args']
            start = args.index('env')
            self.assertEqual(args[start + 1], '-i')
            env = dict(arg.split('=', 1) for arg in args[start + 2:args.index('fixture-claude')])
            yield case, env
        finally:
            self.assertTrue(case.doCleanups(), 'private fixture cleanup must succeed')

    def test_actual_native_launch_disables_the_unrelated_official_clone(self):
        with self.launch() as (case, env):
            self.assertEqual(env.get(FLAG), '1')
            self.assertEqual(Path(env['HOME']), case.root)
            self.assertEqual(Path(env['CLAUDE_CONFIG_DIR']), case.root / '.claude')

    def test_a_late_clone_write_cannot_make_private_home_cleanup_nonempty(self):
        with self.launch() as (case, env):
            ready, finished = threading.Event(), threading.Event()
            writer = None
            original = os.rmdir
            clone = case.root / '.claude/plugins/marketplaces/anthropics-claude-plugins-official..clone/.git'
            if env.get(FLAG) != '1':
                clone.mkdir(parents=True)
                identity = clone.stat()
                def write_late():
                    if ready.wait(3):
                        (clone / 'late-pack').write_bytes(b'synthetic marketplace writer')
                    finished.set()
                writer = threading.Thread(target=write_late)
                writer.start()
            def remove(path, *, dir_fd=None):
                if writer is not None and not ready.is_set():
                    current = os.stat(path, dir_fd=dir_fd, follow_symlinks=False)
                    if (current.st_dev, current.st_ino) == (identity.st_dev, identity.st_ino):
                        ready.set()
                        self.assertTrue(finished.wait(2), 'private writer did not reach its gate')
                return original(path, dir_fd=dir_fd)
            try:
                # Let a controlled private clone write after rmtree's empty
                # scan, exactly before its rmdir. No sleeps or real agent.
                with patch.object(os, 'rmdir', side_effect=remove):
                    fixtures.InputTransport.tearDown(case)
            finally:
                ready.set()
                if writer is not None:
                    writer.join(timeout=3)
                    self.assertFalse(writer.is_alive())

    def test_other_cleanup_errors_still_propagate(self):
        with self.launch() as (case, _):
            denied = PermissionError(errno.EACCES, 'synthetic private cleanup refusal')
            with patch.object(case.temp, 'cleanup', side_effect=denied):
                with self.assertRaises(PermissionError) as error:
                    fixtures.InputTransport.tearDown(case)
            self.assertIs(error.exception, denied)
