"""Indexed archive pages on private synthetic records; no agent or live mark is touched."""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import unittest
import uuid

REPO=Path(__file__).resolve().parents[1]
HGS=Path(os.environ.get('HGS_TEST_BIN',REPO/'target/debug/hgs')).resolve()

class ArchivePages(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(prefix='zerus-archive-pages-');self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name);self.state=self.root/'state';self.archives=self.state/'archives';self.archives.mkdir(parents=True)
        socket=self.root/'tmux-private';socket.mkdir(mode=0o700)
        self.env={k:v for k,v in os.environ.items() if not k.startswith('HGS_') and k not in ('TMUX','TMUX_PANE','TMUX_TMPDIR')}
        self.env.update(HOME=str(self.root),HGS_STATE_DIR=str(self.state),HGS_CONFIG_DIR=str(self.root/'config'),HGS_SELF='archive-fixture',HGS_PEERS='',TMUX_TMPDIR=str(socket))
        # Metadata commands need the ordinary explicit local-session opt-in on Windows.
        (self.root/'config').mkdir();(self.root/'config'/'local-sessions').touch()
    def record(self,index,at=100,**extra):
        ident=str(uuid.UUID(int=index+1));value=dict(version=1,name=f'codex/example/archived-{index}',agent='codex',run_id=f'run-{index}',conversation_id=f'conversation-{index}',state='archived',archive_id=ident,archived_at=at,created=1,cwd='/synthetic/project',**extra)
        path=self.archives/(ident+'.json');temp=path.with_suffix('.tmp');temp.write_text(json.dumps(value),encoding='utf-8');temp.replace(path)
        return value
    def run_query(self,query=None,fail=False):
        r=subprocess.run([str(HGS),'archives','--json'],input=json.dumps(query or {}),capture_output=True,text=True,env=self.env,timeout=30)
        if fail:self.assertNotEqual(r.returncode,0,r.stdout);return r
        self.assertEqual(r.returncode,0,r.stderr);return json.loads(r.stdout)
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
    def preferences(self,args):
        r=subprocess.run([str(HGS),*args],input='{}',capture_output=True,text=True,env=self.env,timeout=30)
        self.assertEqual(r.returncode,0,r.stderr)
        rows=[row for row in json.loads(r.stdout)['sessions'] if row.get('state')=='archived']
        for row in rows:self.assertIn('notification_preferences',row)
        return {row['archive_id']:row['notification_preferences'] for row in rows}
    def test_archived_rows_carry_notification_preferences_exactly_as_ls(self):
        # The relay's preference ledger refuses a known conversation whose row lacks them.
        muted,plain=self.record(0,100),self.record(1,101)
        stored=dict(conversation_id=muted['conversation_id'],muted=True,revision=4,suppressed=dict(reply_id=None,
            attention_signature=None,optional_question_ids=[],main_attention=None,child_attention={}))
        store=self.state/'notifications.json'
        store.write_text(json.dumps(dict(version=1,sessions={muted['name']:{muted['conversation_id']:stored}})),encoding='utf-8')
        listed=self.preferences(['ls','--json','--local'])
        self.assertEqual(listed[muted['archive_id']],stored)
        self.assertEqual(listed[plain['archive_id']],dict(stored,conversation_id=plain['conversation_id'],muted=False,revision=0))
        self.assertEqual(self.preferences(['archives','--json']),listed)
        # Unknown preferences are null, never a fabricated unmuted default.
        store.write_text('{',encoding='utf-8')
        self.assertEqual(self.preferences(['ls','--json','--local']),dict.fromkeys(listed))
        self.assertEqual(self.preferences(['archives','--json']),dict.fromkeys(listed))
    @unittest.skipUnless(os.name=='nt','the Windows mobile archive scope exists only on Windows')
    def test_windows_mobile_archive_rows_carry_notification_preferences(self):
        row=self.record(0,100)
        stored=dict(conversation_id=row['conversation_id'],muted=True,revision=1,suppressed=dict(reply_id=None,
            attention_signature=None,optional_question_ids=[],main_attention=None,child_attention={}))
        (self.state/'notifications.json').write_text(json.dumps(dict(version=1,sessions={row['name']:{row['conversation_id']:stored}})),encoding='utf-8')
        self.assertEqual(self.preferences(['archives','--json','--windows-mobile']),{row['archive_id']:stored})
        self.assertEqual(self.preferences(['archives','--json']),{row['archive_id']:stored})
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

if __name__=='__main__':unittest.main()
