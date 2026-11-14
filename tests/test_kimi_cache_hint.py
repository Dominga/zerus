#!/usr/bin/env python3
"""Kimi cache chooser: synthetic 2.6.2 screen on a private terminal.

The fixture matches cache-hint-dialog.ts embedded in the sealed official 2.6.2
image (SHA-256 c2070d77947b000016d1bc9c189f2e258c51f29d5f9eb537388cc44fa61c5856).
The 80- and 64-column fixtures include Kimi's outer 1+1 gutter and exact ASCII
width truncation. They contain no owner-session output; these tests make no
provider/model request.
"""
import hashlib, json, os, signal, subprocess, unittest, uuid
from pathlib import Path
import test_input as fixtures
from test_questions import Questions
FAKE = fixtures.SIGNAL_SAFE_READ + r'''
import json,os,pathlib,signal,sys,termios,tty
root=pathlib.Path(os.environ['INPUT_FIXTURE']);fd=sys.stdin.fileno();original=termios.tcgetattr(fd);tty.setraw(fd)
os.write(1,b'\x1b[?2004h');selected=0;buf=b''
labels=['Compact and continue','Start a new session','Continue as-is',"Don't ask me again"]
def render(*args):
 global selected
 if (root/'selection').exists(): selected=int((root/'selection').read_text());(root/'selection').unlink()
 if (root/'screen').exists():
  os.write(1,(root/'screen').read_bytes());return
 lines=['─'*96,' This session has been idle for 2h and is ~217k tokens.',' ↑↓ navigate · Enter select · Esc cancel','',' Cache expired — the next message re-sends the entire history at full price.']
 descriptions=['one-time compact cost · cheapest way to keep this topic','zero context cost · best for a new task','full history kept · highest cost per turn','']
 lines+=['  '+('❯' if selected==i else ' ')+' '+label+(' '*(max(map(len,labels))-len(label)+4)+descriptions[i] if descriptions[i] else '') for i,label in enumerate(labels)]
 lines+=['','─'*96,' Ask When Needed K3 thinking: max [1 task running] /work/example main',' context: 22% (217k/1M)']
 os.write(1,b'\x1b[2J\x1b[H'+'\r\n'.join(lines).encode())
signal.signal(signal.SIGUSR1,render);render();(root/'agent-pid').write_text(str(os.getpid()))
def navigating():
 replacement=root/'replace-parent-on-navigation'
 if replacement.exists():
  change=json.loads(replacement.read_text());path=pathlib.Path(change['path'])
  record=json.loads(path.read_text());record['parent']=change['parent']
  temporary=path.with_suffix('.fixture-tmp');temporary.write_text(json.dumps(record));temporary.replace(path)
  replacement.unlink()
try:
 while True:
  value=read_input(fd)
  if not value:break
  with (root/'received').open('ab') as f:f.write(value)
  buf+=value
  while buf:
   if buf.startswith((b'\x1b[A',b'\x1bOA')):selected=max(0,selected-1);buf=buf[3:];navigating();render()
   elif buf.startswith((b'\x1b[B',b'\x1bOB')):selected=min(3,selected+1);buf=buf[3:];navigating();render()
   elif buf.startswith(b'\r'):
    buf=buf[1:]
    if (root/'cache-on-submit').exists():
     (root/'cache-on-submit').unlink()
     (root/'screen').unlink(missing_ok=True);render();continue
    (root/'submitted').write_text(labels[selected])
    if not (root/'hold-dialog').exists():os.write(1,b'\x1b[2J\x1b[HAgent working')
   elif buf[0]==27 and len(buf)<3:break
   else:buf=buf[1:]
finally:termios.tcsetattr(fd,termios.TCSANOW,original)
'''
class CacheHint(unittest.TestCase):
    script=fixtures.InputTransport.script
    tmux=fixtures.InputTransport.tmux
    wait_for=fixtures.InputTransport.wait_for
    received=fixtures.InputTransport.received
    write_record=fixtures.InputTransport.write_record
    tearDown=fixtures.InputTransport.tearDown
    def inspect(self):
        result=subprocess.run([str(fixtures.HGS),'inspect',self.name,'--current-read'],
                              env=self.env,text=True,capture_output=True,timeout=12)
        self.assertEqual(result.returncode,0,result.stderr)
        return json.loads(result.stdout)
    answer=Questions.answer
    def setUp(self):
        original=fixtures.FAKE_AGENT
        try:
            fixtures.FAKE_AGENT=FAKE;fixtures.InputTransport.setUp(self)
        finally:fixtures.FAKE_AGENT=original
        self.record.update(agent='kimi',active_tools={},phase='idle',activity='idle');self.write_record()
        self.card=self.inspect()['pending_questions'][0]
    def payload(self,choice=0,**updates):
        p=dict(request_id=str(uuid.uuid4()),expected_run_id=self.run_id,expected_conversation_id=self.conversation_id,
               question_id=self.card['question_id'],expected_question_hash=self.card['question_hash'],
               answers=[dict(question_id='cache',selected_option_ids=['cache_'+str(choice)],text='')]);p.update(updates);return p
    def test_detects_pending_choice_and_does_not_send_anything(self):
        self.assertTrue(self.card['can_answer']);self.assertEqual(self.card['source'],'kimi_cache_hint')
        self.assertFalse(self.card['questions'][0]['allow_other']);self.assertEqual(self.inspect()['phase'],'input')
        self.assertEqual(self.received(),b'')
        p=fixtures.InputTransport.payload(self)
        e=fixtures.InputTransport.send(self,p,success=False)
        self.assertIn('context choice',e);self.assertEqual(self.received(),b'')
    def test_input_summary_names_the_choice_from_both_busy_and_idle_records(self):
        for activity, phase in [('busy','working'),('idle','idle')]:
            self.record.update(activity=activity,phase=phase);self.write_record()
            before=self.record_path.read_bytes();received=self.received()
            info=self.inspect()
            listing=subprocess.run([str(fixtures.HGS),'ls','--local','--json'],env=self.env,
                                   text=True,capture_output=True,timeout=12)
            self.assertEqual(listing.returncode,0,listing.stderr)
            row=next(r for r in json.loads(listing.stdout)['sessions'] if r['name']==self.name)
            for shown in (info,row):
                self.assertEqual(shown['phase'],'input')
                self.assertEqual(shown['activity_summary'],'Needs attention',activity)
            self.assertEqual(len(info['pending_questions']),1)
            self.assertEqual(info['pending_questions'][0]['source'],'kimi_cache_hint')
            self.assertEqual(self.record_path.read_bytes(),before)
            self.assertEqual(self.received(),received)
    def test_context_actions_wait_for_the_choice_as_dispatch_does(self):
        # The saved record stays idle; the terminal chooser alone keeps both
        # actions unavailable, with the reason, as dispatch refuses them.
        info=self.inspect();self.assertEqual((info['activity'],info['phase']),('busy','input'))
        for action in ('clear_context','compact_context'):
            self.assertFalse(info[action+'_supported'],action)
            self.assertEqual(info[action+'_reason'],"Wait until the agent's question is answered",action)
        for command in ('clear-context','compact-context'):
            request=dict(request_id=str(uuid.uuid4()),expected_run_id=self.run_id,expected_conversation_id=self.conversation_id)
            r=subprocess.run([str(fixtures.HGS),command,self.name,'--json'],env=self.env,input=json.dumps(request),
                             text=True,capture_output=True,timeout=12)
            self.assertNotEqual(r.returncode,0,command);self.assertIn('context choice',r.stderr,command)
        self.assertEqual(self.received(),b'')
    def test_fork_and_settings_wait_for_the_choice_as_dispatch_does(self):
        # The saved record is Ready; the terminal chooser alone keeps a fork
        # unavailable and schedules a settings change, in inspect and the list.
        self.record.update(base=['kimi']);self.write_record()
        reason="Wait until the agent's question is answered"
        ls=subprocess.run([str(fixtures.HGS),'ls','--local','--json'],env=self.env,text=True,capture_output=True,timeout=12)
        self.assertEqual(ls.returncode,0,ls.stderr)
        row=next(r for r in json.loads(ls.stdout)['sessions'] if r['name']==self.name)
        for info in (self.inspect(),row):
            self.assertEqual((info['fork_supported'],info['fork_reason']),(False,reason))
            self.assertNotIn('fork',info['allowed_actions']);self.assertEqual(info['action_reasons']['fork'],reason)
            self.assertEqual(info['settings_apply_when'],'ready')
        r=subprocess.run([str(fixtures.HGS),'fork',self.name,'-n','branch','-d','--dry-run'],env=self.env,
                         text=True,capture_output=True,timeout=12)
        self.assertNotEqual(r.returncode,0,r.stdout);self.assertIn('context choice',r.stderr)
        self.assertEqual(self.received(),b'')
    def test_all_native_options_and_stable_identity_during_navigation(self):
        for choice in range(4):
            with self.subTest(choice=choice):
                (self.root/'selection').write_text('3');os.kill(self.record['pid'],signal.SIGUSR1)
                self.wait_for(lambda:'❯ Don\'t ask' in self.tmux('capture-pane','-p','-t',self.pane))
                self.assertEqual(self.inspect()['pending_questions'][0]['question_hash'],self.card['question_hash'])
                p=self.payload(choice);r=self.answer(p)
                self.assertEqual(r['status'],'answered')
                self.assertEqual((self.root/'submitted').read_text(),self.card['questions'][0]['options'][choice]['label'])
                before=self.received();self.assertEqual(self.answer(p),r);self.assertEqual(self.received(),before)
                self.assertEqual(self.inspect()['pending_questions'],[])
    def test_stale_question_and_free_text_are_rejected_without_keys(self):
        self.answer(self.payload(expected_question_hash='0'*64),success=False)
        self.answer(self.payload(expected_run_id='other'),success=False)
        self.answer(self.payload(answers=[dict(question_id='cache',selected_option_ids=[],text='anything')]),success=False)
        self.assertEqual(self.received(),b'')
    def test_local_agent_cannot_answer_the_context_choice(self):
        self.env.update(HGS_SESSION='codex/fixture/agent',HGS_RUN_ID=str(uuid.uuid4()))
        request=self.payload()
        self.assertIn("person's answer",self.answer(request,success=False))
        self.assertEqual(self.received(),b'')
        self.assertFalse((self.state/'question_receipts'/(request['request_id']+'.json')).exists())

    def cache_parent(self):
        # This private test process really owns hgs's ancestry. Markers alone
        # are not proof; use the production parent-adopt path to record it.
        start = subprocess.check_output(['ps', '-p', str(os.getpid()), '-o', 'lstart='],
            env=dict(self.env, LC_ALL='C', TZ='UTC'), text=True).strip()
        parent = dict(version=1, name='codex/fixture/cache-parent', agent='codex',
            run_id='cache-parent-run', conversation_id='cache-parent-conversation',
            session_id='22222222-2222-4222-8222-222222222222',
            pid=os.getpid(), process_start=start, subagents={})
        self.write_cache_caller(parent)
        initialized = subprocess.run([str(fixtures.HGS), 'reference', parent['name'], '--json'],
            env=self.env, capture_output=True, text=True, timeout=12)
        self.assertEqual(initialized.returncode, 0, initialized.stderr)
        self.env.update(HGS_SESSION=parent['name'], HGS_RUN_ID=parent['run_id'],
                        HGS_SESSION_ID=parent['session_id'])
        adopted = subprocess.run([str(fixtures.HGS), 'parent', self.name, '--json'], env=self.env,
            input=json.dumps(dict(action='adopt', expected_run_id=self.run_id,
                                  expected_conversation_id=self.conversation_id)),
            capture_output=True, text=True, timeout=12)
        self.assertEqual(adopted.returncode, 0, adopted.stderr)
        self.record = json.loads(self.record_path.read_text())
        self.assertEqual(self.record['parent']['session_id'], parent['session_id'])
        # Adopt above proves the actual native caller and resolves its IDs.
        # This fixture models a child whose launcher recorded that same proof;
        # production adoption must never grant the Compact exception.
        self.record['parent']['source'] = 'launch'
        self.write_record()
        return parent

    def write_cache_caller(self, parent):
        path = self.state/(hashlib.sha256(parent['name'].encode()).hexdigest()+'.json')
        path.write_text(json.dumps(parent))

    def test_verified_parent_compacts_once_and_replay_sends_nothing(self):
        self.cache_parent()
        request = self.payload()
        receipt = self.answer(request)
        self.assertEqual(receipt['status'], 'answered')
        self.assertEqual((self.root/'submitted').read_text(), 'Compact and continue')
        before = self.received()
        self.assertEqual(before, b'\r')
        self.assertEqual(self.answer(request), receipt)
        self.assertEqual(self.received(), before)

    def test_parent_compact_keeps_the_original_stashed_input_receipt(self):
        original, _ = self.held_input()
        self.card = self.inspect()['pending_questions'][0]
        path = self.state/'input_receipts'/(original['request_id']+'.json')
        before = path.read_bytes()
        self.cache_parent()
        request = self.payload()
        receipt = self.answer(request)
        self.assertEqual(receipt['status'], 'answered')
        received = self.received()
        self.assertEqual(received.count(b'Original held message'), 1)
        self.assertEqual(received.count(b'\r'), 2)
        self.assertEqual(path.read_bytes(), before)
        self.assertEqual(self.answer(request), receipt)
        self.assertEqual(self.received(), received)

    def test_changed_parent_during_navigation_stops_before_enter_and_never_replays(self):
        self.cache_parent()
        (self.root/'selection').write_text('3')
        os.kill(self.record['pid'], signal.SIGUSR1)
        self.wait_for(lambda:"❯ Don't ask" in self.tmux('capture-pane','-p','-t',self.pane))
        replacement = dict(self.record['parent'], session_id='33333333-3333-4333-8333-333333333333')
        (self.root/'replace-parent-on-navigation').write_text(json.dumps(
            dict(path=str(self.record_path), parent=replacement)))
        request = self.payload()
        self.assertIn('delivery uncertain', self.answer(request, success=False))
        before = self.received()
        self.assertIn(before, (b'\x1b[A', b'\x1bOA'))
        self.assertFalse((self.root/'submitted').exists())
        self.assertIn('delivery uncertain', self.answer(request, success=False))
        self.assertEqual(self.received(), before)

    def test_verified_parent_cannot_choose_other_context_options(self):
        self.cache_parent()
        for choice in (1, 2, 3):
            with self.subTest(choice=choice):
                request = self.payload(choice)
                self.answer(request, success=False)
                self.assertFalse((self.state/'question_receipts'/(request['request_id']+'.json')).exists())
        self.assertEqual(self.received(), b'')

    def test_adopted_manual_claimed_and_forwarded_parents_cannot_compact(self):
        self.cache_parent()
        for source in ('adopt', 'manual', 'claimed', 'forwarded'):
            with self.subTest(source=source):
                self.record['parent']['source'] = source
                self.write_record()
                request = self.payload()
                self.answer(request, success=False)
                self.assertFalse((self.state/'question_receipts'/(request['request_id']+'.json')).exists())
        self.assertEqual(self.received(), b'')

    def test_parent_compact_rejects_wrong_child_question_run_and_conversation(self):
        self.cache_parent()
        for changed in (dict(expected_question_hash='0'*64), dict(expected_run_id='wrong-run'),
                        dict(expected_conversation_id='wrong-conversation'), dict(question_id='kimi-cache:other')):
            with self.subTest(changed=changed):
                self.answer(self.payload(**changed), success=False)
        self.assertEqual(self.received(), b'')

    def test_parent_compact_requires_real_caller_proof(self):
        parent = self.cache_parent()
        for pid, birth in ((self.record['pid'], self.record['process_start']),
                           (os.getpid(), 'stale-parent-birth')):
            with self.subTest(pid=pid, birth=birth):
                # A different live process is not our ancestor; the original
                # parent PID with a stale birth is not its original process.
                self.write_cache_caller(dict(parent, pid=pid, process_start=birth))
                self.answer(self.payload(), success=False)
        self.assertEqual(self.received(), b'')

    def test_unrelated_sibling_and_grandparent_cannot_compact(self):
        parent = self.cache_parent()
        for number, relation in enumerate(('unrelated', 'sibling', 'grandparent'), 3):
            other = dict(parent, name='codex/fixture/'+relation, run_id=relation+'-run',
                conversation_id=relation+'-conversation',
                session_id=f'{number}'*8+'-3333-4333-8333-'+f'{number}'*12)
            if relation == 'sibling':
                other['parent'] = self.record['parent']
            if relation == 'grandparent':
                parent['parent'] = dict(self.record['parent'], **{k:other[k] for k in
                    ('name', 'session_id', 'run_id', 'conversation_id')})
                self.write_cache_caller(parent)
            self.write_cache_caller(other)
            self.env.update(HGS_SESSION=other['name'], HGS_RUN_ID=other['run_id'],
                            HGS_SESSION_ID=other['session_id'])
            with self.subTest(relation=relation):
                proven = subprocess.run([str(fixtures.HGS), 'self', '--json'], env=self.env,
                    capture_output=True, text=True, timeout=12)
                self.assertEqual(proven.returncode, 0, proven.stderr)
                identity = json.loads(proven.stdout)
                self.assertEqual(identity['identity_source'], 'process')
                self.assertEqual(identity['session_id'], other['session_id'])
                self.answer(self.payload(), success=False)
        self.assertEqual(self.received(), b'')

    def test_parent_compact_requires_exact_local_parent_binding(self):
        self.cache_parent()
        original = self.record['parent']
        for field, value in [('machine', '99999999-9999-4999-8999-999999999999'),
                             ('session_id', '33333333-3333-4333-8333-333333333333'),
                             ('run_id', 'different'), ('conversation_id', 'different'),
                             ('run_id', ''), ('conversation_id', ''), ('session_id', ''),
                             ('source', 'forwarded'), ('schema', 2)]:
            with self.subTest(field=field, value=value):
                self.record['parent'] = dict(original, **{field:value})
                self.write_record()
                self.answer(self.payload(), success=False)
        self.record.pop('parent'); self.write_record()
        self.answer(self.payload(), success=False)
        self.assertEqual(self.received(), b'')

    def test_missing_acknowledgement_is_uncertain_and_never_retried(self):
        (self.root/'hold-dialog').touch();p=self.payload()
        self.assertIn('delivery uncertain',self.answer(p,success=False));before=self.received()
        self.assertIn('delivery uncertain',self.answer(p,success=False));self.assertEqual(self.received(),before)

    def held_input(self):
        # Actually deliver an original input once to this private raw terminal.
        # On Enter the native stand-in stashes it behind the exact chooser.
        self.redraw('\x1b[2J\x1b[H ╭──────────────╮\r\n │ >          │\r\n ╰──────────────╯\x1b[2;6H')
        (self.root/'cache-on-submit').touch()
        payload=fixtures.InputTransport.payload(self,text='Original held message')
        receipt=fixtures.InputTransport.send(self,payload)
        self.assertEqual(receipt['status'],'submitted')
        self.wait_for(lambda:'Cache expired' in self.tmux('capture-pane','-p','-t',self.pane))
        self.record=json.loads(self.record_path.read_text())
        return payload,receipt

    def redraw(self,text):
        (self.root/'screen').write_text(text)
        os.kill(self.record['pid'],signal.SIGUSR1)
        # A title written after the screen is an output-order barrier.
        title='cache-frame-'+uuid.uuid4().hex
        (self.root/'screen').write_bytes(text.encode()+b'\x1b]2;'+title.encode()+b'\x1b\\')
        os.kill(self.record['pid'],signal.SIGUSR1)
        self.wait_for(lambda:self.tmux('display-message','-p','-t',self.pane,'#{pane_title}').strip()==title)

    def test_submitted_and_uncertain_receipts_and_original_input_are_preserved(self):
        payload,receipt=self.held_input()
        self.assertEqual(self.record['input_pending_request'],payload['request_id'])
        self.assertEqual(self.record['input_pending_at'],receipt['submitted_at'])
        self.assertLessEqual(self.record['input_seen_at'],receipt['submitted_at'])
        path=self.state/'input_receipts'/(payload['request_id']+'.json')
        received=self.received();record=self.record_path.read_bytes()
        self.assertEqual(received.count(b'Original held message'),1)
        self.assertEqual(received.count(b'\r'),1)
        for uncertain in (False,True):
            if uncertain:
                saved=json.loads(path.read_text());saved['status']='in_progress';saved['delivery']='uncertain'
                path.write_text(json.dumps(saved))
            before=path.read_bytes()
            for _ in range(2):
                info=self.inspect();self.assertEqual((info['activity'],info['phase']),('busy','input'))
                self.assertTrue(info['current_read']['attention_signature'])
                card=info['pending_questions'][0];self.assertTrue(card['can_answer'])
                self.assertEqual(card['input_pending_request'],payload['request_id'])
            # Same-UUID submitted replay returns its receipt; uncertain replay
            # refuses. Neither repeats any terminal input or changes its receipt.
            fixtures.InputTransport.send(self,payload,success=not uncertain)
            self.assertEqual(path.read_bytes(),before)
            self.assertEqual(self.record_path.read_bytes(),record)
            self.assertEqual(self.received(),received)

    def test_person_can_choose_once_without_retyping_the_original_pending_input(self):
        payload,_=self.held_input();self.card=self.inspect()['pending_questions'][0]
        receipt=self.state/'input_receipts'/(payload['request_id']+'.json')
        original=receipt.read_bytes();request=self.payload(2)
        result=self.answer(request);self.assertEqual(result['status'],'answered')
        self.assertEqual((self.root/'submitted').read_text(),'Continue as-is')
        received=self.received()
        self.assertEqual(received.count(b'Original held message'),1)
        self.assertEqual(received.count(b'\r'),2)  # original submit, explicit person selection
        self.assertEqual(self.answer(request),result)
        self.assertEqual(self.received(),received)
        self.assertEqual(receipt.read_bytes(),original)
        self.assertEqual(json.loads(self.record_path.read_text())['input_pending_request'],payload['request_id'])
        self.assertEqual(self.inspect()['pending_questions'],[])

    def test_mismatched_original_receipt_keeps_pending_input_without_claiming_a_chooser(self):
        payload,_=self.held_input();received=self.received();record=self.record_path.read_bytes()
        path=self.state/'input_receipts'/(payload['request_id']+'.json')
        receipt=json.loads(path.read_text());receipt['submitted_at']-=1
        path.write_text(json.dumps(receipt));before=path.read_bytes()
        self.assertEqual(self.inspect()['pending_questions'],[])
        self.assertEqual(path.read_bytes(),before)
        self.assertEqual(self.record_path.read_bytes(),record)
        self.assertEqual(self.received(),received)

    def test_chooser_gone_clears_attention_without_accepting_or_retyping_input(self):
        self.held_input();received=self.received();record=self.record_path.read_bytes()
        self.redraw('\x1b[2J\x1b[HAgent working')
        info=self.inspect();self.assertEqual(info['pending_questions'],[])
        self.assertNotEqual(info['phase'],'input')
        self.assertEqual(self.record_path.read_bytes(),record)
        self.assertEqual(self.received(),received)

    def test_earlier_native_turn_end_does_not_clear_the_held_message(self):
        payload,receipt=self.held_input();received=self.received()
        self.record.update(activity='busy',phase='working',main_done=False,
                           recovery_user_at=receipt['submitted_at']-20,
                           kimi_turn_clock=dict(run_id=self.run_id,conversation_id=self.conversation_id,started=0))
        self.write_record();before=self.record_path.read_bytes()
        wire=self.root/'.kimi-code/sessions/fixture'/self.conversation_id/'agents/main/wire.jsonl'
        wire.parent.mkdir(parents=True)
        wire.write_text(''.join(json.dumps(e)+'\n' for e in [
            dict(type='turn.prompt',agentId='main',turnId=0,promptId='msg_old',time=int((receipt['submitted_at']-10)*1000)),
            dict(type='turn.ended',agentId='main',turnId=0,reason='completed',time=int((receipt['submitted_at']-5)*1000))]))
        info=self.inspect();self.assertEqual(info['phase'],'input')
        self.assertEqual(info['pending_questions'][0]['input_pending_request'],payload['request_id'])
        self.assertEqual(self.record_path.read_bytes(),before)
        self.assertEqual(self.received(),received)

    def test_changed_pending_request_refuses_an_old_answer_without_keys(self):
        self.held_input();self.card=self.inspect()['pending_questions'][0]
        request=self.payload();received=self.received()
        self.record['input_pending_request']=str(uuid.uuid4());self.write_record()
        self.answer(request,success=False)
        self.assertEqual(self.received(),received)

    def test_new_acceptance_clears_attention_even_before_the_old_screen_redraws(self):
        self.held_input();received=self.received()
        event=dict(hook_event_name='UserPromptSubmit',session_id=self.conversation_id,prompt='Original held message')
        result=subprocess.run([str(fixtures.HGS),'__state','hook'],
                              env=dict(self.env,HGS_SESSION=self.name,HGS_RUN_ID=self.run_id),
                              input=json.dumps(event),text=True,capture_output=True,timeout=10)
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertEqual(self.inspect()['pending_questions'],[])
        self.assertEqual(self.received(),received)

    def test_explicit_permission_attention_keeps_precedence(self):
        self.held_input();received=self.received()
        self.record.update(phase='approval',activity='busy',
                           active_tools={'permission':dict(name='Shell',detail='fixture command')})
        self.write_record()
        info=self.inspect();self.assertEqual(info['phase'],'approval')
        self.assertFalse(any(q['source']=='kimi_cache_hint' for q in info['pending_questions']))
        self.assertEqual(self.received(),received)

    def test_a_read_only_dialog_does_not_require_bracketed_paste_mode(self):
        payload,_=self.held_input();received=self.received()
        # Editor input is deliberately disabled while its replacement is shown.
        screen=Path(__file__).with_name('fixtures').joinpath('kimi-cache-expired-2.6.2.txt').read_text()
        self.tmux('resize-window','-t',self.name,'-x','120','-y','30')
        self.redraw('\x1b[?2004l\x1b[2J\x1b[H'+screen.replace('\n','\r\n'))
        self.assertEqual(self.tmux('display-message','-p','-t',self.pane,'#{bracket_paste_flag}').strip(),'0')
        info=self.inspect();self.assertEqual(info['phase'],'input')
        self.assertFalse(info['pending_questions'][0]['can_answer'])
        self.assertEqual(info['pending_questions'][0]['input_pending_request'],payload['request_id'])
        self.assertTrue(info['terminal_supported'])
        listing=subprocess.run([str(fixtures.HGS),'ls','--local','--json'],env=self.env,
                               text=True,capture_output=True,timeout=12)
        self.assertEqual(listing.returncode,0,listing.stderr)
        row=next(row for row in json.loads(listing.stdout)['sessions'] if row['name']==self.name)
        self.assertEqual(row['phase'],'input');self.assertTrue(row['attention_signature'])
        self.assertEqual(self.received(),received)

    def test_width_truncated_dialog_keeps_the_original_input_without_keys(self):
        payload,_=self.held_input();received=self.received()
        record=self.record_path.read_bytes()
        path=self.state/'input_receipts'/(payload['request_id']+'.json')
        receipt=path.read_bytes()
        self.tmux('resize-window','-t',self.name,'-x','120','-y','24')
        full=Path(__file__).with_name('fixtures').joinpath('kimi-cache-expired-2.6.2.txt').read_text()
        self.redraw('\x1b[2J\x1b[H'+full.replace('\n','\r\n'))
        full_hash=self.inspect()['pending_questions'][0]['question_hash']
        for columns in (80,64):
            with self.subTest(columns=columns):
                self.tmux('resize-window','-t',self.name,'-x',str(columns),'-y','24')
                screen=Path(__file__).with_name('fixtures').joinpath(f'kimi-cache-expired-2.6.2-{columns}.txt').read_text()
                self.redraw('\x1b[?2004l\x1b[2J\x1b[H'+screen.replace('\n','\r\n'))
                for _ in range(2):
                    info=self.inspect();self.assertEqual(info['phase'],'input')
                    card=info['pending_questions'][0]
                    self.assertEqual(card['source'],'kimi_cache_hint')
                    self.assertEqual(card['question_hash'],full_hash)
                    self.assertEqual(card['input_pending_request'],payload['request_id'])
                    self.assertFalse(card['can_answer'])
                    self.assertTrue(info['current_read']['attention_signature'])
                self.assertEqual(path.read_bytes(),receipt)
                self.assertEqual(self.record_path.read_bytes(),record)
                self.assertEqual(self.received(),received)

    def test_partial_or_changed_panels_do_not_raise_attention(self):
        self.tmux('resize-window','-t',self.name,'-x','120','-y','30')
        screen=Path(__file__).with_name('fixtures').joinpath('kimi-cache-expired-2.6.2.txt').read_text()
        for before,after in [('Start a new session','Start another session'),
                             ('highest cost per turn','highest cost per turn…'),
                             ("Don't ask me again",''),('260k','NaN')]:
            self.redraw('\x1b[2J\x1b[H'+screen.replace(before,after).replace('\n','\r\n'))
            self.assertEqual(self.inspect()['pending_questions'],[])
        self.assertEqual(self.received(),b'')

if __name__=='__main__':unittest.main()
