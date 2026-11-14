#!/usr/bin/env python3
"""Person-only sandbox choice on a private socket; synthetic setup never elevates."""
import json
import os
import signal
import unittest
import uuid

import test_input as fixtures
from test_questions import Questions

FAKE = fixtures.SIGNAL_SAFE_READ + r'''
import os,pathlib,signal,sys,termios,textwrap,tty
root=pathlib.Path(os.environ['INPUT_FIXTURE']);fd=sys.stdin.fileno();original=termios.tcgetattr(fd);tty.setraw(fd)
os.write(1,b'\x1b[?2004h');selected=0;buf=b''
labels=['Set up default sandbox (requires Administrator permissions)','Use non-admin sandbox (higher risk if prompt injected)','Quit']
body='Set up the Codex agent sandbox to protect your files and control network access. Learn more <https://developers.openai.com/codex/windows>'
def render(*args):
 global selected
 if (root/'selection').exists(): selected=int((root/'selection').read_text());(root/'selection').unlink()
 width=os.get_terminal_size(fd).columns
 lines=['','  >_ OpenAI Codex (v0.169.0)','     /work/example']
 if not (root/'normal').exists():lines+=['  permissions: YOLO mode']
 lines+=['','']
 lines+=['  '+r for r in textwrap.wrap(body,width=width-4,break_on_hyphens=False)]+['','']
 for i,label in enumerate(labels):
  if (root/'changed').exists() and i==1:label='Use a different sandbox'
  lines+=textwrap.wrap(('› ' if selected==i else '  ')+str(i+1)+'. '+label,width=width,subsequent_indent=' '*5,break_on_hyphens=False)
 lines+=['','  enter select · esc back']
 os.write(1,b'\x1b[2J\x1b[H'+'\r\n'.join(lines).encode())
signal.signal(signal.SIGUSR1,render);render();(root/'agent-pid').write_text(str(os.getpid()))
try:
 while True:
  value=read_input(fd)
  if not value:break
  with (root/'received').open('ab') as f:f.write(value)
  buf+=value
  while buf:
   if buf.startswith((b'\x1b[A',b'\x1bOA')):selected=max(0,selected-1);buf=buf[3:];render()
   elif buf.startswith((b'\x1b[B',b'\x1bOB')):selected=min(2,selected+1);buf=buf[3:];render()
   elif buf.startswith(b'\r'):
    buf=buf[1:];(root/'submitted').write_text(labels[selected])
    if not (root/'hold-dialog').exists():
     if selected==2:sys.exit(0)
     os.write(1,b'\x1b[2J\x1b[HSetting up sandbox...\r\nHang tight, this may take a few minutes')
   elif buf[0]==27 and len(buf)<3:break
   else:buf=buf[1:]
finally:termios.tcsetattr(fd,termios.TCSANOW,original)
'''


class SandboxSetup(unittest.TestCase):
    script = fixtures.InputTransport.script
    tmux = fixtures.InputTransport.tmux
    wait_for = fixtures.InputTransport.wait_for
    received = fixtures.InputTransport.received
    write_record = fixtures.InputTransport.write_record
    tearDown = fixtures.InputTransport.tearDown
    inspect = Questions.inspect
    answer = Questions.answer

    def setUp(self):
        original = fixtures.FAKE_AGENT
        try:
            fixtures.FAKE_AGENT = FAKE
            fixtures.InputTransport.setUp(self)
        finally:
            fixtures.FAKE_AGENT = original
        self.conversation_id = ''
        self.record.update(agent='codex', run_identity_version=1, supervisor={}, conversation_id=None,
                           launch_dir='/work/example', phase='unknown', activity='unknown', last_event_at=0)
        self.write_record()
        self.card = self.inspect()['pending_questions'][0]

    def payload(self, choice=0, **updates):
        p = dict(request_id=str(uuid.uuid4()), expected_run_id=self.run_id, expected_conversation_id='',
                 question_id=self.card['question_id'], expected_question_hash=self.card['question_hash'],
                 answers=[dict(question_id='sandbox', selected_option_ids=[f'sandbox_{choice}'], text='')])
        p.update(updates)
        return p

    def redraw(self, file, value, expected):
        (self.root/file).write_text(value)
        os.kill(self.record['pid'], signal.SIGUSR1)
        self.wait_for(lambda: expected in self.tmux('capture-pane', '-p', '-t', self.pane))

    def test_attention_and_blocked_input_never_choose_or_send(self):
        info = self.inspect()
        self.assertEqual(self.card['source'], 'codex_sandbox_setup')
        self.assertEqual(info['attention_id'], self.card['question_id'])
        self.assertEqual(info['phase'], 'approval')
        self.assertEqual(self.received(), b'')
        fixtures.InputTransport.send(self, fixtures.InputTransport.payload(self), success=False)
        self.assertEqual(self.received(), b'')

    def test_agent_caller_cannot_choose_any_sandbox_option(self):
        self.env.update(HGS_SESSION='codex/fixture/other-agent', HGS_RUN_ID=str(uuid.uuid4()))
        for choice in range(3):
            with self.subTest(choice=choice):
                request = self.payload(choice)
                self.assertIn("person", self.answer(request, success=False))
                self.assertFalse((self.state/'question_receipts'/(request['request_id']+'.json')).exists())
        self.assertEqual(self.received(), b'')

    def test_each_setup_choice_only_sends_the_persons_selection_once(self):
        for choice in (0, 1):
            with self.subTest(choice=choice):
                self.redraw('selection', '2', '› 3. Quit')
                self.card = self.inspect()['pending_questions'][0]
                p = self.payload(choice)
                receipt = self.answer(p)
                self.assertEqual(receipt['status'], 'answered')
                self.assertTrue(receipt['open_terminal'])
                self.assertEqual((self.root/'submitted').read_text(), self.card['questions'][0]['options'][choice]['label'])
                before = self.received()
                self.assertEqual(self.answer(p), receipt)
                self.assertEqual(self.received(), before)
                self.assertEqual(self.inspect()['pending_questions'], [])

    def test_explicit_quit_exits_the_bound_process_and_receipt_replay_types_nothing(self):
        p = self.payload(2)
        receipt = self.answer(p)
        self.assertEqual(receipt['status'], 'answered')
        self.assertEqual((self.root/'submitted').read_text(), 'Quit')
        before = self.received()
        self.assertEqual(self.answer(p), receipt)
        self.assertEqual(self.received(), before)

    def test_wrong_run_hash_or_new_pending_input_refuses_before_keys(self):
        self.answer(self.payload(expected_run_id='different'), success=False)
        self.answer(self.payload(expected_question_hash='f'*64), success=False)
        self.record.update(input_pending_at=123., input_pending_request=str(uuid.uuid4()))
        self.write_record()
        self.answer(self.payload(), success=False)
        self.assertEqual(self.received(), b'')

    def test_changed_frame_refuses_the_original_choice(self):
        self.redraw('changed', '1', 'Use a different sandbox')
        self.answer(self.payload(), success=False)
        self.assertEqual(self.received(), b'')
        for card in self.inspect()['pending_questions']:
            self.assertFalse(card.get('can_answer'), card)

    def test_normal_permissions_header_requires_a_fresh_explicit_choice(self):
        old = self.payload(2)
        (self.root/'normal').touch()
        os.kill(self.record['pid'], signal.SIGUSR1)
        self.wait_for(lambda: 'YOLO' not in self.tmux('capture-pane', '-p', '-t', self.pane))
        self.answer(old, success=False)
        self.assertEqual(self.received(), b'')
        self.card = self.inspect()['pending_questions'][0]
        self.assertEqual(self.card['source'], 'codex_sandbox_setup')
        self.assertTrue(self.card['can_answer'])
        self.assertEqual(self.answer(self.payload(2))['status'], 'answered')
        self.assertEqual((self.root/'submitted').read_text(), 'Quit')

    def test_copy_mode_keeps_card_read_only_and_answer_sends_nothing(self):
        self.tmux('copy-mode', '-t', self.pane)
        card = self.inspect()['pending_questions'][0]
        self.assertEqual(card['source'], 'codex_sandbox_setup')
        self.assertFalse(card['can_answer'])
        self.assertEqual(card['answer_transport'], '')
        self.answer(self.payload(), success=False)
        self.assertEqual(self.received(), b'')
        self.tmux('send-keys', '-t', self.pane, '-X', 'cancel')

    def test_unknown_delivery_never_replays_enter(self):
        (self.root/'hold-dialog').touch()
        p = self.payload()
        self.assertIn('delivery uncertain', self.answer(p, success=False))
        before = self.received()
        self.assertEqual(before, b'\r')
        self.assertIn('delivery uncertain', self.answer(p, success=False))
        self.assertEqual(self.received(), before)


if __name__ == '__main__':
    unittest.main()
