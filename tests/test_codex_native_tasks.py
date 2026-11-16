"""Same-process native task hooks must not replace the interactive parent.

The native-like C owner supplies real kernel caller/file proof, with synthetic
MCP results. This is separate from real Codex/localhost-provider qualification.
"""
import copy
from datetime import datetime, timezone
import json
import time
import unittest
import uuid

import test_codex_clear_recovery as recovery


class NativeTaskBinding(recovery.CodexClearRecovery):
    def prepare_task(self):
        self.poisoned()
        for key in ('expected_id', 'error', 'session_end', 'clear_context_request'):
            self.record.pop(key, None)
        self.record.update(conversation_state='active', activity='idle', phase='idle',
                           main_done=True, prompt='Original parent request',
                           last_message='Original parent answer')
        self.write_record()
        self.parent = copy.deepcopy(self.record)

    def creation(self, kind, response=None):
        event = dict(hook_event_name=kind, session_id=self.conversation_id,
                     transcript_path=self.record['transcript'], cwd=str(self.root),
                     tool_name='mcp__codex_tui__create_thread', tool_use_id='native-call')
        if response is not None:
            event['tool_response'] = response
        return self.emit(event)

    def test_task_start_and_completion_preserve_the_parent_binding(self):
        self.prepare_task()
        self.record = self.creation('PreToolUse')
        self.record = self.creation('PostToolUse', dict(isError=False, content=[dict(
            type='text', text=json.dumps(dict(threadId=self.successor)))]))
        self.event = dict(hook_event_name='SessionStart', source='startup',
                          session_id=self.successor, transcript_path=str(self.transcript),
                          cwd=str(self.root))
        self.record = self.emit()
        self.assertEqual(self.record['conversation_id'], self.conversation_id)
        self.assertEqual(self.record['transcript'], self.parent['transcript'])
        self.assertEqual(self.record['run_id'], self.parent['run_id'])
        self.assertEqual(self.record['pid'], self.parent['pid'])
        child_id = 'external:codex:' + self.successor
        self.assertEqual(self.record['subagents'][child_id]['conversation_id'], self.successor)
        self.record = self.emit(dict(self.event, hook_event_name='Stop',
                                     last_assistant_message='Separate task answer'))
        self.assertEqual(self.record['conversation_id'], self.conversation_id)
        self.assertEqual(self.record['last_message'], self.parent['last_message'])
        self.assertEqual(self.record['subagents'][child_id]['state'], 'finished')

    def test_task_can_start_before_the_creation_result_arrives(self):
        self.prepare_task()
        self.record = self.creation('PreToolUse')
        self.event = dict(hook_event_name='SessionStart', source='startup',
                          session_id=self.successor, transcript_path=str(self.transcript),
                          cwd=str(self.root))
        self.record = self.emit()
        self.assertEqual(self.record['conversation_id'], self.conversation_id)
        self.record = self.creation('PostToolUse', dict(isError=False, content=[dict(
            type='text', text=json.dumps(dict(threadId=self.successor)))]))
        self.record = self.emit(dict(self.event, hook_event_name='PreToolUse',
                                     tool_name='Bash', tool_use_id='child-work'))
        self.assertEqual(self.record['conversation_id'], self.conversation_id)
        self.assertEqual(self.record['subagents']['external:codex:' + self.successor]['state'], 'working')

    def legacy_binding(self):
        self.prepare_task()
        old = self.rollout
        parent = self.account / 'sessions' / ('rollout-' + self.conversation_id + '.jsonl')
        old.rename(parent)  # The original C process still holds the same inode.
        stamp = datetime.fromtimestamp(self.record['created'], timezone.utc).isoformat()
        header = dict(type='session_meta', payload=dict(id=self.conversation_id,
            source='cli', timestamp=stamp, cwd=str(self.root), cli_version='0.169.0'))
        created = dict(type='event_msg', timestamp=datetime.now(timezone.utc).isoformat(),
            payload=dict(type='item_completed', item=dict(type='McpToolCall', server='codex_tui',
                tool='create_thread', status='completed', result=dict(isError=False,
                    content=[dict(type='text', text=json.dumps(dict(threadId=self.successor)))]))))
        parent.write_text(json.dumps(header)+'\n'+json.dumps(created)+'\n')
        self.record['transcript'] = str(parent)
        self.write_record()
        self.record = self.emit(dict(hook_event_name='SessionStart',source='resume',
            session_id=self.conversation_id,transcript_path=str(parent),cwd=str(self.root)))
        # Explicit legacy-corrupt record fixture. The installed-base RED above
        # separately proves that the old real hook produces this transition.
        self.record.update(conversation_id=self.successor,transcript=str(self.transcript),
                           agent_home=str(self.account))
        self.write_record()
        self.parent_path = parent
        self.parent_event = dict(hook_event_name='UserPromptSubmit',session_id=self.conversation_id,
            transcript_path=str(parent),cwd=str(self.root),prompt='Fresh original native prompt')

    def test_fresh_original_prompt_repairs_legacy_binding_without_rewriting_receipts(self):
        self.legacy_binding()
        request = str(uuid.uuid4())
        receipt = dict(request_id=request,name=self.name,run_id=self.run_id,
            conversation_id=self.successor,submitted_at=time.time()-1,status='submitted',
            transport='terminal',submitted_text='Original retained message',digest='a'*64,attachments=[])
        receipts = self.state/'input_receipts'
        receipts.mkdir(exist_ok=True)
        path = receipts/(request+'.json')
        original = json.dumps(receipt).encode()
        path.write_bytes(original)
        self.record.update(input_pending_request=request,input_pending_at=receipt['submitted_at'],input_seen_at=0)
        self.write_record()
        current = self.emit(self.parent_event)
        self.assertEqual(current['conversation_id'],self.conversation_id)
        self.assertEqual(current['transcript'],str(self.parent_path))
        self.assertEqual(path.read_bytes(),original)
        self.assertEqual(current['input_pending_request'],request)
        self.assertGreater(current['input_seen_at'],receipt['submitted_at'])
        self.assertEqual(current['codex_binding_recovery']['source'],'original_native_prompt')
        self.assertFalse(current['codex_binding_recovery']['replayed'])
        self.assertEqual(len(list((self.state/'codex-binding-repairs').glob('*.json'))),1)

    def test_completion_alone_never_repairs_legacy_binding_or_input(self):
        self.legacy_binding()
        before = copy.deepcopy(self.record)
        current = self.emit(dict(self.parent_event,hook_event_name='Stop',last_assistant_message='Done'))
        self.assertEqual(current['conversation_id'],self.successor)
        self.assertEqual(current['transcript'],before['transcript'])
        self.assertFalse((self.state/'codex-binding-repairs').exists())

    def test_native_output_text_claim_is_not_creation_proof(self):
        self.legacy_binding()
        rows=self.parent_path.read_text().splitlines()
        rows[1]=json.dumps(dict(type='response_item',payload=dict(type='function_call_output',
            output='threadId '+self.successor)))
        self.parent_path.write_text('\n'.join(rows)+'\n')
        current=self.emit(self.parent_event)
        self.assertEqual(current['conversation_id'],self.successor)
        self.assertFalse((self.state/'codex-binding-repairs').exists())


if __name__ == '__main__':
    unittest.main()
