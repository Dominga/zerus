"""Indexed archive pages on private synthetic records; no agent or live mark is touched."""
import json
import hashlib
import os
import shlex
from pathlib import Path
import sqlite3
import subprocess
import shutil
import sys
import tempfile
import unittest
import uuid
import time
if os.name == 'posix':
    import fcntl

REPO=Path(__file__).resolve().parents[1]
HGS=Path(os.environ.get('HGS_TEST_BIN',REPO/'target/debug/hgs')).resolve()

class ArchivePages(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(prefix='zerus-archive-pages-');self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name);self.state=self.root/'state';self.archives=self.state/'archives';self.archives.mkdir(parents=True)
        socket=self.root/'tmux-private';socket.mkdir(mode=0o700)
        self.env={k:v for k,v in os.environ.items() if not k.startswith('HGS_') and k not in ('TMUX','TMUX_PANE','TMUX_TMPDIR','CLAUDE_CONFIG_DIR','CODEX_HOME','KIMI_CODE_HOME')}
        self.env.update(HOME=str(self.root),HGS_STATE_DIR=str(self.state),HGS_CONFIG_DIR=str(self.root/'config'),HGS_SELF='archive-fixture',HGS_PEERS='',TMUX_TMPDIR=str(socket))
        # Existing projection/cursor controls assert ready data. Bootstrap is
        # asynchronous now: hold only the launch gate, then explicitly drain the
        # private worker before those assertions. The native bootstrap control
        # below releases this gate and exercises the real detached helper.
        self.launch=None
        if os.name=='posix':
            aux=self.state/'archive-catalog-v2';aux.mkdir(mode=0o700)
            self.launch=open(aux/'launch.lock','a+b',opener=lambda path,flags:os.open(path,flags,0o600))
            self.addCleanup(self.launch.close)
            fcntl.flock(self.launch,fcntl.LOCK_EX)
    def refresh(self):
        result=subprocess.run([str(HGS),'__state','archive-catalog-worker'],env=self.env,capture_output=True,text=True,timeout=90)
        self.assertEqual(result.returncode,0,result.stderr)
    def record(self,index,at=100,**extra):
        ident=str(uuid.UUID(int=index+1));value=dict(version=1,name=f'codex/example/archived-{index}',agent='codex',run_id=f'run-{index}',conversation_id=f'conversation-{index}',state='archived',archive_id=ident,archived_at=at,created=1,cwd='/synthetic/project',**extra)
        path=self.archives/(ident+'.json');temp=path.with_suffix('.tmp');temp.write_text(json.dumps(value),encoding='utf-8');temp.replace(path)
        return value
    def run_query(self,query=None,fail=False,bounded=False):
        self.refresh()
        r=subprocess.run([str(HGS),'archives','--json']+(['--bounded'] if bounded else []),input=json.dumps(query or {}),capture_output=True,text=True,env=self.env,timeout=30)
        if fail:self.assertNotEqual(r.returncode,0,r.stdout);return r
        self.assertEqual(r.returncode,0,r.stderr);return json.loads(r.stdout)
    @unittest.skipUnless(os.name=='posix','Private native worker exclusion uses flock')
    def test_cold_requests_report_unknown_and_a_detached_worker_publishes_ready_coverage(self):
        records=[self.record(i,100+i) for i in range(128)]
        originals={path:path.read_bytes() for path in self.archives.glob('*.json')}
        command=[str(HGS),'archives','--json','--bounded']
        # A contended worker proves the first read cannot fall back to an
        # inline rebuild or manufacture an empty/complete result.
        lock=self.state/'archive-catalog-v2/worker.lock'
        with open(lock,'a+b',opener=lambda path,flags:os.open(path,flags,0o600)) as worker:
            fcntl.flock(worker,fcntl.LOCK_EX)
            result=subprocess.run(command,input='{}',env=self.env,capture_output=True,text=True,timeout=5)
            self.assertNotEqual(result.returncode,0)
            self.assertIn('coverage is incomplete',result.stderr)
            self.assertFalse((self.state/'archive-catalog-v1.sqlite3').exists())
            preview=subprocess.run([str(HGS),'ls','--json','--local'],env=self.env,capture_output=True,text=True,timeout=5)
            self.assertEqual(preview.returncode,0,preview.stderr)
            descriptor=json.loads(preview.stdout)['archive_page']
            self.assertTrue(descriptor['partial']);self.assertTrue(descriptor['total_capped'])
            self.assertFalse(descriptor['catalog_complete'])
            self.assertIn('coverage is incomplete',descriptor['error'])
        fcntl.flock(self.launch,fcntl.LOCK_UN)
        try:
            result=subprocess.run(command,input='{}',env=self.env,capture_output=True,text=True,timeout=5)
            self.assertNotEqual(result.returncode,0,'The cold request must leave bootstrap to its helper')
            deadline=time.monotonic()+15
            while time.monotonic()<deadline:
                result=subprocess.run(command,input='{}',env=self.env,capture_output=True,text=True,timeout=5)
                if result.returncode==0:break
                self.assertIn('coverage is incomplete',result.stderr)
                time.sleep(.02)
            self.assertEqual(result.returncode,0,result.stderr)
            page=json.loads(result.stdout)
            self.assertFalse(page['partial']);self.assertFalse(page['total_capped'])
            self.assertEqual(page['total'],len(records));self.assertEqual(len(page['sessions']),50)
            self.assertEqual(set(originals),set(self.archives.glob('*.json')))
            for path,raw in originals.items():self.assertEqual(path.read_bytes(),raw)
        finally:
            fcntl.flock(self.launch,fcntl.LOCK_EX)

    def test_exited_archive_status_is_ended_without_rewriting_the_restore_source(self):
        value=self.record(0,activity='busy',phase='working',pid=0,process_start='exited-fixture',conversation_state='active')
        value.update(agent='kimi',name='kimi/example/completed')
        path=self.archives/(value['archive_id']+'.json')
        path.write_text(json.dumps(value),encoding='utf-8');before=path.read_bytes()
        self.refresh()
        commands=(['ls','--json','--local'],['archives','--json'],['archives','--json','--bounded'],
                  ['inspect',value['name'],'--archive',value['archive_id']])
        for args in commands:
            with self.subTest(command=args):
                result=subprocess.run([str(HGS),*args],input='{}',capture_output=True,text=True,env=self.env,timeout=30)
                self.assertEqual(result.returncode,0,result.stderr)
                output=json.loads(result.stdout)
                row=output if args[0]=='inspect' else output['sessions'][0]
                self.assertEqual(row['activity'],'ended')
                self.assertEqual(row['phase'],'ended')
                self.assertEqual(row['process_state'],'exited')
                self.assertEqual(row['runtime_state'],'stopped')
                self.assertEqual(row['activity_summary'],'Archived')
                for key in ('archive_id','run_id','conversation_id'):
                    self.assertEqual(row[key],value[key])
                self.assertEqual(row['conversation_state'],'active')
                self.assertEqual(path.read_bytes(),before,'Listing never rewrites the restore source')
        # A name alone selects a current binding, not an archived instance.
        result=subprocess.run([str(HGS),'inspect',value['name']],capture_output=True,text=True,env=self.env,timeout=30)
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertFalse(json.loads(result.stdout)['tracked'])
    def test_thousands_bounded_pages_ties_and_complete_no_duplicates(self):
        for i in range(2043):self.record(i,100+i//30)
        first=self.run_query({'limit':50});self.assertEqual(first['total'],2043);self.assertEqual(len(first['sessions']),50)
        index=self.state/'archive-catalog-v1.sqlite3';before=index.stat().st_mtime_ns
        got=[];page=first
        while True:
            self.assertLessEqual(len(page['sessions']),50);self.assertLess(len(json.dumps(page)),1048576)
            got += [(v['archived_at'],v['archive_id']) for v in page['sessions']]
            if not page['next_cursor']:break
            page=self.run_query({'limit':50,'cursor':page['next_cursor']})
        self.assertEqual(len(got),2043);self.assertEqual(len(set(got)),2043);self.assertEqual(got,sorted(got,reverse=True))
        self.assertEqual(index.stat().st_mtime_ns,before,'Warm queries must not rebuild or rewrite the metadata index')
    def test_exclusive_upper_bound_search_literal_unicode_and_cursor_binding(self):
        self.record(0,100);self.record(1,101);self.record(2,102,title='Русский %_needle')
        page=self.run_query({'from':101,'until':102,'limit':1});self.assertEqual([r['run_id'] for r in page['sessions']],['run-1'])
        matched=self.run_query({'query':'РУССКИЙ %_'});self.assertEqual(matched['total'],1);self.assertEqual(matched['sessions'][0]['title'],'Русский %_needle')
        first=self.run_query({'limit':1})
        for change in ({'from':1},{'until':103},{'query':'other'}):self.run_query(dict(limit=1,cursor=first['next_cursor'],**change),True)
        for invalid in ({'from':3,'until':3},{'from':-1},{'until':253402300801},{'limit':51},{'limit':True},{'cursor':'bogus'},{'query':'a\nb'},{'offset':10000}):self.run_query(invalid,True)
    def test_long_unicode_query_keeps_cursor_bounded(self):
        needle='🧭'*256
        self.record(0,title=needle);self.record(1,title=needle)
        page=self.run_query({'query':needle,'limit':1});self.assertTrue(page['next_cursor']);self.assertLessEqual(len(page['next_cursor']),1024)
        self.assertEqual(len(self.run_query({'query':needle,'limit':1,'cursor':page['next_cursor']})['sessions']),1)
    def test_keyset_survives_insert_delete_and_rename(self):
        for i in range(8):self.record(i,100+i)
        first=self.run_query({'limit':3});seen={r['archive_id'] for r in first['sessions']}
        self.record(9,1000)
        (self.archives/(str(uuid.UUID(int=4))+'.json')).unlink()
        self.record(2,102,title='new title')
        rest=self.run_query({'limit':50,'cursor':first['next_cursor']})
        self.assertFalse(seen & {r['archive_id'] for r in rest['sessions']});self.assertNotIn('run-9',[r['run_id'] for r in rest['sessions']])
        self.assertEqual([r['run_id'] for r in rest['sessions']],['run-4','run-2','run-1','run-0'])
        self.assertEqual(self.run_query({'query':'new title'})['total'],1)
        self.assertEqual(self.run_query()['total'],8)
    def test_restore_is_offered_only_for_a_saved_conversation(self):
        # zerus-wyrqg: a Claude or Codex session that exited before its first
        # message reported a conversation ID but wrote no transcript. Its
        # archive does not offer Restore and says why; a saved one does.
        launch=self.root/'project';launch.mkdir()
        transcripts=self.root/'transcripts';transcripts.mkdir()
        (transcripts/'empty.jsonl').write_text('');(transcripts/'saved.jsonl').write_text('{}\n')
        cases={}
        for index,(agent,kind) in enumerate((a,k) for a in ('claude','codex') for k in ('missing','empty','saved')):
            ident=str(uuid.uuid4())
            value=dict(version=1,name=f'{agent}/example/{kind}',agent=agent,run_id=f'run-{index}',conversation_id=f'conversation-{index}',
                       state='archived',archive_id=ident,archived_at=100+index,created=1,cwd=str(launch),launch_dir=str(launch),
                       base=[agent],transcript=str(transcripts/f'{kind}.jsonl'))
            (self.archives/(ident+'.json')).write_text(json.dumps(value),encoding='utf-8')
            cases[ident]=(agent,kind)
        rows={row['archive_id']:row for row in self.run_query({'limit':50})['sessions']}
        for ident,(agent,kind) in cases.items():
            with self.subTest(agent=agent,kind=kind):
                row=rows[ident]
                if kind=='saved':
                    self.assertIn('restore',row['allowed_actions'])
                else:
                    self.assertNotIn('restore',row['allowed_actions'])
                    self.assertIn('ended before its first message',row['action_reasons']['restore'])
    def test_cursor_rejects_other_catalog_and_database_rebuild(self):
        self.record(0);self.record(1);first=self.run_query({'limit':1})
        (self.state/'archive-catalog-v1.sqlite3').unlink()
        self.run_query({'cursor':first['next_cursor']},True)
    def test_live_poll_excludes_archives_without_opening_their_journals(self):
        for i in range(150):self.record(i,100+i)
        first=self.run_query({'limit':1});self.assertEqual(first['total'],150)
        result=subprocess.run([str(HGS),'ls','--json','--local','--no-archives'],env=self.env,capture_output=True,text=True,timeout=20)
        self.assertEqual(result.returncode,0,result.stderr);snapshot=json.loads(result.stdout)
        self.assertTrue(snapshot['archive_paging']);self.assertEqual(snapshot['sessions'],[])
        self.assertNotIn('archive_page',snapshot)
    def test_default_listing_work_is_bounded_by_the_recent_preview(self):
        transcript=self.root/'saved.jsonl';transcript.write_text('{}\n')
        measurements=[];snapshots=[]
        for count in (50,20000):
            for index in range(0 if count==50 else 50,count):
                self.record(index,100+index,base=['codex'],launch_dir=str(self.root),transcript=str(transcript))
            # Existing files are indexed once; ordinary polls reuse that index.
            self.run_query({'limit':1})
            argv=[str(HGS),'ls','--json','--local']
            trace=self.root/'listing.trace'
            if shutil.which('strace'):
                argv=['strace','-f','-qq','-e','trace=%file','-o',str(trace),*argv]
            result=subprocess.run(argv,env=self.env,capture_output=True,text=True,timeout=120)
            self.assertEqual(result.returncode,0,result.stderr)
            snapshot=json.loads(result.stdout)
            rows=[row for row in snapshot['sessions'] if row.get('state')=='archived']
            self.assertEqual(len(rows),50,'Default ls must not summarize the whole archive')
            self.assertEqual([row['run_id'] for row in rows],[f'run-{i}' for i in range(count-1,count-51,-1)])
            snapshots.append(snapshot)
            self.assertIn('restore',rows[0]['allowed_actions'])
            if trace.exists():
                lines=trace.read_text().splitlines()
                files=sum('/archives/' in line for line in lines)
                transcripts=sum(str(transcript) in line for line in lines)
                self.assertLessEqual(files,100,'Only returned archives may be read')
                self.assertLessEqual(transcripts,150,'Action lookup must stay within the preview')
                measurements.append((len(lines),files,transcripts))
        if measurements:
            self.assertLessEqual(measurements[1][0],measurements[0][0]+100,'A larger archive must not add per-record I/O to ls')
        for count,snapshot in zip((50,20000),snapshots):
            descriptor=snapshot['archive_page']
            self.assertEqual(descriptor['total'],50)
            self.assertEqual(descriptor['total_capped'],count>50)
            self.assertEqual(bool(descriptor['next_cursor']),count>50)
            self.assertEqual(descriptor['command'],['archives','--json','--bounded'])
            self.assertNotIn('sessions',descriptor)
    def test_default_preview_cursor_continues_without_duplicates_after_an_insert(self):
        for index in range(137):self.record(index,100+index//5)
        self.refresh()
        result=subprocess.run([str(HGS),'ls','--json','--local'],env=self.env,capture_output=True,text=True,timeout=30)
        self.assertEqual(result.returncode,0,result.stderr);snapshot=json.loads(result.stdout)
        rows=snapshot['sessions'];self.assertEqual(len(rows),50)
        descriptor=snapshot['archive_page'];self.assertTrue(descriptor['total_capped'])
        self.record(200,1000)
        query={key:descriptor[key] for key in ('from','until','query')}
        cursor=descriptor['next_cursor']
        while cursor:
            page=self.run_query(dict(query,cursor=cursor),bounded=True)
            rows.extend(page['sessions']);cursor=page['next_cursor']
        identities=[row['archive_id'] for row in rows]
        self.assertEqual(len(identities),137);self.assertEqual(len(set(identities)),137)
        self.assertNotIn('run-200',[row['run_id'] for row in rows])
        self.assertEqual([(r['archived_at'],r['archive_id']) for r in rows],sorted(((r['archived_at'],r['archive_id']) for r in rows),reverse=True))
    @unittest.skipUnless(os.name=='posix','The private process birth fixture uses ps')
    def test_preview_archive_wins_over_its_retained_binding_without_hiding_other_runs(self):
        child=subprocess.Popen([sys.executable,'-c','import time; time.sleep(120)'],env=self.env)
        try:
            self.env.update(LC_ALL='C',TZ='UTC')
            start=subprocess.check_output(['ps','-p',str(child.pid),'-o','lstart='],env=self.env,text=True).strip()
            self.assertTrue(start)
            archived=self.record(0,1000,pid=child.pid,process_start=start)
            archive_path=self.archives/(archived['archive_id']+'.json');archive_path.unlink()
            binding={key:value for key,value in archived.items() if key not in ('archive_id','archived_at','state')}
            path=self.state/(hashlib.sha256(binding['name'].encode()).hexdigest()+'.json')
            original=json.dumps(binding).encode();path.write_bytes(original)
            archive_path.write_text(json.dumps(archived))
            def listing(*extra):
                self.refresh()
                result=subprocess.run([str(HGS),'ls','--json','--local',*extra],env=self.env,capture_output=True,text=True,timeout=30)
                self.assertEqual(result.returncode,0,result.stderr);return json.loads(result.stdout)
            # The original process birth keeps the binding present across the
            # archive-write/remove-binding crash cut; no real agent is used.
            active=listing('--no-archives')['sessions']
            self.assertEqual(len(active),1);self.assertEqual(active[0]['run_id'],binding['run_id'])
            self.assertIsNone(child.poll());self.assertEqual(path.read_bytes(),original)
            rows=listing()['sessions']
            self.assertEqual(len(rows),1,'The archive and its retained binding must not list twice')
            self.assertEqual(rows[0]['archive_id'],archived['archive_id'])
            self.assertEqual(rows[0]['state'],'archived')
            self.assertEqual(path.read_bytes(),original,'Projection never removes the retained binding')
            for key in ('run_id','conversation_id'):
                changed=dict(binding);changed[key]='another-'+key;path.write_text(json.dumps(changed))
                rows=listing()['sessions']
                self.assertEqual(len(rows),2,'A different run or conversation remains a current binding')
                self.assertEqual(sum(row.get('state')=='archived' for row in rows),1)
            path.write_bytes(original)
            for index in range(1,51):self.record(index,2000+index)
            page=listing()
            self.assertEqual(len(page['sessions']),51,'Archives outside the preview never hide a binding')
            self.assertEqual(sum(row.get('state')=='archived' for row in page['sessions']),50)
            self.assertTrue(page['archive_page']['next_cursor'])
            self.assertEqual(len(listing('--no-archives')['sessions']),1)
        finally:
            if child.poll() is None:child.terminate()
            child.wait(timeout=5)
    def test_preview_preserves_the_original_journal_read_mark_and_row_fields(self):
        owner='session:00000000-0000-4000-8000-000000000050'
        record=self.record(0,journal_name=owner)
        mark=dict(reply_id='read-reply',attention_signature=None,at=123)
        (self.state/'read-marks.json').write_text(json.dumps(dict(version=1,sessions={owner:{record['conversation_id']:mark}})))
        self.refresh()
        result=subprocess.run([str(HGS),'ls','--json','--local'],env=self.env,capture_output=True,text=True,timeout=30)
        self.assertEqual(result.returncode,0,result.stderr);row=json.loads(result.stdout)['sessions'][0]
        self.assertEqual(row['read'],mark)
        self.assertEqual(row['optional_questions'],{'count':0})
        self.assertIsNone(row['attention_signature'])
        explicit=self.run_query()['sessions'][0]
        self.assertNotIn('read',explicit,'Explicit pages retain their existing row projection')
        for key in ('archive_id','archived_at','run_id','conversation_id','state','resumable','allowed_actions','action_reasons','notification_preferences'):
            self.assertEqual(row[key],explicit[key],key)
    def test_archived_preferences_follow_the_original_journal_and_current_store(self):
        owner='session:00000000-0000-4000-8000-000000000050'
        row=self.record(0,journal_name=owner)
        preference=dict(conversation_id=row['conversation_id'],muted=True,revision=3,
                        suppressed=dict(reply_id=None,attention_signature=None,optional_question_ids=[],main_attention=None,child_attention={}))
        store=self.state/'notifications.json'
        store.write_text(json.dumps(dict(version=1,sessions={owner:{row['conversation_id']:preference}})))
        for bounded in (False,True):
            result=self.run_query(bounded=bounded)['sessions'][0]
            self.assertEqual(result.get('notification_preferences'),preference)
            self.assertEqual(result['conversation_id'],row['conversation_id'])
        # A warm metadata index must not retain an earlier preference revision.
        preference.update(muted=False,revision=4)
        store.write_text(json.dumps(dict(version=1,sessions={owner:{row['conversation_id']:preference}})))
        self.assertEqual(self.run_query()['sessions'][0].get('notification_preferences'),preference)
        store.write_text('{broken')
        result=self.run_query()['sessions'][0]
        self.assertIn('notification_preferences',result)
        self.assertIsNone(result['notification_preferences'],'Unreadable preferences remain unknown')
    def test_a_valid_large_preference_fits_an_archive_page_and_the_response_bound(self):
        row=self.record(0)
        children={f'child-{i}':dict(phase='input',request_id='q'*1200,started_at=None) for i in range(220)}
        preference=dict(conversation_id=row['conversation_id'],muted=True,revision=1,
                        suppressed=dict(reply_id=None,attention_signature=None,optional_question_ids=[],main_attention=None,child_attention=children))
        self.assertGreater(len(json.dumps(preference)),250000)
        (self.state/'notifications.json').write_text(json.dumps(dict(version=1,sessions={row['name']:{row['conversation_id']:preference}})))
        page=self.run_query(bounded=True)
        self.assertEqual(page['sessions'][0].get('notification_preferences'),preference)
        self.assertLess(len(json.dumps(page)),1048576)
    def test_non_regular_archive_entries_never_block_catalog(self):
        self.record(0)
        if hasattr(os,'mkfifo'):os.mkfifo(self.archives/(str(uuid.UUID(int=2))+'.json'))
        self.assertEqual(self.run_query()['total'],1)
    def test_exact_lookup_needs_no_catalog_or_transcript_and_rejects_wrong_identity(self):
        row=self.record(0)
        def lookup(value):
            return subprocess.run([str(HGS),'archives','lookup','--json'],input=json.dumps(value),capture_output=True,text=True,env=self.env,timeout=5)
        value={key:row[key] for key in ('name','archive_id')}
        result=lookup(value);self.assertEqual(result.returncode,0,result.stderr)
        self.assertEqual(json.loads(result.stdout),dict(version=1,**value,run_id=row['run_id'],conversation_id=row['conversation_id']))
        self.assertFalse((self.state/'archive-catalog-v1.sqlite3').exists())
        for bad in ({**value,'name':'codex/example/other'},{**value,'archive_id':str(uuid.UUID(int=99))},{**value,'extra':True},{'name':value['name']},{**value,'name':'@peer'}):
            self.assertNotEqual(lookup(bad).returncode,0,bad)
        (self.archives/(row['archive_id']+'.json')).unlink()
        if hasattr(os,'mkfifo'):
            os.mkfifo(self.archives/(row['archive_id']+'.json'))
            self.assertNotEqual(lookup(value).returncode,0)
    def test_bidirectional_keysets_reach_all_rows_with_only_current_window_anchors(self):
        for i in range(257):self.record(i,100+i//4)
        expected=sorted([(100+i//4,str(uuid.UUID(int=i+1))) for i in range(257)],reverse=True)
        page=self.run_query();position=0
        while True:
            self.assertEqual(page['position'],position)
            self.assertEqual([(r['archived_at'],r['archive_id']) for r in page['sessions']],expected[position:position+50])
            self.assertLessEqual(len(page['sessions']),50)
            self.assertEqual(bool(page['previous_cursor']),position>0)
            if not page['next_cursor']:break
            page=self.run_query({'cursor':page['next_cursor']});position+=50
        self.assertEqual(position,250)
        while page['previous_cursor']:
            page=self.run_query({'cursor':page['previous_cursor']});position=max(0,position-50)
            self.assertEqual(page['position'],position)
            self.assertEqual([(r['archived_at'],r['archive_id']) for r in page['sessions']],expected[position:position+50])
        self.assertEqual(position,0)
        next_page=self.run_query({'cursor':page['next_cursor']})
        for change in ({'query':'changed'},{'from':1},{'until':200}):self.run_query(dict(cursor=next_page['previous_cursor'],**change),True)

    # zerus-080n8: bounded pages answer only requests that opt in with --bounded.
    EXACT_KEYS={'version','sessions','next_cursor','previous_cursor','position','total','from','until','query'}
    def test_bounded_pages_are_opt_in_and_the_exact_page_keeps_its_payload(self):
        for i in range(60):self.record(i,100+i)
        exact=self.run_query({'limit':50})
        self.assertEqual(set(exact),self.EXACT_KEYS)
        bounded=self.run_query({'limit':50},bounded=True)
        self.assertEqual(set(bounded),self.EXACT_KEYS|{'total_capped','position_capped','partial','searched'})
        self.assertEqual((bounded['total'],bounded['total_capped'],bounded['partial'],bounded['position'],bounded['position_capped']),(60,False,False,0,False))
        self.assertEqual([r['archive_id'] for r in bounded['sessions']],[r['archive_id'] for r in exact['sessions']])
        self.assertEqual(bounded['searched'],51)
        rest=self.run_query({'limit':50,'cursor':bounded['next_cursor']},bounded=True)
        self.assertEqual((len(rest['sessions']),rest['position'],rest['next_cursor'],rest['partial']),(10,50,'',False))
        # The exact reader also continues a bounded page's cursor: it is the same (at, id) keyset.
        self.assertEqual(len(self.run_query({'limit':50,'cursor':bounded['next_cursor']})['sessions']),10)
        for args in (['archives','lookup','--json','--bounded'],['archives','--bounded','--json'],['archives','--json','--bounded','--bounded']):
            r=subprocess.run([str(HGS),*args],input='{}',capture_output=True,text=True,env=self.env,timeout=30)
            self.assertNotEqual(r.returncode,0,args)
        helped=subprocess.run([str(HGS),'--help'],capture_output=True,text=True,env=self.env,timeout=30)
        self.assertIn('archive-bounded-pages-v1',[line.strip() for line in helped.stdout.splitlines()])
    def test_a_peer_gets_bounded_pages_only_when_its_cli_lists_the_marker(self):
        dry=subprocess.run([str(HGS),'--dry-run','@peer','archives','--json','--bounded'],capture_output=True,text=True,env=dict(self.env,HGS_PEERS='peer'),timeout=30)
        self.assertEqual(dry.returncode,0,dry.stderr)
        words=shlex.split(dry.stdout.strip().splitlines()[-1]);script=words[-1].replace(' --dry-run','')
        self.assertEqual(words[words.index('peer')+1:words.index('peer')+3],['sh','-c'])
        for marker,expected in ((True,['archives','--json','--bounded']),(False,['archives','--json'])):
            home=self.root/('peer-new' if marker else 'peer-old');bin_dir=home/'.local'/'bin';bin_dir.mkdir(parents=True)
            stub=bin_dir/'hgs';stub.write_text('#!/bin/sh\nif [ "$1" = --help ]; then echo usage; %s exit 0; fi\nprintf "%%s\\n" "$@" > "$HOME/called"\n' % ('echo "       archive-bounded-pages-v1";' if marker else ''),encoding='utf-8');stub.chmod(0o700)
            ran=subprocess.run(['sh','-c',script],env={'HOME':str(home),'PATH':'/usr/bin:/bin'},capture_output=True,text=True,timeout=30)
            self.assertEqual(ran.returncode,0,ran.stderr)
            self.assertEqual((home/'called').read_text(encoding='utf-8').split(),expected)

    def test_bounded_pages_cut_by_bytes_reach_every_row_in_both_directions(self):
        # zerus-m0boz: about 9.5 kB of metadata per row, so 250 kB holds 26 of a 50-row page.
        for i in range(80):
            value=self.record(i,100+i,title='\U0001f9ed'*1024);value['cwd']='/synthetic/'+'p'*3000
            path=self.archives/(value['archive_id']+'.json');path.write_text(json.dumps(value),encoding='utf-8')
        expected=[str(uuid.UUID(int=i+1)) for i in reversed(range(80))]
        page=self.run_query({'limit':50},bounded=True);older=[];sizes=[]
        while True:
            older+=[r['archive_id'] for r in page['sessions']];sizes.append(len(page['sessions']))
            if not page['next_cursor']:break
            page=self.run_query({'limit':50,'cursor':page['next_cursor']},bounded=True)
        self.assertEqual(older,expected);self.assertTrue(all(n<50 for n in sizes) and len(sizes)>2,sizes)
        newer=[];pages=[]
        while page['previous_cursor']:
            page=self.run_query({'limit':50,'cursor':page['previous_cursor']},bounded=True)
            ids=[r['archive_id'] for r in page['sessions']];pages.append(ids);newer+=ids
            # Each page's position is the count of rows newer than its first one.
            self.assertEqual(page['position'],expected.index(ids[0]))
        self.assertEqual(sorted(newer),sorted(expected[:80-sizes[-1]]))
        self.assertEqual(len(newer),len(set(newer)))
        for ids in pages:self.assertEqual(ids,sorted(ids,key=expected.index))

if __name__=='__main__':unittest.main()
