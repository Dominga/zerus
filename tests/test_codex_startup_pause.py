"""Portable checks for the native fixture's original-bound pause retry policy."""
import subprocess
import unittest
from unittest import mock
from types import SimpleNamespace

import test_codex_input_native as native


class PauseReadiness(unittest.TestCase):
    def setUp(self):
        self.fixture = native.NativeCodexStartup('test_fresh_native_launch_reaches_its_first_turn_through_the_real_hooks')
        self.fixture.name, self.fixture.env = 'codex/p/startup', {'fixture': 'private'}
        self.record = dict(run_id='run-original', conversation_id='conversation-original',
                           pid=123, process_start='original-birth')
        self.fixture.binding = mock.Mock(side_effect=lambda _: self.record.copy())
        self.clock = 0
        clock = SimpleNamespace(monotonic=lambda: self.clock, sleep=self.advance)
        self.time = mock.patch.object(native, 'time', clock)
        self.time.start()
        self.addCleanup(self.time.stop)

    def advance(self, delay):
        self.clock += delay

    def refusal(self, reason, *, code=1, name=None, stdout=''):
        return subprocess.CompletedProcess([], code, stdout, f'hgs: {name or self.fixture.name}: {reason}\n')

    def test_only_the_two_known_zero_effect_refusals_retry_then_really_pause(self):
        reasons = ('activity changed while checking; retry pause',
                   'agent is busy or unconfirmed; wait for its answer before pausing')
        replies = [self.refusal(reason) for reason in reasons] + [subprocess.CompletedProcess([], 0, 'paused', '')]
        with mock.patch.object(native.subprocess, 'run', side_effect=replies) as run:
            self.assertEqual(self.fixture.pause_settled_original(), 'paused')
        self.assertEqual(run.call_count, 3)
        for call in run.call_args_list:
            self.assertEqual(call.args[0], [str(native.test_pause.HGS), 'pause', self.fixture.name])
            self.assertEqual(call.kwargs['env'], self.fixture.env)
        self.assertEqual([call.kwargs['timeout'] for call in run.call_args_list], [20, 19.9, 19.8])

    def test_possible_effect_other_target_output_and_other_exit_status_never_retry(self):
        transient = 'activity changed while checking; retry pause'
        for reply in [self.refusal('session changed while pausing'),
                      self.refusal('tracked process did not exit after SIGTERM'),
                      self.refusal(transient, name='codex/p/another'),
                      self.refusal(transient, stdout='an action already happened'),
                      self.refusal(transient, code=2)]:
            with self.subTest(reply=reply), mock.patch.object(native.subprocess, 'run', return_value=reply) as run:
                with self.assertRaises(AssertionError): self.fixture.pause_settled_original()
                self.assertEqual(run.call_count, 1)

    def test_an_uncertain_command_timeout_never_retries(self):
        with mock.patch.object(native.subprocess, 'run', side_effect=subprocess.TimeoutExpired('pause', 20)) as run:
            with self.assertRaises(subprocess.TimeoutExpired): self.fixture.pause_settled_original()
        self.assertEqual(run.call_count, 1)

    def test_each_original_identity_field_fences_adoption_between_attempts(self):
        for field in self.record:
            with self.subTest(field=field):
                old = self.record[field]
                def refuse(*_args, **_kwargs):
                    self.record[field] = 'replacement'
                    return self.refusal('activity changed while checking; retry pause')
                with mock.patch.object(native.subprocess, 'run', side_effect=refuse) as run:
                    with self.assertRaisesRegex(AssertionError, 'original changed'):
                        self.fixture.pause_settled_original()
                    self.assertEqual(run.call_count, 1)
                self.record[field] = old

    def test_persistent_busy_uses_one_original_budget_and_never_returns_success(self):
        reply = self.refusal('agent is busy or unconfirmed; wait for its answer before pausing')
        with mock.patch.object(native.subprocess, 'run', return_value=reply) as run:
            with self.assertRaisesRegex(AssertionError, 'within 20 s'):
                self.fixture.pause_settled_original()
        self.assertLessEqual(run.call_count, 201)
        self.assertLessEqual(self.clock, 20)
        self.assertGreater(self.clock, 19.9)


if __name__ == '__main__':
    unittest.main()
