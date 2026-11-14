"""Durable person-answer controls, using only private scripted DeepSeek hosts."""
import copy
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from test_dsh_dispatch_lock import Scenario


@unittest.skipUnless(os.name == 'posix', 'Unix private native hosts')
class DeepSeekAnswers(unittest.TestCase):
    def result(self, child):
        out, err = child.communicate(timeout=8)
        return child.returncode, out, err

    def answered(self, child):
        code, out, err = self.result(child)
        self.assertEqual(code, 0, out + err)
        receipt = json.loads(out)
        self.assertEqual(receipt['status'], 'answered', receipt)
        return receipt

    def receipt(self, scenario, request):
        return scenario.state / 'question_receipts' / (request['request_id'] + '.json')

    def test_reserves_before_delivery_and_keeps_older_bridge_acknowledgements(self):
        with Scenario() as s:
            s.a.mode = 'answer'
            request = s.payload(s.a, 'answer')
            path = self.receipt(s, request)

            def deliver(params, _):
                saved = json.loads(path.read_text())
                self.assertEqual(saved['status'], 'in_progress')
                self.assertEqual(saved['request_id'], request['request_id'])
                self.assertEqual(saved['question_hash'], request['expected_question_hash'])
                self.assertEqual(saved['answers'], request['answers'])
                self.assertEqual(saved['digest'], hashlib.sha256(json.dumps(request).encode()).hexdigest())
                self.assertEqual(params['requestId'], request['request_id'])
                # Every older resident bridge returns this exact shape.
                return {'accepted': True}

            s.a.replies['answer'] = deliver
            receipt = self.answered(s.start(s.a, 'answer', request=request, public=True))
            self.assertEqual(json.loads(path.read_text()), receipt)

    def test_exact_duplicate_returns_the_original_outcome_after_question_and_run_change(self):
        with Scenario() as s:
            s.a.mode = 'answer'
            request = s.payload(s.a, 'answer')
            first = self.answered(s.start(s.a, 'answer', request=request))
            s.a.mode = ''
            s.a.record['run_id'] = 'later-run'
            s.a.binding.write_text(json.dumps(s.a.record))
            calls = len(s.a.calls)
            duplicate = self.answered(s.start(s.a, 'answer', request=request))
            self.assertEqual(duplicate, first)
            self.assertEqual(len(s.a.calls), calls, 'duplicate contacted the native host')

    def test_changed_content_or_target_cannot_reuse_an_answer_uuid(self):
        with Scenario() as s:
            s.a.mode = 'answer'
            request = s.payload(s.a, 'answer')
            self.answered(s.start(s.a, 'answer', request=request))
            for host, change in [(s.a, 'text'), (s.a, 'hash'), (s.b, 'target')]:
                with self.subTest(change=change):
                    changed = copy.deepcopy(request)
                    if change == 'text':
                        changed['answers'][0]['text'] = 'Different answer'
                    elif change == 'hash':
                        changed['expected_question_hash'] = '0' * 64
                    else:
                        changed.update(expected_run_id=host.record['run_id'],
                                       expected_conversation_id=host.record['conversation_id'])
                    before = len(host.calls)
                    code, _, err = self.result(s.start(host, 'answer', request=changed))
                    self.assertNotEqual(code, 0)
                    self.assertIn('already used for different content', err)
                    self.assertEqual(len(host.calls), before)

    def test_concurrent_duplicates_do_not_hold_the_machine_lock_or_deliver_twice(self):
        with Scenario() as s:
            s.a.mode = s.b.mode = 'answer'
            request = s.payload(s.a, 'answer')
            s.a.hold = 'answer'
            original = s.start(s.a, 'answer', request=request)
            self.assertTrue(s.a.entered.wait(5))
            duplicate = s.start(s.a, 'answer', request=request)
            changed = s.payload(s.b, 'answer')
            changed['request_id'] = request['request_id']
            code, _, err = self.result(s.start(s.b, 'answer', request=changed))
            self.assertNotEqual(code, 0)
            self.assertIn('already used for different content', err)
            self.assertIsNone(original.poll())
            s.a.release.set()
            self.assertEqual(self.answered(original), self.answered(duplicate))
            self.assertEqual(sum(call['method'] == 'answer' for call in s.a.calls), 1)
            self.assertFalse(s.b.calls, 'conflicting UUID contacted another native host')

    def test_lost_or_unbound_acknowledgement_stays_unknown_and_never_replays(self):
        for reply in ['lost', 'not-accepted', 'wrong-uuid', 'host-error']:
            with self.subTest(reply=reply), Scenario() as s:
                s.a.mode = 'answer'
                request = s.payload(s.a, 'answer')
                if reply == 'lost':
                    s.a.unknown.add('answer')
                elif reply == 'host-error':
                    s.a.refusals['answer'] = 'Synthetic native refusal'
                else:
                    s.a.replies['answer'] = lambda p, v: {
                        'not-accepted': {'accepted': False},
                        'wrong-uuid': {'accepted': True, 'requestId': '00000000-0000-0000-0000-000000000001'},
                    }[reply]
                code, _, err = self.result(s.start(s.a, 'answer', request=request))
                self.assertNotEqual(code, 0)
                self.assertIn('delivery uncertain', err.lower())
                path = self.receipt(s, request)
                receipt = json.loads(path.read_text())
                self.assertEqual(receipt['status'], 'in_progress')
                self.assertEqual(receipt['outcome'], 'unknown')
                before = len(s.a.calls)
                s.a.unknown.clear()
                s.a.replies.clear()
                s.a.refusals.clear()
                code, _, duplicate_error = self.result(s.start(s.a, 'answer', request=request))
                self.assertNotEqual(code, 0)
                self.assertEqual(duplicate_error, err)
                self.assertEqual(json.loads(path.read_text()), receipt)
                self.assertEqual(len(s.a.calls), before, 'unknown answer was replayed')

    def test_an_interrupted_reservation_or_unwritable_store_never_delivers(self):
        for interrupted in [True, False]:
            with self.subTest(interrupted=interrupted), Scenario() as s:
                s.a.mode = 'answer'
                request = s.payload(s.a, 'answer')
                if interrupted:
                    s.a.hold = 'answer'
                    first = s.start(s.a, 'answer', request=request)
                    self.assertTrue(s.a.entered.wait(5))
                    # Only this fixture's own CLI, after its durable reservation.
                    first.kill()
                    first.communicate(timeout=5)
                    path = self.receipt(s, request)
                    self.assertEqual(json.loads(path.read_text())['status'], 'in_progress')
                    s.a.release.set()
                    for worker in s.a.workers:
                        worker.join(timeout=2)
                    # The one fixture CLI was deliberately killed while its
                    # fake host held the response; that peer close is expected.
                    s.a.errors[:] = [error for error in s.a.errors
                                     if not isinstance(error, (BrokenPipeError, ConnectionResetError))]
                else:
                    (s.state / 'question_receipts').write_text('not a directory')
                before = sum(call['method'] == 'answer' for call in s.a.calls)
                code, _, _ = self.result(s.start(s.a, 'answer', request=request))
                self.assertNotEqual(code, 0)
                self.assertEqual(sum(call['method'] == 'answer' for call in s.a.calls), before)

    def test_unreserved_stale_identity_hash_or_invalid_option_is_a_refusal(self):
        for field in ['expected_run_id', 'expected_conversation_id', 'expected_question_hash', 'answers']:
            with self.subTest(field=field), Scenario() as s:
                s.a.mode = 'answer'
                request = s.payload(s.a, 'answer')
                request[field] = ([{'question_id': 'choice', 'selected_option_ids': ['missing']}]
                                  if field == 'answers' else 'stale')
                code, _, err = self.result(s.start(s.a, 'answer', request=request))
                self.assertNotEqual(code, 0, err)
                self.assertFalse(self.receipt(s, request).exists())
                self.assertFalse(any(call['method'] == 'answer' for call in s.a.calls))


@unittest.skipUnless(os.name == 'posix' and shutil.which('node'), 'Unix Node.js bridge controls')
class DeepSeekAnswerBridge(unittest.TestCase):
    def test_optional_uuid_echo_legacy_shape_and_consumed_live_question(self):
        root = Path(__file__).resolve().parents[1]
        with tempfile.TemporaryDirectory(prefix='zda-', dir='/tmp') as private:
            result = subprocess.run(['node', str(root / 'tests/fixtures/dsh-answer-host.mjs'),
                                     (root / 'src/dsh/bridge.mjs').as_uri(), private],
                                    text=True, capture_output=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == '__main__':
    unittest.main()
