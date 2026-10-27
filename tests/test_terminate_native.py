#!/usr/bin/env python3
"""Real terminal agents launched and terminated by hgs; private homes/local models."""
import http.server
import hashlib
import json
import os
from pathlib import Path
import subprocess
import threading
import time
import unittest

import test_pause
import test_task_context_native as models


class Termination:
    def test_tracked_native_termination_preserves_conversation(self):
        # Unlike manually recorded native UI fixtures, this uses the production
        # hgs supervisor and launch recipe. No inherited account is reachable.
        native=Path(os.environ[self.variable]).resolve()
        (self.bin/self.agent).unlink();(self.bin/self.agent).symlink_to(native)
        handler=models.Claude if self.agent=='claude' else models.Codex
        handler.requests=[]
        server=http.server.ThreadingHTTPServer(('127.0.0.1',0),handler)
        self.addCleanup(server.server_close);self.addCleanup(server.shutdown)
        threading.Thread(target=server.serve_forever,daemon=True).start()
        for key in list(self.env):
            if key.startswith(('ANTHROPIC_','CLAUDE_','OPENAI_','CODEX_')):self.env.pop(key,None)
        base='http://127.0.0.1:'+str(server.server_port)
        if self.agent=='claude':
            home=self.root/'.claude';home.mkdir(exist_ok=True)
            (self.root/'.claude.json').write_text(json.dumps(dict(hasCompletedOnboarding=True,hasSeenAutoDefaultNudge=True,
                bypassPermissionsModeAccepted=True,customApiKeyResponses=dict(approved=['fixture-key'],rejected=[]),
                projects={str(self.project):dict(hasTrustDialogAccepted=True)})))
            (home/'settings.json').write_text(json.dumps({'permissions':{'defaultMode':'bypassPermissions'}}))
            self.env.update(ANTHROPIC_API_KEY='fixture-key',ANTHROPIC_BASE_URL=base,CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC='1')
            args=['--dangerously-skip-permissions','--model','claude-sonnet-4-6','Fixture hello.']
        else:
            home=self.root/'.codex';home.mkdir(exist_ok=True)
            (home/'config.toml').write_text('model="gpt-6.1-sol"\nmodel_provider="fixture"\ncheck_for_update_on_startup=false\n'
                'approval_policy="never"\nsandbox_mode="danger-full-access"\n[features]\nplugins=false\nhooks=true\n'
                '[projects.'+json.dumps(str(self.project))+']\ntrust_level="trusted"\n'
                '[model_providers.fixture]\nname="Fixture"\nbase_url='+json.dumps(base+'/v1')+'\nwire_api="responses"\n'
                'env_key="OPENAI_API_KEY"\nrequest_max_retries=0\nstream_max_retries=0\n')
            self.env.update(CODEX_HOME=str(home),OPENAI_API_KEY='fixture-key')
            args=['--dangerously-bypass-approvals-and-sandbox','--dangerously-bypass-hook-trust',
                '-c','tui.animations='+os.environ.get('HGS_CODEX_TEST_ANIMATIONS','true'),'Fixture hello.']
        name=self.agent+'/p/native-terminate'
        self.hgs(self.agent,'p','--new','-n','native-terminate','-d','--',*args)
        deadline=time.monotonic()+60
        while time.monotonic()<deadline:
            record=self.binding(name)
            if record.get('conversation_id') and handler.requests:break
            time.sleep(.1)
        else:self.fail('Native launch was not confirmed:\n'+self.t('capture-pane','-p','-t','='+name+':'))
        # Assert real execution and the production supervisor relationship, not
        # just a version string, terminal text or a fabricated saved binding.
        parent=subprocess.check_output(['ps','-p',str(record['pid']),'-o','ppid='],text=True).strip()
        self.assertEqual(int(parent),record['supervisor']['pid'])
        if os.path.exists('/proc/'+str(record['pid'])+'/exe'):
            self.assertEqual(Path('/proc/'+str(record['pid'])+'/exe').resolve(),native)
        else:
            image=subprocess.check_output(['ps','-ww','-p',str(record['pid']),'-o','comm='],text=True).strip()
            # macOS workers stage another same-byte copy of the public image.
            # Compare the kernel-reported image to that sealed selection.
            with open(image,'rb') as running, native.open('rb') as selected:
                self.assertEqual(hashlib.file_digest(running,'sha256').digest(),hashlib.file_digest(selected,'sha256').digest())
        self.hgs('terminate',name,'--expected-run-id',record['run_id'])
        states=subprocess.run(['ps','-p',str(record['pid']),'-o','stat='],text=True,capture_output=True).stdout.strip()
        self.assertTrue(not states or states.startswith(('Z','X')),states)
        archives=[json.loads(path.read_text()) for path in (self.root/'state/archives').glob('*.json')]
        self.assertTrue(any(row['conversation_id']==record['conversation_id'] and row['run_id']==record['run_id'] for row in archives))


@unittest.skipUnless(os.environ.get('HGS_CLAUDE_TEST_BIN'),'Set HGS_CLAUDE_TEST_BIN for real Claude termination')
class NativeClaudeTermination(Termination,test_pause.Harness):
    agent='claude';variable='HGS_CLAUDE_TEST_BIN'


@unittest.skipUnless(os.environ.get('HGS_CODEX_TEST_BIN'),'Set HGS_CODEX_TEST_BIN for real Codex termination')
class NativeCodexTermination(Termination,test_pause.Harness):
    agent='codex';variable='HGS_CODEX_TEST_BIN'


if __name__=='__main__':unittest.main()
