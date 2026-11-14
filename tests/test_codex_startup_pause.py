"""Portable checks for the native fixture's original-bound pause retry policy."""
import json
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

    def response(self, status='completed', reason=None, *, code=0, name=None, stdout=None, changes=None):
        def command(argv, **kwargs):
            request = json.loads(kwargs['input'])
            receipt = dict(request_id=request['request_id'], name=name or self.fixture.name,
                           run_id=request['expected_run_id'], conversation_id=request['expected_conversation_id'],
                           status=status)
            if reason:
                receipt['error'] = f'{name or self.fixture.name}: {reason}'
            receipt.update(changes or {})
            return subprocess.CompletedProcess(argv, code, json.dumps(receipt) if stdout is None else stdout, '')
        return command

    def test_only_the_two_known_zero_effect_refusals_retry_then_really_pause(self):
        reasons = ('activity changed while checking; retry pause',
                   'agent is busy or unconfirmed; wait for its answer before pausing')
        replies = iter([self.response('failed', reason) for reason in reasons] + [self.response()])
        with mock.patch.object(native.subprocess, 'run', side_effect=lambda *a, **kw: next(replies)(*a, **kw)) as run:
            self.assertEqual(json.loads(self.fixture.pause_settled_original())['status'], 'completed')
        self.assertEqual(run.call_count, 3)
        requests = []
        for call in run.call_args_list:
            self.assertEqual(call.args[0], [str(native.test_pause.HGS), 'session-action', self.fixture.name, '--json'])
            self.assertEqual(call.kwargs['env'], self.fixture.env)
            requests.append(json.loads(call.kwargs['input']))
        self.assertEqual(len({request['request_id'] for request in requests}), 3,
                         'Only a proved zero-effect refusal allows another native attempt')
        self.assertEqual([call.kwargs['timeout'] for call in run.call_args_list], [20, 19.9, 19.8])

    def test_possible_effect_other_target_output_and_other_exit_status_never_retry(self):
        transient = 'activity changed while checking; retry pause'
        for command in [self.response('failed', 'session changed while pausing'),
                        self.response('failed', 'tracked process did not exit after SIGTERM'),
                        self.response('failed', transient, name='codex/p/another'),
                        self.response('failed', transient, stdout='an action already happened'),
                        self.response('failed', transient, code=2),
                        self.response('uncertain', transient),
                        self.response('claimed', transient)]:
            with self.subTest(command=command), mock.patch.object(native.subprocess, 'run', side_effect=command) as run:
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
                def refuse(*args, **kwargs):
                    self.record[field] = 'replacement'
                    return self.response('failed', 'activity changed while checking; retry pause')(*args, **kwargs)
                with mock.patch.object(native.subprocess, 'run', side_effect=refuse) as run:
                    with self.assertRaisesRegex(AssertionError, 'original changed'):
                        self.fixture.pause_settled_original()
                    self.assertEqual(run.call_count, 1)
                self.record[field] = old

    def test_pause_command_is_scoped_to_the_captured_run_and_conversation(self):
        with mock.patch.object(native.subprocess, 'run', side_effect=self.response()) as run:
            self.fixture.pause_settled_original()
        call = run.call_args
        self.assertEqual(call.args[0], [str(native.test_pause.HGS), 'session-action',
                                      self.fixture.name, '--json'])
        request = json.loads(call.kwargs['input'])
        self.assertEqual(request['action'], 'pause')
        self.assertEqual(request['expected_run_id'], self.record['run_id'])
        self.assertEqual(request['expected_conversation_id'], self.record['conversation_id'])

    def test_success_rechecks_every_original_identity_field_without_retrying(self):
        for field in self.record:
            with self.subTest(field=field):
                old = self.record[field]
                def succeed(*args, **kwargs):
                    self.record[field] = 'replacement'
                    return self.response()(*args, **kwargs)
                with mock.patch.object(native.subprocess, 'run', side_effect=succeed) as run:
                    with self.assertRaisesRegex(AssertionError, 'original changed'):
                        self.fixture.pause_settled_original()
                    self.assertEqual(run.call_count, 1)
                self.record[field] = old

    def test_paused_exit_metadata_keeps_the_same_original_identity(self):
        def succeed(*args, **kwargs):
            self.record.update(paused=True, completion_reason='pause', activity='idle', exited_at=123)
            return self.response()(*args, **kwargs)
        with mock.patch.object(native.subprocess, 'run', side_effect=succeed) as run:
            self.assertEqual(json.loads(self.fixture.pause_settled_original())['status'], 'completed')
        self.assertEqual(run.call_count, 1)

    def test_completed_receipt_must_name_this_request_and_original(self):
        for field in ('request_id', 'name', 'run_id', 'conversation_id'):
            with self.subTest(field=field), mock.patch.object(native.subprocess, 'run', side_effect=
                                                            self.response(changes={field: 'replacement'})) as run:
                with self.assertRaisesRegex(AssertionError, 'receipt'):
                    self.fixture.pause_settled_original()
                self.assertEqual(run.call_count, 1)

    def test_persistent_busy_uses_one_original_budget_and_never_returns_success(self):
        reply = self.response('failed', 'agent is busy or unconfirmed; wait for its answer before pausing')
        with mock.patch.object(native.subprocess, 'run', side_effect=reply) as run:
            with self.assertRaisesRegex(AssertionError, 'within 20 s'):
                self.fixture.pause_settled_original()
        self.assertLessEqual(run.call_count, 201)
        self.assertLessEqual(self.clock, 20)
        self.assertGreater(self.clock, 19.9)


if __name__ == '__main__':
    unittest.main()
