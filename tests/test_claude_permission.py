#!/usr/bin/env python3
"""Claude's auto-mode offer on an isolated terminal; no real settings change."""
import json
import os
import signal
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import unittest
import uuid
from pathlib import Path

import test_input as fixtures
import test_questions as questions
import test_claude_trust as trust

# The fixture's own decline label names the mode it keeps.
FAKE = trust.FAKE.replace("labels=['No, exit','Yes, I trust this folder']",
    "labels=['Yes, set auto mode as my default permission mode',next(row.strip().lstrip('❯ ') for row in "
    "pathlib.Path(os.environ['CLAUDE_TRUST_FIXTURE']).read_text().splitlines() if 'No, keep' in row)]")
FAKE = FAKE.replace("'git config'", "'bypass permissions'")
FAKE = FAKE.replace("screen.replace('❯ No, exit','  No, exit').replace('  Yes, I trust','❯ Yes, I trust')",
    "screen.replace('❯ Yes, set','  Yes, set').replace('  No, keep','❯ No, keep')")
FAKE = FAKE.replace('     if selected==0:sys.exit(0)\n','')
# Claude's composer replaces the answered offer; `suffix` draws it below one.
COMPOSER = '\n'+'─'*80+'\n❯ \n'+'─'*80+'\n  ⏸ manual mode on · ? for shortcuts · ← for agents\n'
FAKE = FAKE.replace("if (root/'prelude').exists():screen=(root/'prelude').read_text()+screen",
    "if (root/'prelude').exists():screen=(root/'prelude').read_text()+screen\n if (root/'suffix').exists():screen+=(root/'suffix').read_text()")
FAKE = FAKE.replace(r"os.write(1,b'\x1b[2J\x1b[HReady for your first message')",
    r"os.write(1,b'\x1b[2J\x1b[H'+(((root/'prelude').read_text() if (root/'prelude').exists() else '')+"
    +repr(COMPOSER)+r").replace('\n','\r\n').encode())")
assert 'suffix' in FAKE and 'Ready for your first message' not in FAKE
AFTER_REPLY = Path(__file__).parent/'fixtures/claude-auto-mode-after-reply.txt'


def guarded_agent():
    """review-q2's paint/Enter probe, with a shared clock instead of a race.

    The model is the reported 150 ms guard, not independent evidence of that
    value in Claude. The fixture clock advances only when hgs waits; its pipe
    barrier ensures the fake has painted before hgs observes the next frame.
    """
    helpers = r'''
clock_path=root/'native-clock'
os.mkfifo(root/'clock-events',0o600)
clock_events=os.open(root/'clock-events',os.O_RDWR|os.O_NONBLOCK)
def clock_now():
 return int.from_bytes(clock_path.read_bytes()[:8],sys.byteorder) if clock_path.exists() else 0
def input_stamp():
 return int.from_bytes((root/'queued-enter').read_bytes(),sys.byteorder)
def acknowledge():
 if clock_path.exists():
  with clock_path.open('r+b',buffering=0) as f:f.seek(8);f.write(clock_now().to_bytes(8,sys.byteorder))
def variant():
 return (root/'change-in-guard').exists() and 0<clock_now()<80000000
def clock_tick():
 if variant()!=paint_variant:render()
 acknowledge()
'''
    fake=FAKE.replace('def render(*args):',helpers+'\ndef render(*args):')
    fake=fake.replace('select.select([fd, wake], [], [])','select.select([fd, wake, clock_events], [], [])')
    fake=fake.replace('        if fd in ready:',
        '        if clock_events in ready:\n            os.read(clock_events,4096);clock_tick()\n        if fd in ready:')
    paint=r" os.write(1,b'\x1b[2J\x1b[H'+screen.replace('\n','\r\n').encode())"
    assert fake.count(paint)==1
    fake=fake.replace(paint,
        " if variant():screen=screen.replace('No, keep ','No, keep changed ')\n"+paint+
        "\n globals()['painted']=clock_now();globals()['paint_variant']=variant();acknowledge()")
    enter="   elif buf.startswith(b'\\r'):\n    buf=buf[1:];"
    assert fake.count(enter)==1
    fake=fake.replace(enter,
        "   elif buf.startswith(b'\\r') and input_stamp()-painted<150000000:\n"
        "    buf=buf[1:];painted=clock_now()\n"
        "    with (root/'refused').open('a') as f:f.write('enter\\n')\n"+enter)
    fake=fake.replace('  while buf:\n',
        "  while buf:\n   if buf.startswith(b'\\r'):\n"
        "    with (root/'enter-clock').open('a') as f:f.write(str(input_stamp())+'\\n')\n")
    return fake

class PermissionMode(unittest.TestCase):
    script = fixtures.InputTransport.script
    tmux = fixtures.InputTransport.tmux
    wait_for = fixtures.InputTransport.wait_for
    received = fixtures.InputTransport.received
    write_record = fixtures.InputTransport.write_record
    tearDown = fixtures.InputTransport.tearDown
    inspect = questions.Questions.inspect
    answer = questions.Questions.answer
    redraw = trust.FolderTrust.redraw

    def setUp(self):
        # Claude Code 2.1.296 and later also make the offer under a finished reply.
        after_reply='_after_reply' in self._testMethodName
        os.environ['CLAUDE_TRUST_FIXTURE']=str(AFTER_REPLY if after_reply else Path(__file__).parent/'fixtures/claude-auto-mode.txt')
        self.addCleanup(lambda:os.environ.pop('CLAUDE_TRUST_FIXTURE',None))
        original=fixtures.FAKE_AGENT
        try:
            fixtures.FAKE_AGENT=getattr(self,'agent_source',FAKE);fixtures.InputTransport.setUp(self)
        finally:fixtures.FAKE_AGENT=original
        self.record.update(agent='claude',run_identity_version=1,supervisor={},
            launch_dir='/work/example',phase='idle',activity='idle',last_event_at=time.time())
        if after_reply:self.record['turn_started']=time.time()-5
        self.write_record();self.card=self.inspect()['pending_questions'][0]

    def payload(self,choice=1,**updates):
        p=dict(request_id=str(uuid.uuid4()),expected_run_id=self.run_id,
            expected_conversation_id=self.conversation_id,question_id=self.card['question_id'],
            expected_question_hash=self.card['question_hash'],
            answers=[dict(question_id='permission_mode',selected_option_ids=['mode_'+str(choice)],text='')])
        p.update(updates);return p

    def test_after_session_start_attention_without_automatic_choice(self):
        state=self.inspect();self.assertEqual(state['phase'],'approval')
        self.assertEqual(state['attention_id'],self.card['question_id'])
        self.assertTrue(self.card['can_answer']);self.assertEqual(self.card['conversation_id'],self.conversation_id)
        self.assertIn('blocks the rest',self.card['questions'][0]['body'])
        self.assertEqual([o['label'] for o in self.card['questions'][0]['options']],
            ['Yes, set auto mode as my default permission mode','No, keep bypass permissions'])
        self.assertFalse(any(o.get('selected') for o in self.card['questions'][0]['options']))
        self.assertIn('permission mode choice',fixtures.InputTransport.send(self,fixtures.InputTransport.payload(self),success=False))
        self.assertEqual(self.received(),b'')

    def test_explicit_keep_mode_is_submitted_once(self):
        p=self.payload();receipt=self.answer(p);self.assertEqual(receipt['status'],'answered')
        self.assertEqual((self.root/'submitted').read_text(),'No, keep bypass permissions')
        self.assertEqual(self.received().count(b'\r'),1)
        before=self.received();self.assertEqual(self.answer(p),receipt);self.assertEqual(self.received(),before)
        self.assertEqual(self.inspect()['pending_questions'],[])

    def test_home_abbreviation_keeps_explicit_choice_and_exact_folder(self):
        self.record['launch_dir']=str(self.root/'work/example');self.write_record()
        self.redraw('folder','~/work/example','~/work/example')
        self.card=self.inspect()['pending_questions'][0]
        self.assertEqual(self.card['source'],'claude_permission_mode')
        self.assertTrue(self.card['can_answer']);self.assertEqual(self.received(),b'')
        p=self.payload();receipt=self.answer(p)
        self.assertEqual(receipt['status'],'answered')
        self.assertEqual((self.root/'submitted').read_text(),'No, keep bypass permissions')
        before=self.received();self.assertEqual(self.answer(p),receipt);self.assertEqual(self.received(),before)
        self.assertEqual(self.inspect()['pending_questions'],[])

    def test_home_abbreviation_does_not_match_an_unrelated_absolute_folder(self):
        self.redraw('folder','~/work/example','~/work/example')
        self.assertEqual(self.inspect()['pending_questions'],[])
        self.answer(self.payload(),success=False);self.assertEqual(self.received(),b'')

    def test_explicit_auto_navigates_back_from_current_selection(self):
        self.redraw('selection','1','❯ No, keep')
        self.assertEqual(self.inspect()['pending_questions'][0]['question_hash'],self.card['question_hash'])
        self.answer(self.payload(0));self.assertEqual((self.root/'submitted').read_text(),'Yes, set auto mode as my default permission mode')
        self.assertEqual(self.received().count(b'\r'),1)

    def test_before_session_start_is_also_answerable(self):
        self.conversation_id='';self.record['conversation_id']=None;self.record['last_event_at']=0
        self.write_record();self.card=self.inspect()['pending_questions'][0]
        self.assertEqual(self.answer(self.payload())['status'],'answered')

    def test_changed_mode_or_run_rejects_without_terminal_input(self):
        self.answer(self.payload(expected_run_id='other'),success=False)
        self.answer(self.payload(expected_conversation_id='other'),success=False)
        self.redraw('target','default permissions','No, keep default permissions')
        self.assertNotEqual(self.inspect()['pending_questions'][0]['question_hash'],self.card['question_hash'])
        self.answer(self.payload(),success=False);self.assertEqual(self.received(),b'')

    def test_quoted_history_or_wrong_folder_is_not_actionable(self):
        # After a turn, panel text above Claude's composer is only quoted.
        self.record['turn_started']=time.time();self.write_record()
        self.redraw('suffix',COMPOSER,'? for shortcuts')
        self.assertEqual(self.inspect()['pending_questions'],[])
        (self.root/'suffix').unlink();self.record.pop('turn_started');self.write_record()
        self.redraw('folder','/other','/other');self.assertEqual(self.inspect()['pending_questions'],[])
        self.answer(self.payload(),success=False);self.assertEqual(self.received(),b'')

    def test_missing_acknowledgement_is_never_retried(self):
        (self.root/'hold-dialog').touch();p=self.payload()
        self.assertIn('delivery uncertain',self.answer(p,success=False));before=self.received()
        self.assertEqual(before.count(b'\r'),1)
        self.assertIn('delivery uncertain',self.answer(p,success=False));self.assertEqual(self.received(),before)

    def test_after_reply_offer_is_answered_from_activity(self):
        state=self.inspect();self.assertEqual(state['phase'],'approval')
        self.assertTrue(self.card['can_answer']);self.assertEqual(self.card['conversation_id'],self.conversation_id)
        self.assertEqual([o['label'] for o in self.card['questions'][0]['options']],
            ['Yes, set auto mode as my default permission mode','No, keep manual mode'])
        self.assertIn('permission mode choice',fixtures.InputTransport.send(self,fixtures.InputTransport.payload(self),success=False))
        self.assertEqual(self.received(),b'')
        p=self.payload();receipt=self.answer(p);self.assertEqual(receipt['status'],'answered')
        self.assertEqual((self.root/'submitted').read_text(),'No, keep manual mode')
        self.assertEqual(self.received().count(b'\r'),1)
        before=self.received();self.assertEqual(self.answer(p),receipt);self.assertEqual(self.received(),before)
        self.assertEqual(self.inspect()['pending_questions'],[])

    def test_after_reply_quoted_offer_still_acknowledges_the_answer(self):
        # The conversation above Claude's composer quotes the answered offer.
        quoted='\n'.join(AFTER_REPLY.read_text().splitlines()[20:])
        (self.root/'prelude').write_text('● Quoting the offer:\n'+quoted+'\n\n● Quoted it above.\n')
        receipt=self.answer(self.payload(0));self.assertEqual(receipt['status'],'answered')
        self.assertEqual((self.root/'submitted').read_text(),'Yes, set auto mode as my default permission mode')
        self.assertEqual(self.received().count(b'\r'),1)
        screen=self.tmux('capture-pane','-p','-t',self.pane)
        self.assertIn('Make auto mode your default permission mode?',screen);self.assertIn('? for shortcuts',screen)

    def test_after_reply_a_later_turn_or_unbound_session_needs_a_new_card(self):
        self.record['turn_started']=time.time();self.write_record()
        self.assertNotEqual(self.inspect()['pending_questions'][0]['question_hash'],self.card['question_hash'])
        self.answer(self.payload(),success=False)
        self.record['conversation_id']=None;self.write_record()
        self.assertEqual(self.inspect()['pending_questions'],[])
        self.assertEqual(self.received(),b'')

@unittest.skipUnless(sys.platform=='linux','the controlled-clock fixture uses Linux preloading')
class PermissionModeGuard(unittest.TestCase):
    script = PermissionMode.script
    tmux = PermissionMode.tmux
    wait_for = PermissionMode.wait_for
    received = PermissionMode.received
    write_record = PermissionMode.write_record
    tearDown = PermissionMode.tearDown
    inspect = PermissionMode.inspect
    payload = PermissionMode.payload
    agent_source = guarded_agent()

    @classmethod
    def setUpClass(cls):
        cls.directory=tempfile.TemporaryDirectory(prefix='hgs-permission-clock-')
        cls.addClassCleanup(cls.directory.cleanup)
        cls.library=Path(cls.directory.name)/'clock.so'
        compiler=shutil.which('cc')
        if not compiler:raise AssertionError('the Linux guard control requires the native C compiler')
        source=Path(__file__).parent/'fixtures/claude_permission_clock.c'
        result=subprocess.run([compiler,'-std=c11','-Wall','-Wextra','-Werror','-shared','-fPIC',
                               '-O2',str(source),'-o',str(cls.library)],capture_output=True,text=True)
        if result.returncode:raise AssertionError(result.stderr)

    def setUp(self):
        PermissionMode.setUp(self)
        self.start_clock()

    def start_clock(self):
        self.clock=self.root/'native-clock'
        self.clock.write_bytes(struct.pack('=QQQ',0,2**64-1,0))
        os.kill(self.record['pid'],signal.SIGUSR1)
        self.wait_for(lambda:self.clock_state()[1]==0)
        # Preserve the key's causal timestamp: post-send acknowledgement polls
        # may advance the clock before the TUI consumes an already queued key.
        self.script('tmux','#!'+sys.executable+'\n'+
            'import os,sys\nfrom pathlib import Path\n'+
            f'CLOCK=Path({str(self.clock)!r})\nSTAMP=Path({str(self.root/"queued-enter")!r})\n'+
            'args=sys.argv[1:]\n'+
            "if args and args[0]=='send-keys':STAMP.write_bytes(CLOCK.read_bytes()[:8])\n"+
            f'os.execv({fixtures.TMUX!r},[{fixtures.TMUX!r},"-S",{str(self.socket)!r},"-f","/dev/null",*args])\n')

    def clock_state(self):
        return struct.unpack('=QQQ',self.clock.read_bytes())

    def call(self,payload):
        events=os.open(self.root/'clock-events',os.O_WRONLY|os.O_NONBLOCK|os.O_NOFOLLOW)
        try:
            env=dict(self.env,LD_PRELOAD=str(self.library),TEST_CLAUDE_CLOCK=str(self.clock),
                     TEST_CLAUDE_CLOCK_EVENTS=str(events))
            env.pop('TEST_CLAUDE_CLOCK_OWNER',None)
            result=subprocess.run([str(questions.HGS),'answer',self.name,'--json'],
                input=json.dumps(payload),capture_output=True,text=True,env=env,
                pass_fds=(events,),timeout=20)
        finally:os.close(events)
        self.assertGreater(self.clock_state()[2],0,'the CLI did not load the fixture clock')
        return result

    def test_fresh_paint_waits_out_the_native_guard_before_its_one_enter(self):
        payload=self.payload(0)
        result=self.call(payload)
        self.assertEqual(result.returncode,0,result.stderr)
        receipt=json.loads(result.stdout)
        self.assertEqual(receipt['status'],'answered')
        self.assertFalse((self.root/'refused').exists())
        self.assertEqual((self.root/'submitted').read_text(),
                         'Yes, set auto mode as my default permission mode')
        self.assertEqual((self.root/'enter-clock').read_text(),'150000000\n')
        self.assertEqual(self.received(),b'\r')
        repeated=self.call(payload)
        self.assertEqual(json.loads(repeated.stdout),receipt)
        self.assertEqual(self.received(),b'\r')

    def test_a_panel_change_inside_the_guard_refuses_before_enter(self):
        # It changes at 40 ms and returns at 80 ms. A final check alone would
        # miss it: the in-guard observation must refuse before any Enter.
        (self.root/'change-in-guard').touch()
        result=self.call(self.payload(0))
        self.assertNotEqual(result.returncode,0)
        self.assertEqual(self.received(),b'',result.stderr)
        self.assertIn('Terminal changed during the native confirmation guard',result.stderr)
        self.assertEqual(self.clock_state()[0],40000000)
        self.assertFalse((self.root/'submitted').exists())
        self.assertFalse((self.root/'enter-clock').exists())


if __name__=='__main__':unittest.main()
