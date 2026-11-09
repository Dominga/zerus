"""The maintained status poll with command results, no CLI, agent or ASR work."""
import json
import os
from pathlib import Path
import subprocess
from types import SimpleNamespace
import unittest
from unittest import mock

if os.name == "posix":
    import test_voice


@unittest.skipUnless(os.name == "posix", "Unix voice fixture")
class VoiceStatusPoll(unittest.TestCase):
    def setUp(self):
        self.fixture = test_voice.VoiceWorker()
        self.fixture.hgs = Path("/synthetic/hgs")
        self.fixture.name = "codex/fixture/voice"
        self.fixture.run_id = "fixture-run"
        self.fixture.conversation_id = "fixture-conversation"
        self.fixture.env = {}
        self.fixture.worker = SimpleNamespace(poll=lambda: None)
        self.clock = 0

    def sleep(self, _seconds):
        self.clock += 1  # A scheduling turn, not a longer polling budget.
        self.assertLessEqual(self.clock, 12, "the original settling budget was renewed")

    def result(self, code=0, out=None, error=""):
        return subprocess.CompletedProcess(
            [], code, json.dumps({"message": out}) if out is not None else "", error)

    def poll(self, responses):
        with mock.patch.object(test_voice.subprocess, "run", side_effect=responses) as calls, \
                mock.patch.object(test_voice.time, "monotonic", side_effect=lambda: self.clock), \
                mock.patch.object(test_voice.time, "sleep", side_effect=self.sleep):
            result = self.fixture.settle("original-message", {"failed"})
        return result, calls

    def test_busy_status_reobserves_the_same_original_without_resubmitting(self):
        expected = {"id": "original-message", "state": "failed"}
        result, calls = self.poll([
            self.result(1, error="hgs: database is locked\n"), self.result(out=expected)])
        self.assertEqual(result, expected)
        self.assertEqual(calls.call_count, 2)
        for call in calls.call_args_list:
            payload = json.loads(call.kwargs["input"])
            self.assertEqual(payload["action"], "status")
            self.assertEqual(payload["message_id"], "original-message")
            self.assertEqual(payload["expected_run_id"], self.fixture.run_id)
            self.assertEqual(payload["expected_conversation_id"], self.fixture.conversation_id)
        self.assertEqual([call.kwargs["timeout"] for call in calls.call_args_list], [12, 11])

    def test_persistent_busy_keeps_the_original_deadline_and_fails(self):
        def busy(*_args, **_kwargs):
            self.assertLess(self.clock, 12)
            return self.result(1, error="hgs: database is locked\n")
        with self.assertRaisesRegex(AssertionError, "voice did not settle:.*status database is locked"):
            self.poll(busy)
        self.assertEqual(self.clock, 12)

    def test_other_status_errors_are_not_retried(self):
        response = self.result(1, error="hgs: Voice message was not found\n")
        with mock.patch.object(test_voice.subprocess, "run", return_value=response) as calls:
            with self.assertRaisesRegex(AssertionError, "Voice message was not found"):
                self.fixture.settle("original-message", {"failed"})
        self.assertEqual(calls.call_count, 1)

    def test_busy_submit_cancel_and_retry_are_not_status_refusals(self):
        for action in ("submit", "cancel", "retry"):
            with self.subTest(action=action), \
                    mock.patch.object(test_voice.subprocess, "run",
                                      return_value=self.result(1, error="hgs: database is locked\n")) as calls:
                with self.assertRaises(AssertionError):
                    self.fixture.voice({"action": action})
                self.assertEqual(calls.call_count, 1)

    def test_unexpected_stdout_or_exit_code_stays_a_failure(self):
        for response in (self.result(1, out={}, error="hgs: database is locked\n"),
                         self.result(2, error="hgs: database is locked\n")):
            with self.subTest(response=response), \
                    mock.patch.object(test_voice.subprocess, "run", return_value=response) as calls:
                with self.assertRaises(AssertionError):
                    self.fixture.settle("original-message", {"failed"})
                self.assertEqual(calls.call_count, 1)

    def test_timed_out_status_is_not_retried(self):
        with mock.patch.object(test_voice.subprocess, "run",
                               side_effect=subprocess.TimeoutExpired("fixture status", 12)) as calls:
            with self.assertRaises(subprocess.TimeoutExpired):
                self.fixture.settle("original-message", {"failed"})
        self.assertEqual(calls.call_count, 1)
