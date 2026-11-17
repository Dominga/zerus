"""Confirm an old receipt only from the original native main's exact input.

The binding may already have recovered through an ordinary native transition.
All processes, histories and receipts below are private kernel-caller fixtures.
"""
import copy
from datetime import datetime, timezone
import json
import time
import unittest
import uuid

import test_codex_native_tasks as binding


class RestoredMainReceipt(unittest.TestCase):
    setUp = binding.NativeTaskBinding.setUp
    tearDown = binding.NativeTaskBinding.tearDown
    script = binding.NativeTaskBinding.script
    write_record = binding.NativeTaskBinding.write_record
    tmux = binding.NativeTaskBinding.tmux
    wait_for = binding.NativeTaskBinding.wait_for
    received = binding.NativeTaskBinding.received
    payload = binding.NativeTaskBinding.payload
    send = binding.NativeTaskBinding.send
    hook = binding.NativeTaskBinding.hook
    inspect = binding.NativeTaskBinding.inspect
    write_header = binding.NativeTaskBinding.write_header
    emit = binding.NativeTaskBinding.emit
    poisoned = binding.NativeTaskBinding.poisoned
    prepare_task = binding.NativeTaskBinding.prepare_task
    legacy_binding = binding.NativeTaskBinding.legacy_binding

    def retained(self):
        self.legacy_binding()
        self.pending = time.time()
        self.request = str(uuid.uuid4())
        self.receipt = dict(request_id=self.request, name=self.name, run_id=self.run_id,
                            conversation_id=self.successor, submitted_at=self.pending,
                            status='submitted', transport='terminal',
                            submitted_text='Original exact private input', digest='a'*64,
                            attachments=[])
        directory = self.state/'input_receipts'
        directory.mkdir(exist_ok=True)
        self.receipt_path = directory/(self.request+'.json')
        self.receipt_bytes = json.dumps(self.receipt).encode()
        self.receipt_path.write_bytes(self.receipt_bytes)
        # This is the already-restored state, not a synthetic UserPromptSubmit.
        self.record.update(conversation_id=self.conversation_id,
                           transcript=str(self.parent_path),
                           input_pending_request=self.request,
                           input_pending_at=self.pending, input_seen_at=0)
        self.record.pop('codex_binding_recovery', None)
        self.write_record()

    def accepted(self, *, large=False, text=None):
        stamp = lambda offset: datetime.fromtimestamp(self.pending+offset,
                                                      timezone.utc).isoformat()
        rows = []
        if large:
            rows.append(dict(type='response_item', timestamp=stamp(.001),
                             payload=dict(type='custom_tool_call_output',
                                          call_id='private-tool', output='x'*(2<<20))))
        rows.extend([
            dict(type='event_msg', timestamp=stamp(.002),
                 payload=dict(type='task_started', turn_id='original-private-turn')),
            dict(type='response_item', timestamp=stamp(.003),
                 payload=dict(type='message', role='user', content=[dict(
                     type='input_text', text=text or self.receipt['submitted_text'])]))])
        with self.parent_path.open('a') as stream:
            stream.writelines(json.dumps(row)+'\n' for row in rows)
        time.sleep(.01)

    def observe(self):
        before_native = self.parent_path.read_bytes()
        before_terminal = self.received()
        self.inspect()
        current = json.loads(self.record_path.read_bytes())
        self.assertEqual(self.parent_path.read_bytes(), before_native)
        self.assertEqual(self.receipt_path.read_bytes(), self.receipt_bytes)
        self.assertEqual(self.received(), before_terminal, 'inspection never resends input')
        self.assertEqual(current['pid'], self.record['pid'])
        self.assertEqual(current['conversation_id'], self.conversation_id)
        self.assertNotIn('codex_binding_recovery', current)
        return current

    def test_already_restored_main_accepts_exact_input_without_rewriting_receipt(self):
        self.retained()
        self.accepted()
        current = self.observe()
        self.assertGreater(current['input_seen_at'], self.pending)
        self.assertEqual(current['input_acceptance']['request_id'], self.request)
        self.assertEqual(current['input_acceptance']['receipt_conversation_id'], self.successor)

    def test_large_unrelated_native_output_keeps_the_original_total_byte_bound(self):
        self.retained()
        self.accepted(large=True)
        current = self.observe()
        self.assertGreater(current['input_seen_at'], self.pending)
        self.assertEqual(current['input_acceptance']['source'], 'native_user_input')

    def test_unrelated_fresh_prompt_cannot_credit_the_old_context(self):
        self.retained()
        self.accepted(text='A different current native prompt')
        current = self.emit(dict(self.parent_event, prompt='A different current native prompt'))
        self.assertEqual(current.get('input_seen_at', 0), 0)
        self.assertEqual(self.observe().get('input_seen_at', 0), 0)

    def test_native_tool_text_is_not_a_closed_creation_result(self):
        self.retained()
        rows = self.parent_path.read_text().splitlines()
        rows[1] = json.dumps(dict(type='response_item', payload=dict(
            type='custom_tool_call_output', output='threadId '+self.successor)))
        self.parent_path.write_text('\n'.join(rows)+'\n')
        self.accepted()
        self.assertEqual(self.observe().get('input_seen_at', 0), 0)

    def test_non_original_native_header_and_wrong_receipt_run_refuse(self):
        self.retained()
        self.accepted()
        original = self.parent_path.read_bytes()
        rows = original.decode().splitlines()
        header = json.loads(rows[0])
        header['payload']['parent_thread_id'] = str(uuid.uuid4())
        rows[0] = json.dumps(header)
        self.parent_path.write_text('\n'.join(rows)+'\n')
        self.assertEqual(self.observe().get('input_seen_at', 0), 0)
        self.parent_path.write_bytes(original)
        wrong = copy.deepcopy(self.receipt)
        wrong['run_id'] = str(uuid.uuid4())
        self.receipt_bytes = json.dumps(wrong).encode()
        self.receipt_path.write_bytes(self.receipt_bytes)
        self.assertEqual(self.observe().get('input_seen_at', 0), 0)

    def test_unseen_creation_beyond_the_original_tail_is_not_inferred(self):
        self.retained()
        with self.parent_path.open('a') as stream:
            stream.write(json.dumps(dict(type='response_item', payload=dict(
                type='custom_tool_call_output', output='x'*(16<<20))))+'\n')
        self.accepted()
        self.assertEqual(self.observe().get('input_seen_at', 0), 0)

    def test_later_creation_is_not_evidence_for_an_older_receipt(self):
        self.retained()
        self.accepted()
        rows = self.parent_path.read_text().splitlines()
        creation = json.loads(rows[1])
        creation['timestamp'] = datetime.fromtimestamp(self.pending+.001,
                                                       timezone.utc).isoformat()
        rows[1] = json.dumps(creation)
        self.parent_path.write_text('\n'.join(rows)+'\n')
        self.assertEqual(self.observe().get('input_seen_at', 0), 0)

    def test_replaced_native_file_is_not_the_original_held_history(self):
        self.retained()
        self.accepted()
        retained = self.parent_path.read_bytes()
        self.parent_path.rename(self.parent_path.with_suffix('.old'))
        self.parent_path.write_bytes(retained)
        self.assertEqual(self.observe().get('input_seen_at', 0), 0)

    def test_large_malformed_output_still_refuses_the_whole_proof(self):
        self.retained()
        self.accepted()
        with self.parent_path.open('a') as stream:
            stream.write('{"not_complete":"'+'x'*(2<<20)+'\n')
        self.assertEqual(self.observe().get('input_seen_at', 0), 0)

    def queued_receipt(self, parent=False):
        self.retained()
        self.receipt.update(transport='codex_queue', native_queued_message_id=str(uuid.uuid4()))
        if parent:
            self.receipt['conversation_id'] = self.conversation_id
        self.receipt_bytes = json.dumps(self.receipt).encode()
        self.receipt_path.write_bytes(self.receipt_bytes)
        self.accepted()

    def test_parent_input_cannot_confirm_a_queue_for_its_child(self):
        self.queued_receipt()
        self.assertEqual(self.observe().get('input_seen_at', 0), 0)

    def test_a_real_binding_repair_cannot_rehome_a_child_queue(self):
        self.queued_receipt()
        self.record.update(conversation_id=self.successor, transcript=str(self.transcript))
        self.write_record()
        current = self.emit(self.parent_event)
        self.assertEqual(current['conversation_id'], self.conversation_id)
        self.assertEqual(current['codex_binding_recovery']['source'], 'original_native_prompt')
        self.assertEqual(current.get('input_seen_at', 0), 0)
        self.assertEqual(self.receipt_path.read_bytes(), self.receipt_bytes)

    def test_same_recipient_queue_keeps_exact_native_input_confirmation(self):
        self.queued_receipt(parent=True)
        current = self.observe()
        self.assertGreater(current['input_seen_at'], self.pending)
        self.assertEqual(current['input_acceptance']['request_id'], self.request)
        self.assertEqual(current['input_acceptance']['conversation_id'], self.conversation_id)
        self.assertNotIn('receipt_conversation_id', current['input_acceptance'])


if __name__ == '__main__':
    unittest.main()
