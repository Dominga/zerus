"""Exact native acceptance repair; private records, tmux and synthetic history."""
import datetime
import json
import os
import subprocess
import time
import unittest

import test_input as fixture


@unittest.skipUnless(os.name == 'posix' and fixture.TMUX, 'Requires a private Unix tmux fixture')
class InputAcceptance(unittest.TestCase):
    def setUp(self):
        self.f = fixture.InputTransport('test_first_message_with_image_starts_lazy_conversation')
        self.f.setUp()
        self.addCleanup(self.f.doCleanups)
        self.addCleanup(self.f.tearDown)
        self.payload = self.f.payload(text='Exact private input\nsecond line')
        self.receipt = self.f.send(self.payload)
        self.record = json.loads(self.f.record_path.read_text())
        self.receipt_path = self.f.state / 'input_receipts' / (self.payload['request_id']+'.json')
        self.original = self.receipt_path.read_bytes()
        self.path = self.f.root / 'rollout.jsonl'
        self.record['transcript'] = str(self.path)
        self.header = dict(type='session_meta', payload=dict(id=self.f.conversation_id))
        self.at = self.record['input_pending_at'] + .001
        self.row = dict(type='response_item', timestamp=datetime.datetime.fromtimestamp(
            self.at, datetime.timezone.utc).isoformat(), payload=dict(type='message', role='user',
            content=[dict(type='input_text', text=self.receipt['submitted_text'])]))

    def inspect(self):
        self.f.record = self.record
        self.f.write_record()
        result = subprocess.run([str(fixture.HGS), 'inspect', self.f.name], env=self.f.env,
                                capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(self.f.record_path.read_text())

    def write(self, rows):
        # Synthetic current turn starts after pending, before its user row.
        start=dict(type='event_msg', timestamp=datetime.datetime.fromtimestamp(
            self.at-.0005, datetime.timezone.utc).isoformat(),
            payload=dict(type='task_started',turn_id='current-native-turn'))
        if self.record['agent']=='codex' and not any(row.get('type')=='event_msg' for row in rows):
            rows=[rows[0],start,*rows[1:]] if rows else []
        self.path.write_text(''.join(json.dumps(row)+'\n' for row in rows))

    def assert_unknown(self):
        record = self.inspect()
        self.assertLess(record['input_seen_at'], record['input_pending_at'])
        self.assertNotIn('input_acceptance', record)

    def test_exact_codex_row_repairs_only_its_request_and_keeps_the_receipt(self):
        self.write([self.header, self.row])
        record = self.inspect()
        self.assertGreaterEqual(record['input_seen_at'], record['input_pending_at'])
        self.assertEqual(record['input_acceptance']['request_id'], self.payload['request_id'])
        self.assertEqual(record['input_acceptance']['digest'], self.receipt['digest'])
        self.assertEqual(self.receipt_path.read_bytes(), self.original)
        self.assertEqual(self.f.send(self.payload), self.receipt)

    def test_late_row_of_an_older_native_turn_cannot_confirm_new_pending(self):
        start = dict(type='event_msg', timestamp=datetime.datetime.fromtimestamp(
            self.record['input_pending_at']-1, datetime.timezone.utc).isoformat(),
            payload=dict(type='task_started', turn_id='older-turn'))
        self.write([self.header, start, self.row])
        self.assert_unknown()

    def test_a_termination_fence_never_admits_a_reconciliation_write(self):
        self.write([self.header, self.row])
        self.record['terminating'] = {}
        self.assert_unknown()

    def test_codex_non_user_partial_early_and_foreign_conversation_are_unknown(self):
        for role in ('assistant', 'tool', 'system'):
            with self.subTest(role=role):
                row = json.loads(json.dumps(self.row)); row['payload']['role'] = role
                self.write([self.header, row]); self.assert_unknown()
        for text in ('Exact private input', self.receipt['submitted_text']+' ', 'prefix '+self.receipt['submitted_text']):
            with self.subTest(text=text):
                row = json.loads(json.dumps(self.row)); row['payload']['content'][0]['text'] = text
                self.write([self.header, row]); self.assert_unknown()
        row = dict(self.row, timestamp='2020-01-01T00:00:00Z')
        self.write([self.header, row]); self.assert_unknown()
        self.write([dict(self.header, payload=dict(id='foreign')), self.row]); self.assert_unknown()

    def test_receipt_and_pending_identity_changes_refuse_even_matching_text(self):
        self.write([self.header, self.row])
        for field, value in [('run_id','foreign'), ('conversation_id','foreign'), ('request_id','foreign'),
                             ('name','other'), ('submitted_at',self.at), ('digest','a'*64),
                             ('submitted_text','changed'), ('attachments',[{'path':'image.png'}])]:
            with self.subTest(field=field):
                receipt = dict(self.receipt); receipt[field] = value
                self.receipt_path.write_text(json.dumps(receipt)); self.assert_unknown()
        self.receipt_path.write_bytes(self.original)
        self.record['input_pending_at'] = self.at + 1
        self.assert_unknown()

    def test_missing_link_directory_fifo_truncated_and_oversize_evidence_are_unknown(self):
        self.assert_unknown()
        self.path.mkdir(); self.assert_unknown(); self.path.rmdir()
        target = self.f.root/'other.jsonl'; target.write_text(''.join(json.dumps(row)+'\n' for row in [self.header,self.row]))
        self.path.symlink_to(target); self.assert_unknown(); self.path.unlink()
        if hasattr(os,'mkfifo'):
            os.mkfifo(self.path); self.assert_unknown(); self.path.unlink()
        self.write([self.header,self.row]); self.path.write_bytes(self.path.read_bytes().rstrip(b'\n')); self.assert_unknown()
        self.write([self.header, dict(type='padding', text='x'*(1024*1024)), self.row]); self.assert_unknown()

    def test_kimi_old_hook_before_prompt_row_never_credits_next_send(self):
        self.f.record=dict(self.record,agent='kimi');self.f.write_record()
        self.f.screen('\x1b[2J\x1b[H ╭──────────────────────────────╮\r\n │ >                            │\r\n ╰──────────────────────────────╯\r\n Never Ask  fixture\x1b[2;6H','5:1')
        self.f.claude_hook('UserPromptSubmit',prompt=self.receipt['submitted_text'],agent_id='main')
        first=json.loads(self.f.record_path.read_text());self.assertGreater(first['input_seen_at'],first['input_pending_at'])
        following=self.f.send(self.f.payload(text=self.payload['text']))
        self.record=json.loads(self.f.record_path.read_text());self.record['agent']='kimi'
        session=self.f.root/'.kimi-code/sessions/workspace'/self.f.conversation_id
        self.path=session/'agents/main/wire.jsonl';self.path.parent.mkdir(parents=True)
        (session/'state.json').write_text(json.dumps(dict(id=self.f.conversation_id,version=2)))
        self.write([dict(type='turn.prompt',agentId='main',turnId=0,time=time.time()*1000,
                         origin=dict(kind='user'),input=[dict(type='text',text=following['submitted_text'])])])
        self.assert_unknown()
        self.f.record=json.loads(self.f.record_path.read_text())
        self.f.claude_hook('TurnStarted',prompt=self.receipt['submitted_text'],agent_id='main',turn_id=0,origin_kind='user')
        current=json.loads(self.f.record_path.read_text());self.assertLess(current['input_seen_at'],current['input_pending_at'])

    def test_kimi_positive_pre_send_turn_floor_keeps_old_rows_out(self):
        self.f.record=dict(self.record,agent='kimi')
        # This reused Codex fixture models a first Kimi send with its actual
        # pre-send snapshot; legacy missing context is tested separately.
        self.f.record['input_pending_native_context']=dict(version=1,request_id=self.payload['request_id'],
            run_id=self.f.run_id,conversation_id=self.f.conversation_id,pending_at=self.record['input_pending_at'],complete=True,after=None)
        self.f.write_record()
        self.f.screen('\x1b[2J\x1b[H ╭──────────────────────────────╮\r\n │ >                            │\r\n ╰──────────────────────────────╯\r\n Never Ask  fixture\x1b[2;6H','5:1')
        self.f.claude_hook('UserPromptSubmit',prompt=self.receipt['submitted_text'],agent_id='main')
        self.f.claude_hook('TurnStarted',prompt=self.receipt['submitted_text'],agent_id='main',turn_id=0,origin_kind='user')
        self.f.send(self.f.payload(text=self.payload['text']))
        self.record=json.loads(self.f.record_path.read_text())
        self.assertTrue(self.record['input_pending_native_context']['complete'])
        self.assertEqual(self.record['input_pending_native_context']['after'],0)
        session=self.f.root/'.kimi-code/sessions/workspace'/self.f.conversation_id
        self.path=session/'agents/main/wire.jsonl';self.path.parent.mkdir(parents=True)
        (session/'state.json').write_text(json.dumps(dict(id=self.f.conversation_id,version=2)))
        row=dict(type='turn.prompt',agentId='main',turnId=0,time=time.time()*1000,
                 origin=dict(kind='user'),input=[dict(type='text',text=self.receipt['submitted_text'])])
        self.write([row]);self.assert_unknown()
        row['turnId']=1;self.write([row]);current=self.inspect()
        self.assertGreater(current['input_seen_at'],current['input_pending_at'])
        self.assertEqual(current['input_acceptance']['native_turn_id'],1)

    def test_kimi_exact_main_user_prompt_only_and_no_later_prompt_stays_unknown(self):
        self.record['agent']='kimi'
        session=self.f.root/'.kimi-code/sessions/workspace'/self.f.conversation_id
        self.path=session/'agents/main/wire.jsonl'; self.path.parent.mkdir(parents=True)
        (session/'state.json').write_text(json.dumps(dict(id=self.f.conversation_id,version=2)))
        row=dict(type='turn.prompt',agentId='main',turnId=7,time=self.at*1000,
                 origin=dict(kind='user'),input=[dict(type='text',text=self.receipt['submitted_text'])])
        self.write([]); self.assert_unknown()
        for change in [dict(agentId='peer'),dict(origin=dict(kind='task')),dict(time=(self.at-10)*1000),
                       dict(input=[dict(type='text',text='prefix'),*row['input']])]:
            self.write([dict(row,**change)]); self.assert_unknown()
        self.write([row]); record=self.inspect()
        self.assertGreaterEqual(record['input_seen_at'],record['input_pending_at'])
        self.assertEqual(self.receipt_path.read_bytes(),self.original)
