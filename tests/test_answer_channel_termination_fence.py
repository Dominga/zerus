#!/usr/bin/env python3
"""Claim admission against private synthetic Terminate markers, not grace.

The Linux control observes this worker's actual flock wait. It never guesses
that a sleep placed the worker between its record read and the claim rename.
"""
import contextlib
import hashlib
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import time
import unittest
import uuid

import test_answer_channel as channel_fixture

QUESTIONS = channel_fixture.QUESTIONS
from test_input import HGS

if sys.platform == 'linux':
    import fcntl


@unittest.skipUnless(sys.platform == 'linux', 'Linux private flock interleaving control')
class AnswerChannelTerminationFence(unittest.TestCase):
    NAME = channel_fixture.AnswerChannelConversation.NAME
    RUN = channel_fixture.AnswerChannelConversation.RUN
    CONVERSATION = channel_fixture.AnswerChannelConversation.CONVERSATION

    def until(self, condition, message):
        deadline = time.monotonic() + 8
        while not condition():
            self.assertLess(time.monotonic(), deadline, message)
            time.sleep(.01)

    def kernel_lock(self, worker, lock, blocked):
        """Only this worker's held lock or waiter on this exact private inode."""
        info = os.fstat(lock.fileno())
        for line in Path('/proc/locks').read_text().splitlines():
            parts = line.split()
            if ('->' in parts) != blocked or 'FLOCK' not in parts:
                continue
            offset = parts.index('FLOCK')
            if parts[offset + 3] != str(worker.pid):
                continue
            major, minor, inode = parts[offset + 4].split(':')
            if (int(major, 16), int(minor, 16), int(inode)) == (
                    os.major(info.st_dev), os.minor(info.st_dev), info.st_ino):
                return True
        return False

    @staticmethod
    def publish(path, value):
        temporary = path.with_suffix('.fixture')
        temporary.write_text(json.dumps(value))
        temporary.chmod(0o600)
        temporary.replace(path)

    @contextlib.contextmanager
    def channel(self):
        with tempfile.TemporaryDirectory(prefix='hgs-channel-fence-') as temporary:
            root = Path(temporary)
            state = root / 'state'
            state.mkdir(mode=0o700)
            record = dict(version=1, name=self.NAME, agent='claude', run_id=self.RUN,
                          conversation_id=self.CONVERSATION, pid=os.getpid(),
                          process_start=channel_fixture.caller_start(),
                          pending_questions=dict(toolu_1=channel_fixture.AnswerChannelConversation().card()))
            path = state / (hashlib.sha256(self.NAME.encode()).hexdigest() + '.json')
            self.publish(path, record)
            (state / '.lock').touch(mode=0o600)
            event = dict(hook_event_name='PermissionRequest', tool_name='AskUserQuestion',
                         session_id=self.CONVERSATION, tool_input=dict(questions=QUESTIONS))
            env = {key: value for key, value in os.environ.items()
                   if not key.startswith('HGS_') and key not in ('TMUX', 'TMUX_PANE')}
            env.update(HGS_STATE_DIR=str(state), HGS_CONFIG_DIR=str(root / 'config'),
                       HGS_SESSION=self.NAME, HGS_RUN_ID=self.RUN, HGS_ANSWER_CHANNEL_SECONDS='15')
            binary = root / 'bin'
            binary.mkdir()
            tmux = binary / 'tmux'
            tmux.write_text('#!/bin/sh\nprintf call >> ' + shlex.quote(str(root / 'native-helpers'))
                            + '\nexit 93\n')
            tmux.chmod(0o700)
            env['PATH'] = str(binary) + os.pathsep + env['PATH']
            command = [str(HGS), '__state', 'answer-channel']
            worker = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                      stderr=subprocess.PIPE, text=True, env=env)
            try:
                worker.stdin.write(json.dumps(event))
                worker.stdin.close()
                worker.stdin = None
                self.until(lambda: list(state.glob('native_answers/*/*/waiting.json')),
                           'private answer channel did not bind')
                directory = next(state.glob('native_answers/*/*/waiting.json')).parent
                offer = dict(request_id=str(uuid.uuid4()), answers={QUESTIONS[0]['question']: 'Preview'})
                yield worker, directory, path, offer, command, env, event
            finally:
                if worker.poll() is None:
                    worker.kill()  # This fixture owns only this hook child.
                worker.communicate(timeout=5)

    def finished(self, worker):
        stdout, stderr = worker.communicate(timeout=10)
        self.assertEqual((worker.returncode, stderr), (0, ''))
        return stdout

    def test_marker_arriving_behind_handover_leaves_offer_unclaimed(self):
        for marker in (None, False, 'damaged', {'request_id': str(uuid.uuid4())}):
            with self.subTest(marker=marker), self.channel() as fixture:
                worker, directory, path, offer, *_ = fixture
                with (directory / 'handover.lock').open('a+b') as handover:
                    fcntl.flock(handover, fcntl.LOCK_EX)
                    self.publish(directory / 'answer.json', offer)
                    self.until(lambda: self.kernel_lock(worker, handover, blocked=True),
                               'worker never reached the held channel handover')
                    with (path.parent / '.lock').open('r+b') as writer:
                        fcntl.flock(writer, fcntl.LOCK_EX | fcntl.LOCK_NB)
                        self.publish(path, dict(json.loads(path.read_text()), terminating=marker))
                self.assertEqual(self.finished(worker), '')
                self.assertFalse((directory / 'claimed.json').exists(),
                                 'a late marker must refuse before the claim rename')
                self.assertEqual(json.loads((directory / 'answer.json').read_text()), offer)
                self.assertFalse((directory / 'emitted.json').exists())
                self.assertEqual(json.loads((directory / 'closed.json').read_text())['reason'],
                                 'session terminating')

    def test_busy_writer_leaves_offer_unclaimed_then_reads_the_marker(self):
        with self.channel() as fixture:
            worker, directory, path, offer, *_ = fixture
            with (path.parent / '.lock').open('r+b') as writer:
                fcntl.flock(writer, fcntl.LOCK_EX)
                self.publish(directory / 'answer.json', offer)
                self.until(lambda: (directory / 'handover.lock').exists(),
                           'worker never attempted claim admission')
                with (directory / 'handover.lock').open('r+b') as handover:
                    # Observe two real admission attempts release handover
                    # while the writer remains busy; no guessed poll delay.
                    for _ in range(2):
                        self.until(lambda: self.kernel_lock(worker, handover, blocked=False),
                                   'worker never held handover for bounded writer admission')
                        self.until(lambda: not self.kernel_lock(worker, handover, blocked=False),
                                   'busy writer admission did not release handover')
                        self.assertIsNone(worker.poll(), 'busy admission must keep waiting')
                        self.assertFalse((directory / 'claimed.json').exists())
                        self.assertEqual(json.loads((directory / 'answer.json').read_text()), offer)
                self.publish(path, dict(json.loads(path.read_text()), terminating=None))
            self.assertEqual(self.finished(worker), '')
            self.assertFalse((directory / 'claimed.json').exists())
            self.assertFalse((directory / 'emitted.json').exists())

    def test_unmarked_offer_emits_once_and_original_claim_is_never_replayed(self):
        with self.channel() as fixture:
            worker, directory, path, offer, command, env, event = fixture
            self.publish(directory / 'answer.json', offer)
            output = json.loads(self.finished(worker))
            decision = output['hookSpecificOutput']['decision']
            self.assertEqual(decision['behavior'], 'allow')
            self.assertEqual(decision['updatedInput']['answers'], offer['answers'])
            self.assertEqual(json.loads((directory / 'claimed.json').read_text()), offer)
            before = {name: (directory / name).read_bytes()
                      for name in ('claimed.json', 'emitted.json', 'closed.json')}
            again = subprocess.run(command, input=json.dumps(event), env=env, text=True,
                                   capture_output=True, timeout=10)
            self.assertEqual((again.returncode, again.stdout, again.stderr), (0, '', ''))
            self.assertEqual({name: (directory / name).read_bytes() for name in before}, before)

    def test_pending_rename_refuses_only_its_session_without_running_recovery(self):
        for related in (True, False):
            with self.subTest(related=related), self.channel() as fixture:
                worker, directory, path, offer, *_ = fixture
                record = json.loads(path.read_text())
                intent = dict(version=1, old=self.NAME, new='claude/project/renamed',
                              session_id='$999', record=record if related else
                              dict(record, run_id='another-original-run', session_id=str(uuid.uuid4())))
                renames = path.parent / 'renames'
                renames.mkdir(mode=0o700)
                saved = renames / (str(uuid.uuid4()) + '.json')
                self.publish(saved, intent)
                before = saved.read_bytes(), path.read_bytes()
                self.publish(directory / 'answer.json', offer)
                stdout = self.finished(worker)
                self.assertFalse((path.parent.parent / 'native-helpers').exists(),
                                 'claim admission must never run native rename recovery')
                self.assertEqual((saved.read_bytes(), path.read_bytes()), before)
                if related:
                    self.assertEqual(stdout, '')
                    self.assertFalse((directory / 'claimed.json').exists())
                    self.assertEqual(json.loads((directory / 'answer.json').read_text()), offer)
                else:
                    self.assertEqual(json.loads(stdout)['hookSpecificOutput']['decision']['behavior'],
                                     'allow', 'a different run using an old name is unrelated')

    def test_missing_linked_or_special_writer_never_creates_or_waits(self):
        for kind in ('missing', 'link', 'fifo'):
            with self.subTest(kind=kind), self.channel() as fixture:
                worker, directory, path, offer, *_ = fixture
                lock = path.parent / '.lock'
                lock.unlink()
                if kind == 'link': lock.symlink_to(path.parent / 'not-a-lock')
                elif kind == 'fifo': os.mkfifo(lock, 0o600)
                self.publish(directory / 'answer.json', offer)
                self.assertEqual(self.finished(worker), '')
                self.assertFalse((directory / 'claimed.json').exists())
                self.assertFalse((path.parent / 'not-a-lock').exists())
                if kind == 'missing': self.assertFalse(lock.exists())


if __name__ == '__main__':
    unittest.main()
