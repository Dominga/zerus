#!/usr/bin/env python3
"""Actual supervisor/CLI cosmetic-theme controls, private scripted native TUI.

This is not official Claude compatibility evidence. The fake never emits native
hooks or a conversation and implements only the reviewed theme and manual login.
"""
import json
import subprocess
import sys
import time
import unittest

import test_pause

Base = type('ThemeFixture', (unittest.TestCase,), {
    name: value for name, value in test_pause.Harness.__dict__.items()
    if name not in ('__dict__', '__weakref__') and not name.startswith('test_')})

AGENT = r'''#!PYTHON
import os, pathlib, select, sys, termios, tty
root=pathlib.Path.home()
if sys.argv[1:]==['--help']:
    print('Usage: claude [options] [prompt]');sys.exit(0)
if sys.argv[1:]==['--version']:
    print('2.1.322 (Claude Code)');sys.exit(0)
fd=sys.stdin.fileno();original=termios.tcgetattr(fd);tty.setraw(fd)
def draw(text):
    os.write(1,('\x1b[?2004h\x1b[2J\x1b[H'+text.replace('\n','\r\n')).encode())
draw((root/'theme-screen').read_text())
(root/'theme-agent-ready').touch()
try:
    while True:
        value=os.read(fd,4096)
        if not value:break
        if value==b'\x04':break
        with (root/'theme-received').open('ab') as log:log.write(value)
        if value==b'\r' and not (root/'theme-remains').exists():
            draw("Welcome to Claude Code v2.1.322\n\nSelect login method:\n\n❯ 1. Claude account with subscription\n  2. Anthropic Console account\n  3. 3rd-party platform\n")
finally:termios.tcsetattr(fd,termios.TCSANOW,original)
'''


class ClaudeTheme(Base):
    def setUp(self):
        super().setUp()
        self.script('claude', AGENT.replace('#!PYTHON', '#!'+sys.executable))
        self.screen = (test_pause.REPO/'tests/fixtures/claude-theme-first-run.txt').read_text()
        # Only the committed isolated official native interface fixture enters fake controls.
        self.screen = self.screen[:self.screen.index('\n')+1] + self.screen[self.screen.index(" Let's get started."):]
        # This scripted raw TUI has no native responsive renderer: fit only
        # decorative rules to its80-column pane and omit captured blank padding.
        # Semantic labels/preview/footer and the committed raw native fixture
        # remain unchanged. The separate native suite never reflows captures.
        self.screen = '\n'.join(' '+('╌'*64) if row.strip() and set(row.strip())=={'╌'} else row
                                for row in self.screen.splitlines()).rstrip()+'\n'
        (self.root/'theme-screen').write_text(self.screen)

    def launch(self, tag='theme', width=80, height=24):
        self.name='claude/p/'+tag
        # Bootstrap reads the private .tmux.conf before creating its server.
        (self.root/'.tmux.conf').write_text(f'set -g default-size {width}x{height}\n')
        self.hgs('claude','p','-n',tag,'-d')
        self.wait(lambda:(self.root/'theme-agent-ready').exists())
        self.wait(lambda:self.binding(self.name).get('pid'))
        return self.binding(self.name)

    def claims(self):
        directory=self.root/'state/claude-theme-confirmations'
        return [json.loads(p.read_text()) for p in directory.glob('*.json') if not p.name.endswith('.result.json')]

    def result(self):
        paths=list((self.root/'state/claude-theme-confirmations').glob('*.result.json'))
        return json.loads(paths[0].read_text()) if len(paths)==1 else {}

    def received(self):
        path=self.root/'theme-received'
        return path.read_bytes() if path.exists() else b''

    def assert_manual_card(self):
        output=json.loads(self.hgs('inspect',self.name))
        cards=output.get('pending_questions',[])
        self.assertTrue(cards,output)
        self.assertTrue(all(not q.get('can_answer') for q in cards),cards)
        self.assertEqual(self.received(),b'')
        self.assertEqual(self.claims(),[])

    def test_original_supervisor_confirms_dark_once_then_leaves_login_manual(self):
        before=self.launch()
        self.wait(lambda:self.result().get('status')=='advanced')
        self.assertEqual(self.received(),b'\r')
        claim,=self.claims()
        self.assertEqual(claim['label'],'Dark mode')
        for key in ('session_id','run_id','pid','process_start','pane'):
            self.assertEqual(claim[key],before[key])
        self.assertEqual(claim['status'],'reserved')
        self.assertEqual(self.binding(self.name)['conversation_id'],None)
        for _ in range(3):
            self.hgs('inspect',self.name)
        time.sleep(.5)
        self.assertEqual(self.received(),b'\r')
        # Ordinary backend still works in the SAME resident process when its
        # actual child exits: finish must record exit rather than sticky refusal.
        subprocess.run([test_pause.TMUX,'-S',self.socket,'send-keys','-t',before['pane'],'C-d'],env=self.env,capture_output=True,check=True)
        self.wait(lambda:self.binding(self.name).get('completion_source')=='supervisor')
        self.assertEqual(self.binding(self.name)['exit_code'],0)

    def test_auto_preserves_the_focused_checked_default_without_navigation(self):
        screen=self.screen.replace('     Auto (match terminal)',' ❯ ✔ Auto (match terminal)').replace(' ❯ ✔ Dark mode','     Dark mode')
        (self.root/'theme-screen').write_text(screen)
        self.launch('auto')
        self.wait(lambda:self.result().get('status')=='advanced')
        self.assertEqual(self.received(),b'\r')
        self.assertEqual(self.claims()[0]['label'],'Auto (match terminal)')

    def test_light_and_mismatched_check_stay_visible_and_never_submit(self):
        screen=self.screen.replace(' ❯ ✔ Dark mode','   ✔ Dark mode').replace('     Light mode\n',' ❯   Light mode\n')
        (self.root/'theme-screen').write_text(screen)
        self.launch('light')
        time.sleep(.5)
        self.assert_manual_card()

    def test_unknown_after_enter_keeps_one_durable_claim_and_never_retries(self):
        (self.root/'theme-remains').touch()
        self.launch('unknown')
        self.wait(lambda:bool(self.claims()))
        time.sleep(3.5)
        self.assertEqual(self.received(),b'\r')
        self.assertEqual(len(self.claims()),1)
        for _ in range(3):self.hgs('inspect',self.name)
        self.assertEqual(self.received(),b'\r')
        self.assertIn(self.result().get('status'),('unknown',None))


if __name__=='__main__':unittest.main()
