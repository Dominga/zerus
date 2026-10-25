"""Cooperative external chats, isolated from owner native state and services."""
import concurrent.futures
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import unittest
import uuid
ROOT=Path(__file__).resolve().parents[1]
BIN=Path(os.environ.get('HGS_TEST_BIN',ROOT/'target/debug'/('hgs.exe' if os.name=='nt' else 'hgs')))

@unittest.skipUnless(BIN.is_file(),'build hgs or set HGS_TEST_BIN')
class ExternalWorkstreams(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(prefix='zerus-external-');self.addCleanup(self.temp.cleanup);self.root=Path(self.temp.name)
        for name in ['home','config','state','tmux','workspace']:(self.root/name).mkdir(mode=0o700)
        self.env={k:v for k,v in os.environ.items() if not k.startswith('HGS_')}
        self.env.update(HOME=str(self.root/'home'),USERPROFILE=str(self.root/'home'),HGS_CONFIG_DIR=str(self.root/'config'),HGS_STATE_DIR=str(self.root/'state'),HGS_SELF='external-fixture',HGS_PEERS='',TMUX_TMPDIR=str(self.root/'tmux'))
    def hgs(self,*args,payload=None,ok=True):
        result=subprocess.run([str(BIN),*args],input=json.dumps(payload or {}).encode(),capture_output=True,env=self.env,timeout=15)
        if ok:self.assertEqual(result.returncode,0,result.stderr.decode(errors='replace'));return json.loads(result.stdout)
        self.assertNotEqual(result.returncode,0);return result.stderr.decode(errors='replace')
    def req(self,**extra):
        return dict(request_id=str(uuid.uuid4()),name='external/fixture/chat',kind='chatgpt_chat',url='https://chatgpt.com/c/'+str(uuid.uuid4()),cwd=str(self.root/'workspace'),summary='Verified checkpoint',next_steps='Continue from this receipt',**extra)
    def register(self):return self.hgs('external','register','--json',payload=self.req())
    def scope(self,row,**extra):return {**dict(request_id=str(uuid.uuid4()),expected_run_id=row['run_id'],expected_conversation_id=row['conversation_id']),**extra}
    def external(self,action,row,**extra):return self.hgs('external',action,'--json',payload=self.scope(row,name=row['name'],**extra))
    def test_empty_listing_does_not_create_a_store(self):
        self.assertEqual(self.hgs('external','list','--json')['sessions'],[])
        self.assertFalse((self.root/'state/external-workstreams').exists())
    def test_declared_sources_remain_distinct_in_normal_listing(self):
        for index,kind in enumerate(['chatgpt_chat','chatgpt_work','codex_chat']):
            p=self.req();p.update(name='external/fixture/chat'+str(index),kind=kind);row=self.hgs('external','register','--json',payload=p)
            self.assertEqual(row['client_context']['kind'],kind);self.assertFalse(row['external_cloud_control']);self.assertFalse(row['terminal_supported'])
        rows=self.hgs('ls','--json','--local')['sessions'];external=[r for r in rows if r['cmd']=='external']
        self.assertEqual(len(external),3);self.assertEqual(len({r['client_context']['kind'] for r in external}),3)
    def test_duplicate_registration_and_checkpoint_are_exactly_once(self):
        p=self.req();row=self.hgs('external','register','--json',payload=p);self.assertEqual(row,self.hgs('external','register','--json',payload=p))
        wrong={**p,'kind':'chatgpt_work'};self.hgs('external','register','--json',payload=wrong,ok=False)
        p=self.scope(row,name=row['name'],summary='Complete: test passed',next_steps='Next step',state='ready')
        first=self.hgs('external','checkpoint','--json',payload=p);self.assertEqual(first,self.hgs('external','checkpoint','--json',payload=p))
        inspected=self.hgs('inspect',row['name'],'--skip-processes');self.assertEqual(inspected['checkpoint']['summary'],'Complete: test passed')
        page=self.hgs('history',row['name'],'--json',payload=self.scope(row,limit=50))
        self.assertEqual(len(page['events']),2);self.assertFalse(page['cloud_transcript_available'])
    def test_queue_read_ack_and_handoff_never_claim_cloud_delivery(self):
        row=self.register();p=self.scope(row,text='Read the test result; do not execute commands')
        receipt=self.hgs('send',row['name'],'--json',payload=p);self.assertEqual(receipt['provider_delivery'],'not_attempted');self.assertEqual(receipt['delivery_state'],'queued')
        self.assertEqual(receipt,self.hgs('send',row['name'],'--json',payload=p))
        read={k:v for k,v in self.scope(row,name=row['name']).items() if k!='request_id'}
        inbox=self.hgs('external','inbox','--json',payload=read);self.assertEqual(inbox['messages'][0]['status'],'queued')
        handoff=self.scope(row,name=row['name'],url='https://chatgpt.com/c/'+str(uuid.uuid4()),kind='chatgpt_work')
        self.hgs('external','handoff','--json',payload=handoff,ok=False)
        self.external('ack',row,message_id=p['request_id'],status='accepted')
        self.hgs('external','handoff','--json',payload=handoff,ok=False)
        self.external('ack',row,message_id=p['request_id'],status='completed')
        changed=self.hgs('external','handoff','--json',payload=handoff);self.assertNotEqual(changed['run_id'],row['run_id'])
        self.hgs('external','inbox','--json',payload=read,ok=False)
        self.hgs('send',row['name'],'--json',payload=self.scope(row,text='late old-chat request'),ok=False)
        current=self.hgs('inspect',row['name']);self.assertEqual(current['checkpoint']['summary'],'Verified checkpoint')
    def test_handoff_checkpoint_history_is_scoped_and_not_a_cloud_transcript(self):
        row=self.register();new=self.external('handoff',row,url='https://chatgpt.com/c/'+str(uuid.uuid4()),kind='codex_chat')
        page=self.hgs('history',new['name'],'--json',payload=self.scope(new));self.assertEqual(len(page['events']),1)
        self.assertIn('carried from previous',page['events'][0]['detail']);self.assertFalse(page['cloud_transcript_available'])
    def test_history_and_inbox_pages_are_bounded(self):
        row=self.register()
        for n in range(5):self.external('checkpoint',row,summary='Step '+str(n),next_steps='next',state='working')
        page=self.hgs('history',row['name'],'--json',payload=self.scope(row,limit=2));self.assertTrue(page['has_more_before'])
        older=self.hgs('history',row['name'],'--json',payload=self.scope(row,limit=2,before=page['next_before']))
        self.assertTrue({e['history_id'] for e in page['events']}.isdisjoint(e['history_id'] for e in older['events']))
        self.hgs('history',row['name'],'--json',payload=self.scope(row,limit=101),ok=False)
        for n in range(3):self.hgs('send',row['name'],'--json',payload=self.scope(row,text='Instruction '+str(n)))
        read={k:v for k,v in self.scope(row,name=row['name'],limit=2).items() if k!='request_id'}
        first=self.hgs('external','inbox','--json',payload=read);self.assertTrue(first['has_more'])
        last=self.hgs('external','inbox','--json',payload={**read,'after':first['next']});self.assertEqual(len(last['messages']),1)
        self.assertGreater(last['messages'][0]['sequence'],first['messages'][-1]['sequence'])
    def test_native_controls_and_path_aliases_are_refused(self):
        row=self.register()
        for command in ['kill','terminate','pause','resume','terminal','interrupt','clear-context','answer','file-read']:
            self.hgs(command,row['name'],'--json',payload=self.scope(row),ok=False)
        self.assertEqual(self.hgs('inspect',row['name'])['run_id'],row['run_id'])
        for changes in [{'kind':'gpt-model-name'},{'name':'external/../unsafe'},{'url':'https://chatgpt.com.evil.test/c/'+str(uuid.uuid4())},{'extra':True}]:
            p=self.req();p.update(changes);self.hgs('external','register','--json',payload=p,ok=False)
    def test_concurrent_duplicate_send_has_one_durable_request(self):
        row=self.register();payload=self.scope(row,text='One queued request')
        def send(_):return self.hgs('send',row['name'],'--json',payload=payload)
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:results=list(pool.map(send,range(4)))
        self.assertTrue(all(v==results[0] for v in results))
        read={k:v for k,v in self.scope(row,name=row['name']).items() if k!='request_id'}
        self.assertEqual(len(self.hgs('external','inbox','--json',payload=read)['messages']),1)
    def test_unknown_schema_is_not_repaired(self):
        row=self.register();database=self.root/'state/external-workstreams/workstreams.sqlite3'
        db=sqlite3.connect(database)
        try:db.execute('pragma user_version=99');db.commit()
        finally:db.close()
        before=database.read_bytes();self.hgs('inspect',row['name'],ok=False);self.assertEqual(database.read_bytes(),before)
    def test_bad_scope_attachments_and_oversized_text_preserve_inbox(self):
        row=self.register()
        for extra in [{'text':'x'*17000},{'text':'hello','attachments':[{'path':'private'}]},{'text':'hello','expected_run_id':str(uuid.uuid4())}]:
            self.hgs('send',row['name'],'--json',payload=self.scope(row,**extra),ok=False)
        read={k:v for k,v in self.scope(row,name=row['name']).items() if k!='request_id'}
        self.assertEqual(self.hgs('external','inbox','--json',payload=read)['messages'],[])
if __name__=='__main__':unittest.main()
