"""Force the existing native pause-control interposer through scoped receipts."""
import json
import subprocess
import unittest
from unittest.mock import patch
import test_codex_input_native as native


class ScopedPauseFixture(unittest.TestCase):
    def test_injected_preflight_refusals_recheck_each_original_before_one_real_pause(self):
        probe = unittest.TestCase()
        probe.name = 'codex/fixture/pause'
        probe.env = {}
        current, physical = {}, []
        observations = []
        def binding(name):
            self.assertEqual(name, probe.name)
            observations.append(current['run_id'])
            return dict(current)
        probe.binding = binding
        probe.pause_settled_original = lambda: native.NativeCodexStartup.pause_settled_original(probe)
        def actual(argv, **kwargs):
            self.assertEqual(argv, [str(native.test_pause.HGS), 'session-action', probe.name, '--json'])
            request = json.loads(kwargs['input'])
            self.assertEqual(request['action'], 'pause')
            self.assertEqual(request['expected_run_id'], current['run_id'])
            self.assertEqual(request['expected_conversation_id'], current['conversation_id'])
            physical.append(current['run_id'])
            result = dict(request_id=request['request_id'], name=probe.name,
                          run_id=current['run_id'], conversation_id=current['conversation_id'], status='completed')
            return subprocess.CompletedProcess(argv, 0, json.dumps(result), '')
        def original_scenario():
            for number in range(3):
                current.update(run_id='run-' + str(number), conversation_id='conversation',
                               pid=1000 + number, process_start='private fixture birth ' + str(number))
                probe.pause_settled_original()
        probe.test_resumed_and_restored_hook_review_blocks_input_until_the_person_answers = original_scenario
        with patch.object(native.subprocess, 'run', side_effect=actual):
            native.NativeCodexStartup.test_hook_review_pause_rechecks_each_original_after_preflight_refusals(probe)
        self.assertEqual(physical, ['run-0', 'run-1', 'run-2'])
        for number in range(3):
            self.assertGreaterEqual(observations.count('run-' + str(number)), 5)
