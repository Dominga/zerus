"""Automatic pre-send compact on a private socket, config, history and receipt."""
import base64, json, os, signal, subprocess, time, unittest, uuid
from pathlib import Path
import test_input
from test_clear_context import AGENT

# Reuse the exact native composer fixture. Only the compact execution is synthetic.
FIXTURE = AGENT.replace('import os, pathlib,', 'import subprocess, time, os, pathlib,').replace("text='';agent='codex'", "text='';agent='codex';escape=0")
FIXTURE = FIXTURE.replace('  for c in value:\n   if c==13:', '''  for c in value:
   if escape:
    if escape==1 and c==91:escape=2
    elif escape==1 or 64<=c<=126:escape=0
    continue
   if c==27:escape=1;continue
   if c==21:text='';continue
   if c==13:
    if text.strip()=='/compact':
     p=pathlib.Path((root/'session-record-path').read_text());r=json.loads(p.read_text())
     (root/'compact-started').touch()
     env=dict(os.environ,HGS_SESSION=r['name'],HGS_RUN_ID=r['run_id'])
     def hook(kind):
      e=dict(hook_event_name=kind,session_id=r['conversation_id'],trigger='manual',turn_id='fixture-compact')
      result=subprocess.run(['hgs','__state','hook'],env=env,input=json.dumps(e),text=True,capture_output=True)
      if result.returncode:(root/'hook-error').write_text(result.stderr)
     mode=(root/'mode').read_text() if (root/'mode').exists() else 'success'
     if mode=='hold':
      while not (root/'release-compact').exists():time.sleep(.02)
     if mode=='unsafe':continue
     text='';show()
     if mode!='timeout':
      hook('PreCompact')
      hook('StopFailure' if mode=='failure' else 'PostCompact')
     continue
    with (root/'messages').open('a') as f:f.write(text+'\\n')
    text=''
''')

class KimiExpiry(unittest.TestCase):
 script=test_input.InputTransport.script
 write_record=test_input.InputTransport.write_record
 tmux=test_input.InputTransport.tmux
 wait_for=test_input.InputTransport.wait_for
 received=test_input.InputTransport.received
 payload=test_input.InputTransport.payload
 tearDown=test_input.InputTransport.tearDown
 def setUp(self):
  old=test_input.FAKE_AGENT;test_input.FAKE_AGENT=FIXTURE
  try:test_input.InputTransport.setUp(self)
  finally:test_input.FAKE_AGENT=old
  (self.root/'session-record-path').write_text(str(self.record_path))
  self.record.update(agent='kimi',model='K3',cache_touched=dict(conversation_id=self.conversation_id,at=time.time()-3600))
  self.write_record();(self.root/'provider').write_text('kimi');os.kill(self.record['pid'],signal.SIGUSR1)
  self.wait_for(lambda:'│ >' in self.tmux('capture-pane','-p','-t',self.pane))
  self.home=self.root/'.kimi-code';self.cache=self.home/'cache/client-configs/mainland-cn%3A%2Fclient_configs%3Aestimated_cache_duration.json'
  self.cache.parent.mkdir(parents=True)
  self.cache.write_text(json.dumps(dict(version=1,fetchedAt=time.time()*1000,config=dict(version=1,config=dict(k3=dict(cache_duration=60,min_tokens_to_hint=100))))))
  wire=self.home/'sessions/fixture'/self.conversation_id/'agents/main/wire.jsonl';wire.parent.mkdir(parents=True)
  wire.write_text('\n'.join(json.dumps(e) for e in [dict(type='llm.request',agentId='main',kind='loop',model='k3',modelAlias='K3'),dict(type='token_counting.turn_recorded',agentId='main',tokens=100,length=2,turnId=1)])+'\n')
 def send(self,p=None,timeout=40):
  return subprocess.run([str(test_input.HGS),'send',self.name,'--json'],input=json.dumps(p or self.payload()),env=self.env,text=True,capture_output=True,timeout=timeout)
 def receipt(self,p):return json.loads((self.state/'input_receipts'/f"{p['request_id']}.json").read_text())
 def assert_sent_once(self,p,result,status='completed',refused=()):
  self.assertEqual(result.returncode,0,result.stderr)
  receipt=json.loads(result.stdout);self.assertEqual(receipt['request_id'],p['request_id']);self.assertEqual(receipt,self.receipt(p))
  self.assertEqual(receipt['status'],'submitted');self.assertEqual(receipt['pre_send_compaction']['outcome']['status'],status)
  self.wait_for(lambda:(self.root/'messages').exists())
  self.assertEqual((self.root/'messages').read_text(),p['text']+'\n')
  self.assertEqual(self.received().count(b'/compact'),1)
  before=self.received();again=self.send(p);self.assertEqual(again.returncode,0,again.stderr);self.assertEqual(self.receipt(p),receipt);self.assertEqual(before,self.received())
  expected={p['request_id']}
  for other in refused:
   test_input.InputTransport.assert_refusal(self,other)
   expected.add(other['request_id'])
  self.assertEqual({path.stem for path in (self.state/'input_receipts').glob('*.json')},expected)
 def test_expired_compact_then_send_original_request_once(self):
  p=self.payload(text='kept original message');self.assert_sent_once(p,self.send(p))
 def test_failed_compact_falls_back_to_ordinary_send_once(self):
  (self.root/'mode').write_text('failure');p=self.payload(text='fallback once');self.assert_sent_once(p,self.send(p),'cancelled')
 def test_timeout_falls_back_once(self):
  (self.root/'mode').write_text('timeout');p=self.payload(text='timeout fallback');self.assert_sent_once(p,self.send(p),'timeout')
 def test_unsafe_partial_compact_keeps_payload_attachment_and_refuses_without_paste(self):
  (self.root/'mode').write_text('unsafe');p=self.payload(text='manual after clearing',attachments=[dict(name='example.txt',mime='text/plain',data_base64=base64.b64encode(b'kept attachment').decode())])
  result=self.send(p);self.assertNotEqual(result.returncode,0);self.assertIn('not confirmed',result.stderr.lower())
  r=self.receipt(p);self.assertEqual(r['text'],p['text']);self.assertEqual(r['delivery'],'not_confirmed')
  self.assertEqual(Path(r['attachments'][0]['path']).read_bytes(),b'kept attachment')
  self.assertEqual(self.received(),b'/compact  \r')
  self.assertNotEqual(self.send(p).returncode,0);self.assertEqual(self.received(),b'/compact  \r')
  # Simulate the person clearing and sending the kept payload in Terminal.
  self.tmux('send-keys','-t',self.pane,'C-u')
  self.tmux('send-keys','-t',self.pane,'-l',r['submitted_text'])
  self.tmux('send-keys','-t',self.pane,'Enter')
  self.wait_for(lambda:(self.root/'messages').exists())
  self.assertEqual((self.root/'messages').read_text(),r['submitted_text']+'\n')
  self.assertEqual(self.receipt(p)['status'],'preparing','manual input cannot fabricate a Zerus receipt')
 def test_fresh_missing_config_missing_baseline_and_option_off_are_ordinary(self):
  # Each case uses a fresh private fixture so no prior send can affect guards.
  for mode in ['fresh','missing','baseline','off']:
   with self.subTest(mode=mode):
    if mode=='fresh':
     self.record['cache_touched']['at']=time.time();self.write_record()
    elif mode=='missing':self.cache.unlink()
    elif mode=='baseline':
     self.record.pop('cache_touched');self.write_record()
    else:
     config=self.root/'config';config.mkdir(exist_ok=True);(config/'accounts.json').write_text(json.dumps(dict(kimi_compact_on_expiry={'native-kimi':False})))
    p=self.payload();result=self.send(p);self.assertEqual(result.returncode,0,result.stderr)
    self.assertNotIn('pre_send_compaction',self.receipt(p));self.assertNotIn(b'/compact',self.received())
    self.tearDown();self.setUp()
 def test_identity_change_keeps_original_payload_and_sends_no_message(self):
  (self.root/'mode').write_text('hold');p=self.payload(text='kept on identity change')
  proc=subprocess.Popen([str(test_input.HGS),'send',self.name,'--json'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=self.env,text=True)
  proc.stdin.write(json.dumps(p));proc.stdin.close();proc.stdin=None
  try:
   self.wait_for(lambda:(self.root/'compact-started').exists())
   current=json.loads(self.record_path.read_text());current['conversation_id']=str(uuid.uuid4());self.record_path.write_text(json.dumps(current))
  finally:(self.root/'release-compact').touch()
  out,err=proc.communicate(timeout=15);self.assertNotEqual(proc.returncode,0,out)
  self.assertIn('not confirmed',err.lower());self.assertEqual(self.receipt(p)['text'],p['text'])
  self.assertEqual(self.received(),b'/compact  \r');self.assertFalse((self.root/'messages').exists())

 def test_original_and_competing_send_cannot_reenter_during_compaction(self):
  (self.root/'mode').write_text('hold');p=self.payload(text='only this one')
  competitor=self.payload(text='competitor')
  proc=subprocess.Popen([str(test_input.HGS),'send',self.name,'--json'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=self.env,text=True)
  proc.stdin.write(json.dumps(p));proc.stdin.close();proc.stdin=None
  try:
   self.wait_for(lambda:(self.root/'compact-started').exists())
   for other in [p,competitor]:
    result=self.send(other);self.assertNotEqual(result.returncode,0,result.stderr)
   self.assertEqual(self.received(),b'/compact  \r');self.assertEqual(self.receipt(p)['status'],'preparing')
  finally:(self.root/'release-compact').touch()
  out,err=proc.communicate(timeout=15);self.assertEqual(proc.returncode,0,err)
  self.assert_sent_once(p,subprocess.CompletedProcess([],0,out,err),refused=(competitor,))
if __name__=='__main__':unittest.main()
