"""Ordinary screen approvals share the native guard, regardless of caller.

This reuses the deterministic 150 ms model; it is not independent upstream
artifact evidence. The hook's JSON response does not go through this chooser.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import unittest
import uuid

import test_claude_permission as permission
import test_input as fixtures
import test_questions as questions

PANEL = "\n" + "─" * 40 + """
 Bash command
 Write hi into out.txt
╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌
 printf hi > out.txt
╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌
 Do you want to proceed?
 ❯ 1. Yes
   2. No

 Esc to cancel · Tab to amend
"""


def guarded_approval():
    fake = permission.guarded_agent()
    labels = next(row for row in fake.splitlines() if row.startswith('labels='))
    fake = fake.replace(labels, "labels=['Yes','No']")
    fake = fake.replace("screen.replace('❯ Yes, set','  Yes, set').replace('  No, keep','❯ No, keep')",
                        "screen.replace('❯ 1. Yes','  1. Yes').replace('  2. No','❯ 2. No')")
    fake = fake.replace("screen.replace('No, keep ','No, keep changed ')",
                        "screen.replace('printf hi > out.txt','printf changed > out.txt')")
    return fake


@unittest.skipUnless(sys.platform == 'linux', 'the controlled-clock fixture uses Linux preloading')
class ApprovalGuard(permission.PermissionModeGuard):
    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.panel = Path(cls.directory.name) / 'bash-panel.txt'
        cls.panel.write_text(PANEL)

    def setUp(self):
        os.environ['CLAUDE_TRUST_FIXTURE'] = str(self.panel)
        self.addCleanup(lambda: os.environ.pop('CLAUDE_TRUST_FIXTURE', None))
        original = fixtures.FAKE_AGENT
        try:
            fixtures.FAKE_AGENT = guarded_approval()
            fixtures.InputTransport.setUp(self)
        finally:
            fixtures.FAKE_AGENT = original
        self.record.update(agent='claude', run_identity_version=1, supervisor={},
                           launch_dir='/work/example', phase='approval', activity='approval',
                           pending_approval_at=time.time(), active_tools={'toolu_one': {'name': 'Bash'}},
                           pending_approval={'hook_event_name': 'PermissionRequest', 'tool_name': 'Bash',
                                             'tool_input': {'command': 'printf hi > out.txt'}})
        self.write_record()
        self.card = self.inspect()['pending_questions'][0]
        self.assertEqual(self.card['source'], 'claude_tool_approval')
        self.assertEqual(self.card['answer_transport'], 'claude_tui')
        self.assertTrue(self.card['can_answer'])
        self.start_clock()

    def payload(self, choice=0, **updates):
        payload = dict(request_id=str(uuid.uuid4()), expected_run_id=self.run_id,
                       expected_conversation_id=self.conversation_id, question_id=self.card['question_id'],
                       expected_question_hash=self.card['question_hash'],
                       answers=[dict(question_id='approval', selected_option_ids=[str(choice + 1)], text='')])
        payload.update(updates)
        return payload

    def delivered_once(self, payload):
        result = self.call(payload)
        self.assertEqual(result.returncode, 0, result.stderr)
        receipt = json.loads(result.stdout)
        self.assertEqual(receipt['status'], 'answered')
        self.assertEqual(receipt['transport'], 'claude_tui')
        self.assertFalse((self.root / 'refused').exists())
        self.assertEqual((self.root / 'submitted').read_text(), 'Yes')
        self.assertEqual((self.root / 'enter-clock').read_text(), '150000000\n')
        self.assertEqual(self.received(), b'\r')
        self.assertEqual(json.loads(self.call(payload).stdout), receipt)
        self.assertEqual(self.received(), b'\r')

    def test_fresh_paint_waits_out_the_native_guard_before_its_one_enter(self):
        self.delivered_once(self.payload())

    def test_an_automatic_one_time_reservation_uses_the_same_screen_guard(self):
        def journal(operation, payload):
            result = subprocess.run([str(questions.HGS), 'approval-log', operation],
                                    input=json.dumps(payload), env=self.env, text=True,
                                    capture_output=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr)
            return json.loads(result.stdout)

        request = dict(host='local', session=self.name, provider='claude', question=self.card)
        reservation = journal('begin', request)
        self.assertTrue(reservation['fresh'])
        payload = self.payload(request_id=reservation['request_id'], answers=reservation['answers'])
        self.assertEqual(payload['answers'][0]['selected_option_ids'], ['1'])
        self.delivered_once(payload)
        journal('finish', dict(id=reservation['id'], request_id=reservation['request_id'],
                               status='approved', detail=''))
        self.assertFalse(journal('begin', request)['fresh'])
        self.assertEqual(self.received(), b'\r')

    def test_an_unconfirmed_enter_is_never_resent(self):
        (self.root / 'hold-dialog').touch()
        payload = self.payload()
        result = self.call(payload)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('delivery uncertain', result.stderr)
        self.assertEqual(self.received(), b'\r')
        self.assertFalse((self.root / 'refused').exists())
        result = self.call(payload)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('do not resend automatically', result.stderr)
        self.assertEqual(self.received(), b'\r')
