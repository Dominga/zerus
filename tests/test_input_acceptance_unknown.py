"""An unknown acknowledgment never identifies a later same-text native input."""
import datetime
import hashlib
import uuid
import json
import os
from pathlib import Path
import subprocess
import time
import unittest

import test_input as fixture
import test_acknowledge_input as acknowledgment


@unittest.skipUnless(os.name == 'posix' and fixture.TMUX, 'Requires a private Unix tmux fixture')
class UnknownPredecessor(unittest.TestCase):
    provider = 'codex'
    request = acknowledgment.AcknowledgeInput.request
    execute = acknowledgment.AcknowledgeInput.execute

    def setUp(self):
        self.f = fixture.InputTransport('test_first_message_with_image_starts_lazy_conversation')
        tmux = self.f.tmux
        def tracked(*args, **kwargs):
            if args[0] == 'new-session' and '-s' in args and args[args.index('-s')+1] == self.f.name:
                args = (args[0], '-e', 'HGS_SESSION='+self.f.name, '-e', 'HGS_RUN_ID='+self.f.run_id, *args[1:])
            return tmux(*args, **kwargs)
        self.f.tmux = tracked
        self.f.setUp()
        self.addCleanup(self.f.doCleanups)
        self.addCleanup(self.f.tearDown)
        for key in ('HGS_SESSION', 'HGS_RUN_ID'):
            self.f.tmux('set-environment', '-u', '-t', '='+self.f.name+':', key)
        self.f.record['agent'] = self.provider
        if self.provider == 'kimi':
            self.f.screen('\x1b[2J\x1b[H ╭──────────────────────────────╮\r\n │ >                            │\r\n ╰──────────────────────────────╯\r\n Never Ask  fixture\x1b[2;6H', '5:1')
        self.f.write_record()
        self.original = self.f.payload(text='Same original and deliberately new input')
        self.receipt = self.f.send(self.original)
        self.pending = json.loads(self.f.record_path.read_text())
        self.before = self.f.received()

    def barrier(self):
        key=hashlib.sha256((self.provider+'\0'+self.f.conversation_id).encode()).hexdigest()
        return self.f.state/'input_acceptance_barriers'/(key+'.json')

    def following(self, text=None):
        result=self.execute(self.request())
        self.assertEqual(result['exit'],0,result)
        following=self.f.payload(text=text or self.original['text'])
        receipt=self.f.send(following)
        self.before=self.f.received()
        return receipt

    def native(self, submitted):
        record=json.loads(self.f.record_path.read_text())
        time.sleep(.01)
        at=time.time()
        if self.provider=='kimi':
            session=self.f.root/'.kimi-code/sessions/workspace'/self.f.conversation_id
            path=session/'agents/main/wire.jsonl';path.parent.mkdir(parents=True,exist_ok=True)
            (session/'state.json').write_text(json.dumps(dict(id=self.f.conversation_id,version=2)))
            rows=[dict(type='turn.prompt',agentId='main',turnId=1,promptId='native_prompt',time=at*1000,
                       origin=dict(kind='user'),input=[dict(type='text',text=submitted)])]
        else:
            path=self.f.root/'native.jsonl'
            rows=[dict(type='session_meta',payload=dict(id=self.f.conversation_id)),
                  dict(type='event_msg',timestamp=datetime.datetime.fromtimestamp(at-.001,datetime.timezone.utc).isoformat(),payload=dict(type='task_started',turn_id='new-native-turn')),
                  dict(type='response_item',timestamp=datetime.datetime.fromtimestamp(at,datetime.timezone.utc).isoformat(),
                       payload=dict(type='message',role='user',content=[dict(type='input_text',text=submitted)]))]
        path.write_text(''.join(json.dumps(row)+'\n' for row in rows))
        record['transcript']=str(path);self.f.record=record;self.f.write_record()

    def inspect(self, unknown=True):
        result=subprocess.run([str(fixture.HGS),'inspect',self.f.name],env=self.f.env,capture_output=True,text=True,timeout=5)
        self.assertEqual(result.returncode,0,result.stderr)
        current=json.loads(self.f.record_path.read_text())
        if unknown:
            self.assertLess(current['input_seen_at'],current['input_pending_at'])
            self.assertNotIn('input_acceptance',current)
        else:
            self.assertGreaterEqual(current['input_seen_at'],current['input_pending_at'])
        original=json.loads((self.f.state/'input_receipts'/(self.original['request_id']+'.json')).read_text())
        self.assertEqual(original,self.receipt)
        return current

    def hook(self, kind, prompt):
        # Spawn below the fixture's actual pinned agent, never borrow markers
        # from a sibling controller. The distinct-prompt control must admit.
        self.f.claude_hook(kind,prompt=prompt,turn_id=1 if self.provider=='kimi' else 'late-turn',
                           agent_id='main' if self.provider=='kimi' else '')

    def test_late_unknown_original_never_confirms_identical_new_request(self):
        receipt=self.following();self.assertEqual(receipt['submitted_text'],self.receipt['submitted_text'])
        self.native(self.receipt['submitted_text']);self.inspect()

    def test_different_prompt_can_still_confirm_but_late_old_hooks_cannot(self):
        receipt=self.following('Distinct deliberately new message')
        self.hook('UserPromptSubmit',self.receipt['submitted_text']);self.inspect()
        self.hook('TurnStarted',self.receipt['submitted_text']);self.inspect()
        self.hook('UserPromptSubmit',receipt['submitted_text']);self.inspect(unknown=False)

    def test_identical_late_hooks_never_confirm_new_pending(self):
        self.following()
        for kind in ('UserPromptSubmit','UserPromptQueued','TurnStarted','TaskStarted'):
            self.hook(kind,self.receipt['submitted_text']);self.inspect()

    def test_uncertainty_survives_record_replacement_and_run_change(self):
        receipt=self.following();self.native(receipt['submitted_text'])
        record=json.loads(self.f.record_path.read_text());record.pop('input_acknowledged_unknown')
        record['run_id']=str(uuid.uuid4())
        if self.provider=='kimi': record['input_pending_native_context']['run_id']=record['run_id']
        self.f.record=record;self.f.write_record()
        path=self.f.state/'input_receipts'/(receipt['request_id']+'.json')
        receipt['run_id']=record['run_id'];path.write_text(json.dumps(receipt))
        self.inspect()
        self.assertLess(self.barrier().stat().st_size,8192)

    def test_a_different_conversation_has_independent_uncertainty(self):
        receipt=self.following()
        current=json.loads(self.f.record_path.read_text())
        self.f.conversation_id=str(uuid.uuid4());current['conversation_id']=self.f.conversation_id
        if self.provider=='kimi': current['input_pending_native_context']['conversation_id']=self.f.conversation_id
        self.f.record=current;self.f.write_record()
        receipt['conversation_id']=self.f.conversation_id
        (self.f.state/'input_receipts'/(receipt['request_id']+'.json')).write_text(json.dumps(receipt))
        self.native(receipt['submitted_text']);self.inspect(unknown=False)

    def test_legacy_unknown_without_ledger_never_grants_absence(self):
        receipt=self.following('Different new input under legacy incomplete history')
        self.barrier().unlink();self.native(receipt['submitted_text']);self.inspect()
        self.hook('UserPromptSubmit',receipt['submitted_text']);self.inspect()

    def test_unavailable_ledger_refuses_before_opening_gate_and_never_replays(self):
        barrier=self.barrier();barrier.parent.mkdir();barrier.mkdir()
        request=self.request();result=self.execute(request)
        self.assertNotEqual(result['exit'],0,result)
        self.inspect()
        result=self.execute(request)
        self.assertNotEqual(result['exit'],0,result)
        self.assertIn('outcome is unknown',result['stderr'])
        audit=self.f.state/'input_acknowledgments'/(request['request_id']+'.json')
        self.assertEqual(json.loads(audit.read_text())['status'],'claimed')
        self.inspect()

    def test_ledger_special_oversize_or_malformed_is_unknown(self):
        receipt=self.following('Unrelated new text')
        barrier=self.barrier();saved=barrier.read_bytes();self.native(receipt['submitted_text'])
        for content in (b'{}',b'x'*8193):
            barrier.write_bytes(content);self.inspect()
        barrier.unlink();barrier.symlink_to(self.f.root/'missing');self.inspect();barrier.unlink()
        if hasattr(os,'mkfifo'):
            os.mkfifo(barrier);self.inspect();barrier.unlink()
        barrier.write_bytes(saved)
        self.inspect(unknown=False)

    def test_ledger_overflow_stays_bounded_and_unknown(self):
        self.following('Different next pending')
        barrier=self.barrier();data=json.loads(barrier.read_text())
        data['requests'].update({str(uuid.uuid4()):'a'*64 for _ in range(31)})
        barrier.write_text(json.dumps(data))
        self.pending=json.loads(self.f.record_path.read_text())
        self.original=self.f.payload(text='Unused') | dict(request_id=self.pending['input_pending_request'])
        request=self.request();result=self.execute(request);self.assertEqual(result['exit'],0,result)
        data=json.loads(barrier.read_text());self.assertTrue(data['all']);self.assertEqual(data['requests'],{})
        self.assertLess(barrier.stat().st_size,8192)


class UnknownKimiPredecessor(UnknownPredecessor):
    provider = 'kimi'

    def assert_late_typed_a_cannot_make_b_ordering_complete_for_c(self, text):
        self.hook('UserPromptSubmit',self.receipt['submitted_text'])
        b=self.f.payload(text=text)
        b_receipt=self.f.send(b)
        self.assertFalse(json.loads(self.f.record_path.read_text())['input_pending_native_context']['complete'])
        self.hook('UserPromptSubmit',b_receipt['submitted_text'])
        self.f.claude_hook('TurnStarted',prompt=self.receipt['submitted_text'],agent_id='main',turn_id=0,origin_kind='user')
        c=self.f.send(self.f.payload(text=b['text']))
        self.assertEqual(c['submitted_text'],b_receipt['submitted_text'])
        self.native(b_receipt['submitted_text'])
        self.inspect()
        self.assertFalse(json.loads(self.f.record_path.read_text())['input_pending_native_context']['complete'])

    def test_late_typed_a_cannot_make_b_ordering_complete_for_c(self):
        self.assert_late_typed_a_cannot_make_b_ordering_complete_for_c('Distinct B deliberately repeated as C')

    def test_identical_typed_a_cannot_make_b_ordering_complete_for_c(self):
        self.assert_late_typed_a_cannot_make_b_ordering_complete_for_c(self.original['text'])
